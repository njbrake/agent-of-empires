//! Main TUI application

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;
use std::time::Duration;

use super::home::HomeView;
use super::styles::Theme;
use crate::session::{get_update_settings, load_config, save_config, Storage};
use crate::tmux::AvailableTools;
use crate::update::{check_for_update, UpdateInfo};

pub struct App {
    home: HomeView,
    should_quit: bool,
    theme: Theme,
    needs_redraw: bool,
    update_info: Option<UpdateInfo>,
    update_rx: Option<tokio::sync::oneshot::Receiver<anyhow::Result<UpdateInfo>>>,
}

impl App {
    pub fn new(profile: &str, available_tools: AvailableTools) -> Result<Self> {
        let storage = Storage::new(profile)?;
        let mut home = HomeView::new(storage, available_tools)?;
        let theme = Theme::default();

        // Check if we need to show welcome or changelog dialogs
        let mut config = load_config()?.unwrap_or_default();
        let current_version = env!("CARGO_PKG_VERSION").to_string();

        if !config.app_state.has_seen_welcome {
            home.show_welcome();
            config.app_state.has_seen_welcome = true;
            config.app_state.last_seen_version = Some(current_version);
            save_config(&config)?;
        } else if config.app_state.last_seen_version.as_deref() != Some(&current_version) {
            home.show_changelog(config.app_state.last_seen_version.clone());
            config.app_state.last_seen_version = Some(current_version);
            save_config(&config)?;
        }

        Ok(Self {
            home,
            should_quit: false,
            theme,
            needs_redraw: true,
            update_info: None,
            update_rx: None,
        })
    }

    pub async fn run(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> Result<()> {
        // Initial render
        terminal.clear()?;
        terminal.draw(|f| self.render(f))?;

        // Refresh tmux session cache
        crate::tmux::refresh_session_cache();

        // Spawn async update check
        let settings = get_update_settings();
        if settings.check_enabled {
            let (tx, rx) = tokio::sync::oneshot::channel();
            self.update_rx = Some(rx);
            tokio::spawn(async move {
                let version = env!("CARGO_PKG_VERSION");
                let _ = tx.send(check_for_update(version, false).await);
            });
        }

        let mut last_status_refresh = std::time::Instant::now();
        let mut last_disk_refresh = std::time::Instant::now();
        const STATUS_REFRESH_INTERVAL: Duration = Duration::from_millis(500);
        const DISK_REFRESH_INTERVAL: Duration = Duration::from_secs(5);

        loop {
            // Force full redraw if needed (e.g., after returning from tmux)
            if self.needs_redraw {
                terminal.clear()?;
                self.needs_redraw = false;
            }

            // Poll with short timeout for responsive input
            if event::poll(Duration::from_millis(50))? {
                if let Event::Key(key) = event::read()? {
                    self.handle_key(key, terminal).await?;

                    // Draw immediately after input for responsiveness
                    terminal.draw(|f| self.render(f))?;

                    if self.should_quit {
                        break;
                    }
                    continue; // Skip status refresh this iteration for responsiveness
                }
            }

            // Check for update result (non-blocking)
            if self.poll_update_check() {
                self.needs_redraw = true;
            }

            // Periodic refreshes (only when no input pending)
            let mut refresh_needed = false;

            // Request status refresh every interval (non-blocking)
            if last_status_refresh.elapsed() >= STATUS_REFRESH_INTERVAL {
                self.home.request_status_refresh();
                last_status_refresh = std::time::Instant::now();
            }

            // Always check for and apply status updates (non-blocking)
            if self.home.apply_status_updates() {
                refresh_needed = true;
            }

            // Check for and apply deletion results (non-blocking)
            if self.home.apply_deletion_results() {
                refresh_needed = true;
            }

            // Periodic disk refresh to sync with other instances
            if last_disk_refresh.elapsed() >= DISK_REFRESH_INTERVAL {
                self.home.reload()?;
                last_disk_refresh = std::time::Instant::now();
                refresh_needed = true;
            }

            // Single draw after all refreshes to avoid flicker
            if refresh_needed {
                terminal.draw(|f| self.render(f))?;
            }

            if self.should_quit {
                break;
            }
        }

        Ok(())
    }

    fn render(&mut self, frame: &mut Frame) {
        self.home
            .render(frame, frame.area(), &self.theme, self.update_info.as_ref());
    }

    /// Poll for update check result (non-blocking).
    /// Returns true if an update is available and was just received.
    fn poll_update_check(&mut self) -> bool {
        let (update_info, update_rx, received) =
            poll_update_receiver(self.update_rx.take(), self.update_info.take());
        self.update_info = update_info;
        self.update_rx = update_rx;
        received
    }
}

/// Polls the update receiver and returns the new state.
/// Returns (update_info, update_rx, was_update_received).
fn poll_update_receiver(
    rx: Option<tokio::sync::oneshot::Receiver<anyhow::Result<UpdateInfo>>>,
    current_info: Option<UpdateInfo>,
) -> (
    Option<UpdateInfo>,
    Option<tokio::sync::oneshot::Receiver<anyhow::Result<UpdateInfo>>>,
    bool,
) {
    if let Some(mut rx) = rx {
        match rx.try_recv() {
            Ok(result) => {
                if let Ok(info) = result {
                    if info.available {
                        return (Some(info), None, true);
                    }
                }
                (current_info, None, false)
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                (current_info, Some(rx), false)
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => (current_info, None, false),
        }
    } else {
        (current_info, None, false)
    }
}

impl App {
    async fn handle_key(
        &mut self,
        key: KeyEvent,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> Result<()> {
        // Global keybindings
        match (key.code, key.modifiers) {
            (KeyCode::Char('c'), KeyModifiers::CONTROL) | (KeyCode::Char('q'), _) => {
                if !self.home.has_dialog() {
                    self.should_quit = true;
                    return Ok(());
                }
            }
            _ => {}
        }

        // Delegate to home view
        if let Some(action) = self.home.handle_key(key) {
            match action {
                Action::Quit => self.should_quit = true,
                Action::AttachSession(id) => {
                    self.attach_session(&id, terminal)?;
                }
                Action::AttachTerminal(id) => {
                    self.attach_terminal(&id, terminal)?;
                }
                Action::SwitchProfile(profile) => {
                    let storage = Storage::new(&profile)?;
                    let tools = self.home.available_tools();
                    self.home = HomeView::new(storage, tools)?;
                }
            }
        }

        Ok(())
    }

    fn attach_session(
        &mut self,
        session_id: &str,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> Result<()> {
        let instance = match self.home.get_instance(session_id) {
            Some(inst) => inst.clone(),
            None => return Ok(()),
        };

        let tmux_session = instance.tmux_session()?;

        if !tmux_session.exists() {
            let mut inst = instance.clone();
            if let Err(e) = inst.start() {
                self.home
                    .set_instance_error(session_id, Some(e.to_string()));
                return Ok(());
            }
            self.home.set_instance_error(session_id, None);
        }

        // Leave TUI mode completely
        crossterm::terminal::disable_raw_mode()?;
        crossterm::execute!(
            terminal.backend_mut(),
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::event::DisableMouseCapture,
            crossterm::cursor::Show
        )?;
        std::io::Write::flush(terminal.backend_mut())?;

        // Attach to tmux session (this blocks until user detaches with Ctrl+b d)
        let attach_result = tmux_session.attach();

        // Re-enter TUI mode
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(
            terminal.backend_mut(),
            crossterm::terminal::EnterAlternateScreen,
            crossterm::event::EnableMouseCapture,
            crossterm::cursor::Hide
        )?;
        std::io::Write::flush(terminal.backend_mut())?;

        // Drain any stale events that accumulated during tmux session
        while event::poll(Duration::from_millis(0))? {
            let _ = event::read();
        }

        // Force terminal to clear and redraw completely
        terminal.clear()?;
        self.needs_redraw = true;

        // Refresh session state since things may have changed
        crate::tmux::refresh_session_cache();
        self.home.reload()?;
        self.home.select_session_by_id(session_id);

        // Log any attach errors but don't fail
        if let Err(e) = attach_result {
            tracing::warn!("tmux attach returned error: {}", e);
        }

        Ok(())
    }

    fn attach_terminal(
        &mut self,
        session_id: &str,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> Result<()> {
        let instance = match self.home.get_instance(session_id) {
            Some(inst) => inst.clone(),
            None => return Ok(()),
        };

        let terminal_session = instance.terminal_tmux_session()?;

        if !terminal_session.exists() {
            // Start the terminal (creates tmux session and updates terminal_info)
            if let Err(e) = self.home.start_terminal_for_instance(session_id) {
                self.home
                    .set_instance_error(session_id, Some(e.to_string()));
                return Ok(());
            }
        }

        // Leave TUI mode completely
        crossterm::terminal::disable_raw_mode()?;
        crossterm::execute!(
            terminal.backend_mut(),
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::event::DisableMouseCapture,
            crossterm::cursor::Show
        )?;
        std::io::Write::flush(terminal.backend_mut())?;

        // Attach to terminal session (this blocks until user detaches with Ctrl+b d)
        let attach_result = terminal_session.attach();

        // Re-enter TUI mode
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(
            terminal.backend_mut(),
            crossterm::terminal::EnterAlternateScreen,
            crossterm::event::EnableMouseCapture,
            crossterm::cursor::Hide
        )?;
        std::io::Write::flush(terminal.backend_mut())?;

        // Drain any stale events that accumulated during tmux session
        while event::poll(Duration::from_millis(0))? {
            let _ = event::read();
        }

        // Force terminal to clear and redraw completely
        terminal.clear()?;
        self.needs_redraw = true;

        // Refresh session state since things may have changed
        crate::tmux::refresh_session_cache();
        self.home.reload()?;
        self.home.select_session_by_id(session_id);

        // Log any attach errors but don't fail
        if let Err(e) = attach_result {
            tracing::warn!("tmux terminal attach returned error: {}", e);
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Quit,
    AttachSession(String),
    AttachTerminal(String),
    SwitchProfile(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_enum() {
        let quit = Action::Quit;
        let attach = Action::AttachSession("test-id".to_string());

        assert_eq!(quit, Action::Quit);
        assert_eq!(attach, Action::AttachSession("test-id".to_string()));
    }

    #[test]
    fn test_action_clone() {
        let original = Action::AttachSession("session-123".to_string());
        let cloned = original.clone();
        assert_eq!(original, cloned);
    }

    #[test]
    fn test_poll_update_check_returns_true_when_update_available() {
        // Create a oneshot channel and send an update notification
        let (tx, rx) = tokio::sync::oneshot::channel();
        let update_info = UpdateInfo {
            available: true,
            current_version: "0.4.0".to_string(),
            latest_version: "0.5.0".to_string(),
        };
        tx.send(Ok(update_info)).unwrap();

        // poll_update_receiver should return true when an update is available
        let (info, rx_out, received) = poll_update_receiver(Some(rx), None);
        assert!(received);
        assert!(info.is_some());
        assert_eq!(info.as_ref().unwrap().latest_version, "0.5.0");
        assert!(rx_out.is_none()); // Channel consumed
    }

    #[test]
    fn test_poll_update_check_returns_false_when_no_update() {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let update_info = UpdateInfo {
            available: false,
            current_version: "0.5.0".to_string(),
            latest_version: "0.5.0".to_string(),
        };
        tx.send(Ok(update_info)).unwrap();

        // poll_update_receiver should return false when no update available
        let (info, rx_out, received) = poll_update_receiver(Some(rx), None);
        assert!(!received);
        assert!(info.is_none());
        assert!(rx_out.is_none()); // Channel consumed even though no update
    }

    #[test]
    fn test_poll_update_check_returns_false_when_channel_empty() {
        let (_tx, rx) = tokio::sync::oneshot::channel::<anyhow::Result<UpdateInfo>>();

        // poll_update_receiver should return false when channel is empty
        let (info, rx_out, received) = poll_update_receiver(Some(rx), None);
        assert!(!received);
        assert!(info.is_none());
        // Receiver should be put back for next poll
        assert!(rx_out.is_some());
    }

    #[test]
    fn test_poll_update_check_preserves_existing_info() {
        // If we already have update info and the channel is closed, preserve the existing info
        let existing_info = UpdateInfo {
            available: true,
            current_version: "0.4.0".to_string(),
            latest_version: "0.5.0".to_string(),
        };

        // No receiver, just existing info
        let (info, rx_out, received) = poll_update_receiver(None, Some(existing_info));
        assert!(!received); // No new update received
        assert!(info.is_some()); // But existing info is preserved
        assert_eq!(info.as_ref().unwrap().latest_version, "0.5.0");
        assert!(rx_out.is_none());
    }
}
