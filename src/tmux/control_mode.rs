//! Long-lived `tmux -C` control-mode client.
//!
//! Background and contract live in
//! [issue #1485](https://github.com/njbrake/agent-of-empires/issues/1485).
//! Short version: live-send mode polls `tmux capture-pane` at ~60Hz and
//! issues a `tmux send-keys` per coalesced keystroke batch. The
//! historical implementation forked a fresh `tmux` process for every
//! one of those calls. Visible cost on phones over mosh / on battery.
//!
//! This module keeps one tmux process alive in control mode for the
//! duration of live-send and pipes every command (`capture-pane`,
//! `send-keys`, `resize-window`) over its stdin. Responses come back
//! via the same socket, framed by `%begin` / `%end` lines.
//!
//! The connection is the *sole* transport for live-send. There is no
//! fork-based fallback: callers that can't get a control-mode client
//! up don't enter live mode at all (`enter_live_send` returns Err and
//! the user sees a "Live send failed" dialog).
//!
//! Failure modes:
//! - Spawn failure (tmux missing, server unreachable, target session
//!   gone): `spawn` returns Err; the home view surfaces the error and
//!   live-send entry is aborted.
//! - Mid-session failure (timeout reading a response, EOF, malformed
//!   frame): the in-flight call returns Err. The render path drops
//!   the client (the worker's `Arc` clone keeps the underlying alive
//!   until the worker also drops, which happens when the user exits
//!   live-send). Typing visibly stops until the user exits and
//!   re-enters; that's the signal that something went wrong.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use crate::tmux::output_decoder::{decode_output_payload, extract_output_data};

/// Default timeout waiting for a single command response. Generous
/// because the worker thread is shared with notification draining and
/// a busy tmux server can take a tick to respond; tight enough that a
/// stuck client gives up before the user notices.
const RESPONSE_TIMEOUT: Duration = Duration::from_millis(750);

/// Env var that opts into the in-process vt100 emulator path.
///
/// When set (any non-empty value), the control-mode reader thread
/// decodes every `%output` payload and feeds the raw bytes into a
/// `vt100::Parser`. The preview-refresh path then reads the parser's
/// screen state instead of running `capture-pane`, eliminating one
/// socket round-trip per preview update. This is experimental: the
/// parser maintains a virtual terminal independently of tmux's own
/// rendering, and any divergence in vt sequence support shows up as
/// visual artifacts in the preview. With the env var unset the
/// historical `capture-pane` path is used.
pub const VT100_ENV_VAR: &str = "AOE_LIVE_VT100";

/// Initial geometry for a freshly spawned vt100 parser. The first
/// preview-pane resize replaces these before any real rendering
/// happens, but the parser needs to be sized before it can accept
/// the seed capture, so we start at a conservative 80x24.
const VT100_INITIAL_ROWS: u16 = 24;
const VT100_INITIAL_COLS: u16 = 80;

/// vt100 scrollback budget. Enough lines that brief Shift+PageUp
/// scrolling has something to show without bloating memory for what
/// is, after all, the live preview pane. Long-term scrollback still
/// flows through `capture-pane -S -N` against the non-vt100 path.
const VT100_SCROLLBACK_LEN: usize = 500;

/// A line received from tmux's stdout, post-tagging by the reader
/// thread. The reader does the minimal parsing required to separate
/// the three relevant cases (response framing, response payload, async
/// notification) so the command-issue path can ignore everything else
/// without re-scanning every line.
#[derive(Debug, Clone)]
enum Line {
    /// `%begin <timestamp> <num> <flags>`. The next zero or more
    /// `Payload` lines are the response body for command number `num`.
    Begin,
    /// `%end <timestamp> <num> <flags>`. Terminates a successful
    /// response; payload between matching Begin/End is the command's
    /// stdout.
    End,
    /// `%error <timestamp> <num> <flags>`. Terminates a failed
    /// response; the accumulated payload is the error message.
    Error,
    /// Anything else inside a Begin/End block. We don't try to parse
    /// the contents; capture-pane responses are raw ANSI text and the
    /// receiver wants them concatenated with newlines.
    Payload(String),
    /// `%output %<paneid> <data>`. The agent rendered new bytes to a
    /// pane. The reader thread uses this to signal the main loop that
    /// the preview cache is stale and a re-capture is warranted; the
    /// `send_command` consumer ignores it (same as any other
    /// notification).
    Output,
    /// `%window-pane-changed`, `%layout-change`, etc. — anything
    /// `%`-prefixed that isn't framing or output. Currently ignored.
    Notification,
    /// stdout closed: tmux exited, server died, or the session was
    /// killed. Any in-flight request fails; subsequent requests fail
    /// too.
    Eof,
}

/// A long-lived `tmux -C` connection.
pub struct ControlModeClient {
    session_name: String,
    /// Lock guards interleaving of multi-step send/receive over the
    /// single tmux connection. Holders write to stdin, then drain the
    /// reader-thread channel until they see `%end` / `%error`.
    inner: Mutex<Inner>,
    /// In-process vt100 emulator that mirrors the live pane. `Some`
    /// when the user opted into the vt100 path via
    /// [`VT100_ENV_VAR`]; the reader thread feeds it decoded
    /// `%output` bytes and the preview-refresh path reads its screen
    /// state via [`Self::screen_dump`]. `None` means the historical
    /// `capture-pane`-per-refresh path is in effect.
    vt100_parser: Option<Arc<Mutex<vt100::Parser>>>,
}

struct Inner {
    /// Set to `None` after the writer's stdin is closed during Drop so
    /// the reader thread sees EOF and exits cleanly.
    stdin: Option<ChildStdin>,
    rx: Receiver<Line>,
    /// Holds the child so we can wait on it in Drop. Wrapped in
    /// `Option` so Drop can take ownership.
    child: Option<Child>,
}

impl ControlModeClient {
    /// Spawn `tmux -C attach-session -t <session_name>` and start the
    /// reader thread. Returns Err if tmux can't be launched or if the
    /// spawned process exits before we manage to take its stdin/stdout
    /// handles.
    ///
    /// `on_output`, when provided, is invoked from the reader thread
    /// every time tmux emits a `%output` notification (the agent
    /// rendered bytes into the pane). The intended caller is the TUI
    /// main loop, which uses it to wake out of an idle `tokio::select!`
    /// so the preview re-captures without waiting for the next timer
    /// tick. The callback must be cheap and non-blocking; the reader
    /// thread is back to consuming stdout the instant it returns. A
    /// good shape is `Box::new(move || { let _ = tx.send(()); })`
    /// wrapping an `UnboundedSender<()>`.
    pub fn spawn(
        session_name: &str,
        on_output: Option<Box<dyn Fn() + Send + 'static>>,
    ) -> Result<Self> {
        let mut child = Command::new("tmux")
            .args(["-C", "attach-session", "-t", session_name])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            // Drop stderr entirely so a spammy `%error` from a peer
            // session doesn't leak into the parent's terminal. The
            // reader thread sees protocol errors via stdout already.
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("spawn tmux -C attach -t {}", session_name))?;

        let stdin = child
            .stdin
            .take()
            .context("control-mode child has no stdin")?;
        let stdout = child
            .stdout
            .take()
            .context("control-mode child has no stdout")?;

        // Optional vt100 parser, gated by the env var. Sized at
        // 80x24 here; the first preview-pane resize will replace
        // these dimensions via `Self::resize`. The reader thread
        // captures a clone of the Arc so it can write bytes; the
        // render path reads via the same Arc.
        let vt100_parser: Option<Arc<Mutex<vt100::Parser>>> = if vt100_enabled() {
            Some(Arc::new(Mutex::new(vt100::Parser::new(
                VT100_INITIAL_ROWS,
                VT100_INITIAL_COLS,
                VT100_SCROLLBACK_LEN,
            ))))
        } else {
            None
        };
        let vt100_for_reader = vt100_parser.clone();

        let (tx, rx) = channel::<Line>();
        thread::Builder::new()
            .name(format!("aoe-tmux-cm-{}", session_name))
            .spawn(move || {
                let reader = BufReader::new(stdout);
                for line_result in reader.lines() {
                    let line = match line_result {
                        Ok(l) => l,
                        Err(_) => {
                            let _ = tx.send(Line::Eof);
                            return;
                        }
                    };
                    // Fast path for `%output`: pull out the payload,
                    // decode the octal escapes, feed the bytes to the
                    // vt100 parser (if enabled), then fire the wake
                    // callback. The decode is cheap and runs once per
                    // `%output` line regardless of the parser state,
                    // so the wake fires the same way in either mode.
                    let is_output = line.starts_with("%output ");
                    if is_output {
                        if let Some(parser_arc) = &vt100_for_reader {
                            if let Some(data) = extract_output_data(&line["%output ".len()..]) {
                                let bytes = decode_output_payload(data);
                                if let Ok(mut p) = parser_arc.lock() {
                                    p.process(&bytes);
                                }
                            }
                        }
                        if let Some(cb) = &on_output {
                            cb();
                        }
                        if tx.send(Line::Output).is_err() {
                            return;
                        }
                        continue;
                    }
                    let parsed = parse_line(line);
                    if tx.send(parsed).is_err() {
                        // Receiver dropped: client is going away.
                        return;
                    }
                }
                let _ = tx.send(Line::Eof);
            })
            .context("spawn control-mode reader thread")?;

        let client = Self {
            session_name: session_name.to_string(),
            inner: Mutex::new(Inner {
                stdin: Some(stdin),
                rx,
                child: Some(child),
            }),
            vt100_parser,
        };

        // Drain the initial handshake. tmux emits a burst of
        // notifications immediately after `attach-session` (session
        // change, layout, etc.) before falling idle. We don't need
        // them, but we do need to make sure we wait briefly so the
        // very first `capture-pane` doesn't fight with a half-arrived
        // handshake. Best-effort: any drain timeout is benign.
        client.drain_initial_notifications(Duration::from_millis(100));

        // Seed the vt100 parser with the current pane contents so
        // the user doesn't see an empty preview while waiting for
        // the agent's next repaint. Best-effort: failure logs at
        // debug and leaves the parser empty (the agent's next
        // SIGWINCH or render will fill it in).
        if client.vt100_parser.is_some() {
            if let Err(err) = client.seed_vt100_from_capture() {
                tracing::debug!(
                    target: "tmux.control_mode",
                    error = %err,
                    "vt100 seed-capture failed; parser starts empty",
                );
            }
        }

        Ok(client)
    }

    /// Run a `capture-pane` against the session's first window's first
    /// pane and return the framed ANSI output. The command is byte-
    /// identical to what `Session::capture_pane_with_size` runs, so
    /// callers can drop this in wherever they were calling that.
    pub fn capture_pane(&self, lines: usize, _width: u16, _height: u16) -> Result<String> {
        // Match Session::capture_pane_with_size's targeting: `:^.0` is
        // the first window's first pane regardless of base-index.
        let target = format!("{}:^.0", self.session_name);
        let command = format!("capture-pane -t {} -p -e -S -{}", target, lines);
        self.send_command(&command)
    }

    /// Render the vt100 parser's current screen as ANSI bytes (with
    /// embedded styles and cursor positioning), or `None` when the
    /// vt100 path is disabled. The returned string is a drop-in
    /// replacement for the `capture_pane` output and can be plugged
    /// into the same preview cache.
    ///
    /// This is the lever that closes most of the typing-latency gap
    /// to a raw tmux attach: instead of paying a socket round-trip to
    /// `capture-pane` for every render, we hand the caller the
    /// in-process parser's state, which the reader thread updated
    /// from the latest `%output` notification with no extra I/O.
    pub fn screen_dump(&self) -> Option<String> {
        let parser_arc = self.vt100_parser.as_ref()?;
        let parser = parser_arc.lock().ok()?;
        let bytes = parser.screen().contents_formatted();
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Best-effort seed of the vt100 parser from `capture-pane`. Runs
    /// the same `capture-pane -e -p` we use elsewhere, then feeds the
    /// resulting ANSI byte stream into the parser so the screen
    /// reflects the current pane content. Returns `Ok(())` even when
    /// the parser is None; the caller is expected to only invoke this
    /// when vt100 mode is on.
    fn seed_vt100_from_capture(&self) -> Result<()> {
        let Some(parser_arc) = self.vt100_parser.as_ref() else {
            return Ok(());
        };
        // Capture only the current viewport (no scrollback) so the
        // parser receives a self-consistent screen rather than a
        // history dump that would scroll past its visible area.
        let target = format!("{}:^.0", self.session_name);
        let command = format!("capture-pane -t {} -p -e", target);
        let payload = self.send_command(&command)?;
        // Move cursor home before replaying so the captured rows
        // land in the expected positions; capture-pane output is
        // just per-row ANSI, no cursor-position escapes.
        let mut seed = Vec::with_capacity(payload.len() + 8);
        seed.extend_from_slice(b"\x1b[H");
        seed.extend_from_slice(payload.as_bytes());
        if let Ok(mut parser) = parser_arc.lock() {
            parser.process(&seed);
        }
        Ok(())
    }

    /// Deliver literal text to the pane via `send-keys -l --`. Runs
    /// over the persistent connection so the caller doesn't pay one
    /// fork per keystroke.
    ///
    /// Returns `Err` when the text contains a control byte (anything
    /// below `0x20` or `DEL`); tmux's command parser splits commands
    /// on newlines and we don't have a safe encoding for arbitrary
    /// raw bytes inside a single command line. live-send never
    /// produces such payloads (`translate` emits `Char(c)` for
    /// printable chars and named keys for everything else), so the
    /// rejection is a guard against future callers, not a hot path.
    pub fn send_literal_no_enter(&self, text: &str) -> Result<()> {
        if text.bytes().any(|b| b < 0x20 || b == 0x7F) {
            bail!("control-mode send_literal rejects control bytes; caller should use fork path");
        }
        let target = format!("{}:^.0", self.session_name);
        // tmux's command parser treats single-quoted strings as
        // literal bytes. The only character that needs escaping is the
        // single quote itself, handled via the standard shell-style
        // close-quote / escaped-quote / reopen trick (`'` → `'\''`).
        let escaped = text.replace('\'', "'\\''");
        let command = format!("send-keys -t {} -l -- '{}'", target, escaped);
        self.send_command(&command).map(|_| ())
    }

    /// Send a single tmux-named key (e.g. `Escape`, `Up`, `C-c`) via
    /// the persistent connection.
    ///
    /// Returns `Err` for any key name containing characters outside
    /// the safe set (`A-Za-z0-9-+_`). The live-send translator never
    /// produces names outside this set, so the rejection is defense
    /// in depth against a future caller introducing an injection
    /// risk.
    pub fn send_named_key(&self, key_name: &str) -> Result<()> {
        if key_name.is_empty()
            || !key_name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '+' || c == '_')
        {
            bail!(
                "control-mode send_named_key rejects unsafe key name {:?}",
                key_name
            );
        }
        let target = format!("{}:^.0", self.session_name);
        let command = format!("send-keys -t {} {}", target, key_name);
        self.send_command(&command).map(|_| ())
    }

    /// Resize the session's window via the persistent connection.
    /// Mirrors `Session::resize`; same `-x` / `-y` semantics, same
    /// side-effect of flipping `window-size` to `manual` (callers
    /// still need to call `Session::reset_size_to_latest_client` on
    /// exit).
    pub fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        let cols = cols.max(1);
        let rows = rows.max(1);
        // Resize the parser FIRST so it accepts subsequent `%output`
        // bytes at the new geometry; without this, tmux's repaint
        // after the resize would land inside an undersized screen and
        // render at the wrong offsets until the parser caught up.
        if let Some(parser_arc) = &self.vt100_parser {
            if let Ok(mut parser) = parser_arc.lock() {
                parser.screen_mut().set_size(rows, cols);
            }
        }
        let command = format!(
            "resize-window -t {} -x {} -y {}",
            self.session_name, cols, rows
        );
        self.send_command(&command).map(|_| ())
    }

    fn send_command(&self, command: &str) -> Result<String> {
        // Recover from poisoning rather than panic. A poisoned mutex
        // means an earlier holder panicked mid-send; the client is in
        // an indeterminate state and we want the caller to drop it
        // and fall back to the fork path, which a returned Err
        // triggers.
        let mut inner = match self.inner.lock() {
            Ok(g) => g,
            Err(_) => bail!("control-mode mutex poisoned; client is unusable"),
        };
        let stdin = inner
            .stdin
            .as_mut()
            .context("control-mode stdin already closed")?;
        writeln!(stdin, "{}", command).context("write control-mode command")?;
        stdin.flush().context("flush control-mode command")?;

        let mut state = ResponseState::Idle;
        let mut payload = String::new();
        let deadline = Instant::now() + RESPONSE_TIMEOUT;

        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .context("control-mode response timed out")?;
            let line = match inner.rx.recv_timeout(remaining) {
                Ok(l) => l,
                Err(RecvTimeoutError::Timeout) => {
                    bail!("control-mode response timed out");
                }
                Err(RecvTimeoutError::Disconnected) => {
                    bail!("control-mode reader thread gone");
                }
            };
            match line {
                Line::Eof => bail!("control-mode connection EOF"),
                Line::Notification | Line::Output => continue,
                Line::Begin => {
                    state = ResponseState::Collecting;
                    payload.clear();
                }
                Line::End => {
                    if matches!(state, ResponseState::Collecting) {
                        return Ok(payload);
                    }
                    // %end without a matching %begin shouldn't happen,
                    // but if it does we fall through and keep waiting.
                }
                Line::Error => {
                    bail!("control-mode command error: {}", payload);
                }
                Line::Payload(s) => {
                    if matches!(state, ResponseState::Collecting) {
                        // Always emit `<line>\n` to match the fork
                        // path: `capture-pane -p` writes each pane row
                        // followed by a newline, and `String::from_utf8_lossy`
                        // on that stdout preserves the trailing one.
                        // Joining with `\n` between lines (only) would
                        // drop the final newline and produce a
                        // one-byte-shorter result than the fork path,
                        // which preview consumers expect.
                        payload.push_str(&s);
                        payload.push('\n');
                    }
                    // Payload outside Collecting is stray output from
                    // tmux startup; drop it.
                }
            }
        }
    }

    fn drain_initial_notifications(&self, budget: Duration) {
        let inner = match self.inner.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        let deadline = Instant::now() + budget;
        loop {
            let remaining = match deadline.checked_duration_since(Instant::now()) {
                Some(r) => r,
                None => return,
            };
            match inner.rx.recv_timeout(remaining) {
                Ok(Line::Notification) | Ok(Line::Output) => continue,
                // Anything else: stop draining. If it was useful, the
                // next send_command will pick it up; if it was an EOF
                // we'll see it again on the first real request.
                _ => return,
            }
        }
    }
}

/// Returns `true` when the user has opted into the in-process vt100
/// emulator path via the [`VT100_ENV_VAR`] environment variable.
/// Empty values are treated as "unset" so the user can clear the env
/// var inside a shell without re-launching `aoe`.
fn vt100_enabled() -> bool {
    std::env::var(VT100_ENV_VAR)
        .map(|v| !v.is_empty())
        .unwrap_or(false)
}

impl Drop for ControlModeClient {
    fn drop(&mut self) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        // Closing stdin makes tmux detach the control-mode client
        // cleanly; the reader thread then sees EOF and exits.
        drop(inner.stdin.take());
        if let Some(mut child) = inner.child.take() {
            // Give tmux a moment to wind down on its own. If it
            // doesn't, fall back to kill so we don't leak a zombie.
            // The reader thread is detached; we don't need to join it.
            for _ in 0..10 {
                match child.try_wait() {
                    Ok(Some(_)) => return,
                    Ok(None) => thread::sleep(Duration::from_millis(20)),
                    Err(_) => break,
                }
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum ResponseState {
    Idle,
    Collecting,
}

/// Parse one line of tmux control-mode output into a `Line` tag.
///
/// Visible for testing; the reader thread is the only non-test caller.
fn parse_line(line: String) -> Line {
    // tmux uses `%begin <ts> <num> <flags>` with single spaces; we
    // match the literal prefix and ignore the body. `%output` is
    // singled out so the reader thread can signal a preview wake on
    // it; the rest (`%window-pane-changed`, `%layout-change`, etc.)
    // collapse into `Notification`.
    if let Some(rest) = line.strip_prefix('%') {
        if rest.starts_with("begin ") || rest == "begin" {
            return Line::Begin;
        }
        if rest.starts_with("end ") || rest == "end" {
            return Line::End;
        }
        if rest.starts_with("error ") || rest == "error" {
            return Line::Error;
        }
        if rest.starts_with("output ") || rest == "output" {
            return Line::Output;
        }
        return Line::Notification;
    }
    Line::Payload(line)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_line_recognizes_begin() {
        let l = parse_line("%begin 1700000000 1 0".to_string());
        assert!(matches!(l, Line::Begin));
    }

    #[test]
    fn parse_line_recognizes_end() {
        let l = parse_line("%end 1700000000 1 0".to_string());
        assert!(matches!(l, Line::End));
    }

    #[test]
    fn parse_line_recognizes_error() {
        let l = parse_line("%error 1700000000 1 0".to_string());
        assert!(matches!(l, Line::Error));
    }

    #[test]
    fn parse_line_treats_other_percent_lines_as_notifications() {
        for input in [
            "%window-pane-changed @1 %1",
            "%layout-change @1 abcd 1 0",
            "%session-changed $0 main",
            "%sessions-changed",
            "%exit",
            "%continue 1",
            "%pause 1",
        ] {
            let l = parse_line(input.to_string());
            assert!(
                matches!(l, Line::Notification),
                "expected Notification for {:?}, got {:?}",
                input,
                l
            );
        }
    }

    #[test]
    fn parse_line_recognizes_output_separately_from_notifications() {
        // %output triggers the preview-wake callback, so it needs its
        // own variant. Regression guard: if someone collapses
        // Output back into Notification, the wake stops firing and
        // typing latency reverts.
        let l = parse_line("%output %1 hello".to_string());
        assert!(matches!(l, Line::Output));
        let l = parse_line("%output".to_string());
        assert!(matches!(l, Line::Output));
    }

    #[test]
    fn parse_line_treats_payload_lines_as_payload() {
        let l = parse_line("some ANSI \x1b[31mtext\x1b[0m".to_string());
        match l {
            Line::Payload(s) => assert!(s.contains("text")),
            other => panic!("expected Payload, got {:?}", other),
        }
    }

    #[test]
    fn parse_line_payload_starting_with_paren_is_payload() {
        // Regression guard: a payload line that starts with a non-`%`
        // character but contains `%begin` later must not be matched.
        let l = parse_line(" %begin not really a marker".to_string());
        match l {
            Line::Payload(s) => assert!(s.contains("%begin")),
            other => panic!("expected Payload, got {:?}", other),
        }
    }

    #[test]
    fn parse_line_payload_with_leading_space_is_payload() {
        // `%begin` must be at the very start to be a marker.
        let l = parse_line(" %end 1 1 1".to_string());
        match l {
            Line::Payload(_) => {}
            other => panic!("expected Payload for leading-space line, got {:?}", other),
        }
    }

    #[test]
    fn parse_line_bare_percent_word_recognized() {
        // tmux sometimes emits `%end` / `%begin` with no trailing
        // fields (different versions vary). Accept the bare form so we
        // don't deadlock waiting for an `%end ...` that never arrives.
        assert!(matches!(parse_line("%begin".to_string()), Line::Begin));
        assert!(matches!(parse_line("%end".to_string()), Line::End));
    }
}
