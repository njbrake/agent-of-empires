//! Long-lived `tmux -C` control-mode client.
//!
//! Background and contract live in
//! [issue #1485](https://github.com/njbrake/agent-of-empires/issues/1485).
//! Short version: while the user is in live-send mode the home view polls
//! `tmux capture-pane` at ~60Hz to keep the preview pane in sync with their
//! keystrokes. The historical implementation forks a fresh `tmux` process
//! per refresh, which is fine on a laptop and visible on phones / over
//! mosh / on battery.
//!
//! This module keeps one tmux process alive in control mode for the
//! duration of live-send and sends `capture-pane` commands over its
//! stdin instead of forking. The output is returned via the same
//! pipe, framed by `%begin` / `%end` lines. The capture command itself
//! is unchanged (`capture-pane -t <session>:^.0 -p -e -S -<lines>`),
//! so the response is byte-identical to what the fork path produces
//! and the existing rendering pipeline doesn't need to change.
//!
//! Failure modes:
//! - Spawn failure (tmux missing, server unreachable, target session
//!   gone): `spawn` returns Err and the caller falls back to the fork
//!   path with no user-visible change.
//! - Mid-session failure (timeout reading a response, EOF, malformed
//!   frame): the in-flight `capture_pane` call returns Err and the
//!   caller is expected to drop the client and continue on the fork
//!   path until live-send is exited and re-entered.
//!
//! Opt-out: set `AOE_DISABLE_TMUX_CONTROL_MODE=1` to skip control mode
//! entirely and use the fork path. Provided as a safety valve while
//! the feature is new.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

/// Env var users can set to force the fork-based capture path even when
/// control mode would otherwise be available. Any non-empty value
/// disables; matches the conventions of the project's other debug
/// switches (`AGENT_OF_EMPIRES_DEBUG`, etc.).
pub const DISABLE_ENV_VAR: &str = "AOE_DISABLE_TMUX_CONTROL_MODE";

/// Default timeout waiting for a single command response. Generous
/// because the worker thread is shared with notification draining and
/// a busy tmux server can take a tick to respond; tight enough that a
/// stuck client gives up before the user notices.
const RESPONSE_TIMEOUT: Duration = Duration::from_millis(750);

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
    /// `%output`, `%window-pane-changed`, etc. Ignored for v1.
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
    /// Returns true when the env-var opt-out is set.
    pub fn disabled_via_env() -> bool {
        std::env::var(DISABLE_ENV_VAR)
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    }

    /// Spawn `tmux -C attach-session -t <session_name>` and start the
    /// reader thread. Returns Err if the env-var opt-out is set, if
    /// tmux can't be launched, or if the spawned process exits before
    /// we manage to take its stdin/stdout handles.
    pub fn spawn(session_name: &str) -> Result<Self> {
        if Self::disabled_via_env() {
            bail!("control mode disabled via {}", DISABLE_ENV_VAR);
        }

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
        };

        // Drain the initial handshake. tmux emits a burst of
        // notifications immediately after `attach-session` (session
        // change, layout, etc.) before falling idle. We don't need
        // them, but we do need to make sure we wait briefly so the
        // very first `capture-pane` doesn't fight with a half-arrived
        // handshake. Best-effort: any drain timeout is benign.
        client.drain_initial_notifications(Duration::from_millis(100));

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
                Line::Notification => continue,
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
                Ok(Line::Notification) => continue,
                // Anything else: stop draining. If it was useful, the
                // next send_command will pick it up; if it was an EOF
                // we'll see it again on the first real request.
                _ => return,
            }
        }
    }
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
    // match the literal prefix and ignore the body. `%output`,
    // `%window-pane-changed`, etc. all start with `%` followed by a
    // non-`begin`/`end`/`error` keyword.
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
        // Any other `%`-prefixed line is an async notification.
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
            "%output %1 some bytes",
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

    #[test]
    fn disabled_via_env_respects_env_var() {
        // Snapshot+restore so we don't pollute the global env for
        // sibling tests. We can't assume the var is unset on the host
        // (CI may set it), so we capture the prior value and replay.
        let prior = std::env::var(DISABLE_ENV_VAR).ok();
        // SAFETY: tests in this file are single-threaded by default
        // (no `#[test]` parallelism between cases in the same module
        // unless they share state, which we deliberately avoid).
        std::env::set_var(DISABLE_ENV_VAR, "1");
        assert!(ControlModeClient::disabled_via_env());
        std::env::set_var(DISABLE_ENV_VAR, "");
        assert!(!ControlModeClient::disabled_via_env());
        match prior {
            Some(v) => std::env::set_var(DISABLE_ENV_VAR, v),
            None => std::env::remove_var(DISABLE_ENV_VAR),
        }
    }
}
