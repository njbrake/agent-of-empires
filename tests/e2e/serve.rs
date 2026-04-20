//! E2E coverage for the serve dialog state machine.
//!
//! Targeted regression tests for the `R`-key ModePicker + Confirm flow
//! introduced with the Tailscale Funnel transport picker. Compiled only
//! with `--features serve` since the serve dialog doesn't exist
//! otherwise; run via:
//!
//! ```sh
//! cargo test --test e2e --features serve -- tui_serve_dialog
//! ```
#![cfg(feature = "serve")]

use serial_test::serial;

use crate::harness::{require_tmux, TuiTestHarness};

/// Pressing `R` from the home screen opens the serve ModePicker,
/// which must render both cards (Local + Internet) and surface the
/// transport-picker-deferred hint on the Tunnel card ("Pick transport
/// on next screen.").
#[test]
#[serial]
fn tui_serve_dialog_opens_to_mode_picker() {
    require_tmux!();

    let mut h = TuiTestHarness::new("serve_mode_picker");
    h.spawn_tui();

    h.wait_for("Agent of Empires");
    h.send_keys("R");

    h.wait_for("How should this be reachable?");
    h.assert_screen_contains("Local network");
    h.assert_screen_contains("Internet (HTTPS)");
    // The Tunnel card defers the transport choice to the next screen.
    // If this line disappears, the ModePicker copy is out of sync with
    // the Confirm-screen picker it hands off to.
    h.assert_screen_contains("Pick transport on next screen.");
}

/// Esc dismisses the serve dialog and returns to the home screen
/// without spawning anything. Regression guard against state-transition
/// bugs where ModePicker might latch onto a stale mode.
#[test]
#[serial]
fn tui_serve_dialog_escape_returns_home() {
    require_tmux!();

    let mut h = TuiTestHarness::new("serve_mode_picker_esc");
    h.spawn_tui();

    h.wait_for("Agent of Empires");
    h.send_keys("R");
    h.wait_for("How should this be reachable?");

    h.send_keys("Escape");
    // Home-screen footer is the tell that we've returned.
    h.wait_for("No sessions yet");
}
