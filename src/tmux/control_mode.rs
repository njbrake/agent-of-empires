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
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

/// How long after the last `%output` notification we wait before firing
/// the wake callback to the main loop. tmux emits one `%output` per
/// line the agent prints, so a multi-line burst (a token-stream chunk,
/// a full-screen TUI repaint) generates dozens of `%output` lines
/// across a few milliseconds. Firing the wake on the FIRST output
/// makes the consumer capture-pane mid-burst, drawing a partial
/// frame; a later wake then redraws the settled state. The user sees
/// "smooth, then sudden jump after a brief lag".
///
/// Debouncing collapses the burst to one wake at the trailing edge,
/// so the very first frame the consumer paints is already complete.
/// 8ms was picked empirically (see PR description): smaller values
/// (4ms) start letting partial frames through on dense bursts; larger
/// values (16ms+) start adding visible latency to single-character
/// echoes. The 50ms tokio ticker still bounds worst-case latency
/// under continuous sustained output (>1 output per 8ms) where the
/// debounce keeps extending.
const OUTPUT_WAKE_DEBOUNCE: Duration = Duration::from_millis(8);

/// Default timeout waiting for a single command response.
///
/// The previous value (750ms) was too tight in practice: under fast
/// typing the worker thread holds the socket mutex for each
/// `send-keys`, the agent echoes each keystroke as a `%output`
/// notification, and the channel between the reader thread and
/// `send_command` fills up with notifications that the main thread's
/// `capture-pane` call has to drain before reaching its own
/// `%begin`/`%end`. A single response that crossed 750ms tripped the
/// caller's drop-on-error path and blanked the preview until the
/// user exited and re-entered live mode (the symptom #1495 was
/// trying to fix didn't fully clear it).
///
/// 3s gives the busy-socket case plenty of headroom while still
/// bounding the per-call cost on a genuinely-wedged connection. The
/// caller still gives up after `MAX_LIVE_CAPTURE_FAILURES` consecutive
/// errors, so a truly hung client won't keep the user staring at
/// stale content forever.
const RESPONSE_TIMEOUT: Duration = Duration::from_secs(3);

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
    /// `%window-pane-changed`, `%layout-change`, etc. Any
    /// `%`-prefixed line that isn't framing or output. Currently
    /// ignored on the channel side.
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
    /// Spawn `tmux -C attach-session -t <session_name>` and start the
    /// reader thread. Returns Err if tmux can't be launched or if the
    /// spawned process exits before we manage to take its stdin/stdout
    /// handles.
    ///
    /// `on_output`, when provided, is invoked from a dedicated
    /// debouncer thread `OUTPUT_WAKE_DEBOUNCE` after the **last**
    /// `%output` notification of a burst, not on every line. The
    /// intended caller is the TUI main loop, which uses it to wake
    /// out of an idle `tokio::select!` so the preview re-captures
    /// without waiting for the next timer tick. Debouncing the
    /// trailing edge ensures the consumer's first capture after a
    /// multi-line burst sees the settled state in one frame, instead
    /// of painting a partial frame and then catching up. The
    /// callback must be cheap and non-blocking; a slow callback
    /// blocks the debouncer thread, not the reader. A good shape is
    /// `Box::new(move || { let _ = tx.try_send(()); })` wrapping a
    /// bounded `tokio::sync::mpsc::Sender<()>` of capacity 1, so any
    /// subsequent bursts that arrive while the consumer is still
    /// drawing coalesce into one pending wake.
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

        // The trailing-edge `%output` wake is debounced on a dedicated
        // thread so the reader stays a tight stdout->channel loop. The
        // reader sends `()` into `trail_tx` on every `%output` line;
        // the debouncer extends its timer on each tick and fires the
        // user-supplied callback `OUTPUT_WAKE_DEBOUNCE` after the
        // last one. Only spawned when a callback is supplied — if the
        // caller didn't ask for wakes (most non-live-send paths) we
        // skip the second thread entirely. The reader holds the only
        // sender; when the reader exits, the debouncer sees
        // `Disconnected` and shuts itself down.
        let trail_tx = if let Some(cb) = on_output {
            let (tx, rx) = channel::<()>();
            let debouncer = thread::Builder::new()
                .name(format!("aoe-tmux-cm-trail-{}", session_name))
                .spawn(move || debouncer_loop(rx, cb))
                .context("spawn control-mode output-wake debouncer thread")?;
            // The handle is dropped (detached) immediately: the
            // debouncer exits on its own when its sender drops, the
            // same way the reader does on EOF.
            let _ = debouncer;
            Some(tx)
        } else {
            None
        };

        let (tx, rx) = channel::<Line>();
        let reader_spawn = thread::Builder::new()
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
                    // Signal the debouncer BEFORE handing the parsed
                    // line off to the channel: a slow `send_command`
                    // consumer must not add to wake-up latency.
                    //
                    // `parse_line` recognizes both `%output` and the
                    // bare `%output` form; we mirror that here so the
                    // wake fires consistently for either.
                    let is_output = line == "%output" || line.starts_with("%output ");
                    let parsed = parse_line(line);
                    if is_output {
                        if let Some(ref t) = trail_tx {
                            // `send` errors only if the debouncer
                            // thread has exited (impossible while we
                            // still hold the only sender). Discard
                            // defensively.
                            let _ = t.send(());
                        }
                    }
                    if tx.send(parsed).is_err() {
                        // Receiver dropped: client is going away.
                        return;
                    }
                }
                let _ = tx.send(Line::Eof);
            });
        if let Err(err) = reader_spawn {
            // The Child is still running but the reader thread that
            // would have driven it to completion never started.
            // `std::process::Child` does NOT reap on Drop, so without
            // this explicit cleanup we'd leak a zombie/orphan tmux
            // process. Best-effort: any error during cleanup is
            // swallowed (we're already on the error path).
            let _ = child.kill();
            let _ = child.wait();
            return Err(anyhow::Error::from(err)).context("spawn control-mode reader thread");
        }

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

/// Trailing-edge debouncer for `%output` wakes. Blocks on the channel
/// for the first tick, then extends with `recv_timeout`: each new tick
/// resets the debounce window, a timeout fires the callback, a
/// disconnect (reader thread gone) exits cleanly. Lives on its own
/// thread so a slow callback never blocks the reader.
///
/// Visible at module level for the unit test below.
fn debouncer_loop(rx: Receiver<()>, cb: Box<dyn Fn() + Send + 'static>) {
    loop {
        // Block on the first event of a quiet period.
        if rx.recv().is_err() {
            return;
        }
        // Extend the debounce on each subsequent tick; fire when the
        // window expires; exit when the sender drops.
        loop {
            match rx.recv_timeout(OUTPUT_WAKE_DEBOUNCE) {
                Ok(()) => continue,
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
        cb();
    }
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

    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    /// Send N `()` ticks back to back over `tx` to mimic a tmux burst:
    /// the reader thread would send one per `%output` line in tight
    /// succession.
    fn fire_burst(tx: &std::sync::mpsc::Sender<()>, count: usize) {
        for _ in 0..count {
            tx.send(()).unwrap();
        }
    }

    #[test]
    fn debouncer_fires_once_per_burst() {
        // A tight burst (20 ticks back-to-back with no inter-tick
        // sleep) should result in exactly one trailing-edge callback,
        // not 20 leading-edge callbacks. Regression guard for the
        // partial-frame artifact this commit fixes.
        let (tx, rx) = channel::<()>();
        let count = Arc::new(AtomicUsize::new(0));
        let count_clone = count.clone();
        let handle = thread::spawn(move || {
            debouncer_loop(
                rx,
                Box::new(move || {
                    count_clone.fetch_add(1, Ordering::SeqCst);
                }),
            );
        });

        fire_burst(&tx, 20);
        // Wait well past the debounce window so the trailing edge has
        // had time to fire. 5x the debounce is generous.
        thread::sleep(OUTPUT_WAKE_DEBOUNCE * 5);
        assert_eq!(
            count.load(Ordering::SeqCst),
            1,
            "tight 20-tick burst should produce one trailing wake, not many leading wakes"
        );

        drop(tx);
        handle.join().expect("debouncer thread join");
    }

    #[test]
    fn debouncer_handles_quiet_periods_between_bursts() {
        // Two bursts separated by more than the debounce window. Each
        // burst should produce its own trailing-edge wake (2 total).
        let (tx, rx) = channel::<()>();
        let count = Arc::new(AtomicUsize::new(0));
        let count_clone = count.clone();
        let handle = thread::spawn(move || {
            debouncer_loop(
                rx,
                Box::new(move || {
                    count_clone.fetch_add(1, Ordering::SeqCst);
                }),
            );
        });

        fire_burst(&tx, 5);
        // First burst's trailing edge must fire before we start the
        // next burst, otherwise the second burst just extends the
        // first window. 3x debounce is more than enough.
        thread::sleep(OUTPUT_WAKE_DEBOUNCE * 3);
        fire_burst(&tx, 5);
        thread::sleep(OUTPUT_WAKE_DEBOUNCE * 3);

        assert_eq!(
            count.load(Ordering::SeqCst),
            2,
            "two bursts separated by a quiet period should produce two wakes"
        );

        drop(tx);
        handle.join().expect("debouncer thread join");
    }

    #[test]
    fn debouncer_exits_on_sender_disconnect() {
        // When the reader thread drops its sender (process EOF or
        // explicit shutdown), the debouncer must exit on its own so
        // its thread doesn't leak. Without this, every spawn would
        // eventually accumulate stuck debouncer threads.
        let (tx, rx) = channel::<()>();
        let handle = thread::spawn(move || {
            debouncer_loop(rx, Box::new(|| {}));
        });
        // Fire one tick and then drop tx so the debouncer is mid-burst
        // when the sender disappears.
        tx.send(()).unwrap();
        drop(tx);

        // Join with a timeout via `thread::JoinHandle::join`, which
        // blocks indefinitely. To bound the test, race it against a
        // sleep on a dedicated thread; if join takes longer than
        // a generous 1s, panic with a clear message.
        let (done_tx, done_rx) = channel();
        thread::spawn(move || {
            let _ = handle.join();
            let _ = done_tx.send(());
        });
        assert!(
            done_rx.recv_timeout(Duration::from_secs(1)).is_ok(),
            "debouncer thread did not exit after sender drop"
        );
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
