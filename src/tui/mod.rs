//! Terminal User Interface module

mod app;
mod attached_status_hooks;
#[cfg(feature = "serve")]
pub(crate) mod cockpit_view;
mod components;
mod creation_poller;
mod deletion_poller;
pub mod dialogs;
pub mod diff;
mod home;
#[cfg(feature = "serve")]
pub(crate) mod remote_home;
pub(crate) mod responsive;
pub mod settings;
mod status_poller;
pub(crate) mod styles;

pub use app::*;

use anyhow::Result;
use crossterm::{
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::io::{self, IsTerminal, Write};

use crate::migrations;

/// Whether the TUI should request mouse capture (`\e[?1000h` etc.) from the
/// terminal. Default ON to preserve the preview-pane mouse-wheel scroll
/// feature added in #795. Set `AOE_MOUSE_CAPTURE=0` (or `false`) on iOS
/// Mosh + Termius/Blink to opt out, so the terminal app's own scrollback
/// buffer handles wheel events (Mosh doesn't reliably forward mouse
/// tracking to mobile clients).
pub fn mouse_capture_requested() -> bool {
    std::env::var("AOE_MOUSE_CAPTURE")
        .map(|v| !(v == "0" || v.eq_ignore_ascii_case("false")))
        .unwrap_or(true)
}
use crate::session::get_update_settings;
use crate::update::check_for_update;

pub async fn run(profile: &str, startup_warning: Option<String>) -> Result<()> {
    // Cross-machine entrypoint: when `AOE_DAEMON_URL` is set, swap the
    // local home view for the remote cockpit picker so the user never
    // sees a session list that doesn't reflect the daemon they pointed
    // us at. Tmux check + migrations are intentionally skipped here:
    // the remote machine owns those, this side is a pure client.
    #[cfg(feature = "serve")]
    if let Some(endpoint) = crate::cockpit::client::discovery::discover_env() {
        let _ = startup_warning; // remote mode skips the local startup-warning channel
        let _ = profile;
        return remote_home::run_standalone(endpoint).await;
    }

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
        if settings.update_check_mode.is_enabled() {
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
    // (mouse_capture_requested defined below; see top-of-file pub fn.)
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableBracketedPaste)?;
    // Mouse capture is ON by default to preserve preview-pane wheel scroll
    // (#795); set AOE_MOUSE_CAPTURE=0 to opt out on iOS Mosh + Termius/Blink,
    // which can't reliably forward mouse-tracking escapes to mobile clients.
    //
    // Additionally: even when explicitly requested, Mosh mangles xterm
    // mouse-tracking escapes (inverted/duplicated scroll on Termius, Blink,
    // Mosh4iOS; broken right-click selection on desktop Mosh). MOSH_CONNECTION
    // is set by mosh-server and propagates through the user's environment;
    // when present, fall back to the terminal's native scroll regardless of
    // AOE_MOUSE_CAPTURE so the user can select text without aoe eating events.
    let mosh_active = std::env::var_os("MOSH_CONNECTION").is_some();
    if mouse_capture_requested() && !mosh_active {
        execute!(stdout, EnableMouseCapture)?;
    }
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Combine the caller-supplied startup warning (e.g. debug-log file
    // failures) with any config-parse failures we detect at startup.
    // `tracing::warn!` events from the `_or_warn` config helpers are dropped
    // by default in TUI mode (no subscriber attached), so we surface them
    // through the same InfoDialog channel here.
    //
    // Detected before `App::new` so we can suppress the first-run welcome /
    // changelog dialogs when there's a warning, both for UX (the warning is
    // the more important thing for the user to see) and to avoid overwriting
    // a malformed config.toml with defaults via `save_config`.
    let combined_warning = match (
        startup_warning,
        crate::session::collect_startup_config_warnings(profile),
    ) {
        (Some(a), Some(b)) => Some(format!("{a}\n\n{b}")),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    };

    // Create app and run
    let mut app = App::new(profile, available_tools, combined_warning.is_some())?;
    if let Some(warning) = combined_warning {
        app.show_startup_warning(&warning);
    }
    let result = app.run(&mut terminal).await;

    crate::session::clear_tui_heartbeat();

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableBracketedPaste
    )?;
    if mouse_capture_requested() && !mosh_active {
        execute!(terminal.backend_mut(), DisableMouseCapture)?;
    }
    terminal.show_cursor()?;

    result
}
