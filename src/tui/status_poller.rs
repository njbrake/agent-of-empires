//! Background status polling for TUI performance
//!
//! This module provides non-blocking status updates for sessions by running
//! tmux subprocess calls in a background thread.

use std::sync::mpsc;
use std::thread;

use crate::session::{Instance, Status};

/// Result of a status check for a single session
#[derive(Debug)]
pub struct StatusUpdate {
    pub id: String,
    pub status: Status,
    pub last_error: Option<String>,
    pub claude_session_id: Option<String>,
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
        while let Ok(instances) = request_rx.recv() {
            crate::tmux::refresh_session_cache();

            let updates: Vec<StatusUpdate> = instances
                .into_iter()
                .map(|mut inst| {
                    inst.update_status();

                    StatusUpdate {
                        id: inst.id,
                        status: inst.status,
                        last_error: inst.last_error,
                        claude_session_id: inst.claude_session_id,
                    }
                })
                .collect();

            if result_tx.send(updates).is_err() {
                // Receiver dropped, exit the loop
                break;
            }
        }
    }

    /// Request a status refresh for all given instances.
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
