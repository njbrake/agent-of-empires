//! Handlers for ACP `terminal/*` requests.
//!
//! ACP terminal methods let agents create a terminal session, read its
//! output, wait for exit, kill it, or release it. aoe runs the command in
//! the session's worktree (or sandbox container if applicable). This is
//! the place where the existing aoe sandbox/worktree security applies to
//! the agent's command execution.
//!
//! `create_and_run` is non-blocking: spawn the child, kick off a
//! background reader, return the terminal id immediately. Subsequent
//! `output` calls return whatever's currently buffered; `wait_for_exit`
//! parks on a per-terminal `Notify` until the child completes; `kill`
//! sends SIGTERM via `start_kill`. Combined buffer size is capped at
//! `max_bytes`; older bytes are dropped from the head once the cap is
//! exceeded and the count is surfaced in `truncated_head_bytes`. See
//! #1075.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use thiserror::Error;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::{Mutex, Notify};
use tracing::{info, warn};

#[derive(Debug, Error)]
pub enum TerminalError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("terminal {0} does not exist")]
    UnknownTerminal(String),
}

/// Identifier returned to the agent on `terminal/create`.
pub type TerminalId = String;

/// One terminal's captured output and exit status.
#[derive(Debug, Clone, Default)]
pub struct TerminalOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    /// Total bytes dropped from the head of the combined buffer to
    /// keep the in-memory size under `max_bytes`. Surfaced so the
    /// renderer can show "…truncated N bytes…" instead of silently
    /// losing the head. See #1075 layer E.
    pub truncated_head_bytes: u64,
}

/// Per-session terminal manager. Holds outputs of running and completed
/// terminals so the agent can fetch them via `terminal/output` even
/// after exit.
#[derive(Debug, Clone)]
pub struct TerminalManager {
    inner: Arc<Mutex<TerminalManagerInner>>,
    /// Soft cap on the combined `stdout + stderr` buffer per terminal.
    /// Snapshotted from `[cockpit] terminal_output_max_bytes` at
    /// manager construction; floored at 16 KiB so a misconfigured
    /// value can't render the partial-output view useless.
    max_bytes: usize,
}

const DEFAULT_MAX_BYTES: usize = 256 * 1024;
const MIN_MAX_BYTES: usize = 16 * 1024;

#[derive(Default, Debug)]
struct TerminalManagerInner {
    terminals: std::collections::HashMap<TerminalId, TerminalEntry>,
}

#[derive(Debug)]
struct TerminalEntry {
    output: TerminalOutput,
    exit_notify: Arc<Notify>,
    /// Set while the child is alive; cleared when the reader task
    /// observes exit. Wrapping in Arc<Mutex<_>> so `kill` can grab a
    /// strong reference without holding the manager's outer lock
    /// across the await.
    child: Option<Arc<Mutex<Child>>>,
}

impl Default for TerminalManager {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalManager {
    pub fn new() -> Self {
        Self::with_max_bytes(DEFAULT_MAX_BYTES)
    }

    pub fn with_max_bytes(max_bytes: usize) -> Self {
        Self {
            inner: Arc::new(Mutex::new(TerminalManagerInner::default())),
            max_bytes: max_bytes.max(MIN_MAX_BYTES),
        }
    }

    /// Spawn a terminal subprocess and return its id immediately. A
    /// background task reads stdout/stderr line-by-line into the
    /// per-terminal buffer; `output` returns whatever's currently
    /// buffered (with `exit_code = None` while the child is alive)
    /// and `wait_for_exit` parks until the child completes.
    pub async fn create_and_run(
        &self,
        session_id: &str,
        command: &str,
        args: Vec<String>,
        cwd: PathBuf,
    ) -> Result<TerminalId, TerminalError> {
        let id = format!("term-{}", uuid::Uuid::new_v4().simple());
        info!(
            target: "cockpit.terminal",
            session = %session_id,
            terminal = %id,
            command = %command,
            cwd = %cwd.display(),
            "terminal/create"
        );

        let mut child = Command::new(command)
            .args(&args)
            .current_dir(&cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let child_handle = Arc::new(Mutex::new(child));

        let exit_notify = Arc::new(Notify::new());

        {
            let mut inner = self.inner.lock().await;
            inner.terminals.insert(
                id.clone(),
                TerminalEntry {
                    output: TerminalOutput::default(),
                    exit_notify: exit_notify.clone(),
                    child: Some(child_handle.clone()),
                },
            );
        }

        let inner_for_task = self.inner.clone();
        let max_bytes = self.max_bytes;
        let session_label = session_id.to_owned();
        let terminal_label = id.clone();
        tokio::spawn(async move {
            let stdout_task = stdout.map(|s| {
                let inner = inner_for_task.clone();
                let id = terminal_label.clone();
                tokio::spawn(async move {
                    let mut reader = BufReader::new(s).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        append_to_terminal(&inner, &id, true, &line, max_bytes).await;
                    }
                })
            });
            let stderr_task = stderr.map(|s| {
                let inner = inner_for_task.clone();
                let id = terminal_label.clone();
                tokio::spawn(async move {
                    let mut reader = BufReader::new(s).lines();
                    while let Ok(Some(line)) = reader.next_line().await {
                        append_to_terminal(&inner, &id, false, &line, max_bytes).await;
                    }
                })
            });
            if let Some(t) = stdout_task {
                let _ = t.await;
            }
            if let Some(t) = stderr_task {
                let _ = t.await;
            }

            let status_result = {
                let mut guard = child_handle.lock().await;
                guard.wait().await
            };
            let exit_code = match status_result {
                Ok(s) => s.code(),
                Err(e) => {
                    warn!(
                        target: "cockpit.terminal",
                        session = %session_label,
                        terminal = %terminal_label,
                        error = %e,
                        "child wait failed"
                    );
                    None
                }
            };
            {
                let mut guard = inner_for_task.lock().await;
                if let Some(entry) = guard.terminals.get_mut(&terminal_label) {
                    // None from `status.code()` covers signal-terminated
                    // children (kill via `start_kill`, for example).
                    // Surface -1 so the agent observes a non-success
                    // and the renderer can flag the terminal as ended.
                    entry.output.exit_code = exit_code.or(Some(-1));
                    entry.child = None;
                }
            }
            exit_notify.notify_waiters();
        });

        Ok(id)
    }

    /// Snapshot of the captured output so far. While the terminal is
    /// still running, `exit_code` is `None` and the buffers reflect
    /// whatever the reader task has consumed up to this point.
    pub async fn output(&self, terminal_id: &str) -> Result<TerminalOutput, TerminalError> {
        let inner = self.inner.lock().await;
        inner
            .terminals
            .get(terminal_id)
            .map(|e| e.output.clone())
            .ok_or_else(|| TerminalError::UnknownTerminal(terminal_id.into()))
    }

    /// Block until the terminal exits, then return its final output.
    /// Returns immediately if the child has already exited.
    pub async fn wait_for_exit(&self, terminal_id: &str) -> Result<TerminalOutput, TerminalError> {
        let notify = {
            let inner = self.inner.lock().await;
            let entry = inner
                .terminals
                .get(terminal_id)
                .ok_or_else(|| TerminalError::UnknownTerminal(terminal_id.into()))?;
            if entry.output.exit_code.is_some() {
                return Ok(entry.output.clone());
            }
            entry.exit_notify.clone()
        };
        notify.notified().await;
        let inner = self.inner.lock().await;
        inner
            .terminals
            .get(terminal_id)
            .map(|e| e.output.clone())
            .ok_or_else(|| TerminalError::UnknownTerminal(terminal_id.into()))
    }

    /// Send SIGTERM to the underlying child. The reader task observes
    /// the resulting EOF on stdout/stderr, the `wait` resolves, and the
    /// exit-notify fires; no separate cancellation path needed.
    pub async fn kill(&self, terminal_id: &str) -> Result<(), TerminalError> {
        let child = {
            let inner = self.inner.lock().await;
            let entry = inner
                .terminals
                .get(terminal_id)
                .ok_or_else(|| TerminalError::UnknownTerminal(terminal_id.into()))?;
            entry.child.clone()
        };
        if let Some(c) = child {
            let mut guard = c.lock().await;
            let _ = guard.start_kill();
        }
        Ok(())
    }

    /// Implements ACP `terminal/release` by dropping the captured output.
    pub async fn release(&self, terminal_id: &str) -> Result<(), TerminalError> {
        let mut inner = self.inner.lock().await;
        if inner.terminals.remove(terminal_id).is_none() {
            return Err(TerminalError::UnknownTerminal(terminal_id.into()));
        }
        Ok(())
    }
}

async fn append_to_terminal(
    inner: &Arc<Mutex<TerminalManagerInner>>,
    terminal_id: &str,
    is_stdout: bool,
    line: &str,
    max_bytes: usize,
) {
    let mut guard = inner.lock().await;
    let Some(entry) = guard.terminals.get_mut(terminal_id) else {
        return;
    };
    let buf = if is_stdout {
        &mut entry.output.stdout
    } else {
        &mut entry.output.stderr
    };
    buf.push_str(line);
    buf.push('\n');

    // Cap the combined buffer. We measure stdout + stderr together
    // because that's what the renderer shows; trim from stdout first
    // (typically the larger contributor) then fall through to stderr
    // if necessary.
    let total = entry.output.stdout.len() + entry.output.stderr.len();
    if total > max_bytes {
        let overflow = total - max_bytes;
        let from_stdout = entry.output.stdout.len().min(overflow);
        if from_stdout > 0 {
            let pos = ceil_char_boundary(&entry.output.stdout, from_stdout);
            entry.output.stdout.drain(..pos);
            entry.output.truncated_head_bytes += pos as u64;
        }
        let still_over = entry.output.stdout.len() + entry.output.stderr.len();
        if still_over > max_bytes {
            let from_stderr = still_over - max_bytes;
            let pos = ceil_char_boundary(&entry.output.stderr, from_stderr);
            entry.output.stderr.drain(..pos);
            entry.output.truncated_head_bytes += pos as u64;
        }
    }
}

/// Round `idx` up to the next UTF-8 char boundary in `s` (or to the
/// end of the string). Strings must remain valid UTF-8 when we drain;
/// `String::drain` panics on a mid-codepoint cut.
fn ceil_char_boundary(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    let mut p = idx;
    while p < s.len() && !s.is_char_boundary(p) {
        p += 1;
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{sleep, timeout, Duration};

    #[tokio::test]
    async fn create_runs_and_captures_output() {
        let mgr = TerminalManager::new();
        let cwd = std::env::temp_dir();
        let id = mgr
            .create_and_run("s-1", "echo", vec!["hello".into()], cwd)
            .await
            .unwrap();
        let out = mgr.wait_for_exit(&id).await.unwrap();
        assert!(out.stdout.contains("hello"));
        assert_eq!(out.exit_code, Some(0));
    }

    #[tokio::test]
    async fn output_returns_partial_while_running() {
        let mgr = TerminalManager::new();
        let cwd = std::env::temp_dir();
        // bash so we can interleave prints with sleeps in one command;
        // skip test gracefully when bash isn't on PATH (unlikely in CI).
        if which::which("bash").is_err() {
            return;
        }
        let id = mgr
            .create_and_run(
                "s-1",
                "bash",
                vec!["-c".into(), "echo first; sleep 1; echo second".into()],
                cwd,
            )
            .await
            .unwrap();
        // Poll briefly for "first" to arrive without waiting for "second".
        let mut saw_partial = false;
        for _ in 0..30 {
            let out = mgr.output(&id).await.unwrap();
            if out.stdout.contains("first") && !out.stdout.contains("second") {
                saw_partial = true;
                assert!(out.exit_code.is_none());
                break;
            }
            sleep(Duration::from_millis(100)).await;
        }
        assert!(saw_partial, "expected to observe partial output mid-run");
        // Drain the rest so the reader task doesn't outlive the test.
        let _ = timeout(Duration::from_secs(5), mgr.wait_for_exit(&id)).await;
    }

    #[tokio::test]
    async fn release_removes_terminal() {
        let mgr = TerminalManager::new();
        let cwd = std::env::temp_dir();
        let id = mgr
            .create_and_run("s-1", "true", vec![], cwd)
            .await
            .unwrap();
        let _ = mgr.wait_for_exit(&id).await;
        mgr.release(&id).await.unwrap();
        let result = mgr.output(&id).await;
        assert!(matches!(result, Err(TerminalError::UnknownTerminal(_))));
    }

    #[tokio::test]
    async fn cap_enforced_drops_head_bytes() {
        // 32 KiB cap; floor is 16 KiB so the cap is honored.
        let mgr = TerminalManager::with_max_bytes(32 * 1024);
        if which::which("bash").is_err() {
            return;
        }
        let cwd = std::env::temp_dir();
        // Print 200 lines of "X..." each ~256 bytes ⇒ ~50 KiB total.
        let id = mgr
            .create_and_run(
                "s-1",
                "bash",
                vec![
                    "-c".into(),
                    "for i in $(seq 1 200); do head -c 256 < /dev/urandom | base64 -w0; echo; done"
                        .into(),
                ],
                cwd,
            )
            .await
            .unwrap();
        let out = mgr.wait_for_exit(&id).await.unwrap();
        assert!(out.exit_code == Some(0));
        assert!(out.truncated_head_bytes > 0, "expected head truncation");
        assert!(out.stdout.len() <= 32 * 1024 + 1024, "buffer overran cap");
    }

    #[tokio::test]
    async fn kill_terminates_running_child() {
        if which::which("bash").is_err() {
            return;
        }
        let mgr = TerminalManager::new();
        let cwd = std::env::temp_dir();
        let id = mgr
            .create_and_run("s-1", "bash", vec!["-c".into(), "sleep 30".into()], cwd)
            .await
            .unwrap();
        // Let it actually start before signalling.
        sleep(Duration::from_millis(100)).await;
        mgr.kill(&id).await.unwrap();
        let out = timeout(Duration::from_secs(5), mgr.wait_for_exit(&id))
            .await
            .expect("kill should resolve wait_for_exit within 5s")
            .unwrap();
        // Signal-terminated → exit_code surfaces as -1 (status.code() is None).
        assert!(out.exit_code.is_some());
    }
}
