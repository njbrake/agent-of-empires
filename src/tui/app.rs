//! Main TUI application

use anyhow::Result;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event,
    EventStream, KeyCode, KeyEvent, KeyModifiers, MouseEventKind,
};
use futures_util::StreamExt;
use ratatui::prelude::*;
use std::path::PathBuf;
use std::time::Duration;

use super::home::{HomeView, TerminalMode};
use super::styles::Theme;
use crate::session::{get_update_settings, load_config, save_config, Config};
use crate::tmux::AvailableTools;
use crate::update::{check_for_update, UpdateInfo};

struct UpdateStatus {
    text: String,
    expires_at: Option<std::time::Instant>,
}

impl UpdateStatus {
    fn persistent(text: String) -> Self {
        Self {
            text,
            expires_at: None,
        }
    }

    fn transient(text: String) -> Self {
        Self {
            text,
            expires_at: Some(std::time::Instant::now() + std::time::Duration::from_secs(10)),
        }
    }

    fn is_expired(&self) -> bool {
        match self.expires_at {
            Some(deadline) => std::time::Instant::now() >= deadline,
            None => false,
        }
    }
}

pub struct App {
    home: HomeView,
    should_quit: bool,
    theme: Theme,
    needs_redraw: bool,
    update_info: Option<UpdateInfo>,
    update_rx: Option<tokio::sync::oneshot::Receiver<anyhow::Result<UpdateInfo>>>,
    update_status: Option<UpdateStatus>,
    update_status_rx: Option<tokio::sync::oneshot::Receiver<anyhow::Result<()>>>,
    /// User dismissed the update bar this session. Resets when the
    /// process exits; on next launch the startup check runs again and
    /// the bar reappears if an update is still available. In-memory
    /// only — no config persistence.
    update_bar_dismissed: bool,
    /// Held in an Option so `with_raw_mode_disabled` can drop it before
    /// spawning child processes. Crossterm's EventStream runs a background
    /// reader thread on stdin; if it's alive when tmux attach-session starts,
    /// the two compete for stdin and tmux fails to initialize its client.
    event_stream: Option<EventStream>,
    /// Tracks whether we currently have xterm mouse-tracking enabled. The TUI
    /// turns it off while a copy-friendly surface is open (`HomeView::
    /// wants_text_selection`) so users can drag-select natively, then turns
    /// it back on when the surface dismisses. Default true to match the
    /// startup `EnableMouseCapture` in `tui::run`.
    mouse_captured: bool,
}

/// Check if the app version changed and return the previous version if changelog should be shown.
/// This is called before App::new to allow async cache refresh.
pub fn check_version_change() -> Result<Option<String>> {
    let config = Config::load_or_warn();
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
    pub fn new(
        profile: &str,
        available_tools: AvailableTools,
        suppress_first_run_dialogs: bool,
    ) -> Result<Self> {
        let no_agents = !available_tools.any_available();
        let active_profile = if profile.is_empty() {
            None // all-profiles mode
        } else {
            Some(profile.to_string())
        };
        let mut home = HomeView::new(active_profile, available_tools)?;

        // Check if we need to show welcome or changelog dialogs
        let mut config = Config::load_or_warn();

        // Load theme from config, defaulting to empire if empty
        let theme_name = if config.theme.name.is_empty() {
            "empire"
        } else {
            &config.theme.name
        };
        let palette_mode = matches!(
            config.theme.color_mode,
            crate::session::config::ColorMode::Palette
        );
        let theme = crate::tui::styles::load_theme_with_mode(theme_name, palette_mode);
        let current_version = env!("CARGO_PKG_VERSION").to_string();

        if no_agents {
            // Show the no-agents onboarding dialog (takes priority over welcome/changelog)
            home.show_no_agents();
        } else if suppress_first_run_dialogs {
            // A startup warning will be shown by the caller; skip welcome and
            // changelog so the warning is what the user sees first, and avoid
            // overwriting a malformed config.toml with defaults via save_config.
        } else if !config.app_state.has_seen_welcome {
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
            update_status: None,
            update_status_rx: None,
            update_bar_dismissed: false,
            event_stream: Some(EventStream::new()),
            mouse_captured: true,
        })
    }

    /// Turn xterm mouse tracking on or off to match the current view state.
    ///
    /// **Contract**: must be called after any handler that may open or close
    /// a surface counted by `HomeView::wants_text_selection`. Currently the
    /// event-loop `Event::Key` arm and the tail of `with_raw_mode_disabled`
    /// cover this; new event sources that mutate dialog state need to call
    /// this too or mouse capture will lag a frame behind reality.
    fn sync_mouse_capture(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> Result<()> {
        let desired = !self.home.wants_text_selection();
        if desired == self.mouse_captured {
            return Ok(());
        }
        if desired {
            crossterm::execute!(terminal.backend_mut(), EnableMouseCapture)?;
        } else {
            crossterm::execute!(terminal.backend_mut(), DisableMouseCapture)?;
        }
        self.mouse_captured = desired;
        Ok(())
    }

    /// Temporarily leave TUI mode, run a closure, and restore TUI mode.
    /// Drops the EventStream before the closure so child processes (tmux,
    /// editors) have exclusive access to stdin, then creates a fresh one.
    fn with_raw_mode_disabled<F, R>(
        &mut self,
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
            DisableBracketedPaste,
            DisableMouseCapture,
            crossterm::cursor::Show
        )?;
        self.mouse_captured = false;
        std::io::Write::flush(terminal.backend_mut())?;

        // Drop the event stream so its background reader releases stdin.
        // Without this, tmux attach-session fails because crossterm's
        // reader thread competes for stdin reads.
        self.event_stream.take();

        let result = f();

        // Recreate the event stream with a fresh reader before re-entering
        // the event loop.
        self.event_stream = Some(EventStream::new());

        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(
            terminal.backend_mut(),
            crossterm::terminal::EnterAlternateScreen,
            EnableBracketedPaste,
            crossterm::cursor::Hide
        )?;
        // Defer mouse-capture restore to sync_mouse_capture so we don't
        // briefly enable it only to disable again when the user returned
        // to the serve view.
        self.sync_mouse_capture(terminal)?;
        std::io::Write::flush(terminal.backend_mut())?;

        terminal.clear()?;

        Ok(result)
    }

    pub fn show_startup_warning(&mut self, message: &str) {
        // Size the dialog to fit the message after wrapping. The message area
        // is `width - 4` columns wide (borders + 1-cell margin on each side),
        // so each \n-separated line wraps to ceil(len / inner_width) visual
        // rows. Borders + margin + the OK button take 6 rows.
        const WIDTH: u16 = 80;
        let inner_width = WIDTH.saturating_sub(4) as usize;
        let visual_lines: usize = message
            .lines()
            .map(|l| {
                if l.is_empty() {
                    1
                } else {
                    l.len().div_ceil(inner_width)
                }
            })
            .sum();
        // +6 for borders/margin/button; +1 safety margin since byte-length
        // wrap estimation under-counts when Paragraph word-wraps mid-line.
        let height = ((visual_lines as u16).saturating_add(7)).clamp(9, 35);
        // Warnings preempt onboarding dialogs so the user sees the problem
        // before the welcome screen.
        self.home.welcome_dialog = None;
        self.home.changelog_dialog = None;
        self.home.info_dialog =
            Some(crate::tui::dialogs::InfoDialog::new("Warning", message).with_size(WIDTH, height));
    }

    pub fn set_theme(&mut self, name: &str) {
        let palette_mode = load_config()
            .ok()
            .flatten()
            .map(|c| {
                matches!(
                    c.theme.color_mode,
                    crate::session::config::ColorMode::Palette
                )
            })
            .unwrap_or(false);
        self.theme = crate::tui::styles::load_theme_with_mode(name, palette_mode);
        self.needs_redraw = true;
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

        // SIGHUP/SIGTERM futures so we exit cleanly when the terminal
        // emulator is force-quit, preventing PTY slot leaks (#541).
        // These are polled directly inside tokio::select!, which guarantees
        // they get scheduled even when no terminal events arrive.
        #[cfg(unix)]
        let (mut sighup, mut sigterm) = {
            use tokio::signal::unix::{signal, SignalKind};
            let hup = signal(SignalKind::hangup());
            let term = signal(SignalKind::terminate());
            if let Err(ref e) = hup {
                tracing::warn!("Failed to register SIGHUP handler: {}", e);
            }
            if let Err(ref e) = term {
                tracing::warn!("Failed to register SIGTERM handler: {}", e);
            }
            (hup.ok(), term.ok())
        };

        let mut refresh_interval = tokio::time::interval(Duration::from_millis(50));
        refresh_interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut last_status_refresh = std::time::Instant::now();
        let mut last_disk_refresh = std::time::Instant::now();
        let mut last_spinner_redraw = std::time::Instant::now();
        let mut last_heartbeat = std::time::Instant::now();
        const STATUS_REFRESH_INTERVAL: Duration = Duration::from_millis(500);
        const DISK_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
        // Fastest spinner (breathe) changes every 180ms; 120ms ensures smooth animation
        const SPINNER_REDRAW_INTERVAL: Duration = Duration::from_millis(120);
        const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);

        // Signal that the TUI is active so the web push consumer can
        // suppress notifications while the user is watching the dashboard.
        crate::session::write_tui_heartbeat();

        loop {
            // Force full redraw if needed (e.g., after returning from tmux).
            // with_raw_mode_disabled drops and recreates the EventStream, so
            // there are no stale events to drain.
            if self.needs_redraw {
                terminal.clear()?;
                self.needs_redraw = false;
            }

            // All event sources are polled cooperatively via tokio::select!.
            // This ensures signal futures actually get scheduled (fixing #608
            // defect 1), and that EOF from a dead tty is detected (defect 2).
            tokio::select! {
                event = self.event_stream.as_mut().expect("event_stream missing").next() => {
                    match event {
                        Some(Ok(Event::Key(key))) => {
                            self.handle_key(key, terminal).await?;
                            self.sync_mouse_capture(terminal)?;

                            // Skip the draw when returning from tmux attach.
                            // needs_redraw triggers a clear + stale event drain
                            // on the next iteration; drawing before that drain
                            // wastes a frame and can flicker.
                            if !self.needs_redraw {
                                terminal.draw(|f| self.render(f))?;
                            }

                            if self.should_quit {
                                break;
                            }
                            continue;
                        }
                        Some(Ok(Event::Mouse(mouse))) => {
                            let hit_scroll_target = if self.home.is_diff_open() {
                                self.home.hit_diff(mouse.column, mouse.row)
                            } else if self.home.has_selected_session() {
                                self.home.hit_preview(mouse.column, mouse.row)
                            } else {
                                false
                            };
                            let handled = match mouse.kind {
                                MouseEventKind::ScrollUp if hit_scroll_target => self.home.handle_scroll_up(),
                                MouseEventKind::ScrollDown if hit_scroll_target => self.home.handle_scroll_down(),
                                _ => false,
                            };
                            if handled {
                                terminal.draw(|f| self.render(f))?;
                            }
                            continue;
                        }
                        Some(Ok(Event::Paste(text))) => {
                            self.home.handle_paste(&text);

                            terminal.draw(|f| self.render(f))?;

                            continue;
                        }
                        Some(Ok(Event::Resize(_, _))) => {
                            // Soft keyboard slides up/down on iPad/iPhone Mosh
                            // (and ordinary terminal resizes) emit Resize. The
                            // catch-all below would silently drop them, leaving
                            // the screen mid-stale until the next refresh tick.
                            // Redraw now so viewport-driven layout
                            // (responsive::dialog_width, STACKED_BREAKPOINT,
                            // etc.) re-evaluates; ratatui's draw() autoresizes
                            // internally before rendering.
                            terminal.draw(|f| self.render(f))?;
                            continue;
                        }
                        Some(Ok(_)) => {}
                        Some(Err(e)) => {
                            // IO error reading from the terminal (broken pipe,
                            // EOF, etc.) means the tty is gone. Exit cleanly
                            // instead of spinning (#608 defect 2).
                            tracing::info!("Terminal event stream error, exiting: {}", e);
                            self.should_quit = true;
                            break;
                        }
                        None => {
                            // EventStream ended (EOF on stdin). The terminal is
                            // gone; exit instead of busy-looping (#608 defect 2).
                            tracing::info!("Terminal event stream ended (EOF), exiting");
                            self.should_quit = true;
                            break;
                        }
                    }
                }
                _ = refresh_interval.tick() => {}
                _ = async {
                    #[cfg(unix)]
                    match sighup {
                        Some(ref mut s) => { s.recv().await; }
                        None => { std::future::pending::<()>().await; }
                    }
                    #[cfg(not(unix))]
                    std::future::pending::<()>().await;
                } => {
                    tracing::info!("Received SIGHUP, exiting");
                    self.should_quit = true;
                    break;
                }
                _ = async {
                    #[cfg(unix)]
                    match sigterm {
                        Some(ref mut s) => { s.recv().await; }
                        None => { std::future::pending::<()>().await; }
                    }
                    #[cfg(not(unix))]
                    std::future::pending::<()>().await;
                } => {
                    tracing::info!("Received SIGTERM, exiting");
                    self.should_quit = true;
                    break;
                }
            }

            // Check for update result (non-blocking)
            if self.poll_update_check() {
                self.needs_redraw = true;
            }

            // Check for in-progress update completion
            if self.poll_update_status() {
                self.needs_redraw = true;
            }

            // Periodic refreshes (only when no input pending)
            let mut refresh_needed = false;

            if last_status_refresh.elapsed() >= STATUS_REFRESH_INTERVAL {
                self.home.request_status_refresh();
                last_status_refresh = std::time::Instant::now();
            }

            if self.home.apply_status_updates() {
                refresh_needed = true;
            }

            if self.home.apply_deletion_results() {
                refresh_needed = true;
            }

            if self.home.apply_session_id_updates() {
                refresh_needed = true;
            }

            if let Some(session_id) = self.home.apply_creation_results() {
                self.attach_session(&session_id, terminal)?;
                refresh_needed = true;
            }

            if self.home.tick_dialog() {
                refresh_needed = true;
            }

            if last_disk_refresh.elapsed() >= DISK_REFRESH_INTERVAL {
                self.home.reload()?;
                last_disk_refresh = std::time::Instant::now();
                refresh_needed = true;
            }

            if last_heartbeat.elapsed() >= HEARTBEAT_INTERVAL {
                crate::session::write_tui_heartbeat();
                last_heartbeat = std::time::Instant::now();
            }

            // Animated spinners (rattles) need periodic redraws, but only at
            // the spinner frame rate to avoid unnecessary widget tree rebuilds
            if last_spinner_redraw.elapsed() >= SPINNER_REDRAW_INTERVAL
                && self.home.has_animated_sessions()
            {
                last_spinner_redraw = std::time::Instant::now();
                refresh_needed = true;
            }

            if refresh_needed {
                terminal.draw(|f| self.render(f))?;
            }

            if self.should_quit {
                break;
            }
        }

        self.home.apply_session_id_updates();
        self.home.cleanup_pending_creation();

        if let Err(e) = self.home.save() {
            tracing::error!("Failed to save on quit: {}", e);
        }

        Ok(())
    }

    fn render(&mut self, frame: &mut Frame) {
        if self.update_status.as_ref().is_some_and(|s| s.is_expired()) {
            self.update_status = None;
        }
        let status_text = self.update_status.as_ref().map(|s| s.text.as_str());
        self.home.render(
            frame,
            frame.area(),
            &self.theme,
            self.update_info.as_ref(),
            status_text,
        );
    }

    /// Poll for update check result (non-blocking).
    /// Returns true if an update is available and was just received.
    fn poll_update_check(&mut self) -> bool {
        let (update_info, update_rx, received) =
            poll_update_receiver(self.update_rx.take(), self.update_info.take());
        self.update_info = update_info;
        self.update_rx = update_rx;
        // If the user dismissed the bar this session, drop the info so
        // render() and the `u` hotkey both see None. Reset on next launch.
        if received && self.update_bar_dismissed {
            self.update_info = None;
            return false;
        }
        received
    }

    /// Poll the in-progress update task for completion.
    /// Returns true when the status line changed and a redraw is needed.
    fn poll_update_status(&mut self) -> bool {
        let Some(mut rx) = self.update_status_rx.take() else {
            return false;
        };
        match rx.try_recv() {
            Ok(Ok(())) => {
                self.update_status = Some(UpdateStatus::persistent(
                    "update complete. Restart aoe to use the new version.".into(),
                ));
                true
            }
            Ok(Err(e)) => {
                self.update_status = Some(UpdateStatus::transient(format!("update failed: {e}")));
                true
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {
                self.update_status_rx = Some(rx);
                false
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                self.update_status = Some(UpdateStatus::transient(
                    "update task ended unexpectedly".into(),
                ));
                true
            }
        }
    }

    /// Dispatch the confirmed update, choosing between a blocking suspend and a
    /// background tokio task based on whether the method requires sudo.
    fn spawn_update(
        &mut self,
        method: crate::update::install::InstallMethod,
        version: String,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> Result<()> {
        use crate::update::install::InstallMethod;

        let needs_sudo = matches!(
            &method,
            InstallMethod::Tarball { binary_path }
                if !crate::update::install::parent_is_writable(binary_path)
        );

        if matches!(method, InstallMethod::Homebrew) || needs_sudo {
            // Suspend the TUI so sudo's password prompt can use the terminal.
            self.update_status = Some(UpdateStatus::transient(format!("updating to v{version}…")));
            let method_clone = method.clone();
            let version_clone = version.clone();
            let result = self.with_raw_mode_disabled(terminal, move || {
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(async {
                        crate::update::install::perform_update(&method_clone, &version_clone, None)
                            .await
                    })
                })
            })?;
            match result {
                Ok(()) => {
                    self.update_status = Some(UpdateStatus::persistent(
                        "update complete. Restart aoe to use the new version.".into(),
                    ));
                }
                Err(e) => {
                    self.update_status =
                        Some(UpdateStatus::transient(format!("update failed: {e}")));
                }
            }
        } else {
            // Background task for writable tarball installs.
            // `perform_update`'s future is !Send because its `on_progress` parameter is
            // `Option<&mut dyn FnMut(...)>` (no Send bound on the trait object), so
            // `tokio::spawn` won't accept it. A std::thread + Handle::block_on lets the
            // async I/O still use the existing tokio runtime while sidestepping the
            // Send constraint.
            self.update_status = Some(UpdateStatus::transient(format!("updating to v{version}…")));
            let (tx, rx) = tokio::sync::oneshot::channel();
            self.update_status_rx = Some(rx);
            let handle = tokio::runtime::Handle::current();
            std::thread::spawn(move || {
                let result = handle.block_on(crate::update::install::perform_update(
                    &method, &version, None,
                ));
                let _ = tx.send(result);
            });
        }
        Ok(())
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
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                if self.home.is_creating_stub_selected() {
                    self.home.cancel_creation();
                    return Ok(());
                }
                if self.home.is_creation_pending() && !self.home.has_dialog() {
                    self.home.show_quit_during_creation_confirm();
                    return Ok(());
                }
                self.should_quit = true;
                return Ok(());
            }
            (KeyCode::Char('q'), _) if !self.home.has_dialog() => {
                if self.home.is_creation_pending() {
                    self.home.show_quit_during_creation_confirm();
                    return Ok(());
                }
                self.should_quit = true;
                return Ok(());
            }
            // Ctrl+x dismisses the update bar / status toast for this
            // session. Gated on something being visible AND no dialog
            // open so it doesn't fire during dialog input. On next launch
            // the startup check runs again and the bar reappears if the
            // update is still available.
            (KeyCode::Char('x'), KeyModifiers::CONTROL)
                if (self.update_info.is_some() || self.update_status.is_some())
                    && !self.home.has_dialog() =>
            {
                self.update_info = None;
                self.update_status = None;
                self.update_bar_dismissed = true;
                self.needs_redraw = true;
                return Ok(());
            }
            _ => {}
        }

        if let Some(action) = self.home.handle_key(key, self.update_info.as_ref()) {
            self.execute_action(action, terminal)?;
        }

        Ok(())
    }

    fn execute_action(
        &mut self,
        action: Action,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> Result<()> {
        match action {
            Action::Quit => self.should_quit = true,
            Action::AttachSession(id) => {
                self.attach_session(&id, terminal)?;
            }
            Action::AttachTerminal(id, mode) => {
                self.attach_terminal(&id, mode, terminal)?;
            }
            Action::EditFile(path) => {
                self.edit_file(&path, terminal)?;
            }
            Action::StopSession(id) => {
                if let Some(inst) = self.home.get_instance(&id) {
                    let inst_clone = inst.clone();
                    // Set Stopped immediately so the status poller won't
                    // override to Error while stop() blocks (docker stop
                    // can take up to 10s).
                    self.home
                        .set_instance_status(&id, crate::session::Status::Stopped);
                    match inst_clone.stop() {
                        Ok(()) => {
                            crate::tmux::refresh_session_cache();
                            self.home.reload()?;
                            self.home
                                .set_instance_status(&id, crate::session::Status::Stopped);
                            self.home.save()?;
                        }
                        Err(e) => {
                            tracing::error!("Failed to stop session: {}", e);
                            self.home.set_instance_error(&id, Some(e.to_string()));
                            self.home
                                .set_instance_status(&id, crate::session::Status::Error);
                            self.home.save()?;
                        }
                    }
                }
            }
            Action::SetTheme(name) => {
                self.set_theme(&name);
            }
            Action::SpawnUpdate(method, version) => {
                if self.update_status_rx.is_some() {
                    self.update_status =
                        Some(UpdateStatus::transient("update already in progress".into()));
                    return Ok(());
                }
                self.spawn_update(method, version, terminal)?;
            }
            Action::SetTransientStatus(text) => {
                self.update_status = Some(UpdateStatus::transient(text));
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

        // Decide whether to restart: if hook status is available or the instance
        // uses a custom command, trust that over shell detection. Wrapper scripts
        // (Devbox, version managers, custom command overrides) run agents via a
        // shell process, so is_pane_running_shell() returns true even when the
        // agent is healthy.
        let exists = tmux_session.exists();
        let pane_dead = if exists {
            tmux_session.is_pane_dead()
        } else {
            false
        };
        let needs_restart = if !exists || pane_dead {
            true
        } else if crate::hooks::read_hook_status(&instance.id).is_some() {
            // Hook status is tracking this session; shell detection is unreliable
            false
        } else if instance.has_command_override() {
            // Custom command overrides run agents through wrapper scripts that
            // appear as shell processes to tmux. Don't restart based on shell
            // detection. (extra_args alone should not suppress this check.)
            false
        } else {
            !instance.expects_shell() && tmux_session.is_pane_running_shell()
        };
        tracing::debug!(
            session_id,
            exists,
            pane_dead,
            needs_restart,
            "attach_session: restart decision"
        );
        if needs_restart {
            // Show warning (once) if custom instruction is configured for an unsupported agent
            if instance.is_sandboxed() {
                let has_instruction = instance
                    .sandbox_info
                    .as_ref()
                    .and_then(|s| s.custom_instruction.as_ref())
                    .is_some_and(|i| !i.is_empty());

                if has_instruction
                    && crate::agents::get_agent(&instance.tool)
                        .is_none_or(|a| a.instruction_flag.is_none())
                {
                    let config = Config::load_or_warn();
                    if !config.app_state.has_seen_custom_instruction_warning {
                        self.home.info_dialog = Some(
                            crate::tui::dialogs::InfoDialog::new(
                                "Custom Instruction Not Supported",
                                &format!(
                                    "'{}' does not support custom instruction injection. The session will launch without the custom instruction.",
                                    instance.tool
                                ),
                            ),
                        );
                        self.home.pending_attach_after_warning = Some(session_id.to_string());

                        // Persist the "seen" flag so it only shows once
                        let mut config = config;
                        config.app_state.has_seen_custom_instruction_warning = true;
                        save_config(&config)?;

                        return Ok(());
                    }
                }
            }

            // Get terminal size to pass to tmux session creation
            // This ensures the session starts at the correct size instead of 80x24 default
            let size = crate::terminal::get_size();

            // Skip on_launch hooks if they already ran in the background creation poller
            let skip_on_launch = self.home.take_on_launch_hooks_ran(session_id);

            self.home
                .set_instance_status(session_id, crate::session::Status::Starting);
            if let Err(e) =
                self.home
                    .restart_instance_with_size_opts(session_id, size, skip_on_launch)
            {
                self.home
                    .set_instance_error(session_id, Some(e.to_string()));
                self.home
                    .set_instance_status(session_id, crate::session::Status::Error);
                return Ok(());
            }
            self.home.set_instance_error(session_id, None);
        }

        let tmux_session = match self.home.get_instance(session_id) {
            Some(inst) => inst.tmux_session()?,
            None => return Ok(()),
        };
        let attach_result = self.with_raw_mode_disabled(terminal, || tmux_session.attach())?;

        self.needs_redraw = true;
        crate::tmux::refresh_session_cache();
        self.home.reload()?;
        self.home.stamp_last_accessed(session_id);
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
                if !container_session.exists() || container_session.is_pane_dead() {
                    if container_session.exists() {
                        let _ = container_session.kill();
                    }
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
                if !terminal_session.exists() || terminal_session.is_pane_dead() {
                    if terminal_session.exists() {
                        let _ = terminal_session.kill();
                    }
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

        let attach_result = self.with_raw_mode_disabled(terminal, attach_fn)?;

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
        let status = self.with_raw_mode_disabled(terminal, move || {
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
    EditFile(PathBuf),
    StopSession(String),
    SetTheme(String),
    SpawnUpdate(crate::update::install::InstallMethod, String),
    SetTransientStatus(String),
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
