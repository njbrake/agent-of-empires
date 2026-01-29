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
mod styles;

pub use app::*;

use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::io;

use crate::session::get_update_settings;
use crate::update::check_for_update;

pub async fn run(profile: &str) -> Result<()> {
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

    // Check for coding tools
    let available_tools = crate::tmux::AvailableTools::detect();
    if !available_tools.any_available() {
        eprintln!("Error: No coding tools found in PATH");
        eprintln!();
        eprintln!("Agent of Empires requires at least one of:");
        eprintln!("  claude    - Anthropic's Claude CLI");
        eprintln!("  opencode  - OpenCode CLI");
        eprintln!();
        eprintln!("Install one of these tools and ensure it's in your PATH.");
        std::process::exit(1);
    }

    // If version changed, refresh the update cache before showing TUI.
    // This ensures we have release notes for the changelog dialog.
    if check_version_change()?.is_some() {
        let settings = get_update_settings();
        if settings.check_enabled {
            let current_version = env!("CARGO_PKG_VERSION");
            let _ = check_for_update(current_version, true).await;
        }
    }

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app and run
    let mut app = App::new(profile, available_tools)?;
    let result = app.run(&mut terminal).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    result
}
