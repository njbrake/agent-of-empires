//! Adaptive polling interval and command channel for session monitoring

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc;
use std::sync::mpsc::RecvTimeoutError;
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

/// Global count of active poller threads for budget enforcement
static ACTIVE_POLLER_COUNT: AtomicU32 = AtomicU32::new(0);

/// Maximum number of concurrent poller threads allowed
const MAX_POLLER_THREADS: u32 = 20;

/// RAII guard that decrements `ACTIVE_POLLER_COUNT` on drop.
///
/// Ensures the counter is always decremented even if the poller thread panics,
/// preventing permanent budget exhaustion.
struct PollerCountGuard;

impl PollerCountGuard {
    /// Atomically check the budget and increment. Returns `None` if at capacity.
    fn try_acquire() -> Option<Self> {
        let mut current = ACTIVE_POLLER_COUNT.load(Ordering::SeqCst);
        loop {
            if current >= MAX_POLLER_THREADS {
                return None;
            }
            match ACTIVE_POLLER_COUNT.compare_exchange_weak(
                current,
                current + 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return Some(Self),
                Err(actual) => current = actual,
            }
        }
    }
}

impl Drop for PollerCountGuard {
    fn drop(&mut self) {
        ACTIVE_POLLER_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
}

const POLL_INITIAL_INTERVAL: Duration = Duration::from_secs(2);
const POLL_MAX_INTERVAL: Duration = Duration::from_secs(60);
const POLL_BACKOFF_FACTOR: f64 = 1.5;
const POLL_STABLE_THRESHOLD: u32 = 3;

/// Timeout for deferred capture completion, with margin for retry delays.
const CAPTURE_GATE_TIMEOUT: Duration = Duration::from_secs(120);

/// Synchronization gate between deferred capture and polling threads.
///
/// Ensures capture completes before polling starts, avoiding concurrent storage writes.
/// The capture thread calls [`CaptureGate::complete`]; the poller calls [`CaptureGate::wait`].
pub struct CaptureGate {
    inner: Mutex<CaptureGateState>,
    cond: Condvar,
}

struct CaptureGateState {
    done: bool,
    captured_id: Option<String>,
}

impl CaptureGate {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(CaptureGateState {
                done: false,
                captured_id: None,
            }),
            cond: Condvar::new(),
        }
    }

    /// Signal that the deferred capture is complete.
    ///
    /// `captured_id` is `Some(id)` on success, `None` if all attempts were exhausted.
    pub fn complete(&self, captured_id: Option<String>) {
        let Ok(mut state) = self.inner.lock() else {
            tracing::warn!("CaptureGate lock poisoned in complete(), captured ID lost");
            return;
        };
        state.done = true;
        state.captured_id = captured_id;
        self.cond.notify_all();
    }

    /// Block until the deferred capture completes, then return the captured ID (if any).
    ///
    /// Uses a timeout so the poller thread is never stuck indefinitely if the capture
    /// thread panics or is cancelled.
    pub fn wait(&self, timeout: Duration) -> Option<String> {
        let Ok(state) = self.inner.lock() else {
            tracing::warn!("CaptureGate lock poisoned in wait(), returning None");
            return None;
        };
        let Ok(result) = self.cond.wait_timeout_while(state, timeout, |s| !s.done) else {
            tracing::warn!("CaptureGate lock poisoned during wait(), returning None");
            return None;
        };
        result.0.captured_id.clone()
    }

    /// Non-blocking check: if complete, take and return the captured ID (once only).
    /// Subsequent calls return `None`.
    pub fn try_take(&self) -> Option<String> {
        let Ok(mut state) = self.inner.lock() else {
            return None;
        };
        if state.done {
            state.captured_id.take()
        } else {
            None
        }
    }
}

impl Default for CaptureGate {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for CaptureGate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let state = self.inner.lock().ok();
        f.debug_struct("CaptureGate")
            .field("done", &state.as_ref().map(|s| s.done))
            .finish()
    }
}

/// Manages adaptive polling intervals that back off when no changes are detected
#[derive(Debug)]
pub struct AdaptiveInterval {
    initial: Duration,
    current: Duration,
    max: Duration,
    backoff_factor: f64,
    stable_threshold: u32,
    stable_count: u32,
}

impl AdaptiveInterval {
    /// Create a new adaptive interval with custom parameters
    pub fn new(
        initial: Duration,
        max: Duration,
        backoff_factor: f64,
        stable_threshold: u32,
    ) -> Self {
        Self {
            initial,
            current: initial,
            max,
            backoff_factor,
            stable_threshold,
            stable_count: 0,
        }
    }

    pub fn current(&self) -> Duration {
        self.current
    }

    /// Record that no changes were detected; increases backoff if threshold is reached.
    ///
    /// Uses `Duration::from_secs_f64` for sub-second precision in the backoff calculation
    /// (e.g., 2.0s * 1.5 = 3.0s, 3.0s * 1.5 = 4.5s).
    pub fn record_no_change(&mut self) {
        self.stable_count += 1;
        if self.stable_count >= self.stable_threshold {
            let next_secs = self.current.as_secs_f64() * self.backoff_factor;
            let next_duration = Duration::from_secs_f64(next_secs);
            self.current = next_duration.min(self.max);
            self.stable_count = 0;
        }
    }

    /// Record that a change was detected; reset to initial interval
    pub fn record_change(&mut self) {
        self.current = self.initial;
        self.stable_count = 0;
    }

    pub fn reset(&mut self) {
        self.record_change();
    }
}

/// Command sent to the session poller thread
#[derive(Debug, Clone, Copy)]
pub enum PollCommand {
    /// Request an immediate poll
    PollNow,
    /// Stop the poller thread
    Stop,
}

/// Manages polling thread lifecycle and inter-thread communication via mpsc channels.
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
    /// Create a new poller (does not start the thread)
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

    /// Start the polling thread with the given callbacks.
    ///
    /// When `capture_gate` is `Some`, the thread blocks until the deferred capture
    /// completes, then uses the captured ID as its initial known value. This
    /// prevents concurrent storage writes between the capture and poller threads.
    ///
    /// Returns `true` if the thread was successfully spawned, `false` if the
    /// poller was already started, the thread budget was exhausted, or spawning failed.
    pub fn start(
        &mut self,
        instance_id: String,
        poll_fn: Box<dyn Fn() -> Option<String> + Send + 'static>,
        on_change: Box<dyn Fn(&str) + Send + 'static>,
        initial_known: Option<String>,
        capture_gate: Option<Arc<CaptureGate>>,
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

        let _guard = match PollerCountGuard::try_acquire() {
            Some(g) => g,
            None => {
                tracing::warn!(
                    "Poller thread budget exhausted ({}/{}), skipping poller for {}",
                    ACTIVE_POLLER_COUNT.load(Ordering::Relaxed),
                    MAX_POLLER_THREADS,
                    instance_id
                );
                self.cmd_rx = Some(cmd_rx);
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
                // Move the guard into the thread so it decrements on exit (including panic)
                let _guard = _guard;

                let mut last_known = initial_known;
                let had_gate = capture_gate.is_some();

                if let Some(gate) = capture_gate {
                    tracing::debug!("Poller for {} waiting on capture gate", instance_id);
                    let captured = gate.wait(CAPTURE_GATE_TIMEOUT);
                    if let Some(ref id) = captured {
                        tracing::debug!("Poller for {} received captured ID: {}", instance_id, id);
                    }
                    last_known = last_known.or(captured);
                }

                let mut interval = AdaptiveInterval::new(
                    POLL_INITIAL_INTERVAL,
                    POLL_MAX_INTERVAL,
                    POLL_BACKOFF_FACTOR,
                    POLL_STABLE_THRESHOLD,
                );

                // Immediate first poll for sessions without a capture gate
                // (e.g. pre-existing sessions loaded from disk)
                if !had_gate {
                    if let Some(new_id) = poll_fn() {
                        if last_known.as_deref() != Some(&new_id) {
                            on_change(&new_id);
                            let _ = result_tx.send((instance_id.clone(), new_id.clone()));
                            last_known = Some(new_id);
                            interval.record_change();
                        }
                    }
                }

                loop {
                    match cmd_rx.recv_timeout(interval.current()) {
                        Ok(PollCommand::Stop) => break,
                        Ok(PollCommand::PollNow) => {}
                        Err(RecvTimeoutError::Timeout) => {}
                        Err(RecvTimeoutError::Disconnected) => break,
                    }

                    if crate::tmux::utils::is_pane_dead(&session_name) {
                        tracing::info!("Pane dead for {}, stopping poller", session_name);
                        break;
                    }

                    if let Some(new_id) = poll_fn() {
                        let changed = last_known.as_deref() != Some(&new_id);
                        if changed {
                            on_change(&new_id);
                            let _ = result_tx.send((instance_id.clone(), new_id.clone()));
                            last_known = Some(new_id);
                            interval.record_change();
                        } else {
                            interval.record_no_change();
                        }
                    } else {
                        interval.record_no_change();
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
                // Restore channels to allow retrying spawn
                let (cmd_tx, cmd_rx) = mpsc::channel();
                self.cmd_tx = cmd_tx;
                self.cmd_rx = Some(cmd_rx);
                let (result_tx, result_rx) = mpsc::channel();
                self.result_tx = result_tx;
                self.result_rx = Some(result_rx);
                false
            }
        }
    }

    /// Drain a pending session ID update, if any. Returns `(instance_id, session_id)`.
    pub fn try_recv_session_update(&self) -> Option<(String, String)> {
        self.result_rx.as_ref()?.try_recv().ok()
    }

    /// Send a poll command
    pub fn poll_now(&self) {
        let _ = self.cmd_tx.send(PollCommand::PollNow);
    }

    /// Stop the poller thread and wait for it to finish
    pub fn stop(&mut self) {
        let _ = self.cmd_tx.send(PollCommand::Stop);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }

    /// Check if the poller thread is running
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
    use serial_test::serial;

    #[test]
    fn test_adaptive_interval_initial() {
        let interval =
            AdaptiveInterval::new(Duration::from_secs(2), Duration::from_secs(60), 1.5, 3);
        assert_eq!(interval.current(), Duration::from_secs(2));
    }

    #[test]
    fn test_adaptive_interval_record_no_change_increments_count() {
        let mut interval =
            AdaptiveInterval::new(Duration::from_secs(2), Duration::from_secs(60), 1.5, 3);
        assert_eq!(interval.stable_count, 0);
        interval.record_no_change();
        assert_eq!(interval.stable_count, 1);
        interval.record_no_change();
        assert_eq!(interval.stable_count, 2);
    }

    #[test]
    fn test_adaptive_interval_backoff_at_threshold() {
        let mut interval =
            AdaptiveInterval::new(Duration::from_secs(2), Duration::from_secs(60), 1.5, 3);
        interval.record_no_change();
        interval.record_no_change();
        interval.record_no_change();
        // After 3 calls: 2 * 1.5 = 3 seconds
        assert_eq!(interval.current(), Duration::from_secs(3));
        assert_eq!(interval.stable_count, 0);
    }

    #[test]
    fn test_adaptive_interval_multiple_backoffs() {
        let mut interval =
            AdaptiveInterval::new(Duration::from_secs(2), Duration::from_secs(60), 1.5, 3);
        // First backoff: 2 -> 3
        for _ in 0..3 {
            interval.record_no_change();
        }
        assert_eq!(interval.current(), Duration::from_secs(3));

        // Second backoff: 3 -> 4.5 (with sub-second precision)
        for _ in 0..3 {
            interval.record_no_change();
        }
        let expected_secs = 3.0 * 1.5;
        assert_eq!(interval.current(), Duration::from_secs_f64(expected_secs));
    }

    #[test]
    fn test_adaptive_interval_respects_max() {
        let mut interval = AdaptiveInterval::new(
            Duration::from_secs(2),
            Duration::from_secs(60),
            1.5,
            1, // threshold of 1 for faster test
        );
        interval.record_no_change(); // 2 * 1.5 = 3.0
        interval.record_no_change(); // 3.0 * 1.5 = 4.5
        interval.record_no_change(); // 4.5 * 1.5 = 6.75
        interval.record_no_change(); // 6.75 * 1.5 = 10.125
        interval.record_no_change(); // 10.125 * 1.5 = 15.1875
        interval.record_no_change(); // 15.1875 * 1.5 = 22.78125
        interval.record_no_change(); // 22.78125 * 1.5 = 34.171875
        interval.record_no_change(); // 34.171875 * 1.5 = 51.2578125
        interval.record_no_change(); // 51.2578125 * 1.5 = 76.88671875 > 60, capped at 60
        assert!(interval.current() <= Duration::from_secs(60));
    }

    #[test]
    fn test_adaptive_interval_record_change_resets() {
        let mut interval =
            AdaptiveInterval::new(Duration::from_secs(2), Duration::from_secs(60), 1.5, 3);
        for _ in 0..3 {
            interval.record_no_change();
        }
        assert_eq!(interval.current(), Duration::from_secs(3));

        interval.record_change();
        assert_eq!(interval.current(), Duration::from_secs(2));
        assert_eq!(interval.stable_count, 0);
    }

    #[test]
    fn test_adaptive_interval_reset_is_alias() {
        let mut interval =
            AdaptiveInterval::new(Duration::from_secs(2), Duration::from_secs(60), 1.5, 3);
        for _ in 0..3 {
            interval.record_no_change();
        }
        assert_eq!(interval.current(), Duration::from_secs(3));

        interval.reset();
        assert_eq!(interval.current(), Duration::from_secs(2));
        assert_eq!(interval.stable_count, 0);
    }

    #[test]
    fn test_session_poller_new() {
        let poller = SessionPoller::new("test-session".to_string());
        assert!(!poller.is_running());
    }

    #[test]
    fn test_session_poller_stop_when_no_thread() {
        let mut poller = SessionPoller::new("test-session".to_string());
        poller.stop(); // Should not panic
        assert!(!poller.is_running());
    }

    #[test]
    fn test_session_poller_double_stop_safe() {
        let mut poller = SessionPoller::new("test-session".to_string());
        poller.stop();
        poller.stop(); // Should not panic
        assert!(!poller.is_running());
    }

    #[test]
    fn test_session_poller_drop_is_clean() {
        let poller = SessionPoller::new("test-session".to_string());
        drop(poller); // Should not panic
    }

    #[test]
    fn test_adaptive_interval_with_constants() {
        let mut interval = AdaptiveInterval::new(
            POLL_INITIAL_INTERVAL,
            POLL_MAX_INTERVAL,
            POLL_BACKOFF_FACTOR,
            POLL_STABLE_THRESHOLD,
        );
        assert_eq!(interval.current(), Duration::from_secs(2));
        for _ in 0..POLL_STABLE_THRESHOLD {
            interval.record_no_change();
        }
        assert_eq!(interval.current(), Duration::from_secs(3));
    }

    #[test]
    fn test_poller_detects_change() {
        use std::sync::{Arc, Mutex};

        let call_count = Arc::new(Mutex::new(0u32));
        let call_count_clone = call_count.clone();

        let poll_fn: Box<dyn Fn() -> Option<String> + Send + 'static> = Box::new(move || {
            let mut count = call_count_clone.lock().unwrap();
            *count += 1;
            if *count <= 3 {
                Some("id-1".to_string())
            } else {
                Some("id-2".to_string())
            }
        });

        let changed_ids: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let changed_ids_clone = changed_ids.clone();

        let on_change: Box<dyn Fn(&str) + Send + 'static> = Box::new(move |id: &str| {
            changed_ids_clone.lock().unwrap().push(id.to_string());
        });

        let mut poller = SessionPoller::new("test-session".to_string());
        poller.start(
            "test-change".to_string(),
            poll_fn,
            on_change,
            Some("id-1".to_string()),
            None,
        );

        // Force polls via PollNow (the default interval is 2s, too slow for tests)
        for _ in 0..5 {
            poller.poll_now();
            std::thread::sleep(Duration::from_millis(50));
        }
        poller.stop();

        let ids = changed_ids.lock().unwrap();
        assert!(
            ids.contains(&"id-2".to_string()),
            "on_change should have been called with id-2, got: {:?}",
            *ids
        );
        assert!(
            !ids.contains(&"id-1".to_string()),
            "on_change should NOT have been called with id-1 (initial known)"
        );
    }

    #[test]
    fn test_poll_now_triggers_poll() {
        use std::sync::{Arc, Mutex};

        let poll_count = Arc::new(Mutex::new(0u32));
        let poll_count_clone = poll_count.clone();

        let poll_fn: Box<dyn Fn() -> Option<String> + Send + 'static> = Box::new(move || {
            let mut count = poll_count_clone.lock().unwrap();
            *count += 1;
            Some("stable-id".to_string())
        });

        let on_change: Box<dyn Fn(&str) + Send + 'static> = Box::new(|_| {});

        let mut poller = SessionPoller::new("test-session".to_string());
        poller.start("test-pollnow".to_string(), poll_fn, on_change, None, None);

        std::thread::sleep(Duration::from_millis(50));
        poller.poll_now();
        poller.poll_now();
        std::thread::sleep(Duration::from_millis(200));

        let count = *poll_count.lock().unwrap();
        assert!(
            count >= 2,
            "poll_fn should have been called at least twice via PollNow, got: {}",
            count
        );

        poller.stop();
    }

    #[test]
    #[serial]
    fn test_thread_budget_cap() {
        let original = ACTIVE_POLLER_COUNT.load(Ordering::SeqCst);
        ACTIVE_POLLER_COUNT.store(MAX_POLLER_THREADS, Ordering::SeqCst);

        let mut poller = SessionPoller::new("test-session".to_string());
        poller.start(
            "test-budget".to_string(),
            Box::new(|| Some("id".to_string())),
            Box::new(|_| {}),
            None,
            None,
        );

        assert!(
            !poller.is_running(),
            "poller should not have spawned when budget exhausted"
        );
        assert!(
            poller.cmd_rx.is_some(),
            "cmd_rx should be returned when budget exhausted"
        );

        ACTIVE_POLLER_COUNT.store(original, Ordering::SeqCst);
    }

    #[test]
    #[serial]
    fn test_poller_is_running_after_start() {
        let mut poller = SessionPoller::new("test-session".to_string());
        poller.start(
            "test-running".to_string(),
            Box::new(|| {
                std::thread::sleep(Duration::from_millis(10));
                Some("id".to_string())
            }),
            Box::new(|_| {}),
            None,
            None,
        );

        assert!(poller.is_running(), "poller should be running after start");
        poller.stop();
    }

    #[test]
    #[serial]
    fn test_poller_cleanup_decrements_counter() {
        use std::sync::{Arc, Mutex};

        let entered = Arc::new(Mutex::new(false));
        let entered_clone = entered.clone();

        let mut poller = SessionPoller::new("test-session".to_string());
        poller.start(
            "test-cleanup".to_string(),
            Box::new(move || {
                *entered_clone.lock().unwrap() = true;
                Some("id".to_string())
            }),
            Box::new(|_| {}),
            None,
            None,
        );

        poller.poll_now();
        std::thread::sleep(Duration::from_millis(50));

        let count_before_stop = ACTIVE_POLLER_COUNT.load(Ordering::SeqCst);
        poller.stop();
        let count_after_stop = ACTIVE_POLLER_COUNT.load(Ordering::SeqCst);

        assert!(
            count_after_stop < count_before_stop,
            "counter should decrement after stop (before_stop={}, after_stop={})",
            count_before_stop,
            count_after_stop
        );
        assert!(*entered.lock().unwrap(), "poll_fn should have been called");
    }

    #[test]
    fn test_interval_exact_at_threshold() {
        let mut interval = AdaptiveInterval::new(
            Duration::from_secs(2),
            Duration::from_secs(60),
            1.5,
            POLL_STABLE_THRESHOLD,
        );

        for _ in 0..POLL_STABLE_THRESHOLD {
            interval.record_no_change();
        }
        // 2 * 1.5 = 3
        assert_eq!(interval.current(), Duration::from_secs(3));
        assert_eq!(interval.stable_count, 0);

        interval.record_no_change();
        assert_eq!(interval.current(), Duration::from_secs(3));
        assert_eq!(interval.stable_count, 1);
    }

    #[test]
    fn test_interval_max_clamping_precision() {
        let mut interval = AdaptiveInterval::new(
            Duration::from_secs(2),
            POLL_MAX_INTERVAL,
            POLL_BACKOFF_FACTOR,
            POLL_STABLE_THRESHOLD,
        );

        for _ in 0..1000 {
            interval.record_no_change();
            assert!(
                interval.current() <= POLL_MAX_INTERVAL,
                "interval {} exceeded max {}",
                interval.current().as_secs(),
                POLL_MAX_INTERVAL.as_secs()
            );
        }
        assert_eq!(interval.current(), POLL_MAX_INTERVAL);
    }

    #[test]
    fn test_interval_change_mid_backoff() {
        let mut interval = AdaptiveInterval::new(
            Duration::from_secs(2),
            Duration::from_secs(60),
            1.5,
            POLL_STABLE_THRESHOLD,
        );

        interval.record_no_change();
        interval.record_no_change();
        assert_eq!(interval.stable_count, 2);
        assert_eq!(interval.current(), Duration::from_secs(2));

        interval.record_change();
        assert_eq!(interval.current(), Duration::from_secs(2));
        assert_eq!(interval.stable_count, 0);
    }

    #[test]
    fn test_capture_gate_blocks_until_complete() {
        let gate = Arc::new(CaptureGate::new());
        let gate_clone = Arc::clone(&gate);

        let waiter = std::thread::spawn(move || gate_clone.wait(Duration::from_secs(5)));

        std::thread::sleep(Duration::from_millis(50));
        gate.complete(Some("ses_123".to_string()));

        let result = waiter.join().unwrap();
        assert_eq!(result, Some("ses_123".to_string()));
    }

    #[test]
    fn test_capture_gate_returns_none_on_exhausted() {
        let gate = Arc::new(CaptureGate::new());
        let gate_clone = Arc::clone(&gate);

        let waiter = std::thread::spawn(move || gate_clone.wait(Duration::from_secs(5)));

        std::thread::sleep(Duration::from_millis(50));
        gate.complete(None);

        let result = waiter.join().unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_capture_gate_timeout() {
        let gate = Arc::new(CaptureGate::new());
        let result = gate.wait(Duration::from_millis(50));
        assert_eq!(result, None);
    }

    #[test]
    fn test_capture_gate_already_complete_returns_immediately() {
        let gate = CaptureGate::new();
        gate.complete(Some("ses_fast".to_string()));
        let result = gate.wait(Duration::from_millis(10));
        assert_eq!(result, Some("ses_fast".to_string()));
    }

    #[test]
    fn test_poller_with_capture_gate() {
        let gate = Arc::new(CaptureGate::new());
        let gate_clone = Arc::clone(&gate);

        let poll_count = Arc::new(Mutex::new(0u32));
        let poll_count_clone = poll_count.clone();

        let poll_fn: Box<dyn Fn() -> Option<String> + Send + 'static> = Box::new(move || {
            let mut count = poll_count_clone.lock().unwrap();
            *count += 1;
            Some("ses_polled".to_string())
        });

        let on_change: Box<dyn Fn(&str) + Send + 'static> = Box::new(|_| {});

        let mut poller = SessionPoller::new("test-session".to_string());
        poller.start(
            "test-gate".to_string(),
            poll_fn,
            on_change,
            None,
            Some(gate_clone),
        );

        std::thread::sleep(Duration::from_millis(100));
        let count_before = *poll_count.lock().unwrap();

        gate.complete(Some("ses_captured".to_string()));

        std::thread::sleep(Duration::from_millis(300));
        poller.poll_now();
        std::thread::sleep(Duration::from_millis(100));

        let count_after = *poll_count.lock().unwrap();
        assert!(
            count_after > count_before,
            "poller should have started polling after gate opened (before={}, after={})",
            count_before,
            count_after
        );

        poller.stop();
    }
}
