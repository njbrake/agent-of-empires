//! Main TUI application

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;
use std::time::Duration;

use super::home::HomeView;
use super::styles::Theme;
use crate::session::{get_update_settings, Storage};
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
        let home = HomeView::new(storage, available_tools)?;
        let theme = Theme::default();

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
            if let Some(mut rx) = self.update_rx.take() {
                match rx.try_recv() {
                    Ok(result) => {
                        if let Ok(info) = result {
                            if info.available {
                                self.update_info = Some(info);
                            }
                        }
                    }
                    Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                        self.update_rx = Some(rx);
                    }
                    Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {}
                }
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

        // Log any attach errors but don't fail
        if let Err(e) = attach_result {
            tracing::warn!("tmux attach returned error: {}", e);
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Quit,
    AttachSession(String),
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
}
