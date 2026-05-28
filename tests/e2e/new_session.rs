use serial_test::serial;
use std::time::Duration;

use crate::harness::{require_tmux, TuiTestHarness};

#[test]
#[serial]
fn test_new_session_dialog_opens() {
    require_tmux!();

    let mut h = TuiTestHarness::new("new_dialog");
    h.spawn_tui();

    h.wait_for(" aoe ");
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

    h.wait_for(" aoe ");
    h.send_keys("n");
    h.wait_for("Title");

    h.send_keys("Escape");
    h.wait_for_absent("Title", Duration::from_secs(5));
    // Back to home screen.
    h.assert_screen_contains("No sessions yet");
}

#[test]
#[serial]
fn test_left_click_on_empty_sidebar_opens_new_session_dialog() {
    // Regression test for the mouse-support PR: a left-click on the
    // sidebar's empty area below the last session row must open the
    // new-session dialog (the click equivalent of pressing `n`), and
    // a subsequent Escape must reach the dialog (not fall through to
    // any underlying surface). The dialog-receives-keys part is the
    // half that previously broke when live-send was active; this test
    // covers the simpler no-live-send path that any user hitting
    // empty-sidebar-click goes through.
    require_tmux!();

    let mut h = TuiTestHarness::new("empty_click");
    h.spawn_tui();

    h.wait_for(" aoe ");
    // Dismiss the first-run welcome dialog so the sidebar is the
    // top-most surface receiving clicks.
    h.send_keys("Enter");
    h.wait_for("No sessions yet");

    // Click well below the empty-state label in the sidebar column.
    // The sidebar's empty area is wide; (col=10, row=15) lands inside
    // `list_inner_area` but past every real row, which is exactly
    // what `handle_empty_list_click` checks for. Coordinates are
    // 1-indexed in SGR.
    h.send_mouse_click(0, 10, 15);

    // Dialog should appear with its trademark fields.
    h.wait_for(" New Session ");
    h.assert_screen_contains("Title");
    h.assert_screen_contains("Path");

    // Keyboard now routes to the dialog. Escape closes it.
    h.send_keys("Escape");
    h.wait_for_absent(" New Session ", Duration::from_secs(5));
    h.assert_screen_contains("No sessions yet");
}

/// Submit the new session dialog, handling the "Path does not exist. Create?"
/// prompt if it appears.
///
/// macOS CI tmux occasionally drops the first Enter when sent right after a
/// long literal-text burst, leaving the dialog stuck in the input state. We
/// detect that by polling for the dialog to transition (close, switch to a
/// loading overlay, or show the create-dir prompt) and re-send Enter if the
/// dialog still shows its hint line after a grace period. Resending Enter is
/// idempotent: by the time the dialog has closed it is already gone, so a
/// late-arriving second Enter falls through to the home view, where Enter is
/// a no-op when no session row is selected (the Creating stub is auto-selected
/// only after the dialog closes).
fn submit_new_session_dialog(h: &TuiTestHarness) {
    h.send_keys("Enter");
    let start = std::time::Instant::now();
    let mut resent = false;
    loop {
        let screen = h.capture_screen();
        if screen.contains("Path does not exist") {
            h.send_keys("y");
            return;
        }
        // Any of these means Enter was accepted by the dialog.
        if !screen.contains(" New Session ")
            || screen.contains("Running Hooks")
            || screen.contains("Creating Session")
            || screen.contains("Creating...")
        {
            return;
        }
        if !resent && start.elapsed() > Duration::from_millis(800) {
            // Dialog still in input state. Assume Enter was lost; resend.
            h.send_keys("Enter");
            resent = true;
        }
        if start.elapsed() > Duration::from_secs(5) {
            // Give up; downstream wait_for will produce the diagnostic.
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Write a global config with on_create hooks so session creation goes through
/// the background CreationPoller and shows a Creating stub in the session list.
fn write_config_with_hooks(h: &TuiTestHarness, hook_cmd: &str) {
    let config_dir = crate::harness::app_dir_in(h.home_path());
    let config_content = format!(
        r#"[hooks]
on_create = ["{hook_cmd}"]

[updates]
update_check_mode = "off"

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

    h.wait_for(" aoe ");

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

    h.wait_for(" aoe ");

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

    h.wait_for(" aoe ");

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

    h.wait_for(" aoe ");

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
    // Use a 5s timeout (the convention in this file) so a CI runner under
    // load has enough headroom for tmux capture-pane to see the dialog
    // after the `q` key triggers the state transition; the previous 3s
    // budget was a flake source on ubuntu-latest (empty screen captures
    // mid-render).
    h.wait_for_timeout("Session Creating", Duration::from_secs(5));
    h.assert_screen_contains("Quit anyway");

    // Decline to quit.
    h.send_keys("n");
    h.wait_for_absent("Session Creating", Duration::from_secs(5));
    // TUI is still running with the Creating stub.
    h.assert_screen_contains("Creating...");

    // Clean up by cancelling creation (stub is selected again).
    h.send_keys("C-c");
    h.wait_for_absent("Creating...", Duration::from_secs(5));
}

/// Write a global config that opts into `new_session_attach_mode = "live_send"`
/// so creation routes into live-send mode instead of the historical tmux
/// attach. No hooks; the sync create path applies (this is the path that
/// originally bypassed the setting).
fn write_config_attach_mode_live_send(h: &TuiTestHarness) {
    let config_dir = crate::harness::app_dir_in(h.home_path());
    let config_content = format!(
        r#"[updates]
update_check_mode = "off"

[app_state]
has_seen_welcome = true
last_seen_version = "{version}"
has_acknowledged_agent_hooks = true

[session]
new_session_attach_mode = "live_send"
"#,
        version = env!("CARGO_PKG_VERSION"),
    );
    std::fs::write(config_dir.join("config.toml"), config_content)
        .expect("write config with attach mode");
}

/// Regression guard for the original "new sessions still attach to tmux even
/// though I picked live mode" bug. Both creation paths (sync and async) must
/// route through `dispatch_new_session_attach` and honor the setting; the
/// sync path was the one that bypassed it (the symptom that made this PR
/// happen in the first place).
#[test]
#[serial]
fn test_new_session_enters_live_mode_when_configured() {
    require_tmux!();

    let mut h = TuiTestHarness::new("attach_live_send");
    write_config_attach_mode_live_send(&h);
    let project = h.project_path();
    h.spawn_tui();

    h.wait_for(" aoe ");

    h.send_keys("n");
    h.wait_for("Title");
    h.send_keys("Tab");
    h.type_text(project.to_str().unwrap());
    submit_new_session_dialog(&h);

    // After creation, the home view stays mounted with the LIVE banner in
    // the footer. A tmux-attach dispatch would replace the entire TUI
    // screen with whatever the agent is rendering, so the banner is the
    // load-bearing tell that the setting was respected.
    h.wait_for_timeout("LIVE", Duration::from_secs(10));
    // Sanity: the home view's title chrome is still on screen, meaning
    // the dispatch didn't flip into the tmux attach view.
    h.assert_screen_contains(" aoe ");
}

// NOTE: a previous version of this file added
// `test_live_send_repeated_entry_exit_remains_responsive`, which
// drove the TUI through two Tab → C-q cycles to validate the
// `ControlModeClient` spawn/drop lifecycle. The test was reliable on
// macOS but flaked on ubuntu-latest because the pane process (a
// short-lived shell, picked by the wizard when no agent is
// installed) exited cleanly within ~2s of session creation: by the
// time the second `Tab` fired, `ensure_pane_ready` saw a dead pane
// and surfaced the "Live send failed" dialog instead of LIVE. The
// e2e test conflated two concerns ("the client lifecycle is clean"
// vs. "the pane survives across cycles"), so the lifecycle
// assertion now lives in `tests/integration/tmux_control_mode.rs`
// (`control_mode_spawn_drop_respawn_against_same_session`), which
// spawns against a raw tmux session that doesn't go anywhere.
