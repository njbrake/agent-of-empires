//! Background status polling for TUI performance
//!
//! This module provides non-blocking status updates for sessions by running
//! tmux subprocess calls in a background thread.

use std::collections::HashMap;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use crate::session::{Instance, Status};

/// Result of a status check for a single session
#[derive(Debug)]
pub struct StatusUpdate {
    pub id: String,
    pub status: Status,
    pub last_error: Option<String>,
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

        while let Ok(instances) = request_rx.recv() {
            crate::tmux::refresh_session_cache();

            // Refresh container health if any sandboxed session exists and interval elapsed
            let has_sandboxed = instances.iter().any(|i| i.is_sandboxed());
            if has_sandboxed && last_container_check.elapsed() >= container_check_interval {
                container_states = crate::containers::batch_container_health();
                last_container_check = Instant::now();
            }

            let updates: Vec<StatusUpdate> = instances
                .into_iter()
                .map(|mut inst| {
                    // For sandboxed sessions, check if the container is dead before
                    // falling through to tmux-based status detection.
                    if inst.is_sandboxed()
                        && !matches!(
                            inst.status,
                            Status::Stopped | Status::Deleting | Status::Starting
                        )
                    {
                        if let Some(sandbox) = &inst.sandbox_info {
                            if let Some(&running) = container_states.get(&sandbox.container_name) {
                                if !running {
                                    return StatusUpdate {
                                        id: inst.id,
                                        status: Status::Error,
                                        last_error: Some("Container is not running".to_string()),
                                    };
                                }
                            }
                        }
                    }

                    inst.update_status();

                    StatusUpdate {
                        id: inst.id,
                        status: inst.status,
                        last_error: inst.last_error,
                    }
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
