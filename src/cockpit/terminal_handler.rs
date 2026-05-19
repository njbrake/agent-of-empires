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

use crate::containers::container_interface::{docker_env_args, EnvEntry};

/// Routing target for a terminal command. Built once per session in
/// `SessionResources::sandbox` and consulted on every `terminal/create`.
#[derive(Debug, Clone)]
pub struct TerminalSandbox {
    pub container_name: String,
    /// Resolved env entries to forward into the container for this command.
    ///
    /// Without this, ACP `terminal/create` would silently rely on whatever
    /// env was baked into the container at `docker run` time (which is set
    /// once and never updated when host vars change or rotate). The tmux
    /// session path already re-resolves per exec; this brings the agent's
    /// shell-command path to parity.
    pub env_entries: Vec<EnvEntry>,
}

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

/// Build the `docker exec` argv and inherit-env pairs for a sandboxed
/// `terminal/create` request. Pulled out of `create_and_run` so the wiring
/// can be unit tested without spawning docker. The runtime binary is
/// intentionally not in the result, because the caller picks it based on
/// the active runtime.
pub(crate) fn build_sandbox_exec_args(
    sandbox: &TerminalSandbox,
    cwd: &std::path::Path,
    command: &str,
    args: &[String],
) -> (Vec<String>, Vec<(String, String)>) {
    let (env_argv, inherit_pairs) = docker_env_args(&sandbox.env_entries);
    let mut full_args: Vec<String> = vec![
        "exec".into(),
        "-w".into(),
        cwd.to_string_lossy().into_owned(),
    ];
    full_args.extend(env_argv);
    full_args.push(sandbox.container_name.clone());
    full_args.push(command.to_string());
    full_args.extend(args.iter().cloned());
    (full_args, inherit_pairs)
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
    /// passing the session's worktree path. When `sandbox` is `Some`
    /// the command is routed through `docker exec` so it runs inside
    /// the session's sandbox container; `cwd` is interpreted as a
    /// container path in that case (the agent already speaks in
    /// container paths).
    pub async fn create_and_run(
        &self,
        session_id: &str,
        command: &str,
        args: Vec<String>,
        cwd: PathBuf,
        sandbox: Option<&TerminalSandbox>,
    ) -> Result<TerminalId, TerminalError> {
        let id = format!("term-{}", uuid::Uuid::new_v4().simple());
        info!(
            target: "cockpit.terminal",
            session = %session_id,
            terminal = %id,
            command = %command,
            cwd = %cwd.display(),
            sandboxed = sandbox.is_some(),
            "terminal/create"
        );

        let mut child = match sandbox {
            Some(s) => {
                let runtime = crate::containers::get_container_runtime();
                let binary = runtime.base.binary;
                let (full_args, inherit_pairs) = build_sandbox_exec_args(s, &cwd, command, &args);
                let mut cmd = Command::new(binary);
                cmd.args(&full_args)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped());
                for (k, v) in inherit_pairs {
                    cmd.env(k, v);
                }
                cmd.spawn()?
            }
            None => Command::new(command)
                .args(&args)
                .current_dir(&cwd)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()?,
        };

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
            .create_and_run("s-1", "echo", vec!["hello".into()], cwd, None)
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
            .create_and_run("s-1", "true", vec![], cwd, None)
            .await
            .unwrap();
        mgr.release(&id).await.unwrap();
        let result = mgr.output(&id).await;
        assert!(matches!(result, Err(TerminalError::UnknownTerminal(_))));
    }

    #[test]
    fn sandbox_exec_args_emit_e_flags_for_each_entry() {
        let sandbox = TerminalSandbox {
            container_name: "aoe-sandbox-test".into(),
            env_entries: vec![
                EnvEntry::Inherit {
                    key: "GH_TOKEN".into(),
                    value: "ghp_secret".into(),
                },
                EnvEntry::Literal {
                    key: "TERM".into(),
                    value: "xterm".into(),
                },
            ],
        };
        let (argv, inherit) = build_sandbox_exec_args(
            &sandbox,
            std::path::Path::new("/workspace"),
            "gh",
            &["pr".into(), "list".into()],
        );

        // exec -w /workspace [-e flags...] container cmd [args...]
        assert_eq!(argv[0], "exec");
        assert_eq!(argv[1], "-w");
        assert_eq!(argv[2], "/workspace");
        // Both -e flags must appear before the container name.
        let container_idx = argv
            .iter()
            .position(|a| a == "aoe-sandbox-test")
            .expect("container name in argv");
        let e_positions: Vec<usize> = argv
            .iter()
            .enumerate()
            .filter_map(|(i, a)| (a == "-e").then_some(i))
            .collect();
        assert_eq!(e_positions.len(), 2, "one -e flag per env entry");
        for pos in &e_positions {
            assert!(
                *pos < container_idx,
                "-e flags must precede the container name"
            );
        }
        // Inherit value must NOT leak into argv.
        assert!(
            !argv.iter().any(|a| a.contains("ghp_secret")),
            "secret leaked into argv: {:?}",
            argv
        );
        // Inherit pairs carry the actual value for cmd.env(k, v).
        assert_eq!(
            inherit,
            vec![("GH_TOKEN".to_string(), "ghp_secret".to_string())]
        );
        // Command + args appear after the container name.
        assert_eq!(argv[container_idx + 1], "gh");
        assert_eq!(argv[container_idx + 2], "pr");
        assert_eq!(argv[container_idx + 3], "list");
    }

    #[test]
    fn sandbox_exec_args_no_env_entries() {
        let sandbox = TerminalSandbox {
            container_name: "aoe-sandbox-test".into(),
            env_entries: vec![],
        };
        let (argv, inherit) = build_sandbox_exec_args(
            &sandbox,
            std::path::Path::new("/workspace"),
            "echo",
            &["hi".into()],
        );
        assert_eq!(
            argv,
            vec![
                "exec".to_string(),
                "-w".to_string(),
                "/workspace".to_string(),
                "aoe-sandbox-test".to_string(),
                "echo".to_string(),
                "hi".to_string(),
            ]
        );
        assert!(inherit.is_empty());
    }
}
