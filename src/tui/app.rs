//! Main TUI application

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent};
use ratatui::prelude::*;
use std::path::PathBuf;
use std::time::Duration;

use super::home::{HomeView, TerminalMode};
use super::styles::Theme;
use crate::session::{get_update_settings, load_config, save_config, Storage};
use crate::tmux::AvailableTools;
use crate::update::{check_for_update, UpdateInfo};

/// Temporarily leave TUI mode, run a closure, and restore TUI mode.
/// Drains stale events and clears the terminal on return.
fn with_raw_mode_disabled<F, R>(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    f: F,
) -> Result<R>
where
    F: FnOnce() -> R,
{
    crossterm::terminal::disable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture,
        crossterm::cursor::Show
    )?;
    std::io::Write::flush(terminal.backend_mut())?;

    let result = f();

    crossterm::terminal::enable_raw_mode()?;
    crossterm::execute!(
        terminal.backend_mut(),
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture,
        crossterm::cursor::Hide
    )?;
    std::io::Write::flush(terminal.backend_mut())?;

    while event::poll(Duration::from_millis(0))? {
        let _ = event::read();
    }

    terminal.clear()?;

    Ok(result)
}

pub struct App {
    home: HomeView,
    should_quit: bool,
    theme: Theme,
    needs_redraw: bool,
    update_info: Option<UpdateInfo>,
    update_rx: Option<tokio::sync::oneshot::Receiver<anyhow::Result<UpdateInfo>>>,
}

/// Check if the app version changed and return the previous version if changelog should be shown.
/// This is called before App::new to allow async cache refresh.
pub fn check_version_change() -> Result<Option<String>> {
    let config = load_config()?.unwrap_or_default();
    let current_version = env!("CARGO_PKG_VERSION");

    if config.app_state.has_seen_welcome
        && config.app_state.last_seen_version.as_deref() != Some(current_version)
    {
        Ok(config.app_state.last_seen_version)
    } else {
        Ok(None)
    }
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
            // Cache should already be refreshed by tui::run() before App::new
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
                match event::read()? {
                    Event::Key(key) => {
                        self.handle_key(key, terminal).await?;

                        // Draw immediately after input for responsiveness
                        terminal.draw(|f| self.render(f))?;

                        if self.should_quit {
                            break;
                        }
                        continue; // Skip status refresh this iteration for responsiveness
                    }
                    Event::Mouse(mouse) => {
                        self.handle_mouse(mouse, terminal).await?;

                        // Draw immediately after input for responsiveness
                        terminal.draw(|f| self.render(f))?;

                        continue;
                    }
                    _ => {}
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

            // Check for and apply creation results (non-blocking)
            if let Some(session_id) = self.home.apply_creation_results() {
                // Creation succeeded - attach to the new session
                self.attach_session(&session_id, terminal)?;
                refresh_needed = true;
            }

            // Tick the dialog spinner if loading
            if self.home.is_creation_pending() {
                self.home.tick_dialog();
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
                Action::AttachTerminal(id, mode) => {
                    self.attach_terminal(&id, mode, terminal)?;
                }
                Action::SwitchProfile(profile) => {
                    let storage = Storage::new(&profile)?;
                    let tools = self.home.available_tools();
                    self.home = HomeView::new(storage, tools)?;
                }
                Action::EditFile(path) => {
                    self.edit_file(&path, terminal)?;
                }
            }
        }

        Ok(())
    }

    async fn handle_mouse(
        &mut self,
        mouse: MouseEvent,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> Result<()> {
        // Delegate to home view
        if let Some(action) = self.home.handle_mouse(mouse) {
            match action {
                Action::Quit => self.should_quit = true,
                Action::AttachSession(id) => {
                    self.attach_session(&id, terminal)?;
                }
                Action::AttachTerminal(id, mode) => {
                    self.attach_terminal(&id, mode, terminal)?;
                }
                Action::SwitchProfile(profile) => {
                    let storage = Storage::new(&profile)?;
                    let tools = self.home.available_tools();
                    self.home = HomeView::new(storage, tools)?;
                }
                Action::EditFile(path) => {
                    self.edit_file(&path, terminal)?;
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
            // Get terminal size to pass to tmux session creation
            // This ensures the session starts at the correct size instead of 80x24 default
            let size = crate::terminal::get_size();

            // Skip on_launch hooks if they already ran in the background creation poller
            let skip_on_launch = self.home.take_on_launch_hooks_ran(session_id);

            let mut inst = instance.clone();
            if let Err(e) = inst.start_with_size_opts(size, skip_on_launch) {
                self.home
                    .set_instance_error(session_id, Some(e.to_string()));
                return Ok(());
            }
            self.home.set_instance_error(session_id, None);
        }

        let attach_result = with_raw_mode_disabled(terminal, || tmux_session.attach())?;

        self.needs_redraw = true;
        crate::tmux::refresh_session_cache();
        self.home.reload()?;
        self.home.select_session_by_id(session_id);

        if let Err(e) = attach_result {
            tracing::warn!("tmux attach returned error: {}", e);
        }

        Ok(())
    }

    fn attach_terminal(
        &mut self,
        session_id: &str,
        mode: TerminalMode,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> Result<()> {
        let instance = match self.home.get_instance(session_id) {
            Some(inst) => inst.clone(),
            None => return Ok(()),
        };

        // Get terminal size to pass to tmux session creation
        let size = crate::terminal::get_size();

        // Prepare the tmux session before leaving TUI mode
        let attach_fn: Box<dyn FnOnce() -> Result<()>> = match mode {
            TerminalMode::Container if instance.is_sandboxed() => {
                let container_session = instance.container_terminal_tmux_session()?;
                if !container_session.exists() {
                    if let Err(e) = self
                        .home
                        .start_container_terminal_for_instance_with_size(session_id, size)
                    {
                        self.home
                            .set_instance_error(session_id, Some(e.to_string()));
                        return Ok(());
                    }
                }
                Box::new(move || container_session.attach())
            }
            _ => {
                let terminal_session = instance.terminal_tmux_session()?;
                if !terminal_session.exists() {
                    if let Err(e) = self
                        .home
                        .start_terminal_for_instance_with_size(session_id, size)
                    {
                        self.home
                            .set_instance_error(session_id, Some(e.to_string()));
                        return Ok(());
                    }
                }
                Box::new(move || terminal_session.attach())
            }
        };

        let attach_result = with_raw_mode_disabled(terminal, attach_fn)?;

        self.needs_redraw = true;
        crate::tmux::refresh_session_cache();
        self.home.reload()?;
        self.home.select_session_by_id(session_id);

        if let Err(e) = attach_result {
            tracing::warn!("tmux terminal attach returned error: {}", e);
        }

        Ok(())
    }

    fn edit_file(
        &mut self,
        path: &std::path::Path,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> Result<()> {
        // Determine which editor to use (prefer vim, fall back to nano)
        let editor = std::env::var("EDITOR")
            .ok()
            .or_else(|| {
                // Check if vim is available
                if std::process::Command::new("vim")
                    .arg("--version")
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .is_ok()
                {
                    Some("vim".to_string())
                } else if std::process::Command::new("nano")
                    .arg("--version")
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status()
                    .is_ok()
                {
                    Some("nano".to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "vim".to_string());

        let path = path.to_owned();
        let editor_clone = editor.clone();
        let status = with_raw_mode_disabled(terminal, move || {
            std::process::Command::new(&editor_clone)
                .arg(&path)
                .status()
        })?;

        self.needs_redraw = true;

        // Refresh diff view if it's open (file may have changed)
        if let Some(ref mut diff_view) = self.home.diff_view {
            if let Err(e) = diff_view.refresh_files() {
                tracing::warn!("Failed to refresh diff after edit: {}", e);
            }
        }

        // Log any editor errors but don't fail
        if let Err(e) = status {
            tracing::warn!("Editor '{}' returned error: {}", editor, e);
        }

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Quit,
    AttachSession(String),
    AttachTerminal(String, TerminalMode),
    SwitchProfile(String),
    EditFile(PathBuf),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_action_enum() {
        let quit = Action::Quit;
        let attach = Action::AttachSession("test-id".to_string());
        let attach_terminal =
            Action::AttachTerminal("test-id".to_string(), TerminalMode::Container);

        assert_eq!(quit, Action::Quit);
        assert_eq!(attach, Action::AttachSession("test-id".to_string()));
        assert_eq!(
            attach_terminal,
            Action::AttachTerminal("test-id".to_string(), TerminalMode::Container)
        );
    }

    #[test]
    fn test_action_clone() {
        let original = Action::AttachSession("session-123".to_string());
        let cloned = original.clone();
        assert_eq!(original, cloned);

        let terminal_action = Action::AttachTerminal("session-123".to_string(), TerminalMode::Host);
        let terminal_cloned = terminal_action.clone();
        assert_eq!(terminal_action, terminal_cloned);
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
