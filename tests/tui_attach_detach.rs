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

/// Test that attach/detach uses terminal backend, not std::io::stdout()
///
/// This test verifies the fix for the terminal corruption bug where
/// using std::io::stdout() instead of terminal.backend_mut() caused
/// file descriptor desynchronization, corrupting tmux sessions.
#[test]
fn test_attach_uses_terminal_backend() {
    let source = std::fs::read_to_string("src/tui/app.rs").expect("Failed to read app.rs");

    let attach_fn_start = source
        .find("fn attach_session(")
        .expect("attach_session function not found");

    let attach_fn_section = &source[attach_fn_start..];
    let fn_end = attach_fn_section
        .find("\n    fn ")
        .or_else(|| attach_fn_section.find("\n}\n"))
        .unwrap_or(attach_fn_section.len());

    let attach_fn_body = &attach_fn_section[..fn_end];

    assert!(
        !attach_fn_body.contains("std::io::stdout()"),
        "attach_session should use terminal.backend_mut() instead of std::io::stdout(). \
         Using std::io::stdout() creates separate file descriptor handles that can \
         corrupt terminal state and cause 'open terminal failed: not a terminal' errors."
    );

    assert!(
        attach_fn_body.contains("terminal.backend_mut()"),
        "attach_session should use terminal.backend_mut() for terminal operations"
    );
}
