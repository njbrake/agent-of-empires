use serial_test::serial;
use std::time::Duration;

use crate::harness::{require_tmux, TuiTestHarness};

#[test]
#[serial]
fn test_new_session_dialog_opens() {
    require_tmux!();

    let mut h = TuiTestHarness::new("new_dialog");
    h.spawn_tui();

    h.wait_for(" aoe [");
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

    h.wait_for(" aoe [");
    h.send_keys("n");
    h.wait_for("Title");

    h.send_keys("Escape");
    h.wait_for_absent("Title", Duration::from_secs(5));
    // Back to home screen.
    h.assert_screen_contains("No sessions yet");
}

/// Submit the new session dialog, handling the "Path does not exist. Create?"
/// prompt if it appears. The 'y' keystroke is harmless when the path exists
/// because there is no 'y' keybinding in the home view.
fn submit_new_session_dialog(h: &TuiTestHarness) {
    h.send_keys("Enter");
    std::thread::sleep(Duration::from_millis(300));
    h.send_keys("y");
}

/// Write a global config with on_create hooks so session creation goes through
/// the background CreationPoller and shows a Creating stub in the session list.
fn write_config_with_hooks(h: &TuiTestHarness, hook_cmd: &str) {
    let config_dir = if cfg!(target_os = "linux") {
        h.home_path().join(".config").join("agent-of-empires")
    } else {
        h.home_path().join(".agent-of-empires")
    };
    let config_content = format!(
        r#"[hooks]
on_create = ["{hook_cmd}"]

[updates]
check_enabled = false

[app_state]
has_seen_welcome = true
last_seen_version = "{version}"
has_acknowledged_agent_hooks = true
"#,
        hook_cmd = hook_cmd,
        version = env!("CARGO_PKG_VERSION"),
    );
    std::fs::write(config_dir.join("config.toml"), config_content)
        .expect("write config with hooks");
}

#[test]
#[serial]
fn test_creating_stub_appears_during_hook_execution() {
    require_tmux!();

    let mut h = TuiTestHarness::new("creating_stub");
    // Use a slow hook so we can observe the Creating state.
    write_config_with_hooks(&h, "sleep 5");
    let project = h.project_path();
    h.spawn_tui();

    h.wait_for(" aoe [");

    // Open new session dialog and fill in the path.
    h.send_keys("n");
    h.wait_for("Title");
    // Tab from Title to Path field.
    h.send_keys("Tab");
    h.type_text(project.to_str().unwrap());
    submit_new_session_dialog(&h);

    // The dialog should close and a Creating stub should appear in the list.
    // The preview pane shows "Creating..." with hook output.
    h.wait_for_timeout("Creating...", Duration::from_secs(10));
    h.assert_screen_contains("Hook Output");
}

#[test]
#[serial]
fn test_creating_stub_cancelled_with_ctrl_c() {
    require_tmux!();

    let mut h = TuiTestHarness::new("creating_cancel");
    write_config_with_hooks(&h, "sleep 10");
    let project = h.project_path();
    h.spawn_tui();

    h.wait_for(" aoe [");

    // Create a session with a slow hook.
    h.send_keys("n");
    h.wait_for("Title");
    h.send_keys("Tab");
    h.type_text(project.to_str().unwrap());
    submit_new_session_dialog(&h);

    h.wait_for_timeout("Creating...", Duration::from_secs(10));

    // Cancel with Ctrl+C.
    h.send_keys("C-c");

    // The Creating stub should be removed and we should be back to empty state.
    h.wait_for_absent("Creating...", Duration::from_secs(5));
    h.assert_screen_contains("No sessions yet");
}

#[test]
#[serial]
fn test_creating_blocks_second_session_creation() {
    require_tmux!();

    let mut h = TuiTestHarness::new("creating_blocks_new");
    write_config_with_hooks(&h, "sleep 10");
    let project = h.project_path();
    h.spawn_tui();

    h.wait_for(" aoe [");

    // Start creating a session.
    h.send_keys("n");
    h.wait_for("Title");
    h.send_keys("Tab");
    h.type_text(project.to_str().unwrap());
    submit_new_session_dialog(&h);

    h.wait_for_timeout("Creating...", Duration::from_secs(10));

    // Try to create another session while one is in progress.
    h.send_keys("n");

    // Should show an info dialog instead of the new session dialog.
    h.wait_for_timeout("Please Wait", Duration::from_secs(3));
    h.assert_screen_contains("already being created");

    // Clean up.
    h.send_keys("Enter");
    h.send_keys("C-c");
    h.wait_for_absent("Creating...", Duration::from_secs(5));
}

#[test]
#[serial]
fn test_quit_during_creation_shows_confirm() {
    require_tmux!();

    let mut h = TuiTestHarness::new("quit_creating");
    write_config_with_hooks(&h, "sleep 10");
    let project = h.project_path();
    h.spawn_tui();

    h.wait_for(" aoe [");

    // Start creating a session.
    h.send_keys("n");
    h.wait_for("Title");
    h.send_keys("Tab");
    h.type_text(project.to_str().unwrap());
    submit_new_session_dialog(&h);

    h.wait_for_timeout("Creating...", Duration::from_secs(10));

    // Navigate away from the creating stub so Ctrl+C triggers quit path.
    // With only one session (the stub), pressing 'q' is more reliable.
    h.send_keys("q");

    // Should show a confirmation dialog instead of quitting.
    h.wait_for_timeout("Session Creating", Duration::from_secs(3));
    h.assert_screen_contains("Quit anyway");

    // Decline to quit.
    h.send_keys("n");
    h.wait_for_absent("Session Creating", Duration::from_secs(3));
    // TUI is still running with the Creating stub.
    h.assert_screen_contains("Creating...");

    // Clean up by cancelling creation (stub is selected again).
    h.send_keys("C-c");
    h.wait_for_absent("Creating...", Duration::from_secs(5));
}
