//! Handlers for ACP `terminal/*` requests.
//!
//! ACP terminal methods let agents create a terminal session, read its
//! output, wait for exit, kill it, or release it. aoe runs the command in
//! the session's worktree (or sandbox container if applicable). This is
//! the place where the existing aoe sandbox/worktree security applies to
//! the agent's command execution.
//!
//! For the MVP we keep the surface narrow: spawn a one-shot command,
//! capture stdout+stderr to a string buffer, and return on exit. Long-
//! running terminals (e.g. live `tail -f`) are a follow-up.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use thiserror::Error;
use tokio::io::AsyncReadExt;
use tokio::process::Command;
use tokio::sync::Mutex;
use tracing::info;

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
#[derive(Debug, Clone)]
pub struct TerminalOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
}

/// Per-session terminal manager. Holds outputs of completed terminals so
/// the agent can fetch them via `terminal/output` even after exit.
#[derive(Debug, Clone, Default)]
pub struct TerminalManager {
    inner: Arc<Mutex<TerminalManagerInner>>,
}

#[derive(Debug, Default)]
struct TerminalManagerInner {
    outputs: std::collections::HashMap<TerminalId, TerminalOutput>,
}

impl TerminalManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Spawn a one-shot terminal: run a command, wait for exit, capture
    /// stdout/stderr. The terminal id is generated from a counter. Returns
    /// the id immediately; the caller should `wait_for_exit` (or trust
    /// `output` after a brief delay) for results.
    ///
    /// `cwd` is the working directory; the caller is responsible for
    /// passing the session's worktree path.
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

        let mut stdout_buf = String::new();
        let mut stderr_buf = String::new();
        if let Some(mut stdout) = child.stdout.take() {
            stdout.read_to_string(&mut stdout_buf).await?;
        }
        if let Some(mut stderr) = child.stderr.take() {
            stderr.read_to_string(&mut stderr_buf).await?;
        }
        let status = child.wait().await?;
        let output = TerminalOutput {
            stdout: stdout_buf,
            stderr: stderr_buf,
            exit_code: status.code(),
        };

        self.inner.lock().await.outputs.insert(id.clone(), output);
        Ok(id)
    }

    /// Returns the captured output of a terminal. Implements ACP
    /// `terminal/output` and `terminal/wait_for_exit` for the one-shot
    /// case where the terminal has already finished.
    pub async fn output(&self, terminal_id: &str) -> Result<TerminalOutput, TerminalError> {
        let inner = self.inner.lock().await;
        inner
            .outputs
            .get(terminal_id)
            .cloned()
            .ok_or_else(|| TerminalError::UnknownTerminal(terminal_id.into()))
    }

    /// Implements ACP `terminal/release` by dropping the captured output.
    pub async fn release(&self, terminal_id: &str) -> Result<(), TerminalError> {
        let mut inner = self.inner.lock().await;
        if inner.outputs.remove(terminal_id).is_none() {
            return Err(TerminalError::UnknownTerminal(terminal_id.into()));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn create_runs_and_captures_output() {
        let mgr = TerminalManager::new();
        let cwd = std::env::temp_dir();
        let id = mgr
            .create_and_run("s-1", "echo", vec!["hello".into()], cwd)
            .await
            .unwrap();
        let out = mgr.output(&id).await.unwrap();
        assert!(out.stdout.contains("hello"));
        assert_eq!(out.exit_code, Some(0));
    }

    #[tokio::test]
    async fn release_removes_terminal() {
        let mgr = TerminalManager::new();
        let cwd = std::env::temp_dir();
        let id = mgr
            .create_and_run("s-1", "true", vec![], cwd)
            .await
            .unwrap();
        mgr.release(&id).await.unwrap();
        let result = mgr.output(&id).await;
        assert!(matches!(result, Err(TerminalError::UnknownTerminal(_))));
    }
}
