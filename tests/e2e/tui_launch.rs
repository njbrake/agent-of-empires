use serial_test::serial;
use std::time::Duration;

use crate::harness::{require_tmux, TuiTestHarness};

#[test]
#[serial]
fn test_tui_launches_and_shows_home_screen() {
    require_tmux!();

    let mut h = TuiTestHarness::new("launch");
    h.spawn_tui();

    h.wait_for(" aoe [");
    h.assert_screen_contains("No sessions yet");
    // Status bar should be visible. Use the j/k key hint (which has no
    // accompanying description in the compact footer).
    h.assert_screen_contains("j/k");
}

#[test]
#[serial]
fn test_tui_quit_with_q() {
    require_tmux!();

    let mut h = TuiTestHarness::new("quit");
    h.spawn_tui();

    h.wait_for(" aoe [");
    h.send_keys("q");
    h.wait_for_exit(Duration::from_secs(5));
    assert!(!h.session_alive(), "session should have exited after 'q'");
}
