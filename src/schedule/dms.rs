//! Dead Man's Switch implementation
//!
//! Monitors system health by checking periodic signals.
//! Inspired by Healthchecks.io pattern.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// State of a monitored job
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobState {
    /// Job is running on schedule
    Up,
    /// Job is late but within grace period
    Late,
    /// Job has stopped reporting
    Down,
    /// Job is newly registered, no data yet
    New,
    /// Job is paused
    Paused,
}

impl JobState {
    /// Get emoji representation
    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Up => "✅",
            Self::Late => "⚠️",
            Self::Down => "🔴",
            Self::New => "🆕",
            Self::Paused => "⏸️",
        }
    }
}

/// Configuration for a monitored check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckConfig {
    /// Expected interval between pings
    pub period: Duration,

    /// Grace period after expected ping
    pub grace: Duration,
}

impl Default for CheckConfig {
    fn default() -> Self {
        Self {
            period: Duration::hours(1),
            grace: Duration::minutes(30),
        }
    }
}

/// A single check/ping record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Check {
    /// Check name/ID
    pub name: String,

    /// Current state
    pub state: JobState,

    /// Configuration
    pub config: CheckConfig,

    /// Last ping time
    pub last_ping: Option<DateTime<Utc>>,

    /// Next expected ping
    pub next_expected: Option<DateTime<Utc>>,

    /// Consecutive missed pings
    pub misses: u32,

    /// Total pings received
    pub total_pings: u64,
}

impl Check {
    /// Create a new check
    pub fn new(name: impl Into<String>, config: CheckConfig) -> Self {
        Self {
            name: name.into(),
            state: JobState::New,
            config,
            last_ping: None,
            next_expected: None,
            misses: 0,
            total_pings: 0,
        }
    }

    /// Record a successful ping
    pub fn ping(&mut self) {
        let now = Utc::now();
        self.last_ping = Some(now);
        self.next_expected = Some(now + self.config.period);
        self.state = JobState::Up;
        self.misses = 0;
        self.total_pings += 1;
    }

    /// Record a failure
    pub fn fail(&mut self) {
        self.misses += 1;
        self.state = JobState::Down;
    }

    /// Evaluate current state based on time
    pub fn evaluate(&mut self, now: DateTime<Utc>) {
        // Paused checks don't change state
        if self.state == JobState::Paused {
            return;
        }

        // New checks stay new until first ping
        if self.state == JobState::New {
            return;
        }

        let next = match self.next_expected {
            Some(t) => t,
            None => return,
        };

        if now > next + self.config.grace {
            // Past grace period
            self.state = JobState::Down;
            self.misses += 1;
            // Update next expected to prevent repeated increment
            self.next_expected = Some(now + self.config.period);
        } else if now > next {
            // Late but within grace
            self.state = JobState::Late;
        } else {
            // On time
            self.state = JobState::Up;
        }
    }

    /// Pause the check
    pub fn pause(&mut self) {
        self.state = JobState::Paused;
    }

    /// Resume the check
    pub fn resume(&mut self) {
        if self.state == JobState::Paused {
            self.state = JobState::Up;
            self.next_expected = Some(Utc::now() + self.config.period);
        }
    }
}

/// Alert callback type
type AlertCallback = Box<dyn Fn(&Check) + Send + Sync>;

/// Dead Man's Switch manager
pub struct DeadManSwitch {
    /// Registered checks
    checks: HashMap<String, Check>,

    /// Alert callback
    #[allow(dead_code)]
    on_alert: Option<AlertCallback>,

    /// Escalation threshold (consecutive misses)
    escalation_threshold: u32,
}

impl DeadManSwitch {
    /// Create a new DMS
    pub fn new() -> Self {
        Self {
            checks: HashMap::new(),
            on_alert: None,
            escalation_threshold: 3,
        }
    }

    /// Set escalation threshold
    pub fn with_escalation_threshold(mut self, threshold: u32) -> Self {
        self.escalation_threshold = threshold;
        self
    }

    /// Register a new check
    pub fn register(&mut self, name: impl Into<String>, config: CheckConfig) {
        let name = name.into();
        self.checks.insert(name.clone(), Check::new(name, config));
    }

    /// Record a ping for a check
    pub fn ping(&mut self, name: &str) -> bool {
        if let Some(check) = self.checks.get_mut(name) {
            check.ping();
            true
        } else {
            false
        }
    }

    /// Record a failure for a check
    pub fn fail(&mut self, name: &str) -> bool {
        if let Some(check) = self.checks.get_mut(name) {
            check.fail();
            true
        } else {
            false
        }
    }

    /// Evaluate all checks
    pub fn evaluate_all(&mut self) {
        let now = Utc::now();
        for check in self.checks.values_mut() {
            let was_down = check.state == JobState::Down;
            check.evaluate(now);

            // Fire alert on transition to Down
            if !was_down && check.state == JobState::Down {
                if let Some(alert) = &self.on_alert {
                    alert(check);
                }
            }
        }
    }

    /// Get all checks
    pub fn checks(&self) -> impl Iterator<Item = &Check> {
        self.checks.values()
    }

    /// Get checks that are down
    pub fn down_checks(&self) -> Vec<&Check> {
        self.checks
            .values()
            .filter(|c| c.state == JobState::Down)
            .collect()
    }

    /// Get checks that need escalation
    pub fn needs_escalation(&self) -> Vec<&Check> {
        self.checks
            .values()
            .filter(|c| c.misses >= self.escalation_threshold)
            .collect()
    }

    /// Get a check by name
    pub fn get(&self, name: &str) -> Option<&Check> {
        self.checks.get(name)
    }

    /// Get summary of all checks
    pub fn summary(&self) -> DmsSummary {
        let mut summary = DmsSummary::default();
        for check in self.checks.values() {
            match check.state {
                JobState::Up => summary.up += 1,
                JobState::Late => summary.late += 1,
                JobState::Down => summary.down += 1,
                JobState::New => summary.new += 1,
                JobState::Paused => summary.paused += 1,
            }
        }
        summary
    }
}

impl Default for DeadManSwitch {
    fn default() -> Self {
        Self::new()
    }
}

/// Summary of DMS state
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DmsSummary {
    pub up: usize,
    pub late: usize,
    pub down: usize,
    pub new: usize,
    pub paused: usize,
}

impl DmsSummary {
    /// Check if all checks are healthy
    pub fn all_healthy(&self) -> bool {
        self.down == 0 && self.late == 0
    }

    /// Get total count
    pub fn total(&self) -> usize {
        self.up + self.late + self.down + self.new + self.paused
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_ping() {
        let mut check = Check::new("test", CheckConfig::default());
        assert_eq!(check.state, JobState::New);

        check.ping();
        assert_eq!(check.state, JobState::Up);
        assert_eq!(check.total_pings, 1);
        assert!(check.last_ping.is_some());
    }

    #[test]
    fn test_check_late() {
        let mut check = Check::new(
            "test",
            CheckConfig {
                period: Duration::minutes(10),
                grace: Duration::minutes(5),
            },
        );

        check.ping();

        // Simulate time passing (11 minutes - past period but within grace)
        let future = Utc::now() + Duration::minutes(11);
        check.evaluate(future);

        assert_eq!(check.state, JobState::Late);
    }

    #[test]
    fn test_check_down() {
        let mut check = Check::new(
            "test",
            CheckConfig {
                period: Duration::minutes(10),
                grace: Duration::minutes(5),
            },
        );

        check.ping();

        // Simulate time passing (20 minutes - past grace period)
        let future = Utc::now() + Duration::minutes(20);
        check.evaluate(future);

        assert_eq!(check.state, JobState::Down);
        assert_eq!(check.misses, 1);
    }

    #[test]
    fn test_dms_register_and_ping() {
        let mut dms = DeadManSwitch::new();
        dms.register("heartbeat", CheckConfig::default());

        assert!(dms.ping("heartbeat"));
        assert!(!dms.ping("nonexistent"));

        let check = dms.get("heartbeat").unwrap();
        assert_eq!(check.state, JobState::Up);
    }

    #[test]
    fn test_dms_summary() {
        let mut dms = DeadManSwitch::new();

        dms.register("up1", CheckConfig::default());
        dms.register("up2", CheckConfig::default());
        dms.register("new1", CheckConfig::default());

        dms.ping("up1");
        dms.ping("up2");
        // new1 stays as New

        let summary = dms.summary();
        assert_eq!(summary.up, 2);
        assert_eq!(summary.new, 1);
        assert!(summary.all_healthy());
    }

    #[test]
    fn test_escalation() {
        let mut dms = DeadManSwitch::new().with_escalation_threshold(3);
        dms.register(
            "flaky",
            CheckConfig {
                period: Duration::minutes(1),
                grace: Duration::minutes(1),
            },
        );

        dms.ping("flaky");

        // Simulate multiple failures by calling fail() directly
        for _ in 0..3 {
            dms.fail("flaky");
        }

        assert_eq!(dms.needs_escalation().len(), 1);
        let check = dms.get("flaky").unwrap();
        assert_eq!(check.misses, 3);
    }
}
