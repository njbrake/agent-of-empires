//! Serve dialog: drives the `aoe serve --daemon` lifecycle (either Local
//! network mode on 0.0.0.0, or Cloudflare Tunnel mode) and shows a QR +
//! URL + (passphrase for Tunnel) + log tail so a phone can connect. The
//! TUI is a controller here, not a host: it spawns the daemon, reads
//! `$APP_DIR/serve.{pid,url,log,mode}` files, and runs `aoe serve --stop`
//! to tear down. The daemon survives across TUI quits, just like tmux
//! sessions or the CLI-invoked daemon path.
//!
//! Only compiled with the `serve` feature, since the tunnel integration
//! (and the qrcode crate it needs) lives there.
#![cfg(feature = "serve")]

use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent};
use qrcode::render::unicode::Dense1x2;
use qrcode::QrCode;
use rand::prelude::IndexedRandom;
use rand::RngExt;
use ratatui::prelude::*;
use ratatui::widgets::*;

use super::DialogResult;
use crate::tui::styles::Theme;

/// Which transport the daemon is serving over. Persisted to
/// `$APP_DIR/serve.mode` so a reattaching TUI can render the right label
/// and the right set of controls (Tab to cycle is Local-only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServeMode {
    Local,
    Tunnel,
}

impl ServeMode {
    fn file_token(self) -> &'static str {
        match self {
            ServeMode::Local => "local",
            ServeMode::Tunnel => "tunnel",
        }
    }

    fn from_file_token(s: &str) -> Option<Self> {
        match s.trim() {
            "local" => Some(ServeMode::Local),
            "tunnel" => Some(ServeMode::Tunnel),
            _ => None,
        }
    }
}

/// One URL we can show in the Active state. Tunnel mode has exactly one.
/// Local mode may have multiple (Tailscale + LAN + localhost), and the
/// user can Tab-cycle between them.
#[derive(Debug, Clone)]
pub struct ServeUrl {
    /// Optional human-readable label ("tailscale", "lan", "localhost").
    /// None for the single tunnel URL, which doesn't need one.
    pub label: Option<String>,
    pub url: String,
}

/// Passphrase cache for daemons this TUI process spawned, so reopening
/// the Remote Access dialog after closing it can re-display the same
/// passphrase instead of the "set at startup" placeholder. Cleared when
/// the daemon is stopped from this process. A daemon spawned by a
/// separate `aoe serve` invocation (outside this TUI) leaves this None,
/// so we correctly fall back to the placeholder for those.
static LAST_SPAWNED_PASSPHRASE: Mutex<Option<String>> = Mutex::new(None);

fn remember_passphrase(pp: &str) {
    if let Ok(mut guard) = LAST_SPAWNED_PASSPHRASE.lock() {
        *guard = Some(pp.to_string());
    }
}

fn recall_passphrase() -> Option<String> {
    LAST_SPAWNED_PASSPHRASE.lock().ok()?.clone()
}

fn forget_passphrase() {
    if let Ok(mut guard) = LAST_SPAWNED_PASSPHRASE.lock() {
        *guard = None;
    }
}

/// How long we wait for `serve.url` to appear after spawning the daemon.
const TUNNEL_STARTUP_TIMEOUT_SECS: u64 = 60;
/// How much of `serve.log` to keep in memory for the tail pane.
const LOG_TAIL_LINES: usize = 200;

pub enum ServeDialogState {
    /// No daemon running; first screen the user sees. They pick Local
    /// (bind 0.0.0.0, token auth only) or Tunnel (cloudflared + passphrase).
    /// `tunnel_available` gates the Tunnel card; `local_available`
    /// is false when the host has no non-loopback interface (dockerized
    /// dev env with only lo).
    ModePicker {
        selected: ServeMode,
        tunnel_available: bool,
        /// True when a logged-in Tailscale is detected; used to label
        /// the Tunnel card correctly ("Tailscale Funnel" vs "Cloudflare
        /// tunnel") and mention the stable-origin advantage.
        prefer_tailscale: bool,
        /// True when the host has a Tailscale-range IP but the CLI is
        /// missing or the daemon isn't logged in. Surfaces a small hint
        /// on the Tunnel card pointing at the installation path.
        suggest_tailscale_install: bool,
        local_available: bool,
        /// Transient flash message shown for ~1s after a rejected keypress
        /// (e.g., picking Tunnel when no tunnel tool is installed).
        flash: Option<(String, Instant)>,
    },
    /// Tunnel-only: show the two-factor explanation and wait for the user
    /// to confirm via Y/Enter/arrows. Local mode never enters Confirm —
    /// it goes ModePicker → Starting directly.
    Confirm {
        confirm_selected: bool,
    },
    /// We issued `aoe serve --daemon`; now polling `serve.url`.
    /// `passphrase` is Some only for Tunnel spawns from this TUI.
    Starting {
        mode: ServeMode,
        passphrase: Option<String>,
        started_at: Instant,
    },
    /// Daemon is live. No child field — the TUI does not own it.
    Active {
        mode: ServeMode,
        urls: Vec<ServeUrl>,
        /// Which `urls` entry is the primary QR target. Starts at 0.
        /// Tab advances; cycles; no-op when urls.len() <= 1.
        url_index: usize,
        /// Only known when this TUI started the daemon. For daemons
        /// started via the CLI we show a "set at startup" placeholder.
        /// Always None for Local mode.
        passphrase: Option<String>,
        opened_at: Instant,
        log_tail: Vec<String>,
        /// Last-seen log-file length so we only read appended bytes.
        log_offset: u64,
    },
    Error(String),
}

pub struct ServeDialog {
    state: ServeDialogState,
    /// Passphrase we will use if the user picks Tunnel and confirms.
    /// Regenerated each time the dialog opens so leaked-to-stdout values
    /// rotate.
    pending_passphrase: String,
}

impl Default for ServeDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl ServeDialog {
    /// Construct the dialog. If a daemon is already running (detected via
    /// `$APP_DIR/serve.pid`), jump straight to Active so the user can see
    /// the URL and stop it; otherwise show ModePicker.
    pub fn new() -> Self {
        if crate::cli::serve::daemon_pid().is_some() {
            // There's already a daemon running. Read its mode from
            // serve.mode (written by the server). If missing (older daemon
            // from pre-mode-split version), assume Tunnel — that was the
            // only mode the TUI could spawn before.
            let mode = read_serve_mode().unwrap_or(ServeMode::Tunnel);
            // Recall the passphrase only for Tunnel (Local has no passphrase).
            let remembered = if matches!(mode, ServeMode::Tunnel) {
                recall_passphrase()
            } else {
                None
            };
            let urls = read_serve_urls();
            if urls.is_empty() {
                Self {
                    state: ServeDialogState::Starting {
                        mode,
                        passphrase: remembered,
                        started_at: Instant::now(),
                    },
                    pending_passphrase: generate_passphrase(),
                }
            } else {
                Self {
                    state: ServeDialogState::Active {
                        mode,
                        urls,
                        url_index: 0,
                        passphrase: remembered,
                        opened_at: Instant::now(),
                        log_tail: initial_log_tail(),
                        log_offset: log_file_size(),
                    },
                    pending_passphrase: generate_passphrase(),
                }
            }
        } else {
            // Tunnel mode is usable if EITHER a logged-in Tailscale or
            // cloudflared is installed. Tailscale is preferred when both
            // are available (stable URL, installable PWAs keep working).
            let tailscale_ok = crate::server::tunnel::tailscale_available_sync();
            let cloudflared_ok = crate::server::tunnel::check_cloudflared().is_ok();
            let tunnel_available = tailscale_ok || cloudflared_ok;
            let prefer_tailscale = tailscale_ok;
            let tagged_ips = crate::server::discover_tagged_ips();
            let local_available = !tagged_ips.is_empty();
            // If the host has a Tailscale-range IP (100.64.0.0/10 CGNAT)
            // but `tailscale` isn't on PATH or the daemon isn't running,
            // the user is reachable over their tailnet and one short
            // install/login away from the stable-URL Funnel flow. Worth
            // surfacing as a one-liner on the Tunnel card.
            let suggest_tailscale_install = !tailscale_ok
                && tagged_ips
                    .iter()
                    .any(|(kind, _)| matches!(kind, crate::server::IpKind::Tailscale));
            // Default highlight: the last mode the user successfully
            // launched (read from serve.last_mode). Fall back to Local as
            // safer first-time default. If Local isn't actually available,
            // prefer Tunnel (and vice versa for cloudflared-missing).
            let remembered_default = read_last_mode().unwrap_or(ServeMode::Local);
            let selected = match remembered_default {
                ServeMode::Local if local_available => ServeMode::Local,
                ServeMode::Local if tunnel_available => ServeMode::Tunnel,
                ServeMode::Tunnel if tunnel_available => ServeMode::Tunnel,
                ServeMode::Tunnel if local_available => ServeMode::Local,
                _ => ServeMode::Local, // no-op default when neither works; picker handles it
            };
            Self {
                state: ServeDialogState::ModePicker {
                    selected,
                    tunnel_available,
                    prefer_tailscale,
                    suggest_tailscale_install,
                    local_available,
                    flash: None,
                },
                pending_passphrase: generate_passphrase(),
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult<()> {
        match &mut self.state {
            ServeDialogState::ModePicker {
                selected,
                tunnel_available,
                prefer_tailscale: _,
                suggest_tailscale_install: _,
                local_available,
                flash,
            } => {
                // Helper: attempt to commit the current `selected` mode,
                // transitioning to Confirm (Tunnel) or Starting (Local).
                // Rejects with a flash message if the mode isn't available.
                let commit = |dialog: &mut ServeDialog| -> DialogResult<()> {
                    let ServeDialogState::ModePicker {
                        selected,
                        tunnel_available,
                        local_available,
                        ..
                    } = &dialog.state
                    else {
                        return DialogResult::Continue;
                    };
                    let mode = *selected;
                    let cf = *tunnel_available;
                    let la = *local_available;
                    match mode {
                        ServeMode::Tunnel if !cf => {
                            if let ServeDialogState::ModePicker { flash, .. } = &mut dialog.state {
                                *flash = Some((
                                    "Install tailscale or cloudflared to enable Tunnel mode."
                                        .to_string(),
                                    Instant::now(),
                                ));
                            }
                            DialogResult::Continue
                        }
                        ServeMode::Local if !la => {
                            if let ServeDialogState::ModePicker { flash, .. } = &mut dialog.state {
                                *flash = Some((
                                    "No non-loopback network interface available.".to_string(),
                                    Instant::now(),
                                ));
                            }
                            DialogResult::Continue
                        }
                        ServeMode::Tunnel => {
                            dialog.state = ServeDialogState::Confirm {
                                confirm_selected: false,
                            };
                            DialogResult::Continue
                        }
                        ServeMode::Local => {
                            match spawn_daemon(ServeMode::Local, None) {
                                Ok(()) => {
                                    remember_last_mode(ServeMode::Local);
                                    dialog.state = ServeDialogState::Starting {
                                        mode: ServeMode::Local,
                                        passphrase: None,
                                        started_at: Instant::now(),
                                    };
                                }
                                Err(e) => dialog.state = ServeDialogState::Error(e),
                            }
                            DialogResult::Continue
                        }
                    }
                };

                // Clear stale flash on any key press (helps the user feel
                // they're making progress even if the next key is invalid).
                if flash
                    .as_ref()
                    .map(|(_, t)| t.elapsed() > Duration::from_millis(1500))
                    .unwrap_or(false)
                {
                    *flash = None;
                }

                match key.code {
                    KeyCode::Left | KeyCode::Char('h') => {
                        *selected = ServeMode::Local;
                        DialogResult::Continue
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        // Only move to Tunnel if it's usable; otherwise
                        // keep Local selected (don't let the user park
                        // the cursor on a dimmed card).
                        if *tunnel_available {
                            *selected = ServeMode::Tunnel;
                        }
                        DialogResult::Continue
                    }
                    KeyCode::Tab => {
                        *selected = match *selected {
                            ServeMode::Local if *tunnel_available => ServeMode::Tunnel,
                            ServeMode::Tunnel if *local_available => ServeMode::Local,
                            other => other,
                        };
                        DialogResult::Continue
                    }
                    KeyCode::Char('t') | KeyCode::Char('T') => {
                        *selected = ServeMode::Tunnel;
                        commit(self)
                    }
                    KeyCode::Char('L') => {
                        // Capital L as the explicit-Local shortcut. Keep
                        // lowercase `l` as "→ move right" per the arrow-key
                        // parallel above, which is the existing convention
                        // in the rest of the TUI.
                        *selected = ServeMode::Local;
                        commit(self)
                    }
                    KeyCode::Enter => commit(self),
                    KeyCode::Esc | KeyCode::Char('q') => DialogResult::Cancel,
                    _ => DialogResult::Continue,
                }
            }
            ServeDialogState::Confirm { confirm_selected } => match key.code {
                // Y always enables, regardless of which button is highlighted.
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    match spawn_daemon(ServeMode::Tunnel, Some(&self.pending_passphrase)) {
                        Ok(()) => {
                            remember_last_mode(ServeMode::Tunnel);
                            self.state = ServeDialogState::Starting {
                                mode: ServeMode::Tunnel,
                                passphrase: Some(self.pending_passphrase.clone()),
                                started_at: Instant::now(),
                            };
                        }
                        Err(e) => {
                            self.state = ServeDialogState::Error(e);
                        }
                    }
                    DialogResult::Continue
                }
                // Enter picks whichever button is currently highlighted.
                KeyCode::Enter => {
                    if *confirm_selected {
                        match spawn_daemon(ServeMode::Tunnel, Some(&self.pending_passphrase)) {
                            Ok(()) => {
                                remember_last_mode(ServeMode::Tunnel);
                                self.state = ServeDialogState::Starting {
                                    mode: ServeMode::Tunnel,
                                    passphrase: Some(self.pending_passphrase.clone()),
                                    started_at: Instant::now(),
                                };
                            }
                            Err(e) => {
                                self.state = ServeDialogState::Error(e);
                            }
                        }
                        DialogResult::Continue
                    } else {
                        DialogResult::Cancel
                    }
                }
                KeyCode::Left | KeyCode::Char('h') => {
                    *confirm_selected = true;
                    DialogResult::Continue
                }
                KeyCode::Right | KeyCode::Char('l') => {
                    *confirm_selected = false;
                    DialogResult::Continue
                }
                KeyCode::Tab => {
                    *confirm_selected = !*confirm_selected;
                    DialogResult::Continue
                }
                KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Char('q') => {
                    DialogResult::Cancel
                }
                _ => DialogResult::Continue,
            },
            ServeDialogState::Starting { .. } => match key.code {
                // Esc just closes the dialog; the daemon keeps coming up.
                KeyCode::Esc | KeyCode::Char('q') => DialogResult::Cancel,
                KeyCode::Char('s') | KeyCode::Char('S') => {
                    // Aborting startup: stop the (half-started) daemon.
                    let _ = stop_daemon();
                    DialogResult::Cancel
                }
                _ => DialogResult::Continue,
            },
            ServeDialogState::Active {
                urls, url_index, ..
            } => match key.code {
                KeyCode::Char('s') | KeyCode::Char('S') => match stop_daemon() {
                    Ok(()) => DialogResult::Cancel,
                    Err(e) => {
                        self.state = ServeDialogState::Error(format!(
                                "Stop failed: {}. Daemon may still be running; retry or use `aoe serve --stop` from a shell.",
                                e
                            ));
                        DialogResult::Continue
                    }
                },
                // Tab cycles URLs in Local mode (Tailscale ↔ LAN ↔ localhost).
                // No-op when there's only one URL (Tunnel mode, or a Local
                // host with just loopback).
                KeyCode::Tab if urls.len() > 1 => {
                    *url_index = (*url_index + 1) % urls.len();
                    DialogResult::Continue
                }
                // Closing without stopping is explicitly allowed — TUI is a
                // controller, the daemon keeps running.
                KeyCode::Esc | KeyCode::Char('q') => DialogResult::Cancel,
                _ => DialogResult::Continue,
            },
            ServeDialogState::Error(_) => match key.code {
                KeyCode::Char('s') | KeyCode::Char('S') => {
                    // Best-effort stop for a daemon that may still be
                    // lingering. Ignore the result — if there's no daemon
                    // to stop, that's the desired state anyway.
                    let _ = stop_daemon();
                    DialogResult::Cancel
                }
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char(' ') | KeyCode::Char('q') => {
                    DialogResult::Cancel
                }
                _ => DialogResult::Continue,
            },
        }
    }

    /// Poll files on disk and drive state transitions. Returns true when
    /// the visible state changed and a redraw is needed.
    pub fn tick(&mut self) -> bool {
        match &mut self.state {
            ServeDialogState::ModePicker { flash, .. } => {
                // Expire the flash message after 1.5s so it doesn't stick
                // around forever without a follow-up key press.
                if let Some((_, t)) = flash {
                    if t.elapsed() > Duration::from_millis(1500) {
                        *flash = None;
                        return true;
                    }
                }
                false
            }
            ServeDialogState::Starting {
                mode,
                passphrase,
                started_at,
            } => {
                let mode = *mode;
                let urls = read_serve_urls();
                if !urls.is_empty() {
                    self.state = ServeDialogState::Active {
                        mode,
                        urls,
                        url_index: 0,
                        passphrase: passphrase.clone(),
                        opened_at: Instant::now(),
                        log_tail: initial_log_tail(),
                        log_offset: log_file_size(),
                    };
                    return true;
                }
                // If the daemon process dies before writing serve.url,
                // fail fast with the last few log lines so the user can see
                // why. Common Local mode causes: port in use, EADDRNOTAVAIL
                // (Tailscale iface went away), permission denied.
                if crate::cli::serve::daemon_pid().is_none() {
                    let tail = initial_log_tail();
                    let joined = tail.join("\n");
                    let hint = diagnose_daemon_exit(&joined, mode);
                    let detail = if joined.is_empty() {
                        String::new()
                    } else {
                        format!("\n\nLast log lines:\n{}", joined)
                    };
                    let prefix = match mode {
                        ServeMode::Tunnel => {
                            "`aoe serve --remote --daemon` exited before the tunnel came up."
                        }
                        ServeMode::Local => {
                            "`aoe serve --daemon` exited before the server started."
                        }
                    };
                    self.state = ServeDialogState::Error(format!("{}{}{}", prefix, hint, detail));
                    return true;
                }
                // Local mode comes up ~instantly; no need for the 60s
                // cloudflared-timeout path. Tunnel mode keeps it.
                if matches!(mode, ServeMode::Tunnel)
                    && started_at.elapsed() > Duration::from_secs(TUNNEL_STARTUP_TIMEOUT_SECS)
                {
                    // Timeout: the daemon is alive but never produced a
                    // tunnel URL (cloudflared rate-limited, captive portal,
                    // etc.). Stop it now so we don't leave a zombie that
                    // can never serve phones but keeps tripping the status
                    // bar indicator. Fall through to a log-tail error view.
                    let stop_note = match stop_daemon() {
                        Ok(()) => "Stuck daemon stopped.".to_string(),
                        Err(e) => format!(
                            "Daemon may still be running \
                             (tried to stop: {}). Stop manually with `aoe serve --stop`.",
                            e
                        ),
                    };
                    let tail = initial_log_tail();
                    let tail_detail = if tail.is_empty() {
                        String::new()
                    } else {
                        format!("\n\nLast log lines:\n{}", tail.join("\n"))
                    };
                    self.state = ServeDialogState::Error(format!(
                        "Cloudflare tunnel did not announce a URL within {}s. \
                         {}\n\n\
                         Most likely cause: `cloudflared` rate-limited, \
                         captive portal, or no internet.{}",
                        TUNNEL_STARTUP_TIMEOUT_SECS, stop_note, tail_detail
                    ));
                    return true;
                }
                false
            }
            ServeDialogState::Active {
                log_tail,
                log_offset,
                ..
            } => append_new_log_lines(log_tail, log_offset),
            _ => false,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        match &self.state {
            ServeDialogState::ModePicker {
                selected,
                tunnel_available,
                prefer_tailscale,
                suggest_tailscale_install,
                local_available,
                flash,
            } => render_mode_picker(
                frame,
                area,
                theme,
                *selected,
                *tunnel_available,
                *prefer_tailscale,
                *suggest_tailscale_install,
                *local_available,
                flash.as_ref().map(|(m, _)| m.as_str()),
            ),
            ServeDialogState::Confirm { confirm_selected } => {
                render_confirm(frame, area, theme, *confirm_selected)
            }
            ServeDialogState::Starting {
                mode, started_at, ..
            } => render_starting(frame, area, theme, *mode, started_at.elapsed()),
            ServeDialogState::Active {
                mode,
                urls,
                url_index,
                passphrase,
                opened_at,
                log_tail,
                ..
            } => render_active(
                frame,
                area,
                theme,
                *mode,
                urls,
                *url_index,
                passphrase.as_deref(),
                opened_at.elapsed(),
                log_tail,
            ),
            ServeDialogState::Error(msg) => render_error(frame, area, theme, msg),
        }
    }
}

/// Spawn the aoe serve daemon in the requested mode. Tunnel requires a
/// passphrase (it's public-internet exposure); Local ignores it.
fn spawn_daemon(mode: ServeMode, passphrase: Option<&str>) -> Result<(), String> {
    use std::process::Command;

    // Guard: refuse to spawn if a daemon is already running. The dialog
    // constructor checks daemon_pid() and skips to Active, but there is
    // a window between that check and reaching here (user navigating
    // ModePicker). A spawn here would overwrite the PID file and orphan
    // the existing daemon.
    if crate::cli::serve::daemon_pid().is_some() {
        return Err(
            "A daemon is already running. Close this dialog and reopen to see it.".to_string(),
        );
    }

    let exe =
        std::env::current_exe().map_err(|e| format!("Could not resolve aoe binary path: {}", e))?;

    // Delete stale serve.url / serve.mode from a previous hard-killed
    // daemon before launching. Without this, Starting-state polling could
    // latch onto the old URL before the new daemon writes the new one.
    if let Ok(dir) = crate::session::get_app_dir() {
        let _ = std::fs::remove_file(dir.join("serve.url"));
        let _ = std::fs::remove_file(dir.join("serve.mode"));
    }

    // Reuse the port from the last TUI-launched daemon so the user can
    // bookmark the URL and not have to re-paste it after every restart.
    // Only generate a fresh random port on the very first launch (or if
    // the persisted file is missing). This avoids colliding with a user's
    // own `aoe serve` on the default 8080.
    let port: u16 = load_or_generate_port();

    let mut cmd = Command::new(&exe);
    cmd.args(["serve", "--daemon", "--port", &port.to_string()]);
    match mode {
        ServeMode::Tunnel => {
            cmd.args(["--remote", "--host", "127.0.0.1"]);
            if let Some(pp) = passphrase {
                cmd.env("AOE_SERVE_PASSPHRASE", pp);
            }
        }
        ServeMode::Local => {
            // 0.0.0.0 makes the server reachable on every local
            // interface (Tailscale, LAN, loopback). The server-side
            // serve.url writer picks Tailscale > LAN > localhost as the
            // primary URL in the QR.
            cmd.args(["--host", "0.0.0.0"]);
        }
    }
    cmd.stdin(std::process::Stdio::null())
        // The daemon path forks and logs to serve.log; we only need its
        // exit status here (it's synchronous since it just double-forks).
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    let status = cmd
        .status()
        .map_err(|e| format!("Failed to launch `aoe serve --daemon`: {}", e))?;

    if !status.success() {
        // If the daemon failed because the port was in use, clear the
        // persisted port so the next attempt picks a fresh one instead
        // of getting stuck on the same occupied port forever.
        let tail = initial_log_tail().join("\n");
        if tail.contains("EADDRINUSE") || tail.contains("Address already in use") {
            if let Ok(dir) = crate::session::get_app_dir() {
                let _ = std::fs::remove_file(dir.join("serve.last_port"));
            }
        }

        let hint = match mode {
            ServeMode::Tunnel => format!(
                "Most likely no tunnel tool is installed (install tailscale \
                 or cloudflared) or port {} is in use.",
                port
            ),
            ServeMode::Local => format!("Most likely port {} is in use.", port),
        };
        return Err(format!(
            "`aoe serve --daemon` exited with {:?}. {}",
            status.code(),
            hint
        ));
    }
    if let Some(pp) = passphrase {
        remember_passphrase(pp);
    }
    Ok(())
}

/// Map a common Linux/BSD errno string found in the daemon log tail to a
/// one-line user hint. Returns either `""` (no recognized error) or a
/// hint prefixed with a blank line, suitable for string concat into an
/// error message.
fn diagnose_daemon_exit(log: &str, mode: ServeMode) -> &'static str {
    if log.contains("EADDRNOTAVAIL") || log.contains("Cannot assign requested address") {
        return match mode {
            ServeMode::Local => {
                "\n\nHint: the interface we tried to bind on went away. \
                 Is Tailscale still up?"
            }
            ServeMode::Tunnel => "",
        };
    }
    if log.contains("EADDRINUSE") || log.contains("Address already in use") {
        return "\n\nHint: the daemon couldn't bind the picked port. \
                Reopen the dialog to try again with a fresh random port.";
    }
    if log.contains("Permission denied") {
        return "\n\nHint: permission denied on bind. Are you trying a \
                privileged port (<1024)? We normally pick a high port.";
    }
    ""
}

fn stop_daemon() -> Result<(), String> {
    use std::process::Command;

    let exe =
        std::env::current_exe().map_err(|e| format!("Could not resolve aoe binary path: {}", e))?;

    let output = Command::new(&exe)
        .args(["serve", "--stop"])
        .output()
        .map_err(|e| format!("Failed to invoke `aoe serve --stop`: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(stderr.trim().to_string());
    }
    forget_passphrase();
    Ok(())
}

/// Read serve.url as a list of labeled URLs. File format:
///
/// ```text
/// <primary-url>           ← line 1, unlabeled (backward-compatible)
/// <label>\t<alt-url>      ← line 2+, tab-separated label/url
/// ```
///
/// Returns `[]` when the file is missing or empty. The primary URL gets
/// `label: None` for rendering. Alternates carry their label.
fn read_serve_urls() -> Vec<ServeUrl> {
    let Some(dir) = crate::session::get_app_dir().ok() else {
        return Vec::new();
    };
    let Ok(raw) = std::fs::read_to_string(dir.join("serve.url")) else {
        return Vec::new();
    };
    let mut out: Vec<ServeUrl> = Vec::new();
    for (i, line) in raw.lines().enumerate() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        if i == 0 {
            // Primary line is the bare URL.
            out.push(ServeUrl {
                label: None,
                url: line.to_string(),
            });
        } else if let Some((label, url)) = line.split_once('\t') {
            out.push(ServeUrl {
                label: Some(label.to_string()),
                url: url.to_string(),
            });
        } else {
            // Defensive: unlabeled extra line. Show as a nameless extra.
            out.push(ServeUrl {
                label: None,
                url: line.to_string(),
            });
        }
    }
    out
}

/// Read the current daemon's mode marker (`serve.mode`). Returns None
/// when the file is absent (pre-mode-split daemon) or unparseable.
fn read_serve_mode() -> Option<ServeMode> {
    let dir = crate::session::get_app_dir().ok()?;
    let raw = std::fs::read_to_string(dir.join("serve.mode")).ok()?;
    ServeMode::from_file_token(&raw)
}

/// Read the last mode the user picked (across TUI restarts). Used to
/// default the ModePicker highlight on subsequent opens. Stored in a
/// separate file from `serve.mode` so it survives `aoe serve --stop`.
fn read_last_mode() -> Option<ServeMode> {
    let dir = crate::session::get_app_dir().ok()?;
    let raw = std::fs::read_to_string(dir.join("serve.last_mode")).ok()?;
    ServeMode::from_file_token(&raw)
}

fn remember_last_mode(mode: ServeMode) {
    if let Ok(dir) = crate::session::get_app_dir() {
        let _ = std::fs::write(dir.join("serve.last_mode"), mode.file_token());
    }
}

/// Load a previously used port from `serve.last_port`, or generate a fresh
/// random one in the ephemeral range and persist it. This keeps the URL
/// stable across TUI daemon restarts so users can bookmark it.
fn load_or_generate_port() -> u16 {
    if let Ok(dir) = crate::session::get_app_dir() {
        let port_path = dir.join("serve.last_port");
        if let Ok(raw) = std::fs::read_to_string(&port_path) {
            if let Ok(port) = raw.trim().parse::<u16>() {
                if port >= 49152 {
                    return port;
                }
            }
        }
        // No valid persisted port; generate and save one.
        let port: u16 = rand::rng().random_range(49152..65535);
        let _ = std::fs::write(&port_path, port.to_string());
        return port;
    }
    // Can't access app dir; fall back to random (won't persist).
    rand::rng().random_range(49152..65535)
}

fn log_file_path() -> Option<PathBuf> {
    crate::cli::serve::daemon_log_path().ok()
}

fn log_file_size() -> u64 {
    log_file_path()
        .and_then(|p| std::fs::metadata(&p).ok())
        .map(|m| m.len())
        .unwrap_or(0)
}

fn initial_log_tail() -> Vec<String> {
    let Some(path) = log_file_path() else {
        return Vec::new();
    };
    let Ok(contents) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    let all: Vec<&str> = contents.lines().collect();
    let start = all.len().saturating_sub(LOG_TAIL_LINES);
    all[start..].iter().map(|s| s.to_string()).collect()
}

/// Read any new bytes appended to the log file since `offset` and push the
/// resulting lines into `tail`, clamped to LOG_TAIL_LINES. Returns true if
/// new content arrived.
fn append_new_log_lines(tail: &mut Vec<String>, offset: &mut u64) -> bool {
    let Some(path) = log_file_path() else {
        return false;
    };
    append_new_log_lines_from(&path, tail, offset)
}

/// Path-explicit inner helper so tests can exercise the real logic
/// against a tempfile.
fn append_new_log_lines_from(
    path: &std::path::Path,
    tail: &mut Vec<String>,
    offset: &mut u64,
) -> bool {
    use std::io::{Read, Seek, SeekFrom};

    let Ok(mut file) = std::fs::File::open(path) else {
        return false;
    };
    let Ok(size) = file.metadata().map(|m| m.len()) else {
        return false;
    };
    if size <= *offset {
        if size < *offset {
            // File was truncated (daemon restart). Reset.
            *offset = 0;
            tail.clear();
        } else {
            return false;
        }
    }

    if file.seek(SeekFrom::Start(*offset)).is_err() {
        return false;
    }
    let mut buf = String::new();
    if file.read_to_string(&mut buf).is_err() {
        return false;
    }
    *offset = size;

    let mut changed = false;
    for line in buf.lines() {
        tail.push(line.to_string());
        changed = true;
    }
    if tail.len() > LOG_TAIL_LINES {
        let drop = tail.len() - LOG_TAIL_LINES;
        tail.drain(..drop);
    }
    changed
}

#[allow(clippy::too_many_arguments)]
fn render_mode_picker(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    selected: ServeMode,
    tunnel_available: bool,
    prefer_tailscale: bool,
    suggest_tailscale_install: bool,
    local_available: bool,
    flash: Option<&str>,
) {
    let dialog = super::centered_rect(area, 72, 16);
    frame.render_widget(Clear, dialog);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.accent))
        .title(Line::styled(
            " Serve ",
            Style::default().fg(theme.accent).bold(),
        ));
    let inner = block.inner(dialog);
    frame.render_widget(block, dialog);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(1), // question
            Constraint::Length(1), // spacer
            Constraint::Min(7),    // cards
            Constraint::Length(1), // flash
            Constraint::Length(1), // keybinds
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "How should this be reachable?",
            Style::default().fg(theme.title).bold(),
        )))
        .alignment(Alignment::Center),
        rows[0],
    );

    let cards = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Length(1),
            Constraint::Percentage(50),
        ])
        .split(rows[2]);

    // ── Local card ────────────────────────────────────────────────────────
    let local_primary = crate::server::discover_tagged_ips()
        .into_iter()
        .next()
        .map(|(kind, ip)| match kind {
            crate::server::IpKind::Tailscale => format!("{} (Tailscale)", ip),
            crate::server::IpKind::Lan => format!("{} (LAN)", ip),
            crate::server::IpKind::Loopback => format!("{} (loopback)", ip),
        })
        .unwrap_or_else(|| "only localhost available".to_string());
    let (local_border, local_title_style, local_body_style) =
        if selected == ServeMode::Local && local_available {
            (theme.accent, theme.accent, theme.text)
        } else if !local_available {
            (theme.dimmed, theme.dimmed, theme.dimmed)
        } else {
            (theme.border, theme.title, theme.text)
        };
    let local_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(local_border))
        .padding(Padding::horizontal(1))
        .title(Line::styled(
            " Local network ",
            Style::default().fg(local_title_style).bold(),
        ));
    let local_inner = local_block.inner(cards[0]);
    frame.render_widget(local_block, cards[0]);
    let local_body = vec![
        Line::from(""),
        Line::from(Span::styled(
            local_primary,
            Style::default().fg(if local_available {
                theme.accent
            } else {
                theme.dimmed
            }),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Token auth, no passphrase.",
            Style::default().fg(local_body_style),
        )),
        Line::from(Span::styled(
            "LAN + Tailscale. Instant.",
            Style::default().fg(local_body_style),
        )),
        if !local_available {
            Line::from(Span::styled(
                "  (no non-loopback interface)",
                Style::default().fg(theme.dimmed),
            ))
        } else {
            Line::from("")
        },
    ];
    frame.render_widget(Paragraph::new(local_body), local_inner);

    // ── Tunnel card ───────────────────────────────────────────────────────
    let (tunnel_border, tunnel_title_style, tunnel_body_style) =
        if selected == ServeMode::Tunnel && tunnel_available {
            (theme.accent, theme.accent, theme.text)
        } else if !tunnel_available {
            (theme.dimmed, theme.dimmed, theme.dimmed)
        } else {
            (theme.border, theme.title, theme.text)
        };
    let tunnel_title = if !tunnel_available {
        " HTTPS tunnel "
    } else if prefer_tailscale {
        " Tailscale Funnel "
    } else {
        " Cloudflare tunnel "
    };
    let tunnel_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(tunnel_border))
        .padding(Padding::horizontal(1))
        .title(Line::styled(
            tunnel_title,
            Style::default().fg(tunnel_title_style).bold(),
        ));
    let tunnel_inner = tunnel_block.inner(cards[2]);
    frame.render_widget(tunnel_block, cards[2]);
    let status_line = if !tunnel_available {
        "no tunnel tool installed"
    } else if prefer_tailscale {
        "stable HTTPS URL"
    } else {
        "public HTTPS URL (rotates)"
    };
    let secondary_line = if !tunnel_available {
        ""
    } else if prefer_tailscale {
        "Installed PWAs stay working."
    } else {
        "URL changes on restart."
    };
    let tunnel_body = vec![
        Line::from(""),
        Line::from(Span::styled(
            status_line,
            Style::default().fg(if tunnel_available {
                theme.accent
            } else {
                theme.dimmed
            }),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Token + passphrase (2FA).",
            Style::default().fg(tunnel_body_style),
        )),
        Line::from(Span::styled(
            if secondary_line.is_empty() {
                "Reachable from anywhere."
            } else {
                secondary_line
            },
            Style::default().fg(tunnel_body_style),
        )),
        if suggest_tailscale_install {
            // User has a Tailscale-range IP on an interface but the CLI
            // is missing or logged out. One install+login away from the
            // stable-URL Funnel flow; prioritized over the generic
            // install hint because it's the specific, actionable path.
            Line::from(Span::styled(
                "  Tailscale VPN detected: install the CLI for a stable URL",
                Style::default().fg(theme.dimmed),
            ))
        } else if !tunnel_available {
            Line::from(Span::styled(
                "  (brew install cloudflared or tailscale up)",
                Style::default().fg(theme.dimmed),
            ))
        } else {
            Line::from("")
        },
    ];
    frame.render_widget(Paragraph::new(tunnel_body), tunnel_inner);

    // ── Flash line ────────────────────────────────────────────────────────
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            flash.unwrap_or(""),
            Style::default().fg(theme.error).bold(),
        )))
        .alignment(Alignment::Center),
        rows[3],
    );

    // ── Keybinds ──────────────────────────────────────────────────────────
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "[←/→] choose    [L] Local    [T] Tunnel    [Enter] confirm    [Esc] cancel",
            Style::default().fg(theme.dimmed),
        )))
        .alignment(Alignment::Center),
        rows[4],
    );
}

fn render_confirm(frame: &mut Frame, area: Rect, theme: &Theme, enable_selected: bool) {
    let dialog = super::centered_rect(area, 70, 22);
    frame.render_widget(Clear, dialog);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.accent))
        .title(Line::styled(
            " Enable HTTPS tunnel? ",
            Style::default().fg(theme.accent).bold(),
        ));
    let inner = block.inner(dialog);
    frame.render_widget(block, dialog);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Min(1), Constraint::Length(2)])
        .split(inner);

    let body = vec![
        Line::from(Span::styled(
            "This lets you reach your agent sessions from your phone",
            Style::default().fg(theme.text),
        )),
        Line::from(Span::styled(
            "(or any browser) via a public HTTPS URL.",
            Style::default().fg(theme.text),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "How it's protected:",
            Style::default().fg(theme.title).bold(),
        )),
        Line::from(vec![
            Span::styled("  \u{2022} ", Style::default().fg(theme.running)),
            Span::styled(
                "HTTPS end-to-end via Tailscale or Cloudflare (encrypted).",
                Style::default().fg(theme.text),
            ),
        ]),
        Line::from(vec![
            Span::styled("  \u{2022} ", Style::default().fg(theme.running)),
            Span::styled(
                "Two factors required to log in: a token (in the URL /",
                Style::default().fg(theme.text),
            ),
        ]),
        Line::from(Span::styled(
            "    QR code) AND a passphrase typed on a login page.",
            Style::default().fg(theme.text),
        )),
        Line::from(vec![
            Span::styled("  \u{2022} ", Style::default().fg(theme.running)),
            Span::styled(
                "Knowing just the URL, or just the passphrase, is useless.",
                Style::default().fg(theme.text),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "What to watch out for:",
            Style::default().fg(theme.title).bold(),
        )),
        Line::from(Span::styled(
            "  If someone gets BOTH the token and the passphrase, they",
            Style::default().fg(theme.text),
        )),
        Line::from(Span::styled(
            "  can run commands as you. Don't post screenshots of the",
            Style::default().fg(theme.text),
        )),
        Line::from(Span::styled(
            "  QR + passphrase together, and stop the tunnel when done.",
            Style::default().fg(theme.text),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Runs as a background daemon. Survives TUI exit.",
            Style::default().fg(theme.dimmed),
        )),
        Line::from(Span::styled(
            "Requires tailscale (recommended) or cloudflared.",
            Style::default().fg(theme.dimmed),
        )),
        Line::from(Span::styled(
            "Stop anytime with [S] here or `aoe serve --stop`.",
            Style::default().fg(theme.dimmed),
        )),
    ];
    frame.render_widget(Paragraph::new(body).wrap(Wrap { trim: true }), chunks[0]);

    let enable_style = if enable_selected {
        Style::default().fg(theme.running).bold()
    } else {
        Style::default().fg(theme.dimmed)
    };
    let cancel_style = if !enable_selected {
        Style::default().fg(theme.accent).bold()
    } else {
        Style::default().fg(theme.dimmed)
    };
    // Enter activates whichever button is highlighted, so only show the
    // hint on that one to avoid the "[Enter] Enable" lie when Cancel is
    // selected.
    let (enable_label, cancel_label) = if enable_selected {
        ("[Enter/Y] Enable", "[Esc/N] Cancel")
    } else {
        ("[Y] Enable", "[Enter/Esc] Cancel")
    };
    let buttons = Line::from(vec![
        Span::styled(enable_label, enable_style),
        Span::raw("    "),
        Span::styled(cancel_label, cancel_style),
    ]);
    frame.render_widget(
        Paragraph::new(buttons).alignment(Alignment::Center),
        chunks[1],
    );
}

fn render_starting(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    mode: ServeMode,
    elapsed: Duration,
) {
    let dialog = super::centered_rect(area, 60, 9);
    frame.render_widget(Clear, dialog);
    let (title, wait_line1, wait_line2) = match mode {
        ServeMode::Tunnel => (
            " Starting HTTPS tunnel... ",
            "Waiting for the daemon to bring the tunnel up",
            "(usually 5\u{2013}15 seconds).",
        ),
        ServeMode::Local => (
            " Starting local server... ",
            "Binding on 0.0.0.0 and discovering interfaces",
            "(usually under a second).",
        ),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border))
        .title(Line::styled(title, Style::default().fg(theme.title).bold()));
    let inner = block.inner(dialog);
    frame.render_widget(block, dialog);

    let body = vec![
        Line::from(""),
        Line::from(Span::styled(wait_line1, Style::default().fg(theme.text))),
        Line::from(Span::styled(wait_line2, Style::default().fg(theme.text))),
        Line::from(""),
        Line::from(Span::styled(
            format!("Elapsed: {}s    [Esc close]  [S stop]", elapsed.as_secs()),
            Style::default().fg(theme.dimmed),
        )),
    ];
    frame.render_widget(Paragraph::new(body).alignment(Alignment::Center), inner);
}

#[allow(clippy::too_many_arguments)]
fn render_active(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    mode: ServeMode,
    urls: &[ServeUrl],
    url_index: usize,
    passphrase: Option<&str>,
    elapsed: Duration,
    log_tail: &[String],
) {
    // Defensive: callers should never hand us an empty urls slice (the
    // Starting → Active transition gates on non-empty serve.url). But if
    // we somehow get here, fall through to an obvious-empty render.
    let Some(active_url) = urls.get(url_index).or_else(|| urls.first()) else {
        let msg = "Daemon started but no URL available yet.";
        render_error(frame, area, theme, msg);
        return;
    };
    let url = &active_url.url;
    let kind_label = active_url.label.as_deref();
    // Encode the full URL including `?token=...` — the server's auth
    // middleware rejects requests on `/` with "invalid or missing auth
    // token" when the query param isn't present, so the phone would hit
    // 401 before ever seeing the passphrase login.
    //
    // Use half-block Unicode rendering (Dense1x2): each terminal row
    // carries two QR module rows via `\u{2580}` / `\u{2584}` / space /
    // full-block. That's roughly 4× smaller than the previous 2\u{00D7}1 char
    // rendering while staying scannable on any phone camera.
    let qr_text = match QrCode::new(url.as_bytes()) {
        Ok(code) => code
            .render::<Dense1x2>()
            .quiet_zone(true)
            .dark_color(Dense1x2::Dark)
            .light_color(Dense1x2::Light)
            .build(),
        Err(_) => String::from("(QR unavailable \u{2014} use the URL below)"),
    };

    let qr_lines: Vec<&str> = qr_text.lines().collect();
    let qr_height = qr_lines.len() as u16;
    let qr_width = qr_lines.first().map(|l| l.chars().count()).unwrap_or(0) as u16;

    // We want to show the full URL (including `?token=...`) on ONE line
    // so the user can triple-click it and paste straight into a browser.
    // That only works when the terminal is wide enough; on narrower
    // terminals the combined string would clip off the right edge, which
    // is worse than the split display because the token half disappears
    // entirely. So: prefer the combined row, fall back to URL + Token on
    // separate rows when we can't fit.
    let full_url = url.as_str();
    let url_prefix = "URL: ";
    let full_url_len = url_prefix.chars().count() + full_url.chars().count();
    let (split_url, split_token) = split_url_and_token(full_url);

    // Dialog wants to fit the full URL on one line. Floor at 80 for
    // breathing room; cap by terminal width.
    let want_width = (qr_width + 6).max((full_url_len + 4) as u16).max(80);
    let log_height: u16 = 6;

    let dialog_width = want_width.min(area.width);
    // Inner width available for a content row after the dialog border
    // (1 col each side) and the layout margin (1 col each side).
    let url_inner_width = dialog_width.saturating_sub(4).max(1) as usize;
    // Combined URL fits on one line = copy-paste friendly rendering.
    // Otherwise fall back to split so at least both halves are visible.
    let url_fits_one_line = full_url_len <= url_inner_width;
    let url_row_height: u16 = if url_fits_one_line {
        1
    } else {
        // Fallback: base URL row + token row (if there is a token).
        if split_token.is_some() {
            2
        } else {
            1
        }
    };

    let want_height = qr_height
        + url_row_height
        + 3 /* passphrase(opt) + elapsed + footer approx */
        + log_height
        + 3 /* borders + margins */;
    let dialog_height = want_height.min(area.height);
    let dialog = super::centered_rect(area, dialog_width, dialog_height);
    frame.render_widget(Clear, dialog);

    let eight_hours = Duration::from_secs(8 * 3600);
    let base_title = match mode {
        ServeMode::Local => " Serving (local) ",
        ServeMode::Tunnel => " Serving (tunnel) ",
    };
    let reminder = if elapsed >= eight_hours {
        format!(
            " Serving ({}) open {}h \u{2014} still need it? ",
            match mode {
                ServeMode::Local => "local",
                ServeMode::Tunnel => "tunnel",
            },
            elapsed.as_secs() / 3600
        )
    } else {
        base_title.to_string()
    };
    let title_color = if elapsed >= eight_hours {
        theme.waiting
    } else {
        theme.title
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border))
        .title(Line::styled(
            reminder,
            Style::default().fg(title_color).bold(),
        ));
    let inner = block.inner(dialog);
    frame.render_widget(block, dialog);

    // Layout: QR, optional kind label, URL row(s) (either a single full
    // URL for copy-paste, or split URL/Token fallback), optional
    // passphrase (Tunnel only), elapsed, log tail, footer.
    let show_passphrase = matches!(mode, ServeMode::Tunnel);
    let show_kind_label = kind_label.is_some();
    let show_split_token = !url_fits_one_line && split_token.is_some();
    let mut constraints = vec![Constraint::Length(qr_height)];
    if show_kind_label {
        constraints.push(Constraint::Length(1)); // kind label
    }
    constraints.push(Constraint::Length(1)); // url (full or base)
    if show_split_token {
        constraints.push(Constraint::Length(1)); // token (split fallback)
    }
    if show_passphrase {
        constraints.push(Constraint::Length(1)); // passphrase
    }
    constraints.extend_from_slice(&[
        Constraint::Length(1), // elapsed
        Constraint::Min(1),    // log tail
        Constraint::Length(1), // footer
    ]);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints(constraints)
        .split(inner);

    let qr_widget: Vec<Line> = qr_lines
        .iter()
        .map(|l| Line::from(Span::styled(*l, Style::default().fg(theme.text))))
        .collect();
    frame.render_widget(
        Paragraph::new(qr_widget).alignment(Alignment::Center),
        chunks[0],
    );

    let mut idx = 1;
    if let Some(label) = kind_label {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!("via {}", label),
                Style::default().fg(theme.dimmed).italic(),
            )))
            .alignment(Alignment::Center),
            chunks[idx],
        );
        idx += 1;
    }
    if url_fits_one_line {
        // Copy-paste path: full URL on one row, left-aligned so a triple-
        // click in iTerm / gnome-terminal / Warp selects the whole URL.
        // No wrap — the dialog width was sized to fit the URL exactly.
        // The leading " URL: " label lives in the same Paragraph so a
        // whole-line select catches both the label and the URL; most
        // users will just triple-click-then-paste, pasting "URL: ..."
        // and editing the label off, which is still faster than typing
        // out a 64-char token by hand.
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(url_prefix, Style::default().fg(theme.dimmed)),
                Span::styled(full_url, Style::default().fg(theme.accent)),
            ]))
            .alignment(Alignment::Left),
            chunks[idx],
        );
        idx += 1;
    } else {
        // Fallback: split URL and Token onto separate centered rows so
        // neither half clips. User has to copy twice, but that's a
        // strict improvement over the combined string getting truncated
        // and the token half disappearing entirely.
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(url_prefix, Style::default().fg(theme.dimmed)),
                Span::styled(split_url.as_str(), Style::default().fg(theme.accent)),
            ]))
            .alignment(Alignment::Center),
            chunks[idx],
        );
        idx += 1;
        if let Some(token) = split_token {
            frame.render_widget(
                Paragraph::new(Line::from(vec![
                    Span::styled("Token: ", Style::default().fg(theme.dimmed)),
                    Span::styled(token, Style::default().fg(theme.accent)),
                ]))
                .alignment(Alignment::Center),
                chunks[idx],
            );
            idx += 1;
        }
    }

    if show_passphrase {
        let (pp_label, pp_style) = match passphrase {
            Some(pp) => (pp.to_string(), Style::default().fg(theme.accent).bold()),
            None => (
                "(set when the daemon started \u{2014} check the shell that ran `aoe serve`)"
                    .to_string(),
                Style::default().fg(theme.dimmed),
            ),
        };
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("Passphrase: ", Style::default().fg(theme.dimmed)),
                Span::styled(pp_label, pp_style),
            ]))
            .alignment(Alignment::Center),
            chunks[idx],
        );
        idx += 1;
    }

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(
                "Open for {}  (daemon keeps running if you close this dialog)",
                format_elapsed(elapsed)
            ),
            Style::default().fg(theme.dimmed),
        )))
        .alignment(Alignment::Center),
        chunks[idx],
    );
    idx += 1;

    let log_chunk = chunks[idx];
    let log_lines: Vec<Line> = log_tail
        .iter()
        .rev()
        .take(log_chunk.height.max(1) as usize)
        .rev()
        .map(|l| Line::from(Span::styled(l.as_str(), Style::default().fg(theme.dimmed))))
        .collect();
    let log_block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.border))
        .title(Line::styled(" Log ", Style::default().fg(theme.dimmed)));
    let log_inner = log_block.inner(log_chunk);
    frame.render_widget(log_block, log_chunk);
    frame.render_widget(Paragraph::new(log_lines), log_inner);
    idx += 1;

    let footer = if urls.len() > 1 {
        "[Tab] switch URL   [S] Stop   [Esc] Close (daemon keeps running)"
    } else {
        "[S] Stop daemon    [Esc] Close (daemon keeps running)"
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            footer,
            Style::default().fg(theme.dimmed),
        )))
        .alignment(Alignment::Center),
        chunks[idx],
    );
}

/// Split a URL of the form `https://host/?token=XYZ` into a "clean" base
/// URL and its token so the dialog can fall back to rendering them on
/// separate rows when the combined string would clip off the right edge
/// of the dialog. Returns `(url, None)` when the query param is missing
/// or empty.
fn split_url_and_token(url: &str) -> (String, Option<&str>) {
    // The server always emits the token as the first query param in
    // `{url}/?token={token}`, so `?token=` is a safe anchor.
    if let Some(q_start) = url.find("?token=") {
        let base = url[..q_start].trim_end_matches('?').to_string();
        let token_start = q_start + "?token=".len();
        // Stop at the next `&` in case other query params ever appear.
        let token_end = url[token_start..]
            .find('&')
            .map(|n| token_start + n)
            .unwrap_or(url.len());
        let token = &url[token_start..token_end];
        if !token.is_empty() {
            return (base, Some(token));
        }
    }
    (url.to_string(), None)
}

fn render_error(frame: &mut Frame, area: Rect, theme: &Theme, msg: &str) {
    let dialog = super::centered_rect(area, 70, 15);
    frame.render_widget(Clear, dialog);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.error))
        .title(Line::styled(
            " Serve failed ",
            Style::default().fg(theme.error).bold(),
        ));
    let inner = block.inner(dialog);
    frame.render_widget(block, dialog);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(inner);

    frame.render_widget(
        Paragraph::new(msg)
            .wrap(Wrap { trim: true })
            .style(Style::default().fg(theme.text)),
        chunks[0],
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "[S] Force-stop daemon    [Enter] Close",
            Style::default().fg(theme.dimmed),
        )))
        .alignment(Alignment::Center),
        chunks[1],
    );
}

fn format_elapsed(d: Duration) -> String {
    let total = d.as_secs();
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{}h {:02}m", h, m)
    } else if m > 0 {
        format!("{}m {:02}s", m, s)
    } else {
        format!("{}s", s)
    }
}

/// Generate a four-word lowercase passphrase (1Password / diceware style).
/// Four words from a ~500-word list gives ~35 bits of entropy, which as a
/// *second* factor on top of the URL token is plenty; far easier to type
/// on a phone keyboard than a random alphanumeric soup.
fn generate_passphrase() -> String {
    let mut rng = rand::rng();
    let words: Vec<&'static str> = (0..4)
        .map(|_| {
            *PASSPHRASE_WORDS
                .choose(&mut rng)
                .expect("wordlist nonempty")
        })
        .collect();
    words.join(" ")
}

/// Curated list of short, unambiguous lowercase English words chosen for
/// phone-typability. No words shorter than 3 letters or longer than 6.
/// No near-homophones (e.g., "their"/"there") or visually confusable pairs.
#[rustfmt::skip]
const PASSPHRASE_WORDS: &[&str] = &[
    "able", "acid", "aged", "acorn", "agent", "alarm", "album", "alert",
    "algae", "alien", "alive", "alley", "alloy", "alpha", "amber", "amigo",
    "amino", "amuse", "angel", "anger", "angle", "angry", "ankle", "anvil",
    "apple", "apron", "arbor", "arena", "argon", "armor", "arrow", "ashen",
    "aside", "aspen", "asset", "atlas", "atom", "audio", "audit", "aunt",
    "avoid", "awake", "award", "aware", "awful", "axis", "bacon", "badge",
    "bagel", "baker", "balmy", "banjo", "baron", "basil", "basin", "basis",
    "batch", "baton", "beach", "beads", "beard", "beast", "beaver", "bench",
    "berry", "bingo", "birch", "bison", "black", "blade", "blaze", "blend",
    "bliss", "block", "bloom", "blues", "blunt", "blush", "board", "boast",
    "bold", "bolt", "bonus", "boost", "booth", "boots", "bored", "boss",
    "botany", "bowl", "brave", "bread", "break", "brick", "bride", "brief",
    "bring", "brisk", "brook", "brown", "brush", "bucket", "bugle", "built",
    "bulk", "bunny", "burly", "butter", "buzz", "cabin", "cable", "cactus",
    "caddy", "camel", "camp", "candle", "candy", "canoe", "canon", "canyon",
    "cape", "caper", "card", "care", "cargo", "carry", "cart", "carve",
    "cash", "cast", "catch", "cedar", "chair", "chalk", "charm", "chart",
    "chase", "cheek", "cheer", "chef", "chess", "chief", "child", "chill",
    "chimp", "chip", "chirp", "choir", "chose", "chunk", "cider", "cinema",
    "civic", "claim", "clamp", "clean", "clerk", "click", "cliff", "climb",
    "cling", "clock", "clone", "cloth", "cloud", "clove", "clown", "club",
    "clue", "coach", "coast", "cobra", "cocoa", "code", "coin", "colon",
    "color", "comet", "coral", "cord", "corn", "cost", "couch", "cover",
    "cozy", "craft", "crane", "crash", "crate", "cream", "crest", "crew",
    "cross", "crowd", "crown", "crumb", "crush", "crust", "cube", "curl",
    "cycle", "daisy", "dance", "dare", "dash", "data", "deal", "deck",
    "delta", "dense", "depth", "derby", "desk", "diary", "dice", "diner",
    "disco", "diver", "dock", "dodo", "dog", "doll", "dolly", "donkey",
    "dough", "dove", "downy", "draft", "dragon", "drape", "dream", "drift",
    "drill", "drive", "drop", "drum", "duck", "dusk", "dusty", "eager",
    "eagle", "early", "earth", "ebony", "echo", "edge", "eject", "elbow",
    "elder", "elf", "elite", "elk", "elm", "email", "empty", "enact",
    "energy", "engine", "enjoy", "enter", "entry", "envoy", "epic", "equal",
    "era", "error", "essay", "ether", "event", "every", "exact", "exile",
    "exit", "extra", "eye", "fable", "face", "fact", "fade", "fair",
    "fairy", "faith", "fall", "false", "fame", "family", "fancy", "farm",
    "fast", "fat", "fate", "fault", "fawn", "fear", "feast", "feed",
    "fern", "ferry", "fever", "few", "fiber", "field", "fifth", "fig",
    "film", "find", "fine", "finer", "finish", "fire", "firm", "first",
    "fish", "five", "fix", "flag", "flame", "flash", "flat", "flax",
    "flex", "flint", "float", "flock", "flood", "floor", "flora", "flour",
    "flow", "flower", "fluff", "fluid", "fluke", "flute", "fly", "foam",
    "fog", "foil", "fold", "folk", "fond", "food", "foot", "force",
    "ford", "forge", "fork", "form", "fort", "forum", "fossil", "fox",
    "frame", "free", "fresh", "friar", "fries", "frog", "from", "front",
    "frost", "froth", "fruit", "fry", "fuel", "full", "fun", "fund",
    "funny", "fur", "fury", "fuse", "gable", "gadget", "gain", "gala",
    "gamma", "gap", "garden", "gargle", "garlic", "gate", "gauge", "gear",
    "gecko", "gem", "gentle", "gift", "ginger", "girl", "glad", "glide",
    "glitch", "globe", "gloom", "gloss", "glove", "glow", "glue", "gnat",
    "goat", "gold", "golf", "gone", "good", "goose", "gospel", "grab",
    "grace", "grade", "grain", "grape", "graph", "grasp", "grass", "grate",
    "gravy", "great", "grid", "grief", "grim", "grin", "grip", "grit",
    "groan", "groom", "gross", "group", "grout", "grove", "grow", "grub",
    "guess", "guide", "guild", "guilt", "guitar", "gulf", "gum", "guru",
    "habit", "haiku", "hair", "half", "hall", "halt", "ham", "hand",
    "hang", "happy", "harbor", "hard", "hare", "harm", "harp", "hash",
    "haste", "hat", "hatch", "have", "haven", "hawk", "hay", "hazel",
    "head", "heal", "heap", "heart", "heat", "heavy", "hedge", "heel",
    "help", "hemp", "hen", "herb", "hero", "hex", "hide", "high",
    "hike", "hill", "hip", "hive", "hobby", "hog", "hold", "hole",
    "hollow", "holy", "home", "honey", "honor", "hood", "hoof", "hook",
    "hoop", "hope", "horn", "horse", "host", "hot", "hound", "hour",
    "house", "hub", "hug", "human", "humble", "humor", "hump", "hunch",
    "hunt", "hurry", "husk", "hut", "hyena", "hymn", "ice", "icon",
    "idea", "igloo", "imp", "index", "indigo", "infant", "inlet", "ink",
    "inlay", "inner", "input", "iris", "iron", "ivory", "ivy", "jade",
    "jam", "jar", "java", "jaw", "jazz", "jeans", "jelly", "jest",
    "jet", "jewel", "jiffy", "jig", "job", "join", "joke", "jolly",
    "joy", "judge", "juice", "jump", "jungle", "junior", "junk", "jury",
    "kayak", "keep", "kept", "kettle", "key", "kick", "kid", "kilt",
    "kind", "king", "kite", "kitten", "knack", "knee", "knife", "knock",
    "koala", "label", "lace", "ladder", "lake", "lamb", "lamp", "lance",
    "land", "lane", "laser", "later", "latte", "laugh", "lava", "lawn",
    "layer", "lazy", "leaf", "lean", "leap", "learn", "lease", "led",
    "ledge", "left", "legal", "lemon", "lend", "lens", "level", "lever",
    "lick", "lid", "life", "lift", "light", "lilac", "lime", "line",
    "link", "lint", "lion", "lip", "list", "live", "load", "loaf",
    "loan", "lobby", "lobe", "local", "lock", "loft", "log", "logic",
    "long", "look", "loop", "loose", "lotus", "loud", "lounge", "love",
    "low", "loyal", "luck", "lunar", "lunch", "lung", "lure", "lush",
    "lute", "lynx", "lyric", "mace", "madam", "made", "magic", "main",
    "make", "mallet", "malt", "mango", "manor", "mantle", "maple", "march",
    "mare", "mark", "mars", "marsh", "mask", "mast", "match", "mate",
    "math", "maze", "meadow", "meal", "meat", "medal", "meet", "mellow",
    "melody", "melt", "memo", "menu", "mercy", "merge", "merit", "merry",
    "mesh", "metal", "meter", "mew", "mice", "midst", "might", "mild",
    "mile", "milk", "mill", "mimic", "mind", "mine", "mint", "minus",
    "mirror", "mist", "moat", "mocha", "modal", "model", "modem", "moist",
    "mole", "money", "month", "moon", "moose", "moral", "more", "moth",
    "motor", "mount", "mouse", "move", "movie", "much", "muffin", "mulch",
    "mule", "muse", "music", "mute", "myth",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passphrase_is_four_lowercase_words() {
        let pw = generate_passphrase();
        let words: Vec<&str> = pw.split(' ').collect();
        assert_eq!(words.len(), 4, "passphrase should be 4 words: {:?}", pw);
        for w in &words {
            assert!(!w.is_empty(), "empty word in passphrase: {:?}", pw);
            assert!(
                w.chars().all(|c| c.is_ascii_lowercase()),
                "non-lowercase-letter in word {:?} of {:?}",
                w,
                pw
            );
        }
    }

    #[test]
    fn passphrase_words_are_from_the_wordlist() {
        let pw = generate_passphrase();
        for w in pw.split(' ') {
            assert!(
                PASSPHRASE_WORDS.contains(&w),
                "word {:?} not in the embedded wordlist",
                w
            );
        }
    }

    #[test]
    fn wordlist_is_well_formed() {
        assert!(
            PASSPHRASE_WORDS.len() >= 256,
            "wordlist too small for reasonable entropy: {}",
            PASSPHRASE_WORDS.len()
        );
        for w in PASSPHRASE_WORDS {
            assert!(!w.is_empty(), "empty word in list");
            assert!(
                w.chars().all(|c| c.is_ascii_lowercase()),
                "non-lowercase word in list: {:?}",
                w
            );
        }
    }

    #[test]
    fn format_elapsed_shows_units() {
        assert_eq!(format_elapsed(Duration::from_secs(5)), "5s");
        assert_eq!(format_elapsed(Duration::from_secs(65)), "1m 05s");
        assert_eq!(format_elapsed(Duration::from_secs(3600 + 120)), "1h 02m");
    }

    #[test]
    fn append_new_log_lines_initial_read() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("serve.log");
        std::fs::write(&path, "first line\nsecond line\n").unwrap();

        let mut tail: Vec<String> = Vec::new();
        let mut offset: u64 = 0;
        let grew = append_new_log_lines_from(&path, &mut tail, &mut offset);
        assert!(grew);
        assert_eq!(tail, vec!["first line", "second line"]);
        assert_eq!(offset, std::fs::metadata(&path).unwrap().len());
    }

    #[test]
    fn append_new_log_lines_detects_growth_and_truncation() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("serve.log");

        // Seed.
        std::fs::write(&path, "one\ntwo\n").unwrap();
        let mut tail: Vec<String> = Vec::new();
        let mut offset: u64 = 0;
        assert!(append_new_log_lines_from(&path, &mut tail, &mut offset));
        assert_eq!(tail, vec!["one", "two"]);
        let after_seed_offset = offset;

        // Append only.
        std::fs::write(&path, "one\ntwo\nthree\n").unwrap();
        assert!(append_new_log_lines_from(&path, &mut tail, &mut offset));
        assert_eq!(tail, vec!["one", "two", "three"]);
        assert!(offset > after_seed_offset);

        // No growth → no change.
        let before = offset;
        assert!(!append_new_log_lines_from(&path, &mut tail, &mut offset));
        assert_eq!(offset, before);

        // Truncation (daemon restart): file shrank, tail resets.
        std::fs::write(&path, "fresh\n").unwrap();
        assert!(append_new_log_lines_from(&path, &mut tail, &mut offset));
        assert_eq!(tail, vec!["fresh"]);
    }

    #[test]
    fn append_new_log_lines_clamps_to_max_lines() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("serve.log");

        // Write well over LOG_TAIL_LINES.
        let mut big = String::new();
        for i in 0..(LOG_TAIL_LINES + 50) {
            big.push_str(&format!("line {}\n", i));
        }
        std::fs::write(&path, big).unwrap();

        let mut tail: Vec<String> = Vec::new();
        let mut offset: u64 = 0;
        assert!(append_new_log_lines_from(&path, &mut tail, &mut offset));
        assert_eq!(tail.len(), LOG_TAIL_LINES);
        assert_eq!(
            tail.last().unwrap(),
            &format!("line {}", LOG_TAIL_LINES + 49)
        );
    }

    // These tests share the module-global LAST_SPAWNED_PASSPHRASE, so they
    // are combined into one #[test] to avoid cross-test interference when
    // cargo runs them in parallel.
    #[test]
    fn passphrase_cache_roundtrip() {
        forget_passphrase();
        assert_eq!(recall_passphrase(), None);

        remember_passphrase("four word diceware phrase");
        assert_eq!(
            recall_passphrase().as_deref(),
            Some("four word diceware phrase")
        );

        remember_passphrase("a different phrase later");
        assert_eq!(
            recall_passphrase().as_deref(),
            Some("a different phrase later")
        );

        forget_passphrase();
        assert_eq!(recall_passphrase(), None);
    }

    #[test]
    fn split_url_and_token_extracts_token() {
        let (base, token) =
            split_url_and_token("https://foo-bar.trycloudflare.com/?token=abc123def456");
        assert_eq!(base, "https://foo-bar.trycloudflare.com/");
        assert_eq!(token, Some("abc123def456"));
    }

    #[test]
    fn split_url_and_token_preserves_url_without_token() {
        let (base, token) = split_url_and_token("https://foo-bar.trycloudflare.com/");
        assert_eq!(base, "https://foo-bar.trycloudflare.com/");
        assert_eq!(token, None);
    }

    #[test]
    fn split_url_and_token_handles_additional_query_params() {
        let (base, token) =
            split_url_and_token("https://foo.trycloudflare.com/?token=abc123&foo=bar");
        assert_eq!(base, "https://foo.trycloudflare.com/");
        assert_eq!(token, Some("abc123"));
    }

    /// Exercises the fit logic that the render path uses: full URL on
    /// one line when it fits, split when it doesn't. Copies the arithmetic
    /// from render_active (url_inner_width = dialog_width - 4).
    fn url_fits_one_line(url: &str, dialog_width: u16) -> bool {
        let url_prefix = "URL: ";
        let full_url_len = url_prefix.chars().count() + url.chars().count();
        let url_inner_width = dialog_width.saturating_sub(4).max(1) as usize;
        full_url_len <= url_inner_width
    }

    #[test]
    fn url_fits_one_line_on_wide_terminal() {
        // Typical tunnel URL: ~115 chars including "URL: " prefix.
        let url = "https://foo-bar.trycloudflare.com/?token=a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
        assert!(
            url_fits_one_line(url, 120),
            "120-wide should fit ~115 chars"
        );
        assert!(
            url_fits_one_line(url, 115),
            "exact-fit boundary should pass"
        );
    }

    #[test]
    fn url_splits_on_narrow_terminal() {
        // 80-col terminal can't fit the combined tunnel URL; force the
        // split fallback so the token doesn't clip off the edge.
        let url = "https://foo-bar.trycloudflare.com/?token=a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
        assert!(!url_fits_one_line(url, 80));
        // Local URL is shorter (~70 with token) — depends on IP/port.
        let local = "http://192.168.1.42:54321/?token=a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
        assert!(!url_fits_one_line(local, 80));
        assert!(url_fits_one_line(local, 110));
    }

    #[test]
    fn serve_mode_file_token_roundtrip() {
        assert_eq!(ServeMode::from_file_token("local"), Some(ServeMode::Local));
        assert_eq!(
            ServeMode::from_file_token("tunnel"),
            Some(ServeMode::Tunnel)
        );
        // Trailing newline (the way the server writes it) still parses.
        assert_eq!(
            ServeMode::from_file_token("local\n"),
            Some(ServeMode::Local)
        );
        assert_eq!(ServeMode::from_file_token("garbage"), None);
        assert_eq!(ServeMode::from_file_token(""), None);
    }

    #[test]
    fn diagnose_daemon_exit_recognizes_common_errnos() {
        // Tailscale drop on Local: EADDRNOTAVAIL
        let hint = diagnose_daemon_exit(
            "ERROR: bind: Cannot assign requested address",
            ServeMode::Local,
        );
        assert!(hint.contains("interface"));
        // Same errno in Tunnel is not actionable in the same way, so we
        // don't surface a hint.
        assert_eq!(
            diagnose_daemon_exit(
                "ERROR: bind: Cannot assign requested address",
                ServeMode::Tunnel,
            ),
            ""
        );
        // Port-in-use
        assert!(diagnose_daemon_exit("Address already in use", ServeMode::Local).contains("port"));
        // Permission denied on privileged port
        assert!(diagnose_daemon_exit("Permission denied", ServeMode::Tunnel).contains("permission"));
        // No match
        assert_eq!(
            diagnose_daemon_exit("some unrelated line", ServeMode::Local),
            ""
        );
    }

    // ── read_serve_urls ───────────────────────────────────────────────────
    //
    // The helper reads from $APP_DIR/serve.url, which is outside our
    // control in unit tests. These tests exercise the parsing logic via a
    // small shim that mirrors read_serve_urls' line-by-line behavior; the
    // integration with the real file lives in e2e.
    fn parse_serve_url_contents(raw: &str) -> Vec<ServeUrl> {
        let mut out: Vec<ServeUrl> = Vec::new();
        for (i, line) in raw.lines().enumerate() {
            let line = line.trim_end_matches('\r');
            if line.is_empty() {
                continue;
            }
            if i == 0 {
                out.push(ServeUrl {
                    label: None,
                    url: line.to_string(),
                });
            } else if let Some((label, url)) = line.split_once('\t') {
                out.push(ServeUrl {
                    label: Some(label.to_string()),
                    url: url.to_string(),
                });
            } else {
                out.push(ServeUrl {
                    label: None,
                    url: line.to_string(),
                });
            }
        }
        out
    }

    #[test]
    fn serve_url_parses_single_line_backward_compat() {
        // Tunnel mode writes a single URL on line 1.
        let out = parse_serve_url_contents("https://foo.trycloudflare.com/?token=abc\n");
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].label, None);
        assert_eq!(out[0].url, "https://foo.trycloudflare.com/?token=abc");
    }

    #[test]
    fn serve_url_parses_multi_line_with_labels() {
        // Local mode writes primary on line 1, `kind\turl` on alternates.
        let raw = "\
http://100.64.0.5:54321/?token=abc\n\
lan\thttp://192.168.1.20:54321/?token=abc\n\
localhost\thttp://localhost:54321/?token=abc\n";
        let out = parse_serve_url_contents(raw);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].label, None);
        assert_eq!(out[0].url, "http://100.64.0.5:54321/?token=abc");
        assert_eq!(out[1].label.as_deref(), Some("lan"));
        assert_eq!(out[1].url, "http://192.168.1.20:54321/?token=abc");
        assert_eq!(out[2].label.as_deref(), Some("localhost"));
    }

    #[test]
    fn serve_url_tolerates_empty_and_unlabeled_extras() {
        // Defensive: if someone hand-edits serve.url and an extra line
        // has no tab, we treat it as an unlabeled alt rather than
        // dropping it.
        let raw = "http://primary/\n\nhttp://no-label-here/\n";
        let out = parse_serve_url_contents(raw);
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].label, None);
        assert_eq!(out[1].url, "http://no-label-here/");
    }

    // ── load_or_generate_port ────────────────────────────────────────────
    //
    // The real function reads from $APP_DIR/serve.last_port, which we can't
    // control in unit tests. These tests exercise the same parse + validate
    // + generate logic via a small shim that mirrors the function's core.

    /// Mirrors load_or_generate_port's logic against an arbitrary directory
    /// so we can test without touching the real app dir.
    fn load_or_generate_port_from(dir: &std::path::Path) -> u16 {
        let port_path = dir.join("serve.last_port");
        if let Ok(raw) = std::fs::read_to_string(&port_path) {
            if let Ok(port) = raw.trim().parse::<u16>() {
                if port >= 49152 {
                    return port;
                }
            }
        }
        let port: u16 = rand::rng().random_range(49152..65535);
        let _ = std::fs::write(&port_path, port.to_string());
        port
    }

    #[test]
    fn load_or_generate_port_generates_and_persists() {
        let tmp = tempfile::tempdir().unwrap();
        let port = load_or_generate_port_from(tmp.path());
        assert!(port >= 49152, "generated port should be in ephemeral range");
        // File was written
        let raw = std::fs::read_to_string(tmp.path().join("serve.last_port")).unwrap();
        assert_eq!(raw, port.to_string());
    }

    #[test]
    fn load_or_generate_port_reuses_persisted() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("serve.last_port"), "55555").unwrap();
        let port = load_or_generate_port_from(tmp.path());
        assert_eq!(port, 55555);
    }

    #[test]
    fn load_or_generate_port_rejects_low_port() {
        let tmp = tempfile::tempdir().unwrap();
        // A port below the ephemeral range should be ignored and regenerated.
        std::fs::write(tmp.path().join("serve.last_port"), "8080").unwrap();
        let port = load_or_generate_port_from(tmp.path());
        assert!(port >= 49152, "low port should be rejected: got {}", port);
    }

    #[test]
    fn load_or_generate_port_handles_garbage_file() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("serve.last_port"), "not-a-number\n").unwrap();
        let port = load_or_generate_port_from(tmp.path());
        assert!(port >= 49152, "garbage content should be regenerated");
    }
}
