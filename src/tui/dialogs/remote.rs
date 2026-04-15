//! Remote access dialog: drives the `aoe serve --remote --daemon` daemon
//! lifecycle and shows a QR + URL + passphrase + log tail so a phone can
//! connect. The TUI is a controller here, not a host: it spawns the daemon,
//! reads `$APP_DIR/serve.{pid,url,log}` files, and runs `aoe serve --stop`
//! to tear down. The daemon survives across TUI quits, just like tmux
//! sessions or the CLI-invoked daemon path.
//!
//! Only compiled with the `serve` feature, since the tunnel integration
//! (and the qrcode crate it needs) lives there.
#![cfg(feature = "serve")]

use std::path::PathBuf;
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

/// How long we wait for `serve.url` to appear after spawning the daemon.
const TUNNEL_STARTUP_TIMEOUT_SECS: u64 = 60;
/// How much of `serve.log` to keep in memory for the tail pane.
const LOG_TAIL_LINES: usize = 200;

pub enum RemoteDialogState {
    /// No daemon running; show the two-factor explanation and wait for the
    /// user to confirm via Y/Enter/arrows. `confirm_selected` tracks which
    /// button (Enable vs Cancel) is currently highlighted so Enter picks it.
    /// Default is Cancel so a stray Enter doesn't expose the tunnel.
    Confirm {
        confirm_selected: bool,
    },
    /// We issued `aoe serve --remote --daemon`; now polling `serve.url`.
    /// If `passphrase` is Some, the TUI spawned the daemon and knows it;
    /// if None, a daemon was already running when the dialog opened.
    Starting {
        passphrase: Option<String>,
        started_at: Instant,
    },
    /// Daemon is live. No child field — the TUI does not own it.
    Active {
        url: String,
        /// Only known when this TUI started the daemon. For daemons
        /// started via the CLI we show a "set at startup" placeholder.
        passphrase: Option<String>,
        opened_at: Instant,
        log_tail: Vec<String>,
        /// Last-seen log-file length so we only read appended bytes.
        log_offset: u64,
    },
    Error(String),
}

pub struct RemoteDialog {
    state: RemoteDialogState,
    /// Passphrase we will use if the user confirms. Regenerated each time
    /// the user opens the Confirm screen so leaked-to-stdout values rotate.
    pending_passphrase: String,
}

impl Default for RemoteDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl RemoteDialog {
    /// Construct the dialog. If a daemon is already running (detected via
    /// `$APP_DIR/serve.pid`), jump straight to Active so the user can see
    /// the URL and stop it; otherwise show Confirm.
    pub fn new() -> Self {
        if crate::cli::serve::daemon_pid().is_some() {
            // There's already a daemon running. We don't know its passphrase
            // (it may have been started from the CLI). Try to read serve.url
            // immediately; if it's there, go Active. If not, wait.
            match read_serve_url() {
                Some(url) => Self {
                    state: RemoteDialogState::Active {
                        url,
                        passphrase: None,
                        opened_at: Instant::now(),
                        log_tail: initial_log_tail(),
                        log_offset: log_file_size(),
                    },
                    pending_passphrase: generate_passphrase(),
                },
                None => Self {
                    state: RemoteDialogState::Starting {
                        passphrase: None,
                        started_at: Instant::now(),
                    },
                    pending_passphrase: generate_passphrase(),
                },
            }
        } else {
            Self {
                state: RemoteDialogState::Confirm {
                    confirm_selected: false,
                },
                pending_passphrase: generate_passphrase(),
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult<()> {
        match &mut self.state {
            RemoteDialogState::Confirm { confirm_selected } => match key.code {
                // Y always enables, regardless of which button is highlighted.
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    match spawn_daemon(&self.pending_passphrase) {
                        Ok(()) => {
                            self.state = RemoteDialogState::Starting {
                                passphrase: Some(self.pending_passphrase.clone()),
                                started_at: Instant::now(),
                            };
                        }
                        Err(e) => {
                            self.state = RemoteDialogState::Error(e);
                        }
                    }
                    DialogResult::Continue
                }
                // Enter picks whichever button is currently highlighted.
                KeyCode::Enter => {
                    if *confirm_selected {
                        match spawn_daemon(&self.pending_passphrase) {
                            Ok(()) => {
                                self.state = RemoteDialogState::Starting {
                                    passphrase: Some(self.pending_passphrase.clone()),
                                    started_at: Instant::now(),
                                };
                            }
                            Err(e) => {
                                self.state = RemoteDialogState::Error(e);
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
            RemoteDialogState::Starting { .. } => match key.code {
                // Esc just closes the dialog; the daemon keeps coming up.
                KeyCode::Esc | KeyCode::Char('q') => DialogResult::Cancel,
                KeyCode::Char('s') | KeyCode::Char('S') => {
                    // Aborting startup: stop the (half-started) daemon.
                    let _ = stop_daemon();
                    DialogResult::Cancel
                }
                _ => DialogResult::Continue,
            },
            RemoteDialogState::Active { .. } => match key.code {
                KeyCode::Char('s') | KeyCode::Char('S') => match stop_daemon() {
                    Ok(()) => DialogResult::Cancel,
                    Err(e) => {
                        self.state = RemoteDialogState::Error(format!(
                                "Stop failed: {}. Daemon may still be running; retry or use `aoe serve --stop` from a shell.",
                                e
                            ));
                        DialogResult::Continue
                    }
                },
                // Closing without stopping is explicitly allowed — TUI is a
                // controller, the daemon keeps running.
                KeyCode::Esc | KeyCode::Char('q') => DialogResult::Cancel,
                _ => DialogResult::Continue,
            },
            RemoteDialogState::Error(_) => match key.code {
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
            RemoteDialogState::Starting {
                passphrase,
                started_at,
            } => {
                if let Some(url) = read_serve_url() {
                    self.state = RemoteDialogState::Active {
                        url,
                        passphrase: passphrase.clone(),
                        opened_at: Instant::now(),
                        log_tail: initial_log_tail(),
                        log_offset: log_file_size(),
                    };
                    return true;
                }
                // If the daemon process dies before writing serve.url,
                // fail fast with the last few log lines so the user can see
                // why.
                if crate::cli::serve::daemon_pid().is_none() {
                    let tail = initial_log_tail();
                    let detail = if tail.is_empty() {
                        String::new()
                    } else {
                        format!("\n\nLast log lines:\n{}", tail.join("\n"))
                    };
                    self.state = RemoteDialogState::Error(format!(
                        "`aoe serve --remote --daemon` exited before the tunnel came up.{}",
                        detail
                    ));
                    return true;
                }
                if started_at.elapsed() > Duration::from_secs(TUNNEL_STARTUP_TIMEOUT_SECS) {
                    // Timeout: the daemon is alive but never produced a
                    // tunnel URL (cloudflared rate-limited, captive portal,
                    // etc.). Stop it now so we don't leave a zombie that
                    // can never serve phones but keeps tripping "● Remote
                    // on" in the status bar. Fall through to a log-tail
                    // error view the user can act on.
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
                    self.state = RemoteDialogState::Error(format!(
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
            RemoteDialogState::Active {
                log_tail,
                log_offset,
                ..
            } => append_new_log_lines(log_tail, log_offset),
            _ => false,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        match &self.state {
            RemoteDialogState::Confirm { confirm_selected } => {
                render_confirm(frame, area, theme, *confirm_selected)
            }
            RemoteDialogState::Starting { started_at, .. } => {
                render_starting(frame, area, theme, started_at.elapsed())
            }
            RemoteDialogState::Active {
                url,
                passphrase,
                opened_at,
                log_tail,
                ..
            } => render_active(
                frame,
                area,
                theme,
                url,
                passphrase.as_deref(),
                opened_at.elapsed(),
                log_tail,
            ),
            RemoteDialogState::Error(msg) => render_error(frame, area, theme, msg),
        }
    }
}

fn spawn_daemon(passphrase: &str) -> Result<(), String> {
    use std::process::Command;

    let exe =
        std::env::current_exe().map_err(|e| format!("Could not resolve aoe binary path: {}", e))?;

    // Delete any stale serve.url from a previous hard-killed daemon
    // before launching. Without this, Starting-state polling could latch
    // onto the old URL before the new daemon writes the new one.
    if let Ok(dir) = crate::session::get_app_dir() {
        let _ = std::fs::remove_file(dir.join("serve.url"));
    }

    // Use a high ephemeral port so we don't collide with a user's own
    // `aoe serve` on 8080.
    let port: u16 = rand::rng().random_range(49152..65535);

    let status = Command::new(&exe)
        .args([
            "serve",
            "--remote",
            "--daemon",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ])
        .env("AOE_SERVE_PASSPHRASE", passphrase)
        .stdin(std::process::Stdio::null())
        // The daemon path forks and logs to serve.log; we only need its
        // exit status here (it's synchronous since it just double-forks).
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("Failed to launch `aoe serve --remote --daemon`: {}", e))?;

    if !status.success() {
        return Err(format!(
            "`aoe serve --remote --daemon` exited with {:?}. \
             Most likely `cloudflared` is not installed \
             (brew install cloudflared) or port {} is in use.",
            status.code(),
            port
        ));
    }
    Ok(())
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
    Ok(())
}

fn read_serve_url() -> Option<String> {
    let dir = crate::session::get_app_dir().ok()?;
    let raw = std::fs::read_to_string(dir.join("serve.url")).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
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

fn render_confirm(frame: &mut Frame, area: Rect, theme: &Theme, enable_selected: bool) {
    let dialog = super::centered_rect(area, 70, 22);
    frame.render_widget(Clear, dialog);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.accent))
        .title(Line::styled(
            " Enable remote access? ",
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
            "(or any browser) via a public Cloudflare URL.",
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
                "HTTPS end-to-end via Cloudflare (traffic is encrypted).",
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
            "Requires `cloudflared` (brew install cloudflared).",
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

fn render_starting(frame: &mut Frame, area: Rect, theme: &Theme, elapsed: Duration) {
    let dialog = super::centered_rect(area, 60, 9);
    frame.render_widget(Clear, dialog);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.border))
        .title(Line::styled(
            " Starting remote access... ",
            Style::default().fg(theme.title).bold(),
        ));
    let inner = block.inner(dialog);
    frame.render_widget(block, dialog);

    let body = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Waiting for the daemon to bring the tunnel up",
            Style::default().fg(theme.text),
        )),
        Line::from(Span::styled(
            "(usually 5\u{2013}15 seconds).",
            Style::default().fg(theme.text),
        )),
        Line::from(""),
        Line::from(Span::styled(
            format!("Elapsed: {}s    [Esc close]  [S stop]", elapsed.as_secs()),
            Style::default().fg(theme.dimmed),
        )),
    ];
    frame.render_widget(Paragraph::new(body).alignment(Alignment::Center), inner);
}

fn render_active(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    url: &str,
    passphrase: Option<&str>,
    elapsed: Duration,
    log_tail: &[String],
) {
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

    let want_width = (qr_width + 6).max(60);
    let log_height: u16 = 6;
    let want_height = qr_height + 4 /* url/passphrase/elapsed */ + log_height + 3 /* borders */;

    let dialog_width = want_width.min(area.width);
    let dialog_height = want_height.min(area.height);
    let dialog = super::centered_rect(area, dialog_width, dialog_height);
    frame.render_widget(Clear, dialog);

    let eight_hours = Duration::from_secs(8 * 3600);
    let reminder = if elapsed >= eight_hours {
        format!(
            " Remote access (open {}h) \u{2014} still need it? ",
            elapsed.as_secs() / 3600
        )
    } else {
        " Remote access ".to_string()
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

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(qr_height),
            Constraint::Length(1), // url
            Constraint::Length(1), // passphrase
            Constraint::Length(1), // elapsed
            Constraint::Min(1),    // log tail
            Constraint::Length(1), // footer
        ])
        .split(inner);

    let qr_widget: Vec<Line> = qr_lines
        .iter()
        .map(|l| Line::from(Span::styled(*l, Style::default().fg(theme.text))))
        .collect();
    frame.render_widget(
        Paragraph::new(qr_widget).alignment(Alignment::Center),
        chunks[0],
    );

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("URL: ", Style::default().fg(theme.dimmed)),
            Span::styled(url, Style::default().fg(theme.accent)),
        ]))
        .alignment(Alignment::Center),
        chunks[1],
    );

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
        chunks[2],
    );

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(
                "Open for {}  (daemon keeps running if you close this dialog)",
                format_elapsed(elapsed)
            ),
            Style::default().fg(theme.dimmed),
        )))
        .alignment(Alignment::Center),
        chunks[3],
    );

    let log_lines: Vec<Line> = log_tail
        .iter()
        .rev()
        .take(chunks[4].height.max(1) as usize)
        .rev()
        .map(|l| Line::from(Span::styled(l.as_str(), Style::default().fg(theme.dimmed))))
        .collect();
    let log_block = Block::default()
        .borders(Borders::TOP)
        .border_style(Style::default().fg(theme.border))
        .title(Line::styled(" Log ", Style::default().fg(theme.dimmed)));
    let log_inner = log_block.inner(chunks[4]);
    frame.render_widget(log_block, chunks[4]);
    frame.render_widget(Paragraph::new(log_lines), log_inner);

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "[S] Stop daemon    [Esc] Close (daemon keeps running)",
            Style::default().fg(theme.dimmed),
        )))
        .alignment(Alignment::Center),
        chunks[5],
    );
}

fn render_error(frame: &mut Frame, area: Rect, theme: &Theme, msg: &str) {
    let dialog = super::centered_rect(area, 70, 15);
    frame.render_widget(Clear, dialog);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme.error))
        .title(Line::styled(
            " Remote access failed ",
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
}
