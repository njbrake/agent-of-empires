//! Live-fire test for the tmux `-C` control-mode client.
//!
//! Verifies the wire format end-to-end: spawn a real tmux session, attach
//! a `ControlModeClient`, run `capture-pane` and `send-keys` over the
//! long-lived socket, and confirm the bytes round-trip.
//!
//! Skipped automatically when tmux is missing (CI runners and dev boxes
//! that don't ship tmux). Uses unique `aoe_test_cm_*` session names so
//! it doesn't collide with the test_helpers serial-test conventions.

use agent_of_empires::tmux::ControlModeClient;
use serial_test::serial;
use std::process::Command;
use std::time::Duration;

fn tmux_available() -> bool {
    Command::new("tmux").arg("-V").output().is_ok()
}

struct TmuxCleanup {
    name: String,
}

impl Drop for TmuxCleanup {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .args(["kill-session", "-t", &self.name])
            .output();
    }
}

fn unique_session_name(tag: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("aoe_test_cm_{}_{}", tag, nanos)
}

fn create_session_with_content(name: &str, content: &str) {
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", name])
        .output();
    let status = Command::new("tmux")
        .args(["new-session", "-d", "-s", name, "-x", "80", "-y", "24"])
        .status()
        .expect("spawn tmux new-session");
    assert!(status.success(), "tmux new-session failed for {name}");
    let cmd = format!("printf '{}'", content);
    let status = Command::new("tmux")
        .args(["send-keys", "-t", name, &cmd, "Enter"])
        .status()
        .expect("tmux send-keys");
    assert!(status.success(), "tmux send-keys failed");
    // Give the shell a tick to flush the printf into the pane buffer
    // before we read it back. 250ms is plenty in practice and far
    // shorter than the test's own startup overhead.
    std::thread::sleep(Duration::from_millis(250));
}

fn create_empty_session(name: &str) {
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", name])
        .output();
    let status = Command::new("tmux")
        .args(["new-session", "-d", "-s", name, "-x", "80", "-y", "24"])
        .status()
        .expect("spawn tmux new-session");
    assert!(status.success(), "tmux new-session failed for {name}");
    std::thread::sleep(Duration::from_millis(100));
}

#[test]
#[serial]
fn control_mode_capture_returns_pane_content() {
    if !tmux_available() {
        eprintln!("Skipping: tmux not available");
        return;
    }

    let name = unique_session_name("capture");
    let _cleanup = TmuxCleanup { name: name.clone() };
    create_session_with_content(&name, "hello-from-control-mode");

    let client = ControlModeClient::spawn(&name, None).expect("ControlModeClient::spawn");

    let output = client
        .capture_pane(50, 80, 24)
        .expect("capture_pane should succeed against a live session");
    assert!(
        output.contains("hello-from-control-mode"),
        "expected pane content in capture, got: {output:?}"
    );
}

/// End-to-end proof that the new send-keys path delivers bytes to the
/// pane: spawn a session, send a literal via the control-mode socket
/// (no fork), then capture-pane and assert the literal landed in the
/// pane. Regression guard for typing-latency improvements.
#[test]
#[serial]
fn control_mode_send_literal_round_trips_through_pane() {
    if !tmux_available() {
        eprintln!("Skipping: tmux not available");
        return;
    }

    let name = unique_session_name("send_literal");
    let _cleanup = TmuxCleanup { name: name.clone() };
    create_empty_session(&name);

    let client = ControlModeClient::spawn(&name, None).expect("ControlModeClient::spawn");
    client
        .send_literal_no_enter("echo control-mode-typed-this")
        .expect("send_literal_no_enter");
    client
        .send_named_key("Enter")
        .expect("send_named_key Enter");
    // Let the shell process the command line.
    std::thread::sleep(Duration::from_millis(250));

    let output = client.capture_pane(50, 80, 24).expect("capture_pane");
    assert!(
        output.contains("control-mode-typed-this"),
        "expected typed literal in pane capture, got: {output:?}"
    );
}

#[test]
#[serial]
fn control_mode_send_literal_rejects_control_bytes() {
    // Pure unit-style assertion against the new method's gating, even
    // though the client lifecycle uses tmux. Spawning is cheap here.
    if !tmux_available() {
        eprintln!("Skipping: tmux not available");
        return;
    }

    let name = unique_session_name("ctrl_bytes");
    let _cleanup = TmuxCleanup { name: name.clone() };
    create_empty_session(&name);

    let client = ControlModeClient::spawn(&name, None).expect("ControlModeClient::spawn");
    // Newline (0x0A) is a control byte. The fork path handled these
    // via paste-buffer; control mode rejects so the caller can decide.
    let err = client.send_literal_no_enter("line1\nline2").unwrap_err();
    assert!(
        format!("{err:#}").contains("control bytes"),
        "expected control-bytes rejection, got: {err}"
    );
}

#[test]
#[serial]
fn control_mode_resize_round_trips() {
    if !tmux_available() {
        eprintln!("Skipping: tmux not available");
        return;
    }

    let name = unique_session_name("resize");
    let _cleanup = TmuxCleanup { name: name.clone() };
    create_empty_session(&name);

    let client = ControlModeClient::spawn(&name, None).expect("ControlModeClient::spawn");
    client.resize(120, 30).expect("resize 120x30");

    // Confirm tmux applied the resize via a fresh subprocess so the
    // assertion doesn't depend on the same connection that issued it.
    let output = Command::new("tmux")
        .args([
            "display-message",
            "-t",
            &name,
            "-p",
            "#{window_width}x#{window_height}",
        ])
        .output()
        .expect("tmux display-message");
    let dims = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert_eq!(dims, "120x30", "expected 120x30 after resize, got {dims:?}");
}

/// Regression guard for the `%output` wake callback: send a literal,
/// wait for tmux to emit `%output` (the pane echoed), and assert that
/// the callback ran at least once. The callback is what wakes the
/// main TUI loop on agent output, so if this stops firing, typing
/// latency reverts to whatever the next tokio ticker gives us.
#[test]
#[serial]
fn control_mode_output_wake_fires_on_pane_output() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    if !tmux_available() {
        eprintln!("Skipping: tmux not available");
        return;
    }

    let name = unique_session_name("output_wake");
    let _cleanup = TmuxCleanup { name: name.clone() };
    create_empty_session(&name);

    let counter = Arc::new(AtomicUsize::new(0));
    let counter_for_cb = counter.clone();
    let on_output: Box<dyn Fn() + Send + 'static> = Box::new(move || {
        counter_for_cb.fetch_add(1, Ordering::SeqCst);
    });

    let client =
        ControlModeClient::spawn(&name, Some(on_output)).expect("ControlModeClient::spawn");
    // The session was empty when we attached; the shell prompt
    // counts as %output too. Snapshot the baseline after a short
    // settle so we count only the bytes we induce.
    std::thread::sleep(Duration::from_millis(200));
    let baseline = counter.load(Ordering::SeqCst);

    client
        .send_literal_no_enter("echo wake-up")
        .expect("send_literal_no_enter");
    client
        .send_named_key("Enter")
        .expect("send_named_key Enter");
    // The shell emits its prompt + echo + result; tmux fires
    // `%output` for each. 250ms is plenty for at least one wake.
    std::thread::sleep(Duration::from_millis(250));

    let observed = counter.load(Ordering::SeqCst);
    assert!(
        observed > baseline,
        "expected %output wake callback to fire (baseline={baseline}, observed={observed})"
    );
}

/// Snapshot/restore helper so vt100 tests don't pollute the global
/// env for sibling tests in the same binary.
struct VtEnvGuard {
    prior: Option<String>,
}

impl VtEnvGuard {
    fn set() -> Self {
        let prior = std::env::var("AOE_LIVE_VT100").ok();
        std::env::set_var("AOE_LIVE_VT100", "1");
        Self { prior }
    }
}

impl Drop for VtEnvGuard {
    fn drop(&mut self) {
        match self.prior.take() {
            Some(v) => std::env::set_var("AOE_LIVE_VT100", v),
            None => std::env::remove_var("AOE_LIVE_VT100"),
        }
    }
}

/// With `AOE_LIVE_VT100` unset, `screen_dump` returns `None` and the
/// caller falls back to `capture_pane`. This guards against an
/// accidental "always-on" regression in the env-var check.
#[test]
#[serial]
fn vt100_disabled_means_screen_dump_returns_none() {
    if !tmux_available() {
        eprintln!("Skipping: tmux not available");
        return;
    }
    // Force the env var off for this test even if the host has it
    // set globally.
    let prior = std::env::var("AOE_LIVE_VT100").ok();
    std::env::remove_var("AOE_LIVE_VT100");

    let name = unique_session_name("vt100_off");
    let _cleanup = TmuxCleanup { name: name.clone() };
    create_empty_session(&name);
    let client = ControlModeClient::spawn(&name, None).expect("ControlModeClient::spawn");
    assert!(client.screen_dump().is_none());

    if let Some(v) = prior {
        std::env::set_var("AOE_LIVE_VT100", v);
    }
}

/// End-to-end proof that the vt100 path captures pane bytes without
/// running `capture-pane`. Set the env var, spawn the client, send a
/// literal through control mode, wait for tmux to emit `%output`, and
/// assert that `screen_dump` (which reads from the in-process parser)
/// contains the typed bytes.
#[test]
#[serial]
fn vt100_screen_dump_reflects_pane_after_send_keys() {
    if !tmux_available() {
        eprintln!("Skipping: tmux not available");
        return;
    }
    let _env = VtEnvGuard::set();

    let name = unique_session_name("vt100_dump");
    let _cleanup = TmuxCleanup { name: name.clone() };
    create_empty_session(&name);

    let client = ControlModeClient::spawn(&name, None).expect("ControlModeClient::spawn");
    // First dump should at least exist (the screen was seeded from
    // capture-pane during spawn); we don't assert content here
    // because the empty-shell prompt varies by host.
    let seed_dump = client
        .screen_dump()
        .expect("vt100 path enabled, screen_dump must be Some");
    assert!(
        !seed_dump.is_empty(),
        "seed dump should be non-empty even on a fresh pane"
    );

    client
        .send_literal_no_enter("echo vt100-typed-this")
        .expect("send_literal_no_enter");
    client
        .send_named_key("Enter")
        .expect("send_named_key Enter");
    // Give tmux time to deliver to the pane, the shell time to
    // process, and the %output stream time to reach our reader
    // thread.
    std::thread::sleep(Duration::from_millis(400));

    let dump = client
        .screen_dump()
        .expect("vt100 path enabled, screen_dump must still be Some");
    assert!(
        dump.contains("vt100-typed-this"),
        "expected typed literal in vt100 screen dump, got: {dump:?}"
    );
}

#[test]
#[serial]
fn control_mode_spawn_fails_for_nonexistent_session() {
    if !tmux_available() {
        eprintln!("Skipping: tmux not available");
        return;
    }
    // tmux still spawns the process; it then writes an error and
    // exits. We just need to confirm that either spawn returns Err
    // OR the first capture returns Err so the caller falls back.
    let name = unique_session_name("missing");
    let result = ControlModeClient::spawn(&name, None);
    if let Ok(client) = result {
        assert!(
            client.capture_pane(10, 80, 24).is_err(),
            "expected capture against missing session to fail"
        );
    }
    // If spawn already returned Err, that's also a valid outcome.
}
