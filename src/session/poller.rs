//! Background polling for session ID changes.
//!
//! Each running instance gets a `SessionPoller` that wakes every
//! `POLL_INTERVAL` seconds, calls the agent-specific `poll_fn` to discover
//! the current session ID on disk, and reports changes back to the TUI via
//! a channel.
//!
//! Polling exists because Claude can rotate its session ID at runtime
//! (`/clear`, `/resume`, `--fork-session`, or a fresh `claude` invocation in
//! the same tmux pane). Without polling, the launch-time UUID stored in
//! `sessions.json` would diverge from the agent's actual current session.

use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::Duration;

const POLL_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Debug)]
enum PollCommand {
    Stop,
}

/// Runs an agent-specific session-id discovery function on a background
/// thread and reports changes through a channel.
pub struct SessionPoller {
    session_name: String,
    cmd_tx: mpsc::Sender<PollCommand>,
    cmd_rx: Option<mpsc::Receiver<PollCommand>>,
    result_tx: mpsc::Sender<(String, String)>,
    result_rx: Option<mpsc::Receiver<(String, String)>>,
    handle: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for SessionPoller {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SessionPoller")
            .field("session_name", &self.session_name)
            .field("running", &self.handle.is_some())
            .finish()
    }
}

impl SessionPoller {
    pub fn new(session_name: String) -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        Self {
            session_name,
            cmd_tx,
            cmd_rx: Some(cmd_rx),
            result_tx,
            result_rx: Some(result_rx),
            handle: None,
        }
    }

    /// Start the polling thread. Returns `false` if already started or if
    /// thread spawn failed.
    pub fn start(
        &mut self,
        instance_id: String,
        poll_fn: Box<dyn Fn() -> Option<String> + Send + 'static>,
        on_change: Box<dyn Fn(&str) + Send + 'static>,
        initial_known: Option<String>,
    ) -> bool {
        let cmd_rx = match self.cmd_rx.take() {
            Some(rx) => rx,
            None => {
                tracing::warn!(
                    "Poller for {} already started, ignoring duplicate start",
                    instance_id
                );
                return false;
            }
        };

        let session_name = self.session_name.clone();
        let thread_label = format!("aoe-poller/{}", instance_id);
        let result_tx = self.result_tx.clone();

        let handle = std::thread::Builder::new()
            .name(thread_label.clone())
            .stack_size(128 * 1024)
            .spawn(move || {
                let mut last_known = initial_known;

                let mut report = |new_id: String| {
                    if last_known.as_deref() != Some(&new_id) {
                        on_change(&new_id);
                        let _ = result_tx.send((instance_id.clone(), new_id.clone()));
                        last_known = Some(new_id);
                    }
                };

                if let Some(new_id) = poll_fn() {
                    report(new_id);
                }

                loop {
                    match cmd_rx.recv_timeout(POLL_INTERVAL) {
                        Ok(PollCommand::Stop) => break,
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => break,
                    }

                    if crate::tmux::utils::is_pane_dead(&session_name) {
                        tracing::info!("Pane dead for {}, stopping poller", session_name);
                        break;
                    }

                    if let Some(new_id) = poll_fn() {
                        report(new_id);
                    }
                }
            });

        match handle {
            Ok(h) => {
                self.handle = Some(h);
                true
            }
            Err(e) => {
                tracing::warn!("Failed to spawn poller thread {}: {}", thread_label, e);
                false
            }
        }
    }

    /// Drain a pending session ID update, if any. Returns `(instance_id, session_id)`.
    pub fn try_recv_session_update(&self) -> Option<(String, String)> {
        self.result_rx.as_ref()?.try_recv().ok()
    }

    /// Stop the poller thread and wait for it to finish.
    pub fn stop(&mut self) {
        let _ = self.cmd_tx.send(PollCommand::Stop);
        if let Some(handle) = self.handle.take() {
            if let Err(e) = handle.join() {
                tracing::warn!("Poller thread panicked: {:?}", e);
            }
        }
    }

    pub fn is_running(&self) -> bool {
        match &self.handle {
            Some(handle) => !handle.is_finished(),
            None => false,
        }
    }
}

impl Default for SessionPoller {
    fn default() -> Self {
        Self::new("default".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[test]
    fn test_poller_reports_initial_value() {
        let mut poller = SessionPoller::new("test-session".to_string());
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_c = calls.clone();

        let started = poller.start(
            "inst-1".to_string(),
            Box::new(move || {
                calls_c.fetch_add(1, Ordering::SeqCst);
                Some("session-abc".to_string())
            }),
            Box::new(|_| {}),
            None,
        );
        assert!(started);

        // Give the immediate first poll time to run and send.
        std::thread::sleep(Duration::from_millis(100));
        let update = poller.try_recv_session_update();
        poller.stop();

        assert_eq!(
            update,
            Some(("inst-1".to_string(), "session-abc".to_string()))
        );
        assert!(calls.load(Ordering::SeqCst) >= 1);
    }

    #[test]
    fn test_poller_skips_unchanged_value() {
        let mut poller = SessionPoller::new("test-session".to_string());

        poller.start(
            "inst-2".to_string(),
            Box::new(|| Some("same-id".to_string())),
            Box::new(|_| {}),
            Some("same-id".to_string()),
        );

        std::thread::sleep(Duration::from_millis(100));
        let update = poller.try_recv_session_update();
        poller.stop();

        // initial_known matches poll result, so no update should fire.
        assert!(update.is_none());
    }

    #[test]
    fn test_poller_stop_is_idempotent() {
        let mut poller = SessionPoller::new("test-session".to_string());
        poller.start(
            "inst-3".to_string(),
            Box::new(|| None),
            Box::new(|_| {}),
            None,
        );
        poller.stop();
        poller.stop();
        assert!(!poller.is_running());
    }

    #[test]
    fn test_poller_double_start_returns_false() {
        let mut poller = SessionPoller::new("test-session".to_string());
        assert!(poller.start(
            "inst-4".to_string(),
            Box::new(|| None),
            Box::new(|_| {}),
            None,
        ));
        assert!(!poller.start(
            "inst-4".to_string(),
            Box::new(|| None),
            Box::new(|_| {}),
            None,
        ));
        poller.stop();
    }
}
