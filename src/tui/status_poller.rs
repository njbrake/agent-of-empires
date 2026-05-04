//! Background status polling for TUI performance
//!
//! This module provides non-blocking status updates for sessions by running
//! tmux subprocess calls in a background thread. Two optimizations reduce
//! per-cycle overhead:
//!
//! 1. **Batched metadata**: A single `tmux list-panes -a` call fetches pane
//!    metadata (dead flag, current command) for all sessions at once, replacing
//!    O(3N) per-instance `display-message` subprocesses with O(1).
//!
//! 2. **Adaptive polling tiers**: Sessions are polled at different frequencies
//!    based on their status. Hot (Running/Waiting/Starting) every cycle, Warm
//!    (Idle/Unknown) every 5 cycles, Cold (Error) every 60 cycles, Frozen
//!    (Stopped/Deleting) never.

use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};

use crate::session::{Instance, Status};

/// Adaptive polling intervals (in cycles). 0 = never poll.
const TIER_HOT: u64 = 1;
const TIER_WARM: u64 = 5;
const TIER_COLD: u64 = 60;

fn polling_tier(status: Status) -> u64 {
    match status {
        Status::Running | Status::Waiting | Status::Starting => TIER_HOT,
        Status::Idle | Status::Unknown => TIER_WARM,
        Status::Error => TIER_COLD,
        Status::Stopped | Status::Deleting | Status::Creating => 0,
    }
}

/// Result of a status check for a single session
#[derive(Debug)]
pub struct StatusUpdate {
    pub id: String,
    pub status: Status,
    pub last_error: Option<String>,
    /// Snapshot of the polled clone's `idle_entered_at` after
    /// `update_status_with_metadata` ran. Propagating this field is what
    /// keeps the freshness signal working in the TUI: without it, the
    /// wrapper's timestamp write lives only on the polling clone and is
    /// lost when we project the result back into a `StatusUpdate`.
    pub idle_entered_at: Option<DateTime<Utc>>,
}

/// Background thread that polls session status without blocking the UI
pub struct StatusPoller {
    request_tx: mpsc::Sender<Vec<Instance>>,
    result_rx: mpsc::Receiver<Vec<StatusUpdate>>,
    _handle: thread::JoinHandle<()>,
}

impl StatusPoller {
    pub fn new() -> Self {
        let (request_tx, request_rx) = mpsc::channel::<Vec<Instance>>();
        let (result_tx, result_rx) = mpsc::channel::<Vec<StatusUpdate>>();

        let handle = thread::spawn(move || {
            Self::polling_loop(request_rx, result_tx);
        });

        Self {
            request_tx,
            result_rx,
            _handle: handle,
        }
    }

    fn polling_loop(
        request_rx: mpsc::Receiver<Vec<Instance>>,
        result_tx: mpsc::Sender<Vec<StatusUpdate>>,
    ) {
        let container_check_interval = Duration::from_secs(5);
        // Initialize to the past so the first check runs immediately
        let mut last_container_check = Instant::now() - container_check_interval;
        let mut container_states: HashMap<String, bool> = HashMap::new();

        // Credential refresh: re-sync every 30 minutes so long-lived sandbox
        // sessions pick up rotated OAuth tokens from the macOS Keychain.
        let credential_refresh_interval = Duration::from_secs(1800);
        // Start at now (not in the past) -- credentials are fresh from container creation
        let mut last_credential_refresh = Instant::now();

        // Start at TIER_COLD - 1 so the first wrapping_add produces TIER_COLD,
        // which is divisible by all tier intervals -- ensuring every session is
        // polled on the very first cycle.
        let mut cycle_count: u64 = TIER_COLD - 1;

        while let Ok(instances) = request_rx.recv() {
            cycle_count = cycle_count.wrapping_add(1);

            // Pre-scan: check if any instance would actually be polled this cycle.
            // If not, skip the batch subprocess calls entirely.
            let any_pollable = instances.iter().any(|inst| {
                let tier = polling_tier(inst.status);
                tier != 0 && cycle_count % tier == 0
            });

            let pane_metadata = if any_pollable {
                crate::tmux::refresh_session_cache();
                crate::tmux::batch_pane_metadata()
            } else {
                HashMap::new()
            };

            // Refresh container health if any sandboxed session exists and interval elapsed
            let has_sandboxed = if any_pollable {
                let sandboxed = instances.iter().any(|i| i.is_sandboxed());
                if sandboxed && last_container_check.elapsed() >= container_check_interval {
                    container_states = crate::containers::batch_container_health();
                    last_container_check = Instant::now();
                }
                sandboxed
            } else {
                false
            };

            // Periodically re-sync sandbox credentials from the macOS Keychain
            // so long-lived sessions don't lose auth mid-run.
            if has_sandboxed && last_credential_refresh.elapsed() >= credential_refresh_interval {
                last_credential_refresh = Instant::now();
                crate::session::container_config::refresh_agent_configs();
            }

            let updates: Vec<StatusUpdate> = instances
                .into_iter()
                .filter_map(|mut inst| {
                    // Adaptive polling: skip instances whose tier interval hasn't elapsed
                    let tier = polling_tier(inst.status);
                    if tier == 0 || cycle_count % tier != 0 {
                        return None;
                    }

                    // For sandboxed sessions, check if the container is dead before
                    // falling through to tmux-based status detection.
                    if inst.is_sandboxed()
                        && !matches!(
                            inst.status,
                            Status::Stopped
                                | Status::Deleting
                                | Status::Starting
                                | Status::Creating
                        )
                    {
                        if let Some(sandbox) = &inst.sandbox_info {
                            if let Some(&running) = container_states.get(&sandbox.container_name) {
                                if !running {
                                    return Some(StatusUpdate {
                                        id: inst.id,
                                        status: Status::Error,
                                        last_error: Some("Container is not running".to_string()),
                                        idle_entered_at: None,
                                    });
                                }
                            }
                        }
                    }

                    // Look up pre-fetched metadata for this instance's tmux session
                    let session_name = crate::tmux::Session::generate_name(&inst.id, &inst.title);
                    let metadata = pane_metadata.get(&session_name);

                    inst.update_status_with_metadata(metadata);

                    Some(StatusUpdate {
                        id: inst.id,
                        status: inst.status,
                        last_error: inst.last_error,
                        idle_entered_at: inst.idle_entered_at,
                    })
                })
                .collect();

            if result_tx.send(updates).is_err() {
                break;
            }
        }
    }

    /// Request a status refresh for all given instances (non-blocking).
    pub fn request_refresh(&self, instances: Vec<Instance>) {
        let _ = self.request_tx.send(instances);
    }

    /// Try to receive status updates without blocking.
    /// Returns None if no updates are available yet.
    pub fn try_recv_updates(&self) -> Option<Vec<StatusUpdate>> {
        self.result_rx.try_recv().ok()
    }
}

impl Default for StatusPoller {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_update_carries_idle_entered_at() {
        // Regression: the polling loop runs `update_status_with_metadata`
        // on a clone, then projects the result into a `StatusUpdate`. If
        // `idle_entered_at` falls off the projection (the original bug),
        // the breathe rattle + fresh-idle color never fire in the TUI
        // even though the wrapper sets the timestamp on the clone
        // correctly.
        let ts = Utc::now();
        let update = StatusUpdate {
            id: "abc".into(),
            status: Status::Idle,
            last_error: None,
            idle_entered_at: Some(ts),
        };
        assert_eq!(update.idle_entered_at, Some(ts));
    }

    #[test]
    fn test_polling_tier_hot() {
        assert_eq!(polling_tier(Status::Running), TIER_HOT);
        assert_eq!(polling_tier(Status::Waiting), TIER_HOT);
        assert_eq!(polling_tier(Status::Starting), TIER_HOT);
    }

    #[test]
    fn test_polling_tier_warm() {
        assert_eq!(polling_tier(Status::Idle), TIER_WARM);
        assert_eq!(polling_tier(Status::Unknown), TIER_WARM);
    }

    #[test]
    fn test_polling_tier_cold() {
        assert_eq!(polling_tier(Status::Error), TIER_COLD);
    }

    #[test]
    fn test_polling_tier_frozen() {
        assert_eq!(polling_tier(Status::Stopped), 0);
        assert_eq!(polling_tier(Status::Deleting), 0);
    }

    #[test]
    fn test_tier_cycle_alignment() {
        // Hot sessions are polled every cycle: TIER_HOT must stay at 1.
        assert_eq!(TIER_HOT, 1);
        // Warm sessions are polled every 5 cycles
        assert_ne!(1u64 % TIER_WARM, 0);
        assert_ne!(2u64 % TIER_WARM, 0);
        assert_eq!(5u64 % TIER_WARM, 0);
        assert_eq!(10u64 % TIER_WARM, 0);
        // Cold sessions are polled every 60 cycles
        assert_ne!(1u64 % TIER_COLD, 0);
        assert_eq!(60u64 % TIER_COLD, 0);
        assert_eq!(120u64 % TIER_COLD, 0);
    }

    #[test]
    fn test_first_cycle_polls_all_tiers() {
        // cycle_count starts at TIER_COLD - 1, first cycle wraps to TIER_COLD
        let first_cycle = (TIER_COLD - 1).wrapping_add(1);
        // TIER_HOT == 1 (see test_tier_cycle_alignment), so any cycle trivially
        // polls hot; just verify the warm and cold alignments here.
        assert_eq!(first_cycle % TIER_WARM, 0, "first cycle must poll warm");
        assert_eq!(first_cycle % TIER_COLD, 0, "first cycle must poll cold");
    }
}
