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

use super::attached_status_hooks::AttachedStatusHookWatcher;
use super::home::{HomeView, TerminalMode};
use super::status_poller::StatusUpdate;
use super::styles::Theme;
use crate::session::{get_update_settings, save_config, Config};
use crate::tmux::AvailableTools;
use crate::update::{check_for_update, UpdateInfo};

/// Inter-key timeout for the paste-burst detector. After any printable Char
/// or Enter, the event loop polls for the next event with this timeout; if
/// another burst-candidate arrives before the deadline, it joins the burst.
/// Mosh strips bracketed-paste markers, so dictation from iOS clients lands
/// as a tightly-packed stream of individual key events; 5ms is comfortably
/// wider than a Mosh paste's inter-key gap and well under any human typing
/// rhythm, so it discriminates between paste and typing without making
/// single-key shortcuts feel laggy.
const PASTE_BURST_INTER_KEY_MS: u64 = 5;

/// Minimum length (in burst-candidate events) for an accumulated burst to be
/// routed through `handle_paste`. Shorter accumulations are replayed as
/// individual key events so genuine typing isn't mistaken for a paste.
const PASTE_BURST_MIN_LEN: usize = 3;

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
    /// Set by `Action::OpenCockpit` so the async main loop can pick it
    /// up and enter the cockpit view (which needs `event_stream` access
    /// the sync `execute_action` can't lend out).
    #[cfg(feature = "serve")]
    pending_cockpit_open: Option<String>,
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
    /// Is this key event a candidate for paste-burst accumulation?
    /// Printable ASCII Char or Enter, with no modifiers (or shift only).
    /// Burst detection ignores Ctrl/Alt-modified chords because those
    /// are genuine intentional shortcuts and never come from a paste-burst.
    /// Enter is included so embedded CR/LF inside a Mosh-stripped paste
    /// (voice/dictation often inserts sentence-break newlines) gets
    /// captured into the burst as \n instead of breaking it in two and
    /// firing Submit/select on the deferred Enter.
    fn is_burst_candidate(key: &KeyEvent) -> bool {
        let mods = key.modifiers;
        let mods_ok = mods.is_empty() || mods == KeyModifiers::SHIFT;
        if !mods_ok {
            return false;
        }
        match key.code {
            KeyCode::Char(c) => c == ' ' || c.is_ascii_graphic(),
            KeyCode::Enter => true,
            _ => false,
        }
    }

    /// Translate a burst-candidate key event back to its text byte for the
    /// accumulated burst string. Char yields the char; Enter yields '\n'.
    fn burst_char_for(key: &KeyEvent) -> Option<char> {
        match key.code {
            KeyCode::Char(c) => Some(c),
            KeyCode::Enter => Some('\n'),
            _ => None,
        }
    }

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

        // Load theme from config, defaulting to the `default` builtin if
        // empty so the TUI matches the web dashboard's empty-name fallback.
        let theme_name = if config.theme.name.is_empty() {
            "default"
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
            // Initial state matches whatever `tui::run` did at startup;
            // capture is on by default, off only if AOE_MOUSE_CAPTURE=0.
            mouse_captured: crate::tui::mouse_capture_requested(),
            #[cfg(feature = "serve")]
            pending_cockpit_open: None,
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
        // Mouse capture is on by default; AOE_MOUSE_CAPTURE=0 opts out so
        // iOS Mosh + Termius/Blink use the terminal app's native scrollback
        // for touch-scroll (Mosh doesn't reliably forward mouse-tracking
        // escapes to mobile clients).
        if !crate::tui::mouse_capture_requested() {
            return Ok(());
        }
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

    /// Draw a frame without exposing ratatui's intermediate cursor moves.
    ///
    /// The backend moves the real terminal cursor while flushing changed
    /// cells. If an IME is composing text, those transient moves can pull the
    /// candidate window toward refreshed UI such as the status list before the
    /// frame's final cursor position is restored. Synchronized update batches
    /// the frame, and hiding the cursor before the batch keeps the only visible
    /// cursor transition at ratatui's final `Frame::set_cursor_position`.
    fn draw(&mut self, terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
        crossterm::execute!(
            terminal.backend_mut(),
            crossterm::terminal::BeginSynchronizedUpdate
        )?;
        let draw_result = (|| -> Result<()> {
            crossterm::execute!(terminal.backend_mut(), crossterm::cursor::Hide)?;
            terminal.draw(|f| self.render(f)).map(|_| ())?;
            Ok(())
        })();
        let end_result = crossterm::execute!(
            terminal.backend_mut(),
            crossterm::terminal::EndSynchronizedUpdate
        );
        draw_result?;
        end_result?;
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
        )?;
        if crate::tui::mouse_capture_requested() {
            crossterm::execute!(terminal.backend_mut(), DisableMouseCapture)?;
        }
        crossterm::execute!(terminal.backend_mut(), crossterm::cursor::Show)?;
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
        // to the serve view. sync_mouse_capture itself respects the
        // AOE_MOUSE_CAPTURE opt-out.
        self.sync_mouse_capture(terminal)?;
        std::io::Write::flush(terminal.backend_mut())?;

        terminal.clear()?;

        Ok(result)
    }

    fn with_attached_status_hooks<F, R>(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
        f: F,
    ) -> Result<(R, Vec<StatusUpdate>)>
    where
        F: FnOnce() -> R,
    {
        let watcher = AttachedStatusHookWatcher::start(self.home.attached_status_hook_sessions());
        let result = self.with_raw_mode_disabled(terminal, f);
        let mut attached_status_updates = Vec::new();

        if let Some(watcher) = watcher {
            attached_status_updates = watcher.stop();
        }
        self.home.reset_status_refresh();

        result.map(|result| (result, attached_status_updates))
    }

    pub fn show_startup_warning(&mut self, message: &str) {
        // Size the dialog to fit the message after wrapping. The message area
        // is `width - 4` columns wide (borders + 1-cell margin on each side),
        // so each \n-separated line wraps to ceil(len / inner_width) visual
        // rows. Borders + margin + the OK button take 6 rows.
        //
        // 96, not 80: at the typical ~35-col sidebar width, a centered
        // 80-wide dialog on a 150-col terminal lands its left border exactly
        // at the sidebar's right border, which makes the modal visually
        // blend into the layout. 96 shifts the coincidence point off the
        // common laptop-fullscreen width and gives long path lines (e.g.
        // `~/.config/agent-of-empires-dev`) more breathing room.
        const WIDTH: u16 = 96;
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
        tracing::info!(target: "tui.dialog", dialog = "warning", "opening warning dialog");
        self.home.info_dialog =
            Some(crate::tui::dialogs::InfoDialog::new("Warning", message).with_size(WIDTH, height));
    }

    pub fn set_theme(&mut self, name: &str) {
        // Honor the saved color_mode (Palette vs Truecolor). If we don't, a
        // SetTheme dispatched from the Settings view preview/apply flow will
        // re-load the theme with raw RGB colors, "breaking the coloration"
        // on terminals that were working with the user's palette preference
        // (Termius/mosh edge cases, 8-bit-only TTYs, etc.).
        let palette_mode = crate::session::resolve_config(
            self.home.active_profile.as_deref().unwrap_or("default"),
        )
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
        self.draw(terminal)?;

        // Refresh tmux session cache
        crate::tmux::refresh_session_cache();

        // Spawn async update check
        let settings = get_update_settings();
        if settings.check_enabled {
            let (tx, rx) = tokio::sync::oneshot::channel();
            self.update_rx = Some(rx);
            tokio::spawn(async move {
                let version = env!("CARGO_PKG_VERSION");
                let mut result = check_for_update(version, false).await;
                // For Homebrew installs, suppress the "update available"
                // ribbon until the formula has caught up to the GitHub
                // release. Otherwise users see the prompt, press 'u', and
                // hit a no-op `brew upgrade` while the formula lags. The
                // brew probes are sync; offload to keep the runtime free.
                if let Ok(info) = &mut result {
                    if info.available {
                        let target = info.latest_version.clone();
                        let actionable = tokio::task::spawn_blocking(move || {
                            crate::update::install::install_method_supports_target(&target)
                        })
                        .await
                        .unwrap_or(true);
                        if !actionable {
                            info.available = false;
                        }
                    }
                }
                let _ = tx.send(result);
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
                tracing::warn!(target: "tui.input", "Failed to register SIGHUP handler: {}", e);
            }
            if let Err(ref e) = term {
                tracing::warn!(target: "tui.input", "Failed to register SIGTERM handler: {}", e);
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
                            // Paste-burst detector for VoiceInk + Mosh ergonomics.
                            // Mosh strips bracketed-paste markers, so pasted
                            // dictation arrives as a stream of individual KeyEvents
                            // that would otherwise fire home-view shortcuts (Q=quit,
                            // N=new, X=stop, D=delete, ...). Look-ahead-poll the
                            // event stream with a short inter-key timeout; if
                            // PASTE_BURST_MIN_LEN printable chars accumulate, route
                            // through handle_paste instead of dispatching them
                            // individually. Below the threshold we replay the
                            // captured keys as normal events.
                            //
                            // Only fire when home accepts paste routing
                            // (`wants_paste_burst`). Non-paste-aware dialogs
                            // — command palette, profile picker, projects,
                            // info, etc. — capture text via `handle_key`
                            // only; bursting through them strands the input
                            // in `pending_paste` and leaves the dialog empty.
                            // CI caught this regression with e2e harnesses
                            // that type fast enough to trip the burst.
                            if self.home.wants_paste_burst() && Self::is_burst_candidate(&key) {
                                let first_char = Self::burst_char_for(&key)
                                    .expect("is_burst_candidate guarantees burst_char_for returns Some");
                                let mut burst_str = String::new();
                                burst_str.push(first_char);
                                let mut burst_keys: Vec<KeyEvent> = vec![key];
                                let mut deferred: Option<Event> = None;
                                loop {
                                    let next = tokio::time::timeout(
                                        Duration::from_millis(PASTE_BURST_INTER_KEY_MS),
                                        self.event_stream.as_mut().expect("event_stream missing").next(),
                                    ).await;
                                    match next {
                                        Ok(Some(Ok(Event::Key(k)))) if Self::is_burst_candidate(&k) => {
                                            if let Some(c) = Self::burst_char_for(&k) {
                                                burst_str.push(c);
                                                burst_keys.push(k);
                                            }
                                        }
                                        Ok(Some(Ok(other))) => {
                                            deferred = Some(other);
                                            break;
                                        }
                                        _ => break,
                                    }
                                }
                                if burst_keys.len() >= PASTE_BURST_MIN_LEN {
                                    tracing::debug!(target: "tui.input",
                                        "paste-burst: routed {} chars via handle_paste (chars={:?})",
                                        burst_str.len(), burst_str
                                    );
                                    self.home.handle_paste(&burst_str);
                                } else {
                                    for k in burst_keys {
                                        self.handle_key(k, terminal).await?;
                                        if self.should_quit { break; }
                                    }
                                }
                                if !self.should_quit {
                                    if let Some(evt) = deferred {
                                        match evt {
                                            Event::Key(k) => { self.handle_key(k, terminal).await?; }
                                            Event::Paste(text) => { self.home.handle_paste(&text); }
                                            Event::Resize(_, _) => { terminal.autoresize()?; self.needs_redraw = true; }
                                            // Mirror the non-burst Mouse arm: scroll wheel
                                            // events can land between burst chars on touch
                                            // devices (scroll-while-dictating). Forward
                                            // ScrollUp/Down to the home view's scroll hit
                                            // targets so they don't get silently dropped.
                                            Event::Mouse(mouse) => {
                                                let hit_list = self.home.hit_list(mouse.column, mouse.row);
                                                let hit_preview = self.home.hit_preview(mouse.column, mouse.row);
                                                let hit_diff = self.home.is_diff_open()
                                                    && self.home.hit_diff(mouse.column, mouse.row);
                                                let hit_scroll_target = hit_diff || hit_list || hit_preview;
                                                match mouse.kind {
                                                    MouseEventKind::ScrollUp if hit_scroll_target => { self.home.handle_scroll_up(mouse.column, mouse.row); }
                                                    MouseEventKind::ScrollDown if hit_scroll_target => { self.home.handle_scroll_down(mouse.column, mouse.row); }
                                                    _ => {}
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                // Mouse-capture state may have changed if the
                                // burst opened or closed a copy-friendly surface
                                // (info/changelog/serve dialog). Keep it in sync
                                // before the next render, matching the
                                // non-burst Event::Key arm below.
                                self.sync_mouse_capture(terminal)?;
                                if !self.needs_redraw {
                                    self.draw(terminal)?;
                                }
                                if self.should_quit {
                                    break;
                                }
                                continue;
                            }

                            self.handle_key(key, terminal).await?;
                            self.sync_mouse_capture(terminal)?;

                            // Skip the draw when returning from tmux attach.
                            // needs_redraw triggers a clear + stale event drain
                            // on the next iteration; drawing before that drain
                            // wastes a frame and can flicker.
                            if !self.needs_redraw {
                                self.draw(terminal)?;
                            }

                            if self.should_quit {
                                break;
                            }
                            continue;
                        }
                        Some(Ok(Event::Mouse(mouse))) => {
                            let hit_list = self.home.hit_list(mouse.column, mouse.row);
                            let hit_preview = self.home.hit_preview(mouse.column, mouse.row);
                            let hit_diff = self.home.is_diff_open()
                                && self.home.hit_diff(mouse.column, mouse.row);
                            let hit_scroll_target = hit_diff || hit_list || hit_preview;
                            let handled = match mouse.kind {
                                MouseEventKind::ScrollUp if hit_scroll_target => {
                                    self.home.handle_scroll_up(mouse.column, mouse.row)
                                }
                                MouseEventKind::ScrollDown if hit_scroll_target => {
                                    self.home.handle_scroll_down(mouse.column, mouse.row)
                                }
                                _ => false,
                            };
                            if handled {
                                self.draw(terminal)?;
                            }
                            continue;
                        }
                        Some(Ok(Event::Paste(text))) => {
                            self.home.handle_paste(&text);

                            self.draw(terminal)?;

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
                            self.draw(terminal)?;
                            continue;
                        }
                        Some(Ok(_)) => {}
                        Some(Err(e)) => {
                            // IO error reading from the terminal (broken pipe,
                            // EOF, etc.) means the tty is gone. Exit cleanly
                            // instead of spinning (#608 defect 2).
                            tracing::info!(target: "tui.input", "Terminal event stream error, exiting: {}", e);
                            self.should_quit = true;
                            break;
                        }
                        None => {
                            // EventStream ended (EOF on stdin). The terminal is
                            // gone; exit instead of busy-looping (#608 defect 2).
                            tracing::info!(target: "tui.input", "Terminal event stream ended (EOF), exiting");
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
                    tracing::info!(target: "tui.input", "Received SIGHUP, exiting");
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
                    tracing::info!(target: "tui.input", "Received SIGTERM, exiting");
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

            if self.home.apply_recovery_updates() {
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
                self.draw(terminal)?;
            }

            if self.should_quit {
                break;
            }
        }

        self.home.apply_session_id_updates();
        self.home.cleanup_pending_creation();

        if let Err(e) = self.home.save() {
            tracing::error!(target: "tui.input", "Failed to save on quit: {}", e);
        }

        Ok(())
    }

    fn render(&mut self, frame: &mut Frame) {
        let start = std::time::Instant::now();
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
        // Sampled / slow-frame only: full-frame trace would dominate the
        // log at `default_level = trace`. Only emit when a frame breaks the
        // 16ms budget (60fps), which is the diagnostic case worth tracing.
        let elapsed = start.elapsed();
        if elapsed.as_millis() > 16
            && tracing::enabled!(target: "tui.render", tracing::Level::TRACE)
        {
            tracing::trace!(
                target: "tui.render",
                frame_ms = elapsed.as_millis() as u64,
                width = frame.area().width,
                height = frame.area().height,
                "slow frame",
            );
        }
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
            //
            // No `needs_redraw = true` here: that forces a `terminal.clear()`
            // before the next event arrives, so the whole screen blanks for
            // a beat (visible flash). Ratatui's diff renderer handles the
            // 1-row layout shrink on the next normal draw.
            (KeyCode::Char('x'), KeyModifiers::CONTROL)
                if (self.update_info.is_some() || self.update_status.is_some())
                    && !self.home.has_dialog() =>
            {
                self.update_info = None;
                self.update_status = None;
                self.update_bar_dismissed = true;
                return Ok(());
            }
            _ => {}
        }

        if let Some(action) = self.home.handle_key(key, self.update_info.as_ref()) {
            self.execute_action(action, terminal)?;
        }

        #[cfg(feature = "serve")]
        if let Some(session_id) = self.pending_cockpit_open.take() {
            self.run_cockpit_view(&session_id, terminal).await?;
        }

        Ok(())
    }

    #[cfg(feature = "serve")]
    async fn run_cockpit_view(
        &mut self,
        session_id: &str,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> Result<()> {
        // The cockpit view borrows the EventStream so it can drive its
        // own tokio::select! loop. Pull it out for the duration of the
        // call; restore on return.
        let mut stream = match self.event_stream.take() {
            Some(s) => s,
            None => return Ok(()),
        };
        let result =
            crate::tui::cockpit_view::run(terminal, &mut stream, &self.theme, session_id).await;
        self.event_stream = Some(stream);
        // Forcing a full redraw on return so the home screen redraws
        // any cells the cockpit view painted over.
        self.needs_redraw = true;
        terminal.clear()?;
        if let Err(e) = result {
            self.update_status = Some(UpdateStatus::transient(format!("cockpit closed: {e}")));
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
                            tracing::error!(target: "tui.input", "Failed to stop session: {}", e);
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
            Action::SendMessage(id, message) => {
                // Flip the row to Starting and show a toast so the user has
                // visible feedback during ensure_pane_ready, which can take
                // several seconds on a cold-start sandboxed session (Docker
                // pull) or while the readiness loop waits for an agent
                // splash to clear. The status poller will correct the row
                // back to the real state after we return.
                self.home
                    .set_instance_status(&id, crate::session::Status::Starting);
                self.update_status = Some(UpdateStatus::transient("Reviving session...".into()));
                self.draw(terminal)?;
                let stale_sid = self.home.execute_send_message(&id, &message);
                match stale_sid {
                    Some(sid) => {
                        self.update_status = Some(UpdateStatus::transient(format!(
                            "Resume failed for sid {sid}; sent to fresh session (history not loaded)"
                        )));
                    }
                    None => {
                        self.update_status = None;
                    }
                }
            }
            Action::AttachToolSession(id, tool_name) => {
                self.attach_tool_session(&id, &tool_name, terminal)?;
            }
            #[cfg(feature = "serve")]
            Action::OpenCockpit(id) => {
                // Stash for the async main loop. The cockpit view needs
                // `event_stream` access that this sync handler can't
                // lend; the loop picks `pending_cockpit_open` up after
                // we return.
                self.pending_cockpit_open = Some(id);
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

        // Cockpit-mode sessions are not backed by tmux. The Enter
        // handler in `home::input` already short-circuits with a
        // transient toast pointing the user at the web dashboard;
        // this function still gets called from `apply_creation_results`
        // after `aoe add --launch`, so guard here too. Falling through
        // would attempt a tmux attach against a non-existent pane.
        if instance.is_cockpit_mode() {
            let _ = terminal;
            return Ok(());
        }

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
        tracing::debug!(target: "tui.input",
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
            match self
                .home
                .restart_instance_with_size_opts(session_id, size, skip_on_launch)
            {
                Err(e) => {
                    let err_str = e.to_string();
                    self.home
                        .set_instance_error(session_id, Some(err_str.clone()));
                    self.home
                        .set_instance_status(session_id, crate::session::Status::Error);
                    // Without a toast, set_instance_error + Status::Error are
                    // invisible to the user: the TUI redraws on home as if Enter
                    // did nothing. Toast text is single-line; the bar truncates
                    // at terminal width without us needing to pre-clip.
                    self.update_status = Some(UpdateStatus::transient(format!(
                        "restart failed: {err_str}"
                    )));
                    return Ok(());
                }
                Ok(crate::session::StartOutcome::Restarted { stale_sid }) => {
                    self.update_status = Some(UpdateStatus::transient(format!(
                        "Resume failed for sid {stale_sid}; started fresh (history not loaded)"
                    )));
                }
                Ok(_) => {}
            }
            self.home.set_instance_error(session_id, None);
        }

        let tmux_session = match self.home.get_instance(session_id) {
            Some(inst) => inst.tmux_session()?,
            None => return Ok(()),
        };
        let (attach_result, attached_status_updates) =
            self.with_attached_status_hooks(terminal, || tmux_session.attach())?;

        self.needs_redraw = true;
        crate::tmux::refresh_session_cache();
        self.home.reload()?;
        self.home
            .apply_status_updates_without_hooks(attached_status_updates);
        self.home.stamp_last_accessed(session_id);
        // Persist so the attach-return bump survives aoe restart. Same
        // reasoning as the send-message path in home/input.rs: without a
        // save() here the aging signal collapses back to startup timestamps
        // on next launch.
        if let Err(e) = self.home.save() {
            tracing::error!("Failed to save after attach-return: {}", e);
        }
        // In Attention sort, jump cursor to the top-attention row instead of
        // pinning it to the session we just came from; that session has
        // typically been bumped down a tier (Waiting → Running) and the next
        // item needing attention is now at row 0.
        if self.home.sort_order() == crate::session::config::SortOrder::Attention {
            self.home.select_top_attention(Some(session_id));
        } else {
            self.home.select_session_by_id(session_id);
        }

        if let Err(e) = attach_result {
            tracing::warn!(target: "tui.input", "tmux attach returned error: {}", e);
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

        let (attach_result, attached_status_updates) =
            self.with_attached_status_hooks(terminal, attach_fn)?;

        self.needs_redraw = true;
        crate::tmux::refresh_session_cache();
        self.home.reload()?;
        self.home
            .apply_status_updates_without_hooks(attached_status_updates);
        if self.home.sort_order() == crate::session::config::SortOrder::Attention {
            self.home.select_top_attention(Some(session_id));
        } else {
            self.home.select_session_by_id(session_id);
        }

        if let Err(e) = attach_result {
            tracing::warn!(target: "tui.input", "tmux terminal attach returned error: {}", e);
        }

        Ok(())
    }

    fn attach_tool_session(
        &mut self,
        session_id: &str,
        tool_name: &str,
        terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ) -> Result<()> {
        let instance = match self.home.get_instance(session_id) {
            Some(inst) => inst.clone(),
            None => return Ok(()),
        };

        let tool_config = match self.home.tool_configs.get(tool_name) {
            Some(tc) => tc.clone(),
            None => return Ok(()),
        };

        if tool_config.command.is_empty() {
            self.home.set_instance_error(
                session_id,
                Some(format!("Tool '{}' has no command configured", tool_name)),
            );
            return Ok(());
        }

        let size = crate::terminal::get_size();
        let tool_session = crate::tmux::ToolSession::new(&instance.id, &instance.title, tool_name);

        if !tool_session.exists() || tool_session.is_pane_dead() {
            if tool_session.exists() {
                let _ = tool_session.kill();
            }
            if let Err(e) =
                tool_session.create_with_size(&instance.project_path, &tool_config.command, size)
            {
                self.home
                    .set_instance_error(session_id, Some(e.to_string()));
                return Ok(());
            }
        }

        let branch = instance
            .worktree_info
            .as_ref()
            .map(|w| w.branch.as_str())
            .or_else(|| instance.workspace_info.as_ref().map(|w| w.branch.as_str()));
        crate::tmux::status_bar::apply_all_tmux_options(
            tool_session.session_name(),
            &format!("{} ({})", instance.title, tool_name),
            branch,
            None,
        );

        let attach_fn: Box<dyn FnOnce() -> Result<()>> = Box::new(move || tool_session.attach());
        let (attach_result, attached_status_updates) =
            self.with_attached_status_hooks(terminal, attach_fn)?;

        self.needs_redraw = true;
        crate::tmux::refresh_session_cache();
        self.home.reload()?;
        self.home
            .apply_status_updates_without_hooks(attached_status_updates);
        self.home.select_session_by_id(session_id);

        if let Err(e) = attach_result {
            tracing::warn!(
                "tmux tool session '{}' attach returned error: {}",
                tool_name,
                e
            );
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
                tracing::warn!(target: "tui.input", "Failed to refresh diff after edit: {}", e);
            }
        }

        // Log any editor errors but don't fail
        if let Err(e) = status {
            tracing::warn!(target: "tui.input", "Editor '{}' returned error: {}", editor, e);
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
    /// Send a message to a session. Deferred to `execute_action` (rather
    /// than handled inline in the dialog Submit branch) so the app loop
    /// can render a "Reviving..." status before the potentially-slow
    /// ensure_pane_ready call.
    SendMessage(String, String),
    /// Attach to a tool session (lazygit, yazi, etc.) for the given agent
    /// session. The tool_name indexes into Config.tools.
    AttachToolSession(String, String),
    /// Open the native cockpit view for `session_id`. The action handler
    /// stashes the id in `pending_cockpit_open`; the main loop drains it
    /// after `execute_action` returns and runs the async cockpit loop
    /// against the borrowed terminal + event stream.
    #[cfg(feature = "serve")]
    OpenCockpit(String),
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

    fn key(code: KeyCode, mods: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, mods)
    }

    #[test]
    fn burst_candidate_accepts_printable_chars_and_enter() {
        assert!(App::is_burst_candidate(&key(
            KeyCode::Char('a'),
            KeyModifiers::NONE
        )));
        assert!(App::is_burst_candidate(&key(
            KeyCode::Char(' '),
            KeyModifiers::NONE
        )));
        assert!(App::is_burst_candidate(&key(
            KeyCode::Char('A'),
            KeyModifiers::SHIFT
        )));
        assert!(App::is_burst_candidate(&key(
            KeyCode::Enter,
            KeyModifiers::NONE
        )));
    }

    #[test]
    fn burst_candidate_rejects_modified_chords_and_nav_keys() {
        // Ctrl/Alt chords are intentional shortcuts, never paste burst chars.
        assert!(!App::is_burst_candidate(&key(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL
        )));
        assert!(!App::is_burst_candidate(&key(
            KeyCode::Char('b'),
            KeyModifiers::ALT
        )));
        // Navigation/control keys are not burst candidates.
        assert!(!App::is_burst_candidate(&key(
            KeyCode::Esc,
            KeyModifiers::NONE
        )));
        assert!(!App::is_burst_candidate(&key(
            KeyCode::Tab,
            KeyModifiers::NONE
        )));
        assert!(!App::is_burst_candidate(&key(
            KeyCode::Up,
            KeyModifiers::NONE
        )));
        assert!(!App::is_burst_candidate(&key(
            KeyCode::Backspace,
            KeyModifiers::NONE
        )));
    }

    #[test]
    fn burst_char_for_matches_is_burst_candidate_domain() {
        // Contract: any key that passes is_burst_candidate must also yield
        // Some from burst_char_for, otherwise the event-loop's expect() panics.
        let candidates = [
            key(KeyCode::Char('a'), KeyModifiers::NONE),
            key(KeyCode::Char(' '), KeyModifiers::NONE),
            key(KeyCode::Char('A'), KeyModifiers::SHIFT),
            key(KeyCode::Char('~'), KeyModifiers::NONE),
            key(KeyCode::Enter, KeyModifiers::NONE),
        ];
        for k in &candidates {
            assert!(App::is_burst_candidate(k));
            assert!(
                App::burst_char_for(k).is_some(),
                "burst_char_for must agree with is_burst_candidate for {:?}",
                k
            );
        }
        assert_eq!(
            App::burst_char_for(&key(KeyCode::Enter, KeyModifiers::NONE)),
            Some('\n'),
            "Enter must map to \\n so embedded sentence-breaks land in the burst"
        );
    }
}
