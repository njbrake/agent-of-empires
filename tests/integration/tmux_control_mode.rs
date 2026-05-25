//! Live-fire test for the tmux `-C` control-mode client.
//!
//! Verifies the wire format end-to-end: spawn a real tmux session, attach
//! a `ControlModeClient`, run `capture-pane` over the long-lived socket,
//! and confirm the returned bytes contain content we put into the pane.
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

    let client = match ControlModeClient::spawn(&name) {
        Ok(c) => c,
        Err(e) => {
            // Spawn failure is the same path the production caller
            // handles silently. We assert non-failure here because
            // this test exists specifically to exercise the success
            // path against a known-good session.
            panic!("ControlModeClient::spawn failed: {e}");
        }
    };

    let output = client
        .capture_pane(50, 80, 24)
        .expect("capture_pane should succeed against a live session");
    assert!(
        output.contains("hello-from-control-mode"),
        "expected pane content in capture, got: {output:?}"
    );
}

#[test]
#[serial]
fn control_mode_env_opt_out_blocks_spawn() {
    if !tmux_available() {
        eprintln!("Skipping: tmux not available");
        return;
    }

    let name = unique_session_name("env_opt_out");
    let _cleanup = TmuxCleanup { name: name.clone() };
    create_session_with_content(&name, "noop");

    let prior = std::env::var(agent_of_empires::tmux::CONTROL_MODE_DISABLE_ENV_VAR).ok();
    std::env::set_var(agent_of_empires::tmux::CONTROL_MODE_DISABLE_ENV_VAR, "1");
    let result = ControlModeClient::spawn(&name);
    match prior {
        Some(v) => std::env::set_var(agent_of_empires::tmux::CONTROL_MODE_DISABLE_ENV_VAR, v),
        None => std::env::remove_var(agent_of_empires::tmux::CONTROL_MODE_DISABLE_ENV_VAR),
    }
    assert!(
        result.is_err(),
        "expected env opt-out to block spawn, got Ok"
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
    let result = ControlModeClient::spawn(&name);
    if let Ok(client) = result {
        assert!(
            client.capture_pane(10, 80, 24).is_err(),
            "expected capture against missing session to fail"
        );
    }
    // If spawn already returned Err, that's also a valid outcome.
}
