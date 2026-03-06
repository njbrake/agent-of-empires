//! Session instance definition and operations

use std::path::Path;
use std::process::Stdio;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use nix::sys::signal::Signal;
use nix::unistd::Pid;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::containers::{self, ContainerRuntimeInterface, DockerContainer};
use crate::tmux;

use super::container_config;
use super::environment::{build_docker_env_args, shell_escape};

const OPENCODE_MAX_RETRY_ATTEMPTS: u32 = 3;
const OPENCODE_RETRY_DELAY: Duration = Duration::from_secs(2);
const OPENCODE_COMMAND_TIMEOUT: Duration = Duration::from_secs(5);
const OPENCODE_CAPTURE_DEADLINE: Duration = Duration::from_secs(15);

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalInfo {
    #[serde(default)]
    pub created: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Running,
    Waiting,
    #[default]
    Idle,
    Unknown,
    Stopped,
    Error,
    Starting,
    Deleting,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeInfo {
    pub branch: String,
    pub main_repo_path: String,
    pub managed_by_aoe: bool,
    pub created_at: DateTime<Utc>,
    #[serde(default = "default_true")]
    pub cleanup_on_delete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxInfo {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_id: Option<String>,
    pub image: String,
    pub container_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    /// Additional environment entries (session-specific).
    /// `KEY` = pass through from host, `KEY=VALUE` = set explicitly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_env: Option<Vec<String>>,
    /// Custom instruction text to inject into agent launch command
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_instruction: Option<String>,
}

/// Deserializer for agent_session_id that converts empty and whitespace-only strings to None
fn deserialize_session_id<'de, D>(deserializer: D) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(deserializer)?;
    Ok(opt.filter(|s| !s.trim().is_empty()))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    pub id: String,
    pub title: String,
    pub project_path: String,
    #[serde(default)]
    pub group_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default)]
    pub command: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub extra_args: String,
    #[serde(default)]
    pub tool: String,
    #[serde(default)]
    pub yolo_mode: bool,
    #[serde(default)]
    pub status: Status,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_accessed_at: Option<DateTime<Utc>>,

    // Git worktree integration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_info: Option<WorktreeInfo>,

    // Docker sandbox integration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_info: Option<SandboxInfo>,

    // Paired terminal session
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_info: Option<TerminalInfo>,

    // Agent session ID for conversation persistence
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_session_id"
    )]
    pub agent_session_id: Option<String>,

    // Runtime state (not serialized)
    #[serde(skip)]
    pub last_error_check: Option<std::time::Instant>,
    #[serde(skip)]
    pub last_start_time: Option<std::time::Instant>,
    #[serde(skip)]
    pub last_error: Option<String>,
}

/// Generate a new UUID for Claude Code session.
///
/// Creates a new universally unique identifier (UUID v4) for use as a Claude Code session ID.
/// This function is used when a new Claude Code session is started without an existing session
/// to resume from. The UUID is converted to a string representation suitable for passing to
/// Claude Code CLI commands.
fn generate_claude_session_id() -> String {
    Uuid::new_v4().to_string()
}

/// Capture session ID from OpenCode CLI with retry logic.
///
/// Attempts up to 3 times to capture an OpenCode session ID, with 2-second delays between
/// retries. Each attempt executes `opencode session list --format json` with a 5-second
/// timeout. When a `project_path` is provided, sessions are filtered to prefer those matching
/// the project directory, sorted by most recently updated. Falls back to the first session
/// if no directory match is found.
///
/// # Errors
///
/// Returns an error if all attempts fail due to:
/// - The `opencode` command cannot be spawned
/// - The command execution fails before completing
/// - The command times out after 5 seconds
/// - The command exits with a non-zero status code
/// - The JSON output cannot be parsed
/// - No sessions are found in the response
fn capture_opencode_session_id(project_path: &str) -> Result<String> {
    let deadline = std::time::Instant::now() + OPENCODE_CAPTURE_DEADLINE;
    let mut last_err = None;

    for attempt in 0..OPENCODE_MAX_RETRY_ATTEMPTS {
        if attempt > 0 {
            if std::time::Instant::now() + OPENCODE_RETRY_DELAY > deadline {
                break;
            }
            std::thread::sleep(OPENCODE_RETRY_DELAY);
            tracing::debug!(
                "Retrying OpenCode session capture (attempt {})",
                attempt + 1
            );
        }

        match try_capture_opencode_session_id(project_path) {
            Ok(id) => return Ok(id),
            Err(e) => {
                tracing::debug!(
                    "OpenCode session capture attempt {} failed: {}",
                    attempt + 1,
                    e
                );
                last_err = Some(e);
            }
        }
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("OpenCode session capture timed out")))
}

/// Single attempt to capture an OpenCode session ID.
///
/// Spawns `opencode session list --format json` with a 5-second timeout, parses the JSON,
/// and selects the best matching session based on project directory and update time.
fn try_capture_opencode_session_id(project_path: &str) -> Result<String> {
    let child = std::process::Command::new("opencode")
        .args(["session", "list", "--format", "json"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to spawn 'opencode session list'")?;

    let child_id = child.id();
    let (tx, rx) = mpsc::channel();

    std::thread::spawn(move || {
        let _ = tx.send(child.wait_with_output());
    });

    let output = match rx.recv_timeout(OPENCODE_COMMAND_TIMEOUT) {
        Ok(Ok(out)) => out,
        Ok(Err(e)) => return Err(anyhow::anyhow!("Failed to execute opencode: {}", e)),
        Err(_) => {
            tracing::debug!("OpenCode session list timed out");
            let _ = nix::sys::signal::kill(Pid::from_raw(child_id as i32), Signal::SIGKILL);
            return Err(anyhow::anyhow!("OpenCode session list timed out"));
        }
    };

    if !output.status.success() {
        anyhow::bail!("OpenCode session list command failed");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let sessions: Vec<serde_json::Value> =
        serde_json::from_str(&stdout).context("Failed to parse OpenCode session list JSON")?;

    // Try to match by project directory first
    let canonical_path = std::fs::canonicalize(project_path)
        .unwrap_or_else(|_| std::path::PathBuf::from(project_path));
    let canonical_str = canonical_path.to_string_lossy();

    let mut matching: Vec<&serde_json::Value> = sessions
        .iter()
        .filter(|s| {
            s.get("directory")
                .or_else(|| s.get("path"))
                .and_then(|v| v.as_str())
                .map(|dir| {
                    let session_path = std::fs::canonicalize(dir)
                        .unwrap_or_else(|_| std::path::PathBuf::from(dir));
                    session_path.to_string_lossy() == canonical_str
                })
                .unwrap_or(false)
        })
        .collect();

    // Sort by updated time (most recent first).
    // OpenCode stores `updated` as a numeric epoch (Date.now() milliseconds), not a string.
    matching.sort_by(|a, b| {
        let a_time = a.get("updated").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let b_time = b.get("updated").and_then(|v| v.as_f64()).unwrap_or(0.0);
        b_time
            .partial_cmp(&a_time)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Use directory match if found, otherwise fall back to first session
    let session = matching.first().copied().or_else(|| sessions.first());

    session
        .and_then(|s| s["id"].as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("No OpenCode sessions found"))
}

/// Capture session ID from Codex filesystem.
///
/// Walks the Codex sessions directory (including date-partitioned `YYYY/MM/DD/` subdirectories)
/// for `.jsonl` rollout files and extracts the UUID from the most recent one.
/// Codex filenames follow the pattern `rollout-<timestamp>-<uuid>.jsonl`.
/// Respects `CODEX_HOME` env var, falling back to `~/.codex`.
fn capture_codex_session_id(project_path: &str) -> Result<String> {
    let codex_home = match std::env::var("CODEX_HOME") {
        Ok(val) => std::path::PathBuf::from(val),
        Err(_) => dirs::home_dir()
            .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?
            .join(".codex"),
    };
    let sessions_dir = codex_home.join("sessions");

    if !sessions_dir.exists() {
        anyhow::bail!(
            "Codex sessions directory not found: {}",
            sessions_dir.display()
        );
    }

    let mut session_entries: Vec<(std::path::PathBuf, std::time::SystemTime)> = Vec::new();
    collect_codex_sessions(&sessions_dir, &mut session_entries)?;

    if session_entries.is_empty() {
        anyhow::bail!("No Codex sessions found in {}", sessions_dir.display());
    }

    session_entries.sort_by(|a, b| b.1.cmp(&a.1));

    let canonical_project = std::fs::canonicalize(project_path)
        .unwrap_or_else(|_| std::path::PathBuf::from(project_path));

    // Prefer the most recent session whose CWD matches the project directory
    let cwd_match = session_entries.iter().find(|(path, _)| {
        extract_codex_cwd_from_file(path)
            .and_then(|cwd| std::fs::canonicalize(&cwd).ok())
            .map(|cwd| cwd == canonical_project)
            .unwrap_or(false)
    });

    let chosen = cwd_match
        .or_else(|| session_entries.first())
        .and_then(|(path, _)| extract_codex_uuid_from_filename(path));

    chosen.ok_or_else(|| anyhow::anyhow!("No valid Codex session files found"))
}

/// Extract UUID from a Codex rollout filename.
///
/// Codex filenames follow the pattern `rollout-YYYY-MM-DDThh-mm-ss-<uuid>.jsonl`.
/// The UUID is the last hyphen-delimited segment before `.jsonl`, comprising 5 groups
/// (e.g. `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`).
fn extract_codex_uuid_from_filename(path: &std::path::Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    // Pattern: rollout-YYYY-MM-DDThh-mm-ss-<uuid>
    // The UUID is always 36 chars (8-4-4-4-12) at the end of the stem
    if stem.len() >= 36 {
        let candidate = &stem[stem.len() - 36..];
        if candidate.chars().filter(|&c| c == '-').count() == 4
            && candidate.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
        {
            return Some(candidate.to_string());
        }
    }
    // Fallback: return the full stem (for non-standard filenames or thread names)
    Some(stem.to_string())
}

/// Extract the working directory from a Codex rollout `.jsonl` file.
///
/// The first line is always a `session_meta` event containing `payload.cwd`.
/// Returns `None` if the file cannot be read or the field is missing.
fn extract_codex_cwd_from_file(path: &std::path::Path) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let reader = std::io::BufReader::new(file);
    let first_line = std::io::BufRead::lines(reader).next()?.ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&first_line).ok()?;
    parsed
        .get("payload")
        .and_then(|p| p.get("cwd"))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

/// Recursively collect Codex session `.jsonl` files, descending into date-partitioned dirs.
///
/// Directories whose names are all ASCII digits (e.g. `2025`, `03`, `06`) are treated as
/// date components and recursed into. Files ending in `.jsonl` are collected as session entries.
fn collect_codex_sessions(
    dir: &std::path::Path,
    entries: &mut Vec<(std::path::PathBuf, std::time::SystemTime)>,
) -> Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.chars().all(|c| c.is_ascii_digit()) {
                collect_codex_sessions(&path, entries)?;
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            entries.push((path, modified));
        }
    }
    Ok(())
}

/// Build resume flags for agent command.
///
/// Constructs a tool-specific command-line flag string to resume an existing session.
/// Each agent tool uses a different flag format, and for unrecognized tools returns
/// an empty string.
///
/// For Claude, the flag depends on whether conversation data already exists:
/// `--resume` only works for sessions that have prior conversation data, while
/// `--session-id` creates or attaches to a session unconditionally.
fn build_resume_flags(tool: &str, session_id: &str, is_existing_session: bool) -> String {
    match tool {
        "claude" if is_existing_session => format!("--resume {}", session_id),
        "claude" => format!("--session-id {}", session_id),
        "opencode" => format!("--session {}", session_id),
        "codex" => format!("resume {}", session_id),
        _ => String::new(),
    }
}

fn append_resume_flags(
    tool: &str,
    session_id: Option<&str>,
    is_existing_session: bool,
    cmd: &mut String,
    context: &str,
) {
    if let Some(session_id) = session_id {
        let resume_flags = build_resume_flags(tool, session_id, is_existing_session);
        if !resume_flags.is_empty() {
            *cmd = format!("{} {}", cmd, resume_flags);
            tracing::debug!(
                "Added resume flags to {} command: {}",
                context,
                resume_flags
            );
        }
    }
}

impl Instance {
    pub fn new(title: &str, project_path: &str) -> Self {
        Self {
            id: generate_id(),
            title: title.to_string(),
            project_path: project_path.to_string(),
            group_path: String::new(),
            parent_session_id: None,
            command: String::new(),
            extra_args: String::new(),
            tool: "claude".to_string(),
            yolo_mode: false,
            status: Status::Idle,
            created_at: Utc::now(),
            last_accessed_at: None,
            worktree_info: None,
            sandbox_info: None,
            terminal_info: None,
            agent_session_id: None,
            last_error_check: None,
            last_start_time: None,
            last_error: None,
        }
    }

    pub fn is_sub_session(&self) -> bool {
        self.parent_session_id.is_some()
    }

    pub fn is_sandboxed(&self) -> bool {
        self.sandbox_info.as_ref().is_some_and(|s| s.enabled)
    }

    pub fn is_yolo_mode(&self) -> bool {
        self.yolo_mode
    }

    /// Generate or capture session ID based on agent type.
    ///
    /// Idempotent method that acquires a session ID for the configured agent tool.
    /// If a session ID has already been obtained, it is returned immediately. Otherwise:
    /// - For Claude: generates a new UUID
    /// - For OpenCode: attempts to capture an existing session ID via CLI
    /// - For Codex: attempts to find an existing session in the filesystem
    /// - For other tools: returns None
    ///
    /// Any errors during capture (e.g., CLI timeout, missing sessions directory) are logged
    /// at debug level and treated as if no session ID was found. The session ID is cached
    /// in `self.agent_session_id` for future calls.
    ///
    /// Returns `(session_id, is_existing)` where `is_existing` indicates whether
    /// the session was previously persisted (relevant for Claude's flag selection).
    pub fn acquire_session_id(&mut self) -> (Option<String>, bool) {
        if self.agent_session_id.is_some() {
            return (self.agent_session_id.clone(), true);
        }

        let session_id = match self.tool.as_str() {
            "claude" => Some(generate_claude_session_id()),
            "opencode" | "codex" if self.is_sandboxed() => {
                self.acquire_session_id_from_container().or_else(|| {
                    tracing::debug!(
                        "Container capture failed for {}, falling back to host",
                        self.tool
                    );
                    self.acquire_session_id_from_host()
                })
            }
            "opencode" => capture_opencode_session_id(&self.project_path)
                .map_err(|e| tracing::debug!("Failed to capture OpenCode session ID: {}", e))
                .ok(),
            "codex" => capture_codex_session_id(&self.project_path)
                .map_err(|e| tracing::debug!("Failed to capture Codex session ID: {}", e))
                .ok(),
            _ => None,
        };

        if let Some(ref id) = session_id {
            tracing::debug!("Session ID for {}: {}", self.tool, id);
            self.agent_session_id = session_id.clone();
        }

        (session_id, false)
    }

    fn acquire_session_id_from_host(&self) -> Option<String> {
        match self.tool.as_str() {
            "opencode" => capture_opencode_session_id(&self.project_path)
                .map_err(|e| tracing::debug!("Failed to capture OpenCode session ID: {}", e))
                .ok(),
            "codex" => capture_codex_session_id(&self.project_path)
                .map_err(|e| tracing::debug!("Failed to capture Codex session ID: {}", e))
                .ok(),
            _ => None,
        }
    }

    fn acquire_session_id_from_container(&self) -> Option<String> {
        let container = DockerContainer::from_session_id(&self.id);
        if !container.is_running().unwrap_or(false) {
            tracing::debug!("Container not running for session {}", self.id);
            return None;
        }

        match self.tool.as_str() {
            "opencode" => {
                let output = container
                    .exec(&["opencode", "session", "list", "--format", "json"])
                    .map_err(|e| tracing::debug!("Container exec failed for opencode: {}", e))
                    .ok()?;

                if !output.status.success() {
                    tracing::debug!("opencode session list failed inside container");
                    return None;
                }

                let stdout = String::from_utf8_lossy(&output.stdout);
                let sessions: Vec<serde_json::Value> = serde_json::from_str(&stdout)
                    .map_err(|e| tracing::debug!("Failed to parse container opencode JSON: {}", e))
                    .ok()?;

                // In container context, just pick the most recently updated session.
                // OpenCode stores `updated` as a numeric epoch (Date.now() milliseconds).
                let mut sorted = sessions;
                sorted.sort_by(|a, b| {
                    let a_time = a.get("updated").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    let b_time = b.get("updated").and_then(|v| v.as_f64()).unwrap_or(0.0);
                    b_time
                        .partial_cmp(&a_time)
                        .unwrap_or(std::cmp::Ordering::Equal)
                });

                sorted
                    .first()
                    .and_then(|s| s["id"].as_str())
                    .map(|s| s.to_string())
            }
            "codex" => {
                let output = container
                    .exec(&[
                        "sh",
                        "-c",
                        "find /root/.codex/sessions/ -name '*.jsonl' -printf '%T@ %p\\n' | sort -rn | head -1",
                    ])
                    .map_err(|e| tracing::debug!("Container exec failed for codex: {}", e))
                    .ok()?;

                if !output.status.success() {
                    tracing::debug!("find codex sessions failed inside container");
                    return None;
                }

                let stdout = String::from_utf8_lossy(&output.stdout);
                stdout
                    .lines()
                    .next()
                    .and_then(|line| line.split_once(' '))
                    .and_then(|(_, path)| {
                        extract_codex_uuid_from_filename(std::path::Path::new(path))
                    })
            }
            _ => None,
        }
    }
    fn has_custom_command(&self) -> bool {
        if !self.extra_args.is_empty() {
            return true;
        }
        if self.command.is_empty() {
            return false;
        }
        crate::agents::get_agent(&self.tool)
            .map(|a| self.command != a.binary)
            .unwrap_or(true)
    }

    pub fn get_tool_command(&self) -> &str {
        if self.command.is_empty() {
            crate::agents::get_agent(&self.tool)
                .map(|a| a.binary)
                .unwrap_or("bash")
        } else {
            &self.command
        }
    }

    pub fn tmux_session(&self) -> Result<tmux::Session> {
        tmux::Session::new(&self.id, &self.title)
    }

    pub fn terminal_tmux_session(&self) -> Result<tmux::TerminalSession> {
        tmux::TerminalSession::new(&self.id, &self.title)
    }

    pub fn has_terminal(&self) -> bool {
        self.terminal_info
            .as_ref()
            .map(|t| t.created)
            .unwrap_or(false)
    }

    pub fn start_terminal(&mut self) -> Result<()> {
        self.start_terminal_with_size(None)
    }

    pub fn start_terminal_with_size(&mut self, size: Option<(u16, u16)>) -> Result<()> {
        let session = self.terminal_tmux_session()?;

        let is_new = !session.exists();
        if is_new {
            session.create_with_size(&self.project_path, None, size)?;
        }

        // Apply all configured tmux options to terminal sessions too
        if is_new {
            self.apply_terminal_tmux_options();
        }

        self.terminal_info = Some(TerminalInfo {
            created: true,
            created_at: Some(Utc::now()),
        });

        Ok(())
    }

    pub fn kill_terminal(&self) -> Result<()> {
        let session = self.terminal_tmux_session()?;
        if session.exists() {
            session.kill()?;
        }
        Ok(())
    }

    pub fn container_terminal_tmux_session(&self) -> Result<tmux::ContainerTerminalSession> {
        tmux::ContainerTerminalSession::new(&self.id, &self.title)
    }

    pub fn has_container_terminal(&self) -> bool {
        self.container_terminal_tmux_session()
            .map(|s| s.exists())
            .unwrap_or(false)
    }

    pub fn start_container_terminal_with_size(&mut self, size: Option<(u16, u16)>) -> Result<()> {
        if !self.is_sandboxed() {
            anyhow::bail!("Cannot create container terminal for non-sandboxed session");
        }

        let container = self.get_container_for_instance()?;
        let sandbox = self.sandbox_info.as_ref().unwrap();

        let env_args = build_docker_env_args(sandbox);
        let env_part = if env_args.is_empty() {
            String::new()
        } else {
            format!("{} ", env_args)
        };

        // Get workspace path inside container (handles bare repo worktrees correctly)
        let container_workdir = self.container_workdir();

        let cmd = container.exec_command(
            Some(&format!("-w {} {}", container_workdir, env_part)),
            "/bin/bash",
        );

        let session = self.container_terminal_tmux_session()?;
        let is_new = !session.exists();
        if is_new {
            session.create_with_size(&self.project_path, Some(&cmd), size)?;
            self.apply_container_terminal_tmux_options();
        }

        Ok(())
    }

    pub fn kill_container_terminal(&self) -> Result<()> {
        let session = self.container_terminal_tmux_session()?;
        if session.exists() {
            session.kill()?;
        }
        Ok(())
    }

    fn sandbox_display(&self) -> Option<crate::tmux::status_bar::SandboxDisplay> {
        self.sandbox_info.as_ref().and_then(|s| {
            if s.enabled {
                Some(crate::tmux::status_bar::SandboxDisplay {
                    container_name: s.container_name.clone(),
                })
            } else {
                None
            }
        })
    }

    /// Apply all configured tmux options to a session with the given name and title.
    fn apply_session_tmux_options(&self, session_name: &str, display_title: &str) {
        let branch = self.worktree_info.as_ref().map(|w| w.branch.as_str());
        let sandbox = self.sandbox_display();
        crate::tmux::status_bar::apply_all_tmux_options(
            session_name,
            display_title,
            branch,
            sandbox.as_ref(),
        );
    }

    fn apply_container_terminal_tmux_options(&self) {
        let name = tmux::ContainerTerminalSession::generate_name(&self.id, &self.title);
        self.apply_session_tmux_options(&name, &format!("{} (container)", self.title));
    }

    pub fn start(&mut self) -> Result<()> {
        self.start_with_size(None)
    }

    pub fn start_with_size(&mut self, size: Option<(u16, u16)>) -> Result<()> {
        self.start_with_size_opts(size, false)
    }

    /// Start the session, optionally skipping on_launch hooks (e.g. when they
    /// already ran in the background creation poller).
    pub fn start_with_size_opts(
        &mut self,
        size: Option<(u16, u16)>,
        skip_on_launch: bool,
    ) -> Result<()> {
        let session = self.tmux_session()?;

        if session.exists() {
            return Ok(());
        }

        let profile = super::config::Config::load()
            .map(|c| c.default_profile)
            .unwrap_or_else(|_| "default".to_string());

        // Resolve on_launch hooks from the full config chain (global > profile > repo).
        // Repo hooks go through trust verification; global/profile hooks are implicitly trusted.
        let on_launch_hooks = if skip_on_launch {
            None
        } else {
            // Start with global+profile hooks as the base
            let mut resolved_on_launch = super::profile_config::resolve_config(&profile)
                .map(|c| c.hooks.on_launch)
                .unwrap_or_default();

            // Check if repo has trusted hooks that override
            match super::repo_config::check_hook_trust(Path::new(&self.project_path)) {
                Ok(super::repo_config::HookTrustStatus::Trusted(hooks))
                    if !hooks.on_launch.is_empty() =>
                {
                    resolved_on_launch = hooks.on_launch.clone();
                }
                _ => {}
            }

            if resolved_on_launch.is_empty() {
                None
            } else {
                Some(resolved_on_launch)
            }
        };

        let cmd = if self.is_sandboxed() {
            let container = self.get_container_for_instance()?;
            // Run on_launch hooks inside the container
            if let Some(ref hook_cmds) = on_launch_hooks {
                if let Some(ref sandbox) = self.sandbox_info {
                    let workdir = self.container_workdir();
                    if let Err(e) = super::repo_config::execute_hooks_in_container(
                        hook_cmds,
                        &sandbox.container_name,
                        &workdir,
                    ) {
                        tracing::warn!("on_launch hook failed in container: {}", e);
                    }
                }
            }

            let (session_id, is_existing) = self.acquire_session_id();

            let sandbox = self.sandbox_info.as_ref().unwrap();
            let agent = crate::agents::get_agent(&self.tool);
            let base_cmd = if self.extra_args.is_empty() {
                self.get_tool_command().to_string()
            } else {
                format!("{} {}", self.get_tool_command(), self.extra_args)
            };
            let mut tool_cmd = if self.is_yolo_mode() {
                if let Some(ref yolo) = agent.and_then(|a| a.yolo.as_ref()) {
                    match yolo {
                        crate::agents::YoloMode::CliFlag(flag) => {
                            format!("{} {}", base_cmd, flag)
                        }
                        crate::agents::YoloMode::EnvVar(..) => base_cmd,
                    }
                } else {
                    base_cmd
                }
            } else {
                base_cmd
            };
            if let Some(ref instruction) = sandbox.custom_instruction {
                if !instruction.is_empty() {
                    if let Some(flag_template) = agent.and_then(|a| a.instruction_flag) {
                        let escaped = shell_escape(instruction);
                        let flag = flag_template.replace("{}", &escaped);
                        tool_cmd = format!("{} {}", tool_cmd, flag);
                    }
                }
            }

            append_resume_flags(
                &self.tool,
                session_id.as_deref(),
                is_existing,
                &mut tool_cmd,
                "sandboxed",
            );

            let env_args = build_docker_env_args(sandbox);
            let env_part = if env_args.is_empty() {
                String::new()
            } else {
                format!("{} ", env_args)
            };
            Some(wrap_command_ignore_suspend(
                &container.exec_command(Some(&env_part), &tool_cmd),
            ))
        } else {
            // Run on_launch hooks on host for non-sandboxed sessions
            if let Some(ref hook_cmds) = on_launch_hooks {
                if let Err(e) =
                    super::repo_config::execute_hooks(hook_cmds, Path::new(&self.project_path))
                {
                    tracing::warn!("on_launch hook failed: {}", e);
                }
            }

            if self.command.is_empty() {
                crate::agents::get_agent(&self.tool)
                    .filter(|a| a.supports_host_launch)
                    .map(|a| {
                        let mut cmd = a.binary.to_string();
                        if !self.extra_args.is_empty() {
                            cmd = format!("{} {}", cmd, self.extra_args);
                        }
                        if self.is_yolo_mode() {
                            if let Some(ref yolo) = a.yolo {
                                match yolo {
                                    crate::agents::YoloMode::CliFlag(flag) => {
                                        cmd = format!("{} {}", cmd, flag);
                                    }
                                    crate::agents::YoloMode::EnvVar(key, value) => {
                                        cmd = format!("{}={} {}", key, value, cmd);
                                    }
                                }
                            }
                        }
                        let (session_id, is_existing) = self.acquire_session_id();
                        append_resume_flags(
                            &self.tool,
                            session_id.as_deref(),
                            is_existing,
                            &mut cmd,
                            "host agent",
                        );
                        wrap_command_ignore_suspend(&cmd)
                    })
            } else {
                let mut cmd = self.command.clone();
                if !self.extra_args.is_empty() {
                    cmd = format!("{} {}", cmd, self.extra_args);
                }
                if self.is_yolo_mode() {
                    let agent = crate::agents::get_agent(&self.tool);
                    if let Some(ref yolo) = agent.and_then(|a| a.yolo.as_ref()) {
                        match yolo {
                            crate::agents::YoloMode::CliFlag(flag) => {
                                cmd = format!("{} {}", cmd, flag);
                            }
                            crate::agents::YoloMode::EnvVar(key, value) => {
                                cmd = format!("{}={} {}", key, value, cmd);
                            }
                        }
                    }
                }
                let (session_id, is_existing) = self.acquire_session_id();
                append_resume_flags(
                    &self.tool,
                    session_id.as_deref(),
                    is_existing,
                    &mut cmd,
                    "host custom",
                );
                Some(wrap_command_ignore_suspend(&cmd))
            }
        };

        tracing::debug!("container cmd: {}", cmd.as_ref().map_or("none", |v| v));
        session.create_with_size(&self.project_path, cmd.as_deref(), size)?;

        self.persist_session_id(&profile);

        // Apply all configured tmux options (status bar, mouse, etc.)
        self.apply_tmux_options();

        self.status = Status::Starting;
        self.last_start_time = Some(std::time::Instant::now());

        Ok(())
    }

    /// Persist the agent session ID to storage so it survives TUI restarts.
    fn persist_session_id(&self, profile: &str) {
        if self.agent_session_id.is_none() {
            return;
        }
        match super::storage::Storage::new(profile) {
            Ok(storage) => match storage.load() {
                Ok(mut instances) => {
                    if let Some(inst) = instances.iter_mut().find(|i| i.id == self.id) {
                        inst.agent_session_id = self.agent_session_id.clone();
                    }
                    if let Err(e) = storage.save(&instances) {
                        tracing::debug!(
                            "Failed to save instances for session ID persistence: {}",
                            e
                        );
                    } else {
                        tracing::debug!("Session ID persisted successfully");
                    }
                }
                Err(e) => {
                    tracing::debug!("Failed to load instances for session ID persistence: {}", e)
                }
            },
            Err(e) => {
                tracing::debug!("Failed to create storage for session ID persistence: {}", e)
            }
        }
    }

    fn apply_tmux_options(&self) {
        let name = tmux::Session::generate_name(&self.id, &self.title);
        self.apply_session_tmux_options(&name, &self.title);
    }

    fn apply_terminal_tmux_options(&self) {
        let name = tmux::TerminalSession::generate_name(&self.id, &self.title);
        self.apply_session_tmux_options(&name, &format!("{} (terminal)", self.title));
    }

    pub fn get_container_for_instance(&mut self) -> Result<containers::DockerContainer> {
        let sandbox = self
            .sandbox_info
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Cannot ensure container for non-sandboxed session"))?;

        let image = &sandbox.image;
        let container = DockerContainer::new(&self.id, image);

        if container.is_running()? {
            container_config::refresh_agent_configs();
            return Ok(container);
        }

        if container.exists()? {
            container_config::refresh_agent_configs();
            container.start()?;
            return Ok(container);
        }

        // Ensure image is available (always pulls to get latest)
        let runtime = containers::get_container_runtime();
        runtime.ensure_image(image)?;

        let config = self.build_container_config()?;
        let container_id = container.create(&config)?;

        if let Some(ref mut sandbox) = self.sandbox_info {
            sandbox.container_id = Some(container_id);
            sandbox.created_at = Some(Utc::now());
        }

        Ok(container)
    }

    /// Get the container working directory for this instance.
    pub fn container_workdir(&self) -> String {
        container_config::compute_volume_paths(Path::new(&self.project_path), &self.project_path)
            .map(|(_, wd)| wd)
            .unwrap_or_else(|_| "/workspace".to_string())
    }

    fn build_container_config(&self) -> Result<crate::containers::ContainerConfig> {
        container_config::build_container_config(
            &self.project_path,
            self.sandbox_info.as_ref().unwrap(),
            &self.tool,
            self.is_yolo_mode(),
        )
    }

    pub fn restart(&mut self) -> Result<()> {
        self.restart_with_size(None)
    }

    pub fn restart_with_size(&mut self, size: Option<(u16, u16)>) -> Result<()> {
        let session = self.tmux_session()?;

        if session.exists() {
            session.kill()?;
        }

        // Small delay to ensure tmux cleanup
        std::thread::sleep(std::time::Duration::from_millis(100));

        self.start_with_size(size)
    }

    pub fn kill(&self) -> Result<()> {
        let session = self.tmux_session()?;
        if session.exists() {
            session.kill()?;
        }
        Ok(())
    }

    /// Stop the session: kill the tmux session and stop the Docker container
    /// (if sandboxed). The container is stopped but not removed, so it can be
    /// restarted on re-attach.
    pub fn stop(&self) -> Result<()> {
        self.kill()?;

        if self.is_sandboxed() {
            let container = containers::DockerContainer::from_session_id(&self.id);
            if container.is_running().unwrap_or(false) {
                container.stop()?;
            }
        }
        Ok(())
    }

    pub fn update_status(&mut self) {
        if self.status == Status::Stopped {
            return;
        }

        // Skip expensive checks for recently errored sessions
        if self.status == Status::Error {
            if let Some(last_check) = self.last_error_check {
                if last_check.elapsed().as_secs() < 30 {
                    return;
                }
            }
        }

        // Grace period for starting sessions
        if let Some(start_time) = self.last_start_time {
            if start_time.elapsed().as_secs() < 3 {
                self.status = Status::Starting;
                return;
            }
        }

        let session = match self.tmux_session() {
            Ok(s) => s,
            Err(_) => {
                self.status = Status::Error;
                self.last_error_check = Some(std::time::Instant::now());
                return;
            }
        };

        if !session.exists() {
            self.status = Status::Error;
            self.last_error_check = Some(std::time::Instant::now());
            return;
        }

        // Detect status from pane content
        let detected = match session.detect_status(&self.tool) {
            Ok(status) => status,
            Err(_) => Status::Idle,
        };
        tracing::trace!(
            "status detection '{}' (tool={}, custom_cmd={}): {:?}",
            self.title,
            self.tool,
            self.has_custom_command(),
            detected
        );
        self.status = match detected {
            Status::Idle if self.has_custom_command() => {
                if session.is_pane_dead() {
                    Status::Error
                } else {
                    Status::Unknown
                }
            }
            Status::Idle if session.is_pane_dead() => Status::Error,
            other => other,
        };

        // Clear stale error now that the session is healthy
        self.last_error = None;
    }

    pub fn capture_output_with_size(
        &self,
        lines: usize,
        width: u16,
        height: u16,
    ) -> Result<String> {
        let session = self.tmux_session()?;
        session.capture_pane_with_size(lines, Some(width), Some(height))
    }
}

fn generate_id() -> String {
    Uuid::new_v4().to_string().replace("-", "")[..16].to_string()
}

/// Wrap a command to disable Ctrl-Z (SIGTSTP) suspension.
///
/// When running agents directly as tmux session commands (without a parent shell),
/// pressing Ctrl-Z suspends the process with no way to recover via job control.
/// This wrapper disables the suspend character at the terminal level before exec'ing
/// the actual command.
///
/// Uses POSIX-standard `stty susp undef` which works on both Linux and macOS.
/// Single quotes in `cmd` are escaped with the `'\''` technique to prevent
/// breaking out of the outer `bash -c '...'` wrapper.
fn wrap_command_ignore_suspend(cmd: &str) -> String {
    let escaped = cmd.replace('\'', "'\\''");
    format!("bash -c 'stty susp undef; exec {}'", escaped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_instance() {
        let inst = Instance::new("test", "/tmp/test");
        assert_eq!(inst.title, "test");
        assert_eq!(inst.project_path, "/tmp/test");
        assert_eq!(inst.status, Status::Idle);
        assert_eq!(inst.id.len(), 16);
    }

    #[test]
    fn test_is_sub_session() {
        let mut inst = Instance::new("test", "/tmp/test");
        assert!(!inst.is_sub_session());

        inst.parent_session_id = Some("parent123".to_string());
        assert!(inst.is_sub_session());
    }

    #[test]
    fn test_all_agents_have_yolo_support() {
        for agent in crate::agents::AGENTS {
            assert!(
                agent.yolo.is_some(),
                "Agent '{}' should have YOLO mode configured",
                agent.name
            );
        }
    }

    #[test]
    fn test_yolo_mode_helper() {
        let mut inst = Instance::new("test", "/tmp/test");
        assert!(!inst.is_yolo_mode());

        inst.yolo_mode = true;
        assert!(inst.is_yolo_mode());

        inst.yolo_mode = false;
        assert!(!inst.is_yolo_mode());
    }

    #[test]
    fn test_yolo_mode_without_sandbox() {
        let mut inst = Instance::new("test", "/tmp/test");
        assert!(!inst.is_sandboxed());

        inst.yolo_mode = true;
        assert!(inst.is_yolo_mode());
        assert!(!inst.is_sandboxed());
    }

    // Additional tests for is_sandboxed
    #[test]
    fn test_is_sandboxed_without_sandbox_info() {
        let inst = Instance::new("test", "/tmp/test");
        assert!(!inst.is_sandboxed());
    }

    #[test]
    fn test_is_sandboxed_with_disabled_sandbox() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.sandbox_info = Some(SandboxInfo {
            enabled: false,
            container_id: None,
            image: "test-image".to_string(),
            container_name: "test".to_string(),
            created_at: None,
            extra_env: None,
            custom_instruction: None,
        });
        assert!(!inst.is_sandboxed());
    }

    #[test]
    fn test_is_sandboxed_with_enabled_sandbox() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.sandbox_info = Some(SandboxInfo {
            enabled: true,
            container_id: None,
            image: "test-image".to_string(),
            container_name: "test".to_string(),
            created_at: None,
            extra_env: None,
            custom_instruction: None,
        });
        assert!(inst.is_sandboxed());
    }

    // Tests for get_tool_command
    #[test]
    fn test_get_tool_command_default_claude() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.tool = "claude".to_string();
        assert_eq!(inst.get_tool_command(), "claude");
    }

    #[test]
    fn test_get_tool_command_opencode() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.tool = "opencode".to_string();
        assert_eq!(inst.get_tool_command(), "opencode");
    }

    #[test]
    fn test_get_tool_command_codex() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.tool = "codex".to_string();
        assert_eq!(inst.get_tool_command(), "codex");
    }

    #[test]
    fn test_get_tool_command_gemini() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.tool = "gemini".to_string();
        assert_eq!(inst.get_tool_command(), "gemini");
    }

    #[test]
    fn test_get_tool_command_unknown_tool() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.tool = "unknown".to_string();
        assert_eq!(inst.get_tool_command(), "bash");
    }

    #[test]
    fn test_get_tool_command_custom_command() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.tool = "claude".to_string();
        inst.command = "claude --resume abc123".to_string();
        assert_eq!(inst.get_tool_command(), "claude --resume abc123");
    }

    // Tests for Status enum
    #[test]
    fn test_status_default() {
        let status = Status::default();
        assert_eq!(status, Status::Idle);
    }

    #[test]
    fn test_status_serialization() {
        let statuses = vec![
            Status::Running,
            Status::Waiting,
            Status::Idle,
            Status::Unknown,
            Status::Stopped,
            Status::Error,
            Status::Starting,
            Status::Deleting,
        ];

        for status in statuses {
            let json = serde_json::to_string(&status).unwrap();
            let deserialized: Status = serde_json::from_str(&json).unwrap();
            assert_eq!(status, deserialized);
        }
    }

    // Tests for WorktreeInfo
    #[test]
    fn test_worktree_info_serialization() {
        let info = WorktreeInfo {
            branch: "feature/test".to_string(),
            main_repo_path: "/home/user/repo".to_string(),
            managed_by_aoe: true,
            created_at: Utc::now(),
            cleanup_on_delete: true,
        };

        let json = serde_json::to_string(&info).unwrap();
        let deserialized: WorktreeInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(info.branch, deserialized.branch);
        assert_eq!(info.main_repo_path, deserialized.main_repo_path);
        assert_eq!(info.managed_by_aoe, deserialized.managed_by_aoe);
        assert_eq!(info.cleanup_on_delete, deserialized.cleanup_on_delete);
    }

    #[test]
    fn test_worktree_info_default_cleanup_on_delete() {
        // Deserialize without cleanup_on_delete field - should default to true
        let json = r#"{"branch":"test","main_repo_path":"/path","managed_by_aoe":true,"created_at":"2024-01-01T00:00:00Z"}"#;
        let info: WorktreeInfo = serde_json::from_str(json).unwrap();
        assert!(info.cleanup_on_delete);
    }

    // Tests for SandboxInfo
    #[test]
    fn test_sandbox_info_serialization() {
        let info = SandboxInfo {
            enabled: true,
            container_id: Some("abc123".to_string()),
            image: "myimage:latest".to_string(),
            container_name: "test_container".to_string(),
            created_at: Some(Utc::now()),
            extra_env: Some(vec!["MY_VAR".to_string(), "OTHER_VAR".to_string()]),
            custom_instruction: None,
        };

        let json = serde_json::to_string(&info).unwrap();
        let deserialized: SandboxInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(info.enabled, deserialized.enabled);
        assert_eq!(info.container_id, deserialized.container_id);
        assert_eq!(info.image, deserialized.image);
        assert_eq!(info.container_name, deserialized.container_name);
        assert_eq!(info.extra_env, deserialized.extra_env);
    }

    #[test]
    fn test_sandbox_info_minimal_serialization() {
        // Required fields: enabled, image, container_name
        let json = r#"{"enabled":false,"image":"test-image","container_name":"test"}"#;
        let info: SandboxInfo = serde_json::from_str(json).unwrap();

        assert!(!info.enabled);
        assert_eq!(info.image, "test-image");
        assert_eq!(info.container_name, "test");
        assert!(info.container_id.is_none());
        assert!(info.created_at.is_none());
    }

    // Tests for Instance serialization
    #[test]
    fn test_instance_serialization_roundtrip() {
        let mut inst = Instance::new("Test Project", "/home/user/project");
        inst.tool = "claude".to_string();
        inst.group_path = "work/clients".to_string();
        inst.command = "claude --resume xyz".to_string();

        let json = serde_json::to_string(&inst).unwrap();
        let deserialized: Instance = serde_json::from_str(&json).unwrap();

        assert_eq!(inst.id, deserialized.id);
        assert_eq!(inst.title, deserialized.title);
        assert_eq!(inst.project_path, deserialized.project_path);
        assert_eq!(inst.group_path, deserialized.group_path);
        assert_eq!(inst.tool, deserialized.tool);
        assert_eq!(inst.command, deserialized.command);
    }

    #[test]
    fn test_instance_serialization_skips_runtime_fields() {
        let mut inst = Instance::new("Test", "/tmp/test");
        inst.last_error_check = Some(std::time::Instant::now());
        inst.last_start_time = Some(std::time::Instant::now());
        inst.last_error = Some("test error".to_string());

        let json = serde_json::to_string(&inst).unwrap();

        // Runtime fields should not appear in JSON
        assert!(!json.contains("last_error_check"));
        assert!(!json.contains("last_start_time"));
        assert!(!json.contains("last_error"));
    }

    #[test]
    fn test_instance_with_worktree_info() {
        let mut inst = Instance::new("Test", "/tmp/worktree");
        inst.worktree_info = Some(WorktreeInfo {
            branch: "feature/abc".to_string(),
            main_repo_path: "/tmp/main".to_string(),
            managed_by_aoe: true,
            created_at: Utc::now(),
            cleanup_on_delete: true,
        });

        let json = serde_json::to_string(&inst).unwrap();
        let deserialized: Instance = serde_json::from_str(&json).unwrap();

        assert!(deserialized.worktree_info.is_some());
        let wt = deserialized.worktree_info.unwrap();
        assert_eq!(wt.branch, "feature/abc");
        assert!(wt.managed_by_aoe);
    }

    // Test generate_id function properties
    #[test]
    fn test_generate_id_uniqueness() {
        let ids: Vec<String> = (0..100).map(|_| Instance::new("t", "/t").id).collect();
        let unique_ids: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(ids.len(), unique_ids.len());
    }

    #[test]
    fn test_generate_id_format() {
        let inst = Instance::new("test", "/tmp/test");
        // ID should be 16 hex characters
        assert_eq!(inst.id.len(), 16);
        assert!(inst.id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_has_terminal_false_by_default() {
        let inst = Instance::new("test", "/tmp/test");
        assert!(!inst.has_terminal());
    }

    #[test]
    fn test_has_terminal_true_when_created() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.terminal_info = Some(TerminalInfo {
            created: true,
            created_at: Some(Utc::now()),
        });
        assert!(inst.has_terminal());
    }

    #[test]
    fn test_terminal_info_none_means_no_terminal() {
        let inst = Instance::new("test", "/tmp/test");
        assert!(inst.terminal_info.is_none());
        assert!(!inst.has_terminal());
    }

    #[test]
    fn test_terminal_info_created_false_means_no_terminal() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.terminal_info = Some(TerminalInfo {
            created: false,
            created_at: None,
        });
        assert!(!inst.has_terminal());
    }

    // Tests for agent_session_id field
    #[test]
    fn test_agent_session_id_none_by_default() {
        let inst = Instance::new("test", "/tmp/test");
        assert!(inst.agent_session_id.is_none());
    }

    #[test]
    fn test_agent_session_id_serialization() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.agent_session_id = Some("session-123".to_string());

        let json = serde_json::to_string(&inst).unwrap();
        let deserialized: Instance = serde_json::from_str(&json).unwrap();

        assert_eq!(
            deserialized.agent_session_id,
            Some("session-123".to_string())
        );
    }

    #[test]
    fn test_agent_session_id_skips_none() {
        let inst = Instance::new("test", "/tmp/test");
        let json = serde_json::to_string(&inst).unwrap();

        // agent_session_id should not appear in JSON when None
        assert!(!json.contains("agent_session_id"));
    }

    #[test]
    fn test_agent_session_id_defaults_to_none() {
        let json = r#"{"id":"test123","title":"Test","project_path":"/tmp/test","group_path":"","command":"","tool":"claude","yolo_mode":false,"status":"idle","created_at":"2024-01-01T00:00:00Z"}"#;
        let inst: Instance = serde_json::from_str(json).unwrap();

        assert!(inst.agent_session_id.is_none());
    }

    #[test]
    fn test_build_claude_resume_flags_existing() {
        let session_id = "abc123-def456";
        let flags = build_resume_flags("claude", session_id, true);
        assert_eq!(flags, "--resume abc123-def456");
    }

    #[test]
    fn test_build_claude_session_id_flags_new() {
        let session_id = "abc123-def456";
        let flags = build_resume_flags("claude", session_id, false);
        assert_eq!(flags, "--session-id abc123-def456");
    }

    #[test]
    fn test_build_opencode_resume_flags() {
        let session_id = "session-789";
        let flags = build_resume_flags("opencode", session_id, false);
        assert_eq!(flags, "--session session-789");
    }

    #[test]
    fn test_build_codex_resume_flags() {
        let session_id = "codex-session-xyz";
        let flags = build_resume_flags("codex", session_id, false);
        assert_eq!(flags, "resume codex-session-xyz");
    }

    // Tests for Claude session ID generation
    #[test]
    fn test_generate_claude_session_id() {
        let id = generate_claude_session_id();

        // Should be a valid UUID format
        assert!(uuid::Uuid::parse_str(&id).is_ok());
    }

    #[test]
    fn test_generate_claude_session_id_uniqueness() {
        let ids: Vec<String> = (0..100).map(|_| generate_claude_session_id()).collect();
        let unique_ids: std::collections::HashSet<_> = ids.iter().collect();

        assert_eq!(ids.len(), unique_ids.len());
    }

    // Test that instance with agent_session_id can be serialized and deserialized
    #[test]
    fn test_instance_with_agent_session_id_roundtrip() {
        let mut inst = Instance::new("Test", "/home/user/project");
        inst.tool = "claude".to_string();
        inst.agent_session_id = Some("session-abc-123".to_string());

        let json = serde_json::to_string(&inst).unwrap();
        let deserialized: Instance = serde_json::from_str(&json).unwrap();

        assert_eq!(inst.id, deserialized.id);
        assert_eq!(inst.title, deserialized.title);
        assert_eq!(inst.project_path, deserialized.project_path);
        assert_eq!(inst.tool, deserialized.tool);
        assert_eq!(inst.agent_session_id, deserialized.agent_session_id);
    }

    // Test: agent switch clears session ID
    #[test]
    fn test_agent_switch_clears_session_id() {
        let mut inst = Instance::new("Test", "/home/user/project");
        inst.tool = "claude".to_string();
        inst.agent_session_id = Some("claude-session-123".to_string());

        // Simulate agent switch by clearing session ID
        inst.agent_session_id = None;
        inst.tool = "opencode".to_string();

        // Session ID should be None after switch
        assert!(inst.agent_session_id.is_none());
        assert_eq!(inst.tool, "opencode");
    }

    // Test: empty string session ID treated as None (this test is now covered by test_empty_string_deserializes_to_none)
    // Kept for backwards compatibility check that the deserializer works correctly
    #[test]
    fn test_empty_string_session_id_treated_as_none() {
        // When deserializing, empty string should be treated as None
        let json = r#"{"id":"test123","title":"Test","project_path":"/tmp/test","group_path":"","command":"","tool":"claude","yolo_mode":false,"status":"idle","created_at":"2024-01-01T00:00:00Z","agent_session_id":""}"#;
        let inst: Instance = serde_json::from_str(json).unwrap();

        // Empty string should deserialize to None, not Some("")
        assert_eq!(inst.agent_session_id, None);
    }

    // Test: capture failure doesn't block startup
    #[test]
    fn test_capture_failure_doesnt_block_startup() {
        // Test that acquire_session_id handles errors gracefully
        let mut inst = Instance::new("Test", "/nonexistent/path");
        inst.tool = "codex".to_string();

        let (session_id, is_existing) = inst.acquire_session_id();

        assert!(session_id.is_none());
        assert!(!is_existing);
        assert!(inst.agent_session_id.is_none());
    }

    // Test: resume with invalid session ID
    #[test]
    fn test_resume_with_invalid_session_id() {
        // Test that an invalid session ID is still stored and used
        let mut inst = Instance::new("Test", "/home/user/project");
        inst.tool = "claude".to_string();
        inst.agent_session_id = Some("invalid-session-id".to_string());

        // With an existing (persisted) session, should use --resume
        let flags = build_resume_flags(&inst.tool, inst.agent_session_id.as_ref().unwrap(), true);
        assert_eq!(flags, "--resume invalid-session-id");

        // The method should return the existing session ID and mark it as existing
        let (session_id, is_existing) = inst.acquire_session_id();
        assert_eq!(session_id, Some("invalid-session-id".to_string()));
        assert!(is_existing);
    }

    // Test: backwards compatibility - load old JSON without agent_session_id
    #[test]
    fn test_backwards_compatibility() {
        // Old JSON without agent_session_id field
        let old_json = r#"{"id":"old-session-123","title":"Old Session","project_path":"/home/user/old","group_path":"","command":"","tool":"claude","yolo_mode":false,"status":"idle","created_at":"2024-01-01T00:00:00Z"}"#;

        let inst: Instance = serde_json::from_str(old_json).unwrap();

        // Should parse successfully with agent_session_id defaulting to None
        assert_eq!(inst.id, "old-session-123");
        assert_eq!(inst.title, "Old Session");
        assert_eq!(inst.project_path, "/home/user/old");
        assert_eq!(inst.tool, "claude");
        assert!(inst.agent_session_id.is_none());

        // After loading, can set a new session ID
        let mut inst = inst;
        inst.agent_session_id = Some("new-session-456".to_string());
        assert_eq!(inst.agent_session_id, Some("new-session-456".to_string()));
    }

    #[test]
    fn test_empty_string_deserializes_to_none() {
        let json = r#"{"id":"test123","title":"Test","project_path":"/tmp/test","group_path":"","command":"","tool":"claude","yolo_mode":false,"status":"idle","created_at":"2024-01-01T00:00:00Z","agent_session_id":""}"#;
        let inst: Instance = serde_json::from_str(json).unwrap();
        assert!(inst.agent_session_id.is_none());
    }

    #[test]
    fn test_whitespace_string_deserializes_to_none() {
        let json = r#"{"id":"test123","title":"Test","project_path":"/tmp/test","group_path":"","command":"","tool":"claude","yolo_mode":false,"status":"idle","created_at":"2024-01-01T00:00:00Z","agent_session_id":"   "}"#;
        let inst: Instance = serde_json::from_str(json).unwrap();
        assert!(inst.agent_session_id.is_none());
    }

    #[test]
    fn test_valid_session_id_preserved() {
        let json = r#"{"id":"test123","title":"Test","project_path":"/tmp/test","group_path":"","command":"","tool":"claude","yolo_mode":false,"status":"idle","created_at":"2024-01-01T00:00:00Z","agent_session_id":"abc-123"}"#;
        let inst: Instance = serde_json::from_str(json).unwrap();
        assert_eq!(inst.agent_session_id, Some("abc-123".to_string()));
    }

    #[test]
    fn test_build_unknown_tool_resume_flags() {
        let flags = build_resume_flags("mistral", "session-123", false);
        assert!(flags.is_empty());
    }

    #[test]
    fn test_acquire_session_id_idempotence() {
        let mut inst = Instance::new("Test", "/tmp/test");
        inst.tool = "claude".to_string();

        let (first, first_existing) = inst.acquire_session_id();
        let (second, second_existing) = inst.acquire_session_id();

        assert!(first.is_some());
        assert!(!first_existing);
        assert!(second_existing);
        assert_eq!(first, second);
    }

    // Tests the timeout+SIGKILL pattern, not the actual capture_opencode_session_id
    // function, since that requires the opencode binary to be installed.
    #[test]
    fn test_opencode_timeout_returns_error() {
        let result = slow_command_with_timeout();
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("timed out"));
    }

    fn slow_command_with_timeout() -> Result<String> {
        let child = std::process::Command::new("sleep")
            .args(["10"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("Failed to spawn sleep command")?;

        let child_id = child.id();
        let (tx, rx) = mpsc::channel();

        std::thread::spawn(move || {
            let _ = tx.send(child.wait_with_output());
        });

        let _output = match rx.recv_timeout(Duration::from_secs(1)) {
            Ok(Ok(out)) => out,
            Ok(Err(e)) => return Err(anyhow::anyhow!("Failed to execute: {}", e)),
            Err(_) => {
                tracing::debug!("Command timed out after 1 second");
                let _ = nix::sys::signal::kill(Pid::from_raw(child_id as i32), Signal::SIGKILL);
                return Err(anyhow::anyhow!("Command timed out"));
            }
        };

        Ok("dummy".to_string())
    }

    #[test]
    fn test_has_custom_command_empty() {
        let inst = Instance::new("test", "/tmp/test");
        assert!(!inst.has_custom_command());
    }

    #[test]
    fn test_has_custom_command_same_as_agent_binary() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.tool = "claude".to_string();
        inst.command = "claude".to_string();
        assert!(!inst.has_custom_command());
    }

    #[test]
    fn test_has_custom_command_override() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.tool = "claude".to_string();
        inst.command = "my-wrapper".to_string();
        assert!(inst.has_custom_command());
    }

    #[test]
    fn test_has_custom_command_unknown_tool() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.tool = "unknown_agent".to_string();
        inst.command = "some-binary".to_string();
        assert!(inst.has_custom_command());
    }

    #[test]
    fn test_status_unknown_serialization() {
        let status = Status::Unknown;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"unknown\"");
        let deserialized: Status = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, Status::Unknown);
    }

    #[test]
    fn test_opencode_directory_matching() {
        let sessions_json = serde_json::json!([
            {"id": "wrong-session", "directory": "/home/user/other-project", "updated": "2025-01-01T00:00:00Z"},
            {"id": "correct-session", "directory": "/tmp/my-project", "updated": "2025-01-02T00:00:00Z"},
            {"id": "older-match", "directory": "/tmp/my-project", "updated": "2025-01-01T00:00:00Z"},
        ]);
        let sessions: Vec<serde_json::Value> = serde_json::from_value(sessions_json).unwrap();

        let project_path = "/tmp/my-project";
        let canonical_path = std::fs::canonicalize(project_path)
            .unwrap_or_else(|_| std::path::PathBuf::from(project_path));
        let canonical_str = canonical_path.to_string_lossy();

        let mut matching: Vec<&serde_json::Value> = sessions
            .iter()
            .filter(|s| {
                s.get("directory")
                    .or_else(|| s.get("path"))
                    .and_then(|v| v.as_str())
                    .map(|dir| {
                        let session_path = std::fs::canonicalize(dir)
                            .unwrap_or_else(|_| std::path::PathBuf::from(dir));
                        session_path.to_string_lossy() == canonical_str
                    })
                    .unwrap_or(false)
            })
            .collect();

        matching.sort_by(|a, b| {
            let a_time = a.get("updated").and_then(|v| v.as_str()).unwrap_or("");
            let b_time = b.get("updated").and_then(|v| v.as_str()).unwrap_or("");
            b_time.cmp(a_time)
        });

        let session = matching.first().copied().or_else(|| sessions.first());
        let id = session.and_then(|s| s["id"].as_str()).unwrap();

        assert_eq!(id, "correct-session");
        assert_eq!(matching.len(), 2);
    }

    #[test]
    fn test_codex_most_recent_session_selected() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions_dir = tmp.path().join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        let uuid_old = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
        let uuid_new = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
        std::fs::write(
            sessions_dir.join(format!("rollout-2025-01-01T00-00-00-{}.jsonl", uuid_old)),
            "{}",
        )
        .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(50));
        std::fs::write(
            sessions_dir.join(format!("rollout-2025-01-02T00-00-00-{}.jsonl", uuid_new)),
            "{}",
        )
        .unwrap();

        let mut entries: Vec<(std::path::PathBuf, std::time::SystemTime)> = Vec::new();
        collect_codex_sessions(&sessions_dir, &mut entries).unwrap();
        entries.sort_by(|a, b| b.1.cmp(&a.1));

        let selected = entries
            .first()
            .and_then(|(p, _)| extract_codex_uuid_from_filename(p))
            .unwrap();
        assert_eq!(selected, uuid_new);
    }

    #[test]
    fn test_codex_respects_codex_home_env() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions_dir = tmp.path().join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        let uuid = "cccccccc-cccc-cccc-cccc-cccccccccccc";
        std::fs::write(
            sessions_dir.join(format!("rollout-2025-03-06T10-30-00-{}.jsonl", uuid)),
            "{}",
        )
        .unwrap();

        let old_val = std::env::var("CODEX_HOME").ok();
        std::env::set_var("CODEX_HOME", tmp.path());

        let result = capture_codex_session_id("/tmp/test");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), uuid);

        match old_val {
            Some(v) => std::env::set_var("CODEX_HOME", v),
            None => std::env::remove_var("CODEX_HOME"),
        }
    }

    #[test]
    fn test_codex_walks_date_partitioned_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions_dir = tmp.path().join("sessions");

        let date_path = sessions_dir.join("2025").join("03").join("06");
        std::fs::create_dir_all(&date_path).unwrap();

        let uuid_deep = "dddddddd-dddd-dddd-dddd-dddddddddddd";
        let uuid_flat = "eeeeeeee-eeee-eeee-eeee-eeeeeeeeeeee";
        std::fs::write(
            date_path.join(format!("rollout-2025-03-06T12-00-00-{}.jsonl", uuid_deep)),
            "{}",
        )
        .unwrap();
        std::fs::write(
            sessions_dir.join(format!("rollout-2025-01-01T00-00-00-{}.jsonl", uuid_flat)),
            "{}",
        )
        .unwrap();

        let mut entries: Vec<(std::path::PathBuf, std::time::SystemTime)> = Vec::new();
        collect_codex_sessions(&sessions_dir, &mut entries).unwrap();

        let uuids: Vec<String> = entries
            .iter()
            .filter_map(|(p, _)| extract_codex_uuid_from_filename(p))
            .collect();

        assert!(uuids.contains(&uuid_deep.to_string()));
        assert!(uuids.contains(&uuid_flat.to_string()));
        assert_eq!(uuids.len(), 2);
    }

    #[test]
    fn test_extract_codex_uuid_from_filename() {
        let uuid = "abcdef01-2345-6789-abcd-ef0123456789";
        let path = std::path::PathBuf::from(format!("rollout-2025-03-06T12-00-00-{}.jsonl", uuid));
        assert_eq!(
            extract_codex_uuid_from_filename(&path),
            Some(uuid.to_string())
        );
    }

    #[test]
    fn test_extract_codex_uuid_fallback_for_non_standard_filename() {
        let path = std::path::PathBuf::from("my-thread-name.jsonl");
        assert_eq!(
            extract_codex_uuid_from_filename(&path),
            Some("my-thread-name".to_string())
        );
    }
}
