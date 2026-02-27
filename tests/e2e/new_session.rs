use serial_test::serial;
use std::time::Duration;

use crate::harness::{require_tmux, TuiTestHarness};

#[test]
#[serial]
fn test_new_session_dialog_opens() {
    require_tmux!();

    let mut h = TuiTestHarness::new("new_dialog");
    h.spawn_tui();

    h.wait_for("Agent of Empires");
    h.send_keys("n");
    h.wait_for("Title");
    h.assert_screen_contains("Path");
}

#[test]
#[serial]
fn test_new_session_dialog_escape_cancels() {
    require_tmux!();

    let mut h = TuiTestHarness::new("new_esc");
    h.spawn_tui();

    h.wait_for("Agent of Empires");
    h.send_keys("n");
    h.wait_for("Title");

    h.send_keys("Escape");
    h.wait_for_absent("Title", Duration::from_secs(5));
    // Back to home screen.
    h.assert_screen_contains("No sessions yet");
}
