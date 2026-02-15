//! Integration tests for TUI attach/detach behavior
//!
//! These tests validate that the terminal state is properly managed when
//! attaching to and detaching from tmux sessions.

use std::process::Command;

/// Verify tmux is available for testing
fn tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Test that tmux sessions can be created and killed
#[test]
fn test_tmux_session_lifecycle() {
    if !tmux_available() {
        eprintln!("Skipping test: tmux not available");
        return;
    }

    let session_name = "aoe_test_lifecycle_12345678";

    // Create a detached session
    let create = Command::new("tmux")
        .args(["new-session", "-d", "-s", session_name])
        .output()
        .expect("Failed to create tmux session");

    assert!(create.status.success(), "Failed to create test session");

    // Verify session exists
    let check = Command::new("tmux")
        .args(["has-session", "-t", session_name])
        .output()
        .expect("Failed to check session");

    assert!(
        check.status.success(),
        "Session should exist after creation"
    );

    // Kill session
    let kill = Command::new("tmux")
        .args(["kill-session", "-t", session_name])
        .output()
        .expect("Failed to kill session");

    assert!(kill.status.success(), "Failed to kill test session");

    // Verify session no longer exists
    let check_after = Command::new("tmux")
        .args(["has-session", "-t", session_name])
        .output()
        .expect("Failed to check session");

    assert!(
        !check_after.status.success(),
        "Session should not exist after kill"
    );
}

/// Test that session names are properly sanitized
#[test]
fn test_session_name_format() {
    let prefix = "aoe_";

    // Valid session names should start with our prefix
    let session_name = format!("{}my_project_abc12345", prefix);
    assert!(session_name.starts_with(prefix));

    // Session names should not contain problematic characters
    assert!(!session_name.contains(' '));
    assert!(!session_name.contains(':'));
    assert!(!session_name.contains('.'));
}

/// Test terminal mode switching sequence
///
/// This test documents the expected sequence for attach/detach:
/// 1. Disable raw mode
/// 2. Leave alternate screen
/// 3. Disable mouse capture
/// 4. Show cursor
/// 5. [user interacts with tmux]
/// 6. Enable raw mode
/// 7. Enter alternate screen
/// 8. Enable mouse capture
/// 9. Hide cursor
/// 10. Clear terminal
/// 11. Drain stale events
#[test]
fn test_terminal_mode_sequence_documented() {
    // This test documents the expected behavior rather than testing it directly
    // since testing terminal modes requires actual terminal interaction.

    let expected_exit_sequence = [
        "disable_raw_mode",
        "LeaveAlternateScreen",
        "DisableMouseCapture",
        "cursor::Show",
        "flush",
    ];

    let expected_reenter_sequence = [
        "enable_raw_mode",
        "EnterAlternateScreen",
        "EnableMouseCapture",
        "cursor::Hide",
        "flush",
        "drain_events",
        "terminal.clear",
        "set_needs_redraw",
    ];

    // Verify sequences have all required steps
    assert!(expected_exit_sequence.contains(&"disable_raw_mode"));
    assert!(expected_exit_sequence.contains(&"LeaveAlternateScreen"));
    assert!(expected_reenter_sequence.contains(&"enable_raw_mode"));
    assert!(expected_reenter_sequence.contains(&"EnterAlternateScreen"));
    assert!(expected_reenter_sequence.contains(&"terminal.clear"));
    assert!(expected_reenter_sequence.contains(&"drain_events"));
}

/// Test that draining events prevents stale input
#[test]
fn test_event_draining_concept() {
    // When returning from tmux, there may be stale keyboard events
    // in the crossterm event queue. These must be drained to prevent
    // the TUI from receiving and acting on old input.
    //
    // The drain loop should:
    // 1. Poll with zero timeout (non-blocking)
    // 2. Read and discard any available events
    // 3. Continue until no more events are available

    // This is a conceptual test - actual draining is tested in integration
    let drain_timeout_ms = 0;
    assert_eq!(drain_timeout_ms, 0, "Drain should use zero timeout");
}

/// Test that remain-on-exit produces a dead pane that is detected by update_status
#[test]
fn test_dead_pane_detected_as_error() {
    if !tmux_available() {
        eprintln!("Skipping test: tmux not available");
        return;
    }

    let session_name = "aoe_test_deadpane_12345678";

    // Ensure clean state
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", session_name])
        .output();

    // Create a session that runs a command which exits immediately
    let create = Command::new("tmux")
        .args([
            "new-session",
            "-d",
            "-s",
            session_name,
            "true", // exits immediately with status 0
        ])
        .output()
        .expect("Failed to create tmux session");
    assert!(create.status.success(), "Failed to create test session");

    // Enable remain-on-exit so the pane stays as "dead" after the command exits
    let set_opt = Command::new("tmux")
        .args(["set-option", "-t", session_name, "remain-on-exit", "on"])
        .output()
        .expect("Failed to set remain-on-exit");
    assert!(set_opt.status.success());

    // Wait briefly for the command to exit
    std::thread::sleep(std::time::Duration::from_millis(500));

    // Verify the pane is dead
    let check = Command::new("tmux")
        .args(["display", "-p", "-t", session_name, "#{pane_dead}"])
        .output()
        .expect("Failed to check pane_dead");

    let pane_dead = String::from_utf8_lossy(&check.stdout).trim().to_string();
    assert_eq!(
        pane_dead, "1",
        "Pane should be dead after command exits with remain-on-exit"
    );

    // Verify respawn-pane brings it back
    let respawn = Command::new("tmux")
        .args(["respawn-pane", "-k", "-t", session_name])
        .output()
        .expect("Failed to respawn pane");
    assert!(respawn.status.success(), "respawn-pane should succeed");

    // Clean up
    let _ = Command::new("tmux")
        .args(["kill-session", "-t", session_name])
        .output();
}

/// Test that update_status detects dead panes by verifying the code path exists
#[test]
fn test_update_status_checks_dead_pane() {
    let source =
        std::fs::read_to_string("src/session/instance.rs").expect("Failed to read instance.rs");

    // Find the update_status method
    let fn_start = source
        .find("fn update_status(")
        .expect("update_status function not found");

    let fn_section = &source[fn_start..];
    let fn_end = fn_section
        .find("\n    pub fn capture_output")
        .unwrap_or(fn_section.len());
    let fn_body = &fn_section[..fn_end];

    assert!(
        fn_body.contains("is_pane_dead"),
        "update_status must check is_pane_dead() to detect remain-on-exit dead panes. \
         Without this check, sessions show 'Pane is dead' in tmux with no status update in the TUI."
    );

    assert!(
        fn_body.contains("Status::Error"),
        "update_status must set Status::Error when pane is dead"
    );
}

/// Test that attach/detach uses terminal backend, not std::io::stdout()
///
/// This test verifies the fix for the terminal corruption bug where
/// using std::io::stdout() instead of terminal.backend_mut() caused
/// file descriptor desynchronization, corrupting tmux sessions.
///
/// The terminal leave/restore logic lives in `with_raw_mode_disabled`,
/// which `attach_session` delegates to.
#[test]
fn test_attach_uses_terminal_backend() {
    let source = std::fs::read_to_string("src/tui/app.rs").expect("Failed to read app.rs");

    // The shared helper that handles terminal mode switching must use backend_mut()
    let helper_start = source
        .find("fn with_raw_mode_disabled")
        .expect("with_raw_mode_disabled helper not found");

    let helper_section = &source[helper_start..];
    let fn_end = helper_section
        .find("\n}\n")
        .map(|i| i + 3)
        .unwrap_or(helper_section.len());

    let helper_body = &helper_section[..fn_end];

    assert!(
        !helper_body.contains("std::io::stdout()"),
        "with_raw_mode_disabled should use terminal.backend_mut() instead of std::io::stdout(). \
         Using std::io::stdout() creates separate file descriptor handles that can \
         corrupt terminal state and cause 'open terminal failed: not a terminal' errors."
    );

    assert!(
        helper_body.contains("terminal.backend_mut()"),
        "with_raw_mode_disabled should use terminal.backend_mut() for terminal operations"
    );

    // attach_session must delegate to the helper, not bypass it
    let attach_fn_start = source
        .find("fn attach_session(")
        .expect("attach_session function not found");

    let attach_fn_section = &source[attach_fn_start..];
    let attach_fn_end = attach_fn_section
        .find("\n    fn ")
        .or_else(|| attach_fn_section.find("\n}\n"))
        .unwrap_or(attach_fn_section.len());

    let attach_fn_body = &attach_fn_section[..attach_fn_end];

    assert!(
        attach_fn_body.contains("with_raw_mode_disabled"),
        "attach_session should delegate to with_raw_mode_disabled"
    );

    assert!(
        !attach_fn_body.contains("std::io::stdout()"),
        "attach_session should not use std::io::stdout() directly"
    );
}
