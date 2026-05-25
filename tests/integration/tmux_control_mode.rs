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
use std::time::{Duration, Instant};

fn tmux_available() -> bool {
    // Check the exit status, not just that the process spawned.
    // A tmux that exists on PATH but exits non-zero (e.g. a broken
    // install) shouldn't pretend to be available.
    Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Poll `tmux capture-pane` against `name` until `needle` appears in
/// the pane buffer, or the deadline elapses. Used in place of a
/// fixed sleep after `send-keys`, which races on slower CI runners.
fn wait_for_pane_contains(name: &str, needle: &str, timeout: Duration) {
    let deadline = Instant::now() + timeout;
    loop {
        let out = Command::new("tmux")
            .args(["capture-pane", "-pt", name])
            .output()
            .expect("tmux capture-pane");
        let pane = String::from_utf8_lossy(&out.stdout);
        if pane.contains(needle) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for pane content {:?} in session {:?}",
            needle,
            name
        );
        std::thread::sleep(Duration::from_millis(25));
    }
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
    // Poll for the content to land in the pane instead of a fixed
    // sleep; slow CI runners (especially ubuntu-latest under load)
    // can take longer than the previous 250ms budget.
    wait_for_pane_contains(name, content, Duration::from_secs(2));
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

/// Regression guard for the `ControlModeClient` spawn/drop/respawn
/// lifecycle. The user-visible scenario is "enter live mode, exit,
/// enter again" — if Drop left the tmux server in a state that
/// blocks a fresh attach, the second `enter_live_send` would fail
/// with the "Live send failed" dialog. This test exercises the same
/// lifecycle directly against a raw tmux session that doesn't carry
/// the agent-pane-death risk of an e2e variant (see the deletion
/// note in `tests/e2e/new_session.rs`).
#[test]
#[serial]
fn control_mode_spawn_drop_respawn_against_same_session() {
    if !tmux_available() {
        eprintln!("Skipping: tmux not available");
        return;
    }

    let name = unique_session_name("respawn");
    let _cleanup = TmuxCleanup { name: name.clone() };
    create_empty_session(&name);

    // Cycle 1: spawn, capture, drop at scope exit.
    {
        let client =
            ControlModeClient::spawn(&name, None).expect("first spawn against fresh session");
        client
            .capture_pane(50, 80, 24)
            .expect("first capture should succeed");
    }

    // Give tmux a tick to process the client detach. Without this the
    // next spawn might race with the prior client's exit, which tmux
    // can briefly surface as a transient attach error.
    std::thread::sleep(Duration::from_millis(100));

    // Cycle 2: spawn against the same session must work. If Drop on
    // the first client wedged stdin or otherwise left the server in
    // a half-attached state, this is where the bug would show.
    {
        let client =
            ControlModeClient::spawn(&name, None).expect("respawn against same session after drop");
        let out = client
            .capture_pane(50, 80, 24)
            .expect("second capture should succeed");
        assert!(
            !out.is_empty(),
            "second capture returned empty payload; expected the seeded shell prompt"
        );
    }
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
