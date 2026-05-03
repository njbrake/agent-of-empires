use serial_test::serial;
use std::time::Duration;

use crate::harness::{require_tmux, TuiTestHarness};

#[test]
#[serial]
fn test_command_palette_opens_with_ctrl_k() {
    require_tmux!();

    let mut h = TuiTestHarness::new("palette_open");
    h.spawn_tui();

    h.wait_for("[all]");
    h.send_keys("C-k");
    h.wait_for("Commands");
    // Built-in entries are visible.
    h.assert_screen_contains("Settings");
    h.assert_screen_contains("Rename");
}

#[test]
#[serial]
fn test_command_palette_esc_closes() {
    require_tmux!();

    let mut h = TuiTestHarness::new("palette_close");
    h.spawn_tui();

    h.wait_for("[all]");
    h.send_keys("C-k");
    h.wait_for("Commands");

    h.send_keys("Escape");
    h.wait_for_absent("Commands", Duration::from_secs(5));
}

#[test]
#[serial]
fn test_command_palette_fuzzy_search_settings() {
    require_tmux!();

    let mut h = TuiTestHarness::new("palette_fuzzy");
    h.spawn_tui();

    h.wait_for("[all]");
    h.send_keys("C-k");
    h.wait_for("Commands");

    // Type "set" to filter to "Open settings"
    h.type_text("set");
    std::thread::sleep(Duration::from_millis(100));
    h.assert_screen_contains("Open settings");

    // Enter should run it (opens settings view).
    h.send_keys("Enter");
    h.wait_for_absent("Commands", Duration::from_secs(5));
    h.wait_for("Settings");
}

#[test]
#[serial]
fn test_status_bar_shows_palette_hint() {
    require_tmux!();

    let mut h = TuiTestHarness::new("palette_hint");
    h.spawn_tui();

    h.wait_for("[all]");
    // The footer should mention the Ctrl+K shortcut so users discover it.
    h.assert_screen_contains("^K");
}

#[test]
#[serial]
fn test_help_lists_command_palette() {
    require_tmux!();

    let mut h = TuiTestHarness::new("palette_help");
    h.spawn_tui();

    h.wait_for("[all]");
    h.send_keys("?");
    h.wait_for("Keyboard Shortcuts");
    h.assert_screen_contains("Ctrl+K");
    h.assert_screen_contains("Command palette");
}
