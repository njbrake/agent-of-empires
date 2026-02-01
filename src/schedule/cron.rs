//! Cron job monitoring

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::openclaw::config::CronJob;

/// Health status of a cron job
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CronJobHealth {
    /// Job is running normally
    Up,
    /// Job is late but within grace period
    Late,
    /// Job has failed or is overdue
    Down,
    /// Job is disabled
    Disabled,
    /// Unknown status (no data)
    Unknown,
}

impl CronJobHealth {
    /// Get emoji for status
    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Up => "✅",
            Self::Late => "⚠️",
            Self::Down => "🔴",
            Self::Disabled => "⏸️",
            Self::Unknown => "❓",
        }
    }

    /// Get label for status
    pub fn label(&self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Late => "late",
            Self::Down => "down",
            Self::Disabled => "disabled",
            Self::Unknown => "unknown",
        }
    }
}

/// Monitored cron job state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoredJob {
    /// Job ID
    pub id: String,

    /// Job name
    pub name: String,

    /// Current health status
    pub health: CronJobHealth,

    /// Grace period for this job
    pub grace_period: Duration,

    /// Consecutive failure count
    pub consecutive_failures: u32,

    /// Last check time
    pub last_check: Option<DateTime<Utc>>,

    /// Last successful run
    pub last_success: Option<DateTime<Utc>>,

    /// Next expected run
    pub next_run: Option<DateTime<Utc>>,
}

/// Cron job monitor
pub struct CronMonitor {
    /// Monitored jobs
    jobs: HashMap<String, MonitoredJob>,

    /// Default grace period
    default_grace: Duration,

    /// Escalation threshold (consecutive failures)
    escalation_threshold: u32,
}

impl CronMonitor {
    /// Create a new monitor with default settings
    pub fn new() -> Self {
        Self {
            jobs: HashMap::new(),
            default_grace: Duration::hours(1),
            escalation_threshold: 3,
        }
    }

    /// Set the default grace period
    pub fn with_default_grace(mut self, grace: Duration) -> Self {
        self.default_grace = grace;
        self
    }

    /// Set the escalation threshold
    pub fn with_escalation_threshold(mut self, threshold: u32) -> Self {
        self.escalation_threshold = threshold;
        self
    }

    /// Configure grace period for a specific job
    pub fn set_grace_period(&mut self, job_id: &str, grace: Duration) {
        if let Some(job) = self.jobs.get_mut(job_id) {
            job.grace_period = grace;
        }
    }

    /// Update job state from OpenClaw cron job data
    pub fn update_job(&mut self, cron_job: &CronJob) {
        let now = Utc::now();

        let health = self.calculate_health(cron_job, now);

        let entry = self
            .jobs
            .entry(cron_job.id.clone())
            .or_insert_with(|| MonitoredJob {
                id: cron_job.id.clone(),
                name: cron_job.display_name().to_string(),
                health: CronJobHealth::Unknown,
                grace_period: self.default_grace,
                consecutive_failures: 0,
                last_check: None,
                last_success: None,
                next_run: None,
            });

        // Update consecutive failures
        if health == CronJobHealth::Down {
            if entry.health != CronJobHealth::Down {
                entry.consecutive_failures = 1;
            } else {
                entry.consecutive_failures += 1;
            }
        } else if health == CronJobHealth::Up {
            entry.consecutive_failures = 0;
            entry.last_success = Some(now);
        }

        entry.health = health;
        entry.last_check = Some(now);

        // Update next run from cron job state
        if let Some(state) = &cron_job.state {
            entry.next_run = state
                .next_run_at_ms
                .map(|ms| DateTime::from_timestamp_millis(ms).unwrap_or(now));
        }
    }

    /// Calculate health status for a cron job
    fn calculate_health(&self, cron_job: &CronJob, now: DateTime<Utc>) -> CronJobHealth {
        if !cron_job.enabled {
            return CronJobHealth::Disabled;
        }

        let state = match &cron_job.state {
            Some(s) => s,
            None => return CronJobHealth::Unknown,
        };

        // Check last status
        let last_ok = state
            .last_status
            .as_ref()
            .map(|s| s == "ok")
            .unwrap_or(false);

        if !last_ok && state.last_status.is_some() {
            return CronJobHealth::Down;
        }

        // Check if overdue
        if let Some(next_ms) = state.next_run_at_ms {
            let next_run = DateTime::from_timestamp_millis(next_ms).unwrap_or(now);
            let grace = self
                .jobs
                .get(&cron_job.id)
                .map(|j| j.grace_period)
                .unwrap_or(self.default_grace);

            if now > next_run + grace {
                return CronJobHealth::Down;
            } else if now > next_run {
                return CronJobHealth::Late;
            }
        }

        CronJobHealth::Up
    }

    /// Get all jobs
    pub fn jobs(&self) -> impl Iterator<Item = &MonitoredJob> {
        self.jobs.values()
    }

    /// Get jobs that need escalation
    pub fn needs_escalation(&self) -> Vec<&MonitoredJob> {
        self.jobs
            .values()
            .filter(|j| j.consecutive_failures >= self.escalation_threshold)
            .collect()
    }

    /// Get jobs that are down
    pub fn down_jobs(&self) -> Vec<&MonitoredJob> {
        self.jobs
            .values()
            .filter(|j| j.health == CronJobHealth::Down)
            .collect()
    }

    /// Get overall health summary
    pub fn summary(&self) -> HealthSummary {
        let mut summary = HealthSummary::default();

        for job in self.jobs.values() {
            match job.health {
                CronJobHealth::Up => summary.up += 1,
                CronJobHealth::Late => summary.late += 1,
                CronJobHealth::Down => summary.down += 1,
                CronJobHealth::Disabled => summary.disabled += 1,
                CronJobHealth::Unknown => summary.unknown += 1,
            }
        }

        summary
    }
}

impl Default for CronMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// Summary of cron job health
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HealthSummary {
    pub up: usize,
    pub late: usize,
    pub down: usize,
    pub disabled: usize,
    pub unknown: usize,
}

impl HealthSummary {
    /// Check if all jobs are healthy
    pub fn all_healthy(&self) -> bool {
        self.down == 0 && self.late == 0
    }

    /// Get total job count
    pub fn total(&self) -> usize {
        self.up + self.late + self.down + self.disabled + self.unknown
    }

    /// Format as status line
    pub fn status_line(&self) -> String {
        if self.all_healthy() {
            format!("✅ Cron: {}/{} up", self.up, self.total())
        } else if self.down > 0 {
            format!("🔴 Cron: {} down, {} late", self.down, self.late)
        } else {
            format!("⚠️ Cron: {} late", self.late)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openclaw::config::{CronJobState, CronPayload, CronSchedule};

    fn make_test_job(id: &str, enabled: bool, last_status: Option<&str>) -> CronJob {
        CronJob {
            id: id.to_string(),
            name: Some(format!("Test Job {}", id)),
            enabled,
            schedule: CronSchedule {
                kind: "cron".to_string(),
                expr: Some("0 9 * * *".to_string()),
                tz: None,
            },
            session_target: "main".to_string(),
            payload: CronPayload {
                kind: "systemEvent".to_string(),
                text: Some("test".to_string()),
                message: None,
            },
            state: Some(CronJobState {
                next_run_at_ms: Some(Utc::now().timestamp_millis() + 3600000),
                last_run_at_ms: Some(Utc::now().timestamp_millis() - 3600000),
                last_status: last_status.map(String::from),
                last_error: None,
                last_duration_ms: Some(100),
            }),
        }
    }

    #[test]
    fn test_health_up() {
        let mut monitor = CronMonitor::new();
        let job = make_test_job("job1", true, Some("ok"));

        monitor.update_job(&job);

        let monitored = monitor.jobs.get("job1").unwrap();
        assert_eq!(monitored.health, CronJobHealth::Up);
    }

    #[test]
    fn test_health_disabled() {
        let mut monitor = CronMonitor::new();
        let job = make_test_job("job2", false, Some("ok"));

        monitor.update_job(&job);

        let monitored = monitor.jobs.get("job2").unwrap();
        assert_eq!(monitored.health, CronJobHealth::Disabled);
    }

    #[test]
    fn test_health_down() {
        let mut monitor = CronMonitor::new();
        let job = make_test_job("job3", true, Some("error"));

        monitor.update_job(&job);

        let monitored = monitor.jobs.get("job3").unwrap();
        assert_eq!(monitored.health, CronJobHealth::Down);
    }

    #[test]
    fn test_consecutive_failures() {
        let mut monitor = CronMonitor::new().with_escalation_threshold(3);
        let job = make_test_job("job4", true, Some("error"));

        // Simulate 3 consecutive failures
        monitor.update_job(&job);
        monitor.update_job(&job);
        monitor.update_job(&job);

        let monitored = monitor.jobs.get("job4").unwrap();
        assert_eq!(monitored.consecutive_failures, 3);
        assert_eq!(monitor.needs_escalation().len(), 1);
    }

    #[test]
    fn test_summary() {
        let mut monitor = CronMonitor::new();

        monitor.update_job(&make_test_job("up1", true, Some("ok")));
        monitor.update_job(&make_test_job("up2", true, Some("ok")));
        monitor.update_job(&make_test_job("down1", true, Some("error")));
        monitor.update_job(&make_test_job("disabled1", false, Some("ok")));

        let summary = monitor.summary();
        assert_eq!(summary.up, 2);
        assert_eq!(summary.down, 1);
        assert_eq!(summary.disabled, 1);
        assert!(!summary.all_healthy());
    }
}
