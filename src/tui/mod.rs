//! Terminal User Interface module

mod app;
mod components;
mod creation_poller;
mod deletion_poller;
pub mod dialogs;
pub mod diff;
mod home;
pub mod settings;
mod status_poller;
pub(crate) mod styles;

pub use app::*;

use anyhow::Result;
use crossterm::{
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    Command,
};
use ratatui::prelude::*;
use std::io::{self, IsTerminal, Write};

use crate::migrations;
use crate::session::get_update_settings;
use crate::update::check_for_update;

/// Mouse capture without drag tracking. Enables xterm modes 1000
/// (button press/release, which delivers wheel events as button codes
/// 64/65) and 1006 (SGR extended coordinates). Skipping modes 1002
/// (button-motion) and 1003 (any-motion) leaves the terminal's native
/// drag-to-select intact so users can still copy text from the TUI.
///
/// We still capture single-click button events (mode 1000) and ignore
/// them in the event loop. That's intentional: dropping mode 1000 too
/// would also drop wheel-scroll reports, since terminals encode wheel
/// ticks as button codes 64/65 under the same mode.
///
/// `DisableMouseCapture` cleans up the full mode set, so it's safe to
/// keep using on teardown even though we never enabled 1002/1003/1015.
pub(crate) struct ScrollOnlyMouseCapture;

impl Command for ScrollOnlyMouseCapture {
    fn write_ansi(&self, f: &mut impl std::fmt::Write) -> std::fmt::Result {
        f.write_str("\x1B[?1000h\x1B[?1006h")
    }

    #[cfg(windows)]
    fn execute_winapi(&self) -> std::io::Result<()> {
        // The Windows console doesn't expose separate drag-tracking modes,
        // so fall back to crossterm's full capture there.
        crossterm::event::EnableMouseCapture.execute_winapi()
    }

    #[cfg(windows)]
    fn is_ansi_code_supported(&self) -> bool {
        false
    }
}

pub async fn run(profile: &str, startup_warning: Option<String>) -> Result<()> {
    // Run pending migrations with a spinner so users see progress
    if migrations::has_pending_migrations() {
        const SPINNER_FRAMES: &[char] = &['◐', '◓', '◑', '◒'];
        let migration_handle = tokio::task::spawn_blocking(migrations::run_migrations);
        tokio::pin!(migration_handle);
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(120));
        let mut frame = 0usize;
        loop {
            tokio::select! {
                result = &mut migration_handle => {
                    print!("\r\x1b[2K");
                    let _ = io::stdout().flush();
                    result??;
                    break;
                }
                _ = tick.tick() => {
                    print!("\r  {} Running data migrations...", SPINNER_FRAMES[frame % SPINNER_FRAMES.len()]);
                    let _ = io::stdout().flush();
                    frame += 1;
                }
            }
        }
    }

    // Check for tmux
    if !crate::tmux::is_tmux_available() {
        eprintln!("Error: tmux not found in PATH");
        eprintln!();
        eprintln!("Agent of Empires requires tmux. Install with:");
        eprintln!("  brew install tmux     # macOS");
        eprintln!("  apt install tmux      # Debian/Ubuntu");
        eprintln!("  pacman -S tmux        # Arch");
        std::process::exit(1);
    }

    // Check for coding tools (no-agents case is handled inside the TUI)
    let available_tools = crate::tmux::AvailableTools::detect();

    // If version changed, refresh the update cache before showing TUI.
    // This ensures we have release notes for the changelog dialog.
    if check_version_change()?.is_some() {
        let settings = get_update_settings();
        if settings.check_enabled {
            let current_version = env!("CARGO_PKG_VERSION");
            // Don't let a network issue block startup
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                check_for_update(current_version, true),
            )
            .await;
        }
    }

    // Bail early if stdin is not a terminal. Running without a tty would
    // cause the event loop to busy-loop after the parent terminal dies.
    if !io::stdin().is_terminal() {
        anyhow::bail!("stdin is not a terminal; aoe requires an interactive TTY");
    }

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableBracketedPaste,
        ScrollOnlyMouseCapture
    )?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app and run
    let mut app = App::new(profile, available_tools)?;
    if let Some(warning) = startup_warning {
        app.show_startup_warning(&warning);
    }
    let result = app.run(&mut terminal).await;

    crate::session::clear_tui_heartbeat();

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableBracketedPaste,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scroll_only_mouse_capture_emits_only_modes_1000_and_1006() {
        // The whole point of ScrollOnlyMouseCapture is to omit the drag-tracking
        // modes (1002/1003) that hijack the terminal's native drag-to-select.
        // Snapshot the exact byte sequence so a future "tidy-up" can't silently
        // re-introduce them.
        let mut buf = String::new();
        ScrollOnlyMouseCapture.write_ansi(&mut buf).unwrap();
        assert_eq!(buf, "\x1B[?1000h\x1B[?1006h");
    }
}
