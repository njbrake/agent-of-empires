use serial_test::serial;
use std::time::Duration;

use crate::harness::{require_tmux, TuiTestHarness};

#[test]
#[serial]
fn test_tui_launches_and_shows_home_screen() {
    require_tmux!();

    let mut h = TuiTestHarness::new("launch");
    h.spawn_tui();

    h.wait_for("Agent of Empires");
    h.assert_screen_contains("No sessions yet");
    // Status bar navigation hints should be visible.
    h.assert_screen_contains("Nav");
}

#[test]
#[serial]
fn test_tui_quit_with_q() {
    require_tmux!();

    let mut h = TuiTestHarness::new("quit");
    h.spawn_tui();

    h.wait_for("Agent of Empires");
    h.send_keys("q");
    h.wait_for_exit(Duration::from_secs(5));
    assert!(!h.session_alive(), "session should have exited after 'q'");
}
