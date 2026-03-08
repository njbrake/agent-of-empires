//! Session instance definition and operations

use std::collections::HashSet;
use std::path::Path;
use std::process::Stdio;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
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
use super::poller::CaptureGate;
use super::poller::SessionPoller;

/// Iterate directory entries, silently skipping unreadable ones.
///
/// Wraps `std::fs::read_dir` and filters out individual entry errors (e.g.
/// broken symlinks, transient permission failures) so that one bad entry
/// doesn't abort the entire directory scan.
fn resilient_read_dir(
    dir: &std::path::Path,
) -> Result<impl Iterator<Item = std::fs::DirEntry> + '_> {
    Ok(std::fs::read_dir(dir)?.filter_map(move |entry| {
        entry
            .map_err(|e| tracing::debug!("Skipping unreadable entry in {}: {}", dir.display(), e))
            .ok()
    }))
}

/// Validate a captured session ID, logging a warning if it fails.
///
/// Single checkpoint used at every capture boundary (host, container,
/// retroactive) so that invalid IDs never propagate into storage or tmux env.
fn validated_session_id(id: String) -> Option<String> {
    if is_valid_session_id(&id) {
        Some(id)
    } else {
        tracing::warn!("Captured session ID failed validation: {:?}", id);
        None
    }
}

/// Load session timing configuration from disk (or fall back to defaults).
fn session_timing() -> super::config::SessionConfig {
    super::config::Config::load()
        .map(|c| c.session)
        .unwrap_or_default()
}

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

/// Deserialize agent_session_id, treating empty/whitespace strings as None.
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
    #[serde(skip)]
    pub session_id_poller: Option<Arc<Mutex<SessionPoller>>>,
    #[serde(skip)]
    pub(crate) deferred_capture_handle: Option<Arc<Mutex<Option<std::thread::JoinHandle<()>>>>>,
    #[serde(skip)]
    pub(crate) capture_gate: Option<Arc<CaptureGate>>,
}

/// Generate a new UUID v4 for a Claude Code session.
fn generate_claude_session_id() -> String {
    Uuid::new_v4().to_string()
}

/// Create a polling closure for Claude that reads the `~/.claude/debug/latest` symlink.
///
/// The symlink retargets within ~100ms of `/new`, `/clear`, or session switches.
/// The target is a file whose name contains the session UUID, e.g.
/// `~/.claude/debug/<UUID>.txt`. Returns `None` on any failure (missing
/// symlink, invalid path, non-UUID segment).
pub fn claude_poll_fn() -> impl Fn() -> Option<String> + Send + 'static {
    move || {
        let latest = dirs::home_dir()?.join(".claude/debug/latest");
        extract_uuid_from_symlink_target(&latest)
    }
}

/// Check whether a string is a valid UUID (8-4-4-4-12 hex-and-dash format).
fn is_uuid_format(s: &str) -> bool {
    s.len() == 36
        && s.chars().filter(|&c| c == '-').count() == 4
        && s.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

/// Read a symlink and walk its target path components looking for a UUID segment.
///
/// Returns the first path component that matches the UUID format (8-4-4-4-12).
/// Also handles filenames with an extension (e.g. `<UUID>.txt`, `<UUID>.log`)
/// by stripping it before checking. Returns `None` if the symlink is missing,
/// broken, or its target contains no UUID-shaped component.
fn extract_uuid_from_symlink_target(symlink_path: &std::path::Path) -> Option<String> {
    let target = std::fs::read_link(symlink_path).ok()?;
    let mut path = target.as_path();
    loop {
        let name = path.file_name()?.to_str()?;
        if is_uuid_format(name) {
            return Some(name.to_string());
        }
        // Handle filenames like `<UUID>.txt` where the stem is the UUID
        if let Some(stem) = std::path::Path::new(name)
            .file_stem()
            .and_then(|s| s.to_str())
        {
            if stem != name && is_uuid_format(stem) {
                return Some(stem.to_string());
            }
        }
        path = path.parent()?;
    }
}

/// Create a polling closure for OpenCode that re-runs `try_capture_opencode_session_id`.
///
/// Each invocation rebuilds the exclusion set from other AoE instances and invokes
/// the single-attempt capture. The poller's adaptive interval prevents hammering;
/// under stable conditions polls back off to 60s.
pub fn opencode_poll_fn(
    project_path: String,
    instance_id: String,
    launch_time_ms: f64,
) -> impl Fn() -> Option<String> + Send + 'static {
    let timing = session_timing();
    move || {
        let exclusion = build_exclusion_set(&instance_id);
        try_capture_opencode_session_id(&project_path, &exclusion, launch_time_ms, &timing)
            .map_err(|e| tracing::debug!("OpenCode poll capture failed: {}", e))
            .ok()
    }
}

/// Build a set of session IDs already claimed by other AoE instances.
///
/// Lists all tmux sessions with the AoE prefix, reads each one's hidden env vars
/// to find its instance ID and captured session ID, and collects all captured IDs
/// from instances other than `current_instance_id`.
///
/// NOTE: This is a point-in-time snapshot. Two instances that call this
/// concurrently may both see an empty set and claim the same session (TOCTOU).
/// The deferred capture loop mitigates this by re-reading on each attempt, and
/// the poller provides an additional layer of correction. Full mutual exclusion
/// would require a file lock or atomic tmux compare-and-set, which is not
/// currently justified given the low collision probability in practice.
fn build_exclusion_set(current_instance_id: &str) -> HashSet<String> {
    let output = match std::process::Command::new("tmux")
        .args(["list-sessions", "-F", "#{session_name}"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return HashSet::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut excluded = HashSet::new();

    for session_name in stdout.lines() {
        if !session_name.starts_with(crate::tmux::SESSION_PREFIX) {
            continue;
        }

        let owner =
            crate::tmux::env::get_hidden_env(session_name, crate::tmux::env::AOE_INSTANCE_ID_KEY);

        if owner.as_deref() == Some(current_instance_id) {
            continue;
        }

        if let Some(captured) = crate::tmux::env::get_hidden_env(
            session_name,
            crate::tmux::env::AOE_CAPTURED_SESSION_ID_KEY,
        ) {
            excluded.insert(captured);
        }
    }

    excluded
}

/// Filter, sort, and deduplicate OpenCode sessions by project directory.
///
/// Given a list of parsed OpenCode session JSON values:
/// 1. Filters to sessions matching `project_path` (canonicalized comparison on `directory`/`path`)
/// 2. Sorts by `updated` timestamp descending (most recent first)
/// 3. If `launch_time_ms` is `Some`, removes sessions older than that threshold
/// 4. Removes sessions whose IDs appear in `exclusion`
fn filter_agent_sessions<'a>(
    session_entries: &'a [serde_json::Value],
    project_path: Option<&str>,
    exclusion: &HashSet<String>,
    launch_time_ms: Option<f64>,
) -> Vec<&'a serde_json::Value> {
    let mut matching: Vec<&serde_json::Value> = if let Some(path) = project_path {
        let canonical_path =
            std::fs::canonicalize(path).unwrap_or_else(|_| std::path::PathBuf::from(path));
        let canonical_str = canonical_path.to_string_lossy();

        session_entries
            .iter()
            .filter(|s| {
                s.get("directory")
                    .and_then(|v| v.as_str())
                    .map(|dir| {
                        let session_path = std::fs::canonicalize(dir)
                            .unwrap_or_else(|_| std::path::PathBuf::from(dir));
                        session_path.to_string_lossy() == canonical_str
                    })
                    .unwrap_or(false)
            })
            .collect()
    } else {
        session_entries.iter().collect()
    };

    matching.sort_by(|a, b| {
        let a_time = a.get("updated").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let b_time = b.get("updated").and_then(|v| v.as_f64()).unwrap_or(0.0);
        b_time
            .partial_cmp(&a_time)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    if let Some(threshold) = launch_time_ms {
        matching.retain(|s| s.get("updated").and_then(|v| v.as_f64()).unwrap_or(0.0) >= threshold);
    }

    matching.retain(|s| {
        s.get("id")
            .and_then(|v| v.as_str())
            .map(|id| !exclusion.contains(id))
            .unwrap_or(true)
    });

    matching
}

/// Capture session ID from OpenCode CLI with retry logic.
///
/// Retries with configurable attempts, delays, and an overall deadline (see
/// `SessionConfig`). Each attempt runs `opencode session list --format json`
/// and selects the best match by project directory and update time.
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
fn capture_opencode_session_id(
    project_path: &str,
    exclusion: &HashSet<String>,
    launch_time_ms: f64,
    timing: &super::config::SessionConfig,
) -> Result<String> {
    let deadline =
        std::time::Instant::now() + Duration::from_secs(timing.opencode_capture_deadline_secs);
    let mut last_err = None;

    for attempt in 0..timing.opencode_max_retry_attempts {
        if attempt > 0 {
            let retry_delay = Duration::from_secs(timing.opencode_retry_delay_secs);
            if std::time::Instant::now() + retry_delay > deadline {
                break;
            }
            std::thread::sleep(retry_delay);
            tracing::debug!(
                "Retrying OpenCode session capture (attempt {})",
                attempt + 1
            );
        }

        match try_capture_opencode_session_id(project_path, exclusion, launch_time_ms, timing) {
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
/// Spawns `opencode session list --format json` with a configurable timeout
/// (`opencode_command_timeout_secs`), parses the JSON, and selects the best
/// matching session based on project directory and update time.
fn try_capture_opencode_session_id(
    project_path: &str,
    exclusion: &HashSet<String>,
    launch_time_ms: f64,
    timing: &super::config::SessionConfig,
) -> Result<String> {
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

    let output = match rx.recv_timeout(Duration::from_secs(timing.opencode_command_timeout_secs)) {
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
    let session_entries: Vec<serde_json::Value> =
        serde_json::from_str(&stdout).context("Failed to parse OpenCode session list JSON")?;

    let matching = filter_agent_sessions(
        &session_entries,
        Some(project_path),
        exclusion,
        Some(launch_time_ms),
    );

    // Use directory match if found, otherwise fall back to most recent
    // non-excluded session (without directory filter, but still respecting
    // exclusion and launch-time constraints).
    let session = matching.first().copied().or_else(|| {
        filter_agent_sessions(&session_entries, None, exclusion, Some(launch_time_ms))
            .into_iter()
            .next()
    });

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
fn capture_codex_session_id(project_path: &str, exclusion: &HashSet<String>) -> Result<String> {
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

    session_entries.retain(|(path, _)| {
        !exclusion.contains(
            extract_codex_uuid_from_filename(path)
                .as_deref()
                .unwrap_or(""),
        )
    });

    let canonical_project = std::fs::canonicalize(project_path)
        .unwrap_or_else(|_| std::path::PathBuf::from(project_path));

    // Prefer the most recent session whose CWD matches the project directory
    let cwd_match = session_entries.iter().find(|(path, _)| {
        extract_codex_cwd_from_file(path)
            .and_then(|cwd| std::fs::canonicalize(&cwd).ok())
            .map(|cwd| cwd == canonical_project)
            .unwrap_or(false)
    });

    let chosen = cwd_match.and_then(|(path, _)| extract_codex_uuid_from_filename(path));

    chosen.ok_or_else(|| anyhow::anyhow!("No Codex session found matching project path"))
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
    None
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
    for entry in resilient_read_dir(dir)? {
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

/// Capture session ID from Gemini CLI filesystem.
///
/// Gemini stores sessions at `~/.gemini/tmp/<project_hash>/chats/session-*.json`.
/// The project hash is a SHA-256 of the project path. Each session file contains
/// an `id` field. We find all session files, pick the most recently modified one
/// that matches the project, and return its filename stem (e.g. `session-12345`)
/// as the session ID.
fn capture_gemini_session_id(project_path: &str, exclusion: &HashSet<String>) -> Result<String> {
    let gemini_home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?
        .join(".gemini");
    let tmp_dir = gemini_home.join("tmp");

    if !tmp_dir.exists() {
        anyhow::bail!("Gemini tmp directory not found: {}", tmp_dir.display());
    }

    // Gemini hashes the project root to create the subdirectory name.
    // We scan all subdirs and check for chat sessions rather than
    // reimplementing their hash, since it may vary across versions.
    let mut candidates: Vec<(std::path::PathBuf, std::time::SystemTime)> = Vec::new();

    for project_entry in resilient_read_dir(&tmp_dir)? {
        let chats_dir = project_entry.path().join("chats");
        if !chats_dir.is_dir() {
            continue;
        }
        for chat_entry in resilient_read_dir(&chats_dir)? {
            let path = chat_entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("json")
                && path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("session-"))
            {
                let modified = chat_entry
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                candidates.push((path, modified));
            }
        }
    }

    if candidates.is_empty() {
        anyhow::bail!("No Gemini session files found in {}", tmp_dir.display());
    }

    candidates.sort_by(|a, b| b.1.cmp(&a.1));

    candidates.retain(|(path, _)| {
        let id = extract_gemini_session_id_from_file(path).unwrap_or_default();
        !exclusion.contains(&id)
    });

    let canonical_project = std::fs::canonicalize(project_path)
        .unwrap_or_else(|_| std::path::PathBuf::from(project_path));

    let project_match = candidates.iter().find(|(path, _)| {
        extract_gemini_cwd_from_file(path)
            .and_then(|cwd| std::fs::canonicalize(&cwd).ok())
            .map(|cwd| cwd == canonical_project)
            .unwrap_or(false)
    });

    let chosen = project_match;

    chosen
        .and_then(|(path, _)| extract_gemini_session_id_from_file(path))
        .ok_or_else(|| anyhow::anyhow!("No Gemini session found matching project path"))
}

/// Extract session ID from a Gemini session JSON file, falling back to filename stem.
fn extract_gemini_session_id_from_file(path: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    parsed
        .get("id")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| path.file_stem().and_then(|s| s.to_str()).map(String::from))
}

fn extract_cwd_from_json(parsed: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| parsed.get(*key).and_then(|v| v.as_str()))
        .map(String::from)
}

fn extract_gemini_cwd_from_file(path: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    extract_cwd_from_json(&parsed, &["cwd", "projectPath", "workingDirectory"])
}

/// Capture session ID from Mistral Vibe filesystem.
///
/// Vibe stores sessions at `~/.vibe/logs/session/<session_id>/meta.json`.
/// Each `meta.json` contains session metadata including `cwd`. We find the
/// most recently modified session directory matching the project path.
fn capture_vibe_session_id(project_path: &str, exclusion: &HashSet<String>) -> Result<String> {
    let vibe_home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?
        .join(".vibe");
    let sessions_dir = vibe_home.join("logs").join("session");

    if !sessions_dir.exists() {
        anyhow::bail!(
            "Vibe sessions directory not found: {}",
            sessions_dir.display()
        );
    }

    let mut candidates: Vec<(String, std::time::SystemTime)> = Vec::new();

    for entry in resilient_read_dir(&sessions_dir)? {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let session_id = entry
            .file_name()
            .to_str()
            .map(String::from)
            .unwrap_or_default();
        if session_id.is_empty() || exclusion.contains(&session_id) {
            continue;
        }
        let meta_path = path.join("meta.json");
        let modified = if meta_path.exists() {
            std::fs::metadata(&meta_path)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        } else {
            entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        };
        candidates.push((session_id, modified));
    }

    if candidates.is_empty() {
        anyhow::bail!(
            "No Vibe session directories found in {}",
            sessions_dir.display()
        );
    }

    candidates.sort_by(|a, b| b.1.cmp(&a.1));

    let canonical_project = std::fs::canonicalize(project_path)
        .unwrap_or_else(|_| std::path::PathBuf::from(project_path));

    let project_match = candidates.iter().find(|(session_id, _)| {
        let meta_path = sessions_dir.join(session_id).join("meta.json");
        extract_vibe_cwd_from_meta(&meta_path)
            .and_then(|cwd| std::fs::canonicalize(&cwd).ok())
            .map(|cwd| cwd == canonical_project)
            .unwrap_or(false)
    });

    let chosen = project_match;

    chosen
        .map(|(id, _)| id.clone())
        .ok_or_else(|| anyhow::anyhow!("No Vibe session found matching project path"))
}

fn extract_vibe_cwd_from_meta(path: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    extract_cwd_from_json(&parsed, &["cwd", "working_directory", "project_path"])
}

pub(crate) fn is_valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
}

/// Apply yolo mode to a command string. For `CliFlag`, appends the flag. For `EnvVar`,
/// prepends `KEY=VALUE` as an inline shell variable (only applicable on the host;
/// sandboxed sessions pass env vars via Docker `-e` flags instead).
fn apply_yolo_mode(cmd: &mut String, yolo: &crate::agents::YoloMode, is_sandboxed: bool) {
    match yolo {
        crate::agents::YoloMode::CliFlag(flag) => {
            *cmd = format!("{} {}", cmd, flag);
        }
        crate::agents::YoloMode::EnvVar(key, value) if !is_sandboxed => {
            *cmd = format!("{}={} {}", key, value, cmd);
        }
        crate::agents::YoloMode::EnvVar(..) => {}
    }
}

/// Build resume flags for agent command.
///
/// Uses the agent's `ResumeStrategy` (from the registry) to construct the correct
/// CLI flag or subcommand string. Returns an empty string for agents that don't
/// support resume or for invalid session IDs.
fn build_resume_flags(tool: &str, session_id: &str, is_existing_session: bool) -> String {
    use crate::agents::{get_agent, ResumeStrategy};

    if !is_valid_session_id(session_id) {
        tracing::warn!(
            "Refusing to build resume flags: invalid session ID {:?}",
            session_id
        );
        return String::new();
    }
    let Some(agent) = get_agent(tool) else {
        return String::new();
    };
    match &agent.resume_strategy {
        ResumeStrategy::Flag(flag) => format!("{} {}", flag, session_id),
        ResumeStrategy::FlagPair {
            existing,
            new_session,
        } => {
            let flag = if is_existing_session {
                existing
            } else {
                new_session
            };
            format!("{} {}", flag, session_id)
        }
        ResumeStrategy::Subcommand(sub) => format!("{} {}", sub, session_id),
        ResumeStrategy::Unsupported => String::new(),
    }
}

fn append_resume_flags(
    tool: &str,
    session_id: Option<&str>,
    is_existing_session: bool,
    cmd: &mut String,
    context: &str,
) {
    use crate::agents::{get_agent, ResumeStrategy};

    if let Some(session_id) = session_id {
        let resume_part = build_resume_flags(tool, session_id, is_existing_session);
        if resume_part.is_empty() {
            return;
        }
        let is_subcommand = matches!(
            get_agent(tool).map(|a| &a.resume_strategy),
            Some(ResumeStrategy::Subcommand(_))
        );
        if is_subcommand {
            // Subcommand resume must appear right after the binary name.
            // Assumes unquoted binary path (no spaces in $PATH lookups).
            if let Some(space_pos) = cmd.find(' ') {
                let binary = &cmd[..space_pos];
                let flags = &cmd[space_pos..];
                *cmd = format!("{} {}{}", binary, resume_part, flags);
            } else {
                *cmd = format!("{} {}", cmd, resume_part);
            }
        } else {
            *cmd = format!("{} {}", cmd, resume_part);
        }
        tracing::debug!("Added resume flags to {} command: {}", context, resume_part);
    }
}

fn capture_from_host(
    tool: &str,
    project_path: &str,
    exclusion: &HashSet<String>,
    launch_time_ms: f64,
    timing: &super::config::SessionConfig,
) -> Option<String> {
    let captured = match tool {
        "opencode" => capture_opencode_session_id(project_path, exclusion, launch_time_ms, timing)
            .map_err(|e| tracing::debug!("Deferred host capture (opencode): {}", e))
            .ok(),
        "codex" => capture_codex_session_id(project_path, exclusion)
            .map_err(|e| tracing::debug!("Deferred host capture (codex): {}", e))
            .ok(),
        "gemini" => capture_gemini_session_id(project_path, exclusion)
            .map_err(|e| tracing::debug!("Deferred host capture (gemini): {}", e))
            .ok(),
        "vibe" => capture_vibe_session_id(project_path, exclusion)
            .map_err(|e| tracing::debug!("Deferred host capture (vibe): {}", e))
            .ok(),
        _ => None,
    };
    captured.and_then(validated_session_id)
}

fn capture_from_container(
    instance_id: &str,
    tool: &str,
    exclusion: &HashSet<String>,
) -> Option<String> {
    let container = DockerContainer::from_session_id(instance_id);
    if !container.is_running().unwrap_or(false) {
        tracing::debug!(
            "Container not running for deferred capture: {}",
            instance_id
        );
        return None;
    }

    match tool {
        "opencode" => {
            let output = container
                .exec(&["opencode", "session", "list", "--format", "json"])
                .map_err(|e| tracing::debug!("Deferred container exec (opencode): {}", e))
                .ok()?;

            if !output.status.success() {
                return None;
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            let session_entries: Vec<serde_json::Value> = serde_json::from_str(&stdout)
                .map_err(|e| tracing::debug!("Deferred container JSON parse: {}", e))
                .ok()?;

            let matching = filter_agent_sessions(&session_entries, None, exclusion, None);
            matching
                .first()
                .and_then(|s| s["id"].as_str())
                .map(|s| s.to_string())
        }
        "codex" => {
            let output = container
                .exec(&[
                    "sh",
                    "-c",
                    "SESS_DIR=\"${CODEX_HOME:-$HOME/.codex}/sessions\"; find \"$SESS_DIR\" -name '*.jsonl' 2>/dev/null | xargs ls -1t 2>/dev/null | head -1",
                ])
                .map_err(|e| tracing::debug!("Deferred container exec (codex): {}", e))
                .ok()?;

            if !output.status.success() {
                return None;
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            let uuid = stdout
                .lines()
                .next()
                .filter(|line| !line.is_empty())
                .and_then(|path| {
                    extract_codex_uuid_from_filename(std::path::Path::new(path.trim()))
                });

            uuid.filter(|id| !exclusion.contains(id))
        }
        _ => {
            tracing::debug!(
                "Container capture not implemented for agent {:?}, instance {}",
                tool,
                instance_id
            );
            None
        }
    }
    .and_then(validated_session_id)
}

/// Persist an agent session ID to storage and tmux env for a given instance.
///
/// Used only during synchronous pre-launch (e.g. `persist_session_id` for
/// Claude) when no poller is active yet. Post-launch persistence goes
/// exclusively through the poller channel -> `apply_session_id_updates()`
/// in the TUI thread to avoid concurrent writes to `sessions.json`.
fn persist_session_to_storage(profile: &str, instance_id: &str, session_id: &str) {
    debug_assert!(
        std::thread::current()
            .name()
            .map_or(true, |n| n == "main" || !n.starts_with("aoe-")),
        "persist_session_to_storage must not be called from background threads (was: {:?})",
        std::thread::current().name()
    );

    if !is_valid_session_id(session_id) {
        tracing::warn!(
            "Refusing to persist invalid session ID {:?} for {}",
            session_id,
            instance_id
        );
        return;
    }

    let storage = match super::storage::Storage::new(profile) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Failed to create storage for session ID persistence: {}", e);
            return;
        }
    };
    let mut instances = match storage.load() {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!("Failed to load instances for session ID persistence: {}", e);
            return;
        }
    };

    let Some(inst) = instances.iter_mut().find(|i| i.id == instance_id) else {
        return;
    };

    let tmux_name = crate::tmux::Session::generate_name(instance_id, &inst.title);
    inst.agent_session_id = Some(session_id.to_string());

    if let Err(e) = storage.save(&instances) {
        tracing::warn!("Failed to save instances for session ID persistence: {}", e);
    } else {
        tracing::debug!("Session ID persisted for {}", instance_id);
        if let Err(e) = publish_session_to_tmux_env(&tmux_name, session_id) {
            tracing::warn!("{}", e);
        }
    }
}

/// Publish a captured session ID to the tmux environment only.
///
/// Background threads (deferred capture, poller on_change) call this instead
/// of `persist_session_to_storage` so they never race with the TUI thread's
/// `save()`. The tmux env is the source of truth for `build_exclusion_set()`
/// (cross-instance dedup), while `sessions.json` is written exclusively by
/// the TUI thread via `apply_session_id_updates()`.
fn publish_session_to_tmux_env(tmux_session_name: &str, session_id: &str) -> Result<()> {
    crate::tmux::env::set_hidden_env(
        tmux_session_name,
        crate::tmux::env::AOE_CAPTURED_SESSION_ID_KEY,
        session_id,
    )
    .map_err(|e| anyhow::anyhow!("Failed to write captured session ID to tmux env: {}", e))
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
            session_id_poller: None,
            deferred_capture_handle: None,
            capture_gate: None,
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

    /// Whether this agent uses a session ID poller for live tracking.
    ///
    /// Pollers continuously monitor for session ID changes via agent-specific
    /// poll functions (e.g. reading Claude's JSONL logs, querying OpenCode's
    /// session DB). Agents without a poll function use one-shot deferred
    /// capture instead.
    pub fn supports_session_poller(&self) -> bool {
        matches!(self.tool.as_str(), "claude" | "opencode")
    }

    /// Whether this agent creates its own session on startup, requiring
    /// post-launch ID capture.
    ///
    /// Derived from the agent's `ResumeStrategy`: agents with `Flag` or
    /// `Subcommand` strategies create their own sessions (OpenCode, Codex,
    /// Gemini, Vibe). Claude uses `FlagPair` with a pre-launch UUID, and
    /// Cursor has `Unsupported` -- neither needs deferred capture.
    pub fn supports_deferred_capture(&self) -> bool {
        use crate::agents::{get_agent, ResumeStrategy};
        get_agent(&self.tool).is_some_and(|a| {
            matches!(
                a.resume_strategy,
                ResumeStrategy::Flag(_) | ResumeStrategy::Subcommand(_)
            )
        })
    }

    /// Acquire a pre-launch session ID for the agent.
    ///
    /// Returns `(session_id, is_existing)`. If a persisted ID exists, returns it
    /// with `is_existing = true`. Otherwise, only Claude gets a new UUID here
    /// (it requires `--session-id <uuid>` at launch). Other agents create their
    /// own sessions on startup; their IDs are captured post-launch by
    /// `deferred_capture_session_id()`.
    pub fn acquire_session_id(&mut self) -> (Option<String>, bool) {
        if self.agent_session_id.is_some() {
            return (self.agent_session_id.clone(), true);
        }

        // Try retroactive capture: query the agent CLI for the most recent
        // session matching this project. This covers the case where the
        // deferred capture never ran (e.g., session created with an older
        // binary) or failed silently.
        if let Some(id) = self.try_retroactive_capture() {
            tracing::info!(
                "Retroactive capture found session ID for {}: {}",
                self.tool,
                id
            );
            self.agent_session_id = Some(id);
            return (self.agent_session_id.clone(), true);
        }

        // Only Claude needs a pre-launch ID (--session-id <uuid> creates a new session).
        // Other agents create their own sessions; the ID is captured post-launch
        // via deferred_capture_session_id().
        let session_id = match self.tool.as_str() {
            "claude" => Some(generate_claude_session_id()),
            _ => None,
        };

        if let Some(ref id) = session_id {
            tracing::debug!("Session ID for {}: {}", self.tool, id);
            self.agent_session_id = session_id.clone();
        }

        (session_id, false)
    }

    pub(crate) fn try_retroactive_capture(&self) -> Option<String> {
        let exclusion = build_exclusion_set(&self.id);
        let result = match self.tool.as_str() {
            "opencode" => {
                let timing = session_timing();
                // Single attempt with no time filter -- this runs synchronously
                // before the agent starts, so we only do one quick probe.
                try_capture_opencode_session_id(&self.project_path, &exclusion, 0.0, &timing).ok()
            }
            "codex" => capture_codex_session_id(&self.project_path, &exclusion).ok(),
            "gemini" => capture_gemini_session_id(&self.project_path, &exclusion).ok(),
            "vibe" => capture_vibe_session_id(&self.project_path, &exclusion).ok(),
            _ => None,
        };
        result.and_then(validated_session_id)
    }

    fn apply_session_flags(&mut self, cmd: &mut String, context: &str) {
        let (session_id, is_existing) = self.acquire_session_id();
        append_resume_flags(&self.tool, session_id.as_deref(), is_existing, cmd, context);
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

    pub fn expects_shell(&self) -> bool {
        crate::tmux::utils::is_shell_command(self.get_tool_command())
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
        let sandbox = self
            .sandbox_info
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("sandbox_info missing for sandboxed session"))?;

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

        let profile = super::config::resolve_default_profile();
        let on_launch_hooks = self.resolve_on_launch_hooks(skip_on_launch);
        let agent = crate::agents::get_agent(&self.tool);

        self.install_agent_status_hooks(agent);

        let cmd = if self.is_sandboxed() {
            self.build_sandboxed_command(agent, &on_launch_hooks)?
        } else {
            self.build_host_command(agent, &on_launch_hooks)
        };

        tracing::debug!("container cmd: {}", cmd.as_ref().map_or("none", |v| v));
        session.create_with_size(&self.project_path, cmd.as_deref(), size)?;

        self.finalize_launch(session.name(), &profile);

        Ok(())
    }

    /// Resolve on_launch hooks from the full config chain (global > profile > repo).
    ///
    /// Repo hooks go through trust verification; global/profile hooks are
    /// implicitly trusted. Returns `None` when skipped or no hooks are configured.
    fn resolve_on_launch_hooks(&self, skip_on_launch: bool) -> Option<Vec<String>> {
        if skip_on_launch {
            return None;
        }

        let profile = super::config::resolve_default_profile();

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
    }

    /// Install status-detection hooks for agents that support them.
    ///
    /// For sandboxed sessions hooks are installed via `build_container_config`,
    /// so this only acts on host sessions by writing to the user's home directory.
    fn install_agent_status_hooks(&self, agent: Option<&'static crate::agents::AgentDef>) {
        if let Some(hook_cfg) = agent.and_then(|a| a.hook_config.as_ref()) {
            if self.is_sandboxed() {
                // For sandboxed sessions, hooks are installed via build_container_config
            } else {
                // Install hooks in the user's home directory settings
                if let Some(home) = dirs::home_dir() {
                    let settings_path = home.join(hook_cfg.settings_rel_path);
                    if let Err(e) = crate::hooks::install_hooks(&settings_path) {
                        tracing::warn!("Failed to install agent hooks: {}", e);
                    }
                }
            }
        }
    }

    /// Build the tmux command for a sandboxed (Docker) session.
    ///
    /// Runs on_launch hooks inside the container, constructs the tool command
    /// with yolo mode / custom instructions / session flags, and wraps it in a
    /// `docker exec` invocation.
    fn build_sandboxed_command(
        &mut self,
        agent: Option<&'static crate::agents::AgentDef>,
        on_launch_hooks: &Option<Vec<String>>,
    ) -> Result<Option<String>> {
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

        let sandbox = self
            .sandbox_info
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("sandbox_info missing for sandboxed session"))?;
        let base_cmd = if self.extra_args.is_empty() {
            self.get_tool_command().to_string()
        } else {
            format!("{} {}", self.get_tool_command(), self.extra_args)
        };
        let mut tool_cmd = base_cmd;
        if self.is_yolo_mode() {
            if let Some(yolo) = agent.and_then(|a| a.yolo.as_ref()) {
                apply_yolo_mode(&mut tool_cmd, yolo, true);
            }
        }
        if let Some(ref instruction) = sandbox.custom_instruction {
            if !instruction.is_empty() {
                if let Some(flag_template) = agent.and_then(|a| a.instruction_flag) {
                    let escaped = shell_escape(instruction);
                    let flag = flag_template.replace("{}", &escaped);
                    tool_cmd = format!("{} {}", tool_cmd, flag);
                }
            }
        }

        let mut env_args = build_docker_env_args(sandbox);
        // Pass AOE_INSTANCE_ID into the container
        env_args = format!("{} -e AOE_INSTANCE_ID={}", env_args, self.id);

        self.apply_session_flags(&mut tool_cmd, "sandboxed");
        let env_part = format!("{} ", env_args);
        Ok(Some(wrap_command_ignore_suspend(
            &container.exec_command(Some(&env_part), &tool_cmd),
        )))
    }

    /// Build the tmux command for a host (non-sandboxed) session.
    ///
    /// Runs on_launch hooks on the host, then constructs the command from either
    /// the agent's default binary or a user-supplied custom command, applying
    /// yolo mode, session flags, and the AOE_INSTANCE_ID env prefix.
    fn build_host_command(
        &mut self,
        agent: Option<&'static crate::agents::AgentDef>,
        on_launch_hooks: &Option<Vec<String>>,
    ) -> Option<String> {
        // Run on_launch hooks on host for non-sandboxed sessions
        if let Some(ref hook_cmds) = on_launch_hooks {
            if let Err(e) =
                super::repo_config::execute_hooks(hook_cmds, Path::new(&self.project_path))
            {
                tracing::warn!("on_launch hook failed: {}", e);
            }
        }

        // Prepend AOE_INSTANCE_ID env var if this agent supports hooks
        let env_prefix = if agent.and_then(|a| a.hook_config.as_ref()).is_some() {
            format!("AOE_INSTANCE_ID={} ", self.id)
        } else {
            String::new()
        };

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
                            apply_yolo_mode(&mut cmd, yolo, false);
                        }
                    }
                    self.apply_session_flags(&mut cmd, "host agent");
                    wrap_command_ignore_suspend(&format!("{}{}", env_prefix, cmd))
                })
        } else {
            let mut cmd = self.command.clone();
            if !self.extra_args.is_empty() {
                cmd = format!("{} {}", cmd, self.extra_args);
            }
            if self.is_yolo_mode() {
                if let Some(yolo) = agent.and_then(|a| a.yolo.as_ref()) {
                    apply_yolo_mode(&mut cmd, yolo, false);
                }
            }
            self.apply_session_flags(&mut cmd, "host custom");
            Some(wrap_command_ignore_suspend(&format!(
                "{}{}",
                env_prefix, cmd
            )))
        }
    }

    /// Post-launch setup: store the instance ID in tmux env, persist session
    /// state, start the status poller, apply tmux options, and mark as starting.
    fn finalize_launch(&mut self, session_name: &str, profile: &str) {
        if let Err(e) = crate::tmux::env::set_hidden_env(
            session_name,
            crate::tmux::env::AOE_INSTANCE_ID_KEY,
            &self.id,
        ) {
            tracing::warn!("Failed to set AOE_INSTANCE_ID in tmux env: {}", e);
        }

        self.persist_session_id(profile);
        self.deferred_capture_session_id();
        let poller_launch_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as f64)
            .unwrap_or(0.0);
        self.maybe_start_poller_with_time(Some(poller_launch_time));

        // Apply all configured tmux options (status bar, mouse, etc.)
        self.apply_tmux_options();

        self.status = Status::Starting;
        self.last_start_time = Some(std::time::Instant::now());
    }

    fn persist_session_id(&self, profile: &str) {
        if let Some(ref sid) = self.agent_session_id {
            persist_session_to_storage(profile, &self.id, sid);
        }
    }

    /// Spawn a background thread to capture the session ID after the agent starts.
    ///
    /// Some agents (OpenCode, Codex, Gemini, Vibe) create their own sessions on
    /// launch, so the ID cannot be known in advance. This method polls the agent's
    /// CLI or filesystem until a session appears, then signals the `CaptureGate`
    /// so the poller can propagate it through the channel to the TUI thread.
    fn deferred_capture_session_id(&mut self) {
        if self.agent_session_id.is_some() {
            return;
        }
        if !self.supports_deferred_capture() {
            return;
        }

        let gate = Arc::new(CaptureGate::new());
        let gate_for_thread = Arc::clone(&gate);
        self.capture_gate = Some(gate);

        let instance_id = self.id.clone();
        let tool = self.tool.clone();
        let project_path = self.project_path.clone();
        let is_sandboxed = self.is_sandboxed();
        let tmux_session_name = self
            .tmux_session()
            .map(|s| s.name().to_string())
            .unwrap_or_default();

        let launch_time_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as f64)
            .unwrap_or(0.0);

        match std::thread::Builder::new()
            .name(format!("deferred-capture-{}", instance_id))
            .spawn(move || {
                let timing = session_timing();
                std::thread::sleep(Duration::from_secs(
                    timing.deferred_capture_initial_delay_secs,
                ));

                for attempt in 1..=timing.deferred_capture_max_attempts {
                    let exclusion = build_exclusion_set(&instance_id);

                    let captured = if is_sandboxed {
                        capture_from_container(&instance_id, &tool, &exclusion)
                    } else {
                        capture_from_host(&tool, &project_path, &exclusion, launch_time_ms, &timing)
                    };

                    if let Some(ref session_id) = captured {
                        tracing::debug!(
                            "Deferred capture succeeded for {} (attempt {}): {}",
                            instance_id,
                            attempt,
                            session_id
                        );
                        if !tmux_session_name.is_empty() {
                            if let Err(e) =
                                publish_session_to_tmux_env(&tmux_session_name, session_id)
                            {
                                tracing::warn!("{}", e);
                            }
                        }
                        gate_for_thread.complete(Some(session_id.clone()));
                        return;
                    }

                    if attempt < timing.deferred_capture_max_attempts {
                        tracing::debug!(
                            "Deferred capture attempt {}/{} found nothing for {}, retrying",
                            attempt,
                            timing.deferred_capture_max_attempts,
                            instance_id
                        );
                        std::thread::sleep(Duration::from_secs(
                            timing.deferred_capture_retry_delay_secs,
                        ));
                    }
                }

                tracing::debug!(
                    "Deferred capture exhausted all {} attempts for {}",
                    timing.deferred_capture_max_attempts,
                    instance_id
                );
                gate_for_thread.complete(None);
            }) {
            Ok(handle) => {
                self.deferred_capture_handle = Some(Arc::new(Mutex::new(Some(handle))));
            }
            Err(e) => {
                tracing::error!(
                    session = %self.id,
                    error = %e,
                    "Failed to spawn deferred session capture thread"
                );
                if let Some(ref gate) = self.capture_gate {
                    gate.complete(None);
                }
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
            &self.id,
        )
    }

    pub fn maybe_start_poller(&mut self) {
        self.maybe_start_poller_with_time(None);
    }

    /// Start the session ID poller with an explicit launch time filter.
    ///
    /// When `launch_time_ms` is `Some(t)`, the OpenCode poll function only
    /// considers sessions updated at or after `t` (used after freshly spawning
    /// the agent so we don't pick up stale sessions). When `None`, no time
    /// filter is applied -- the poller discovers any matching session for the
    /// project, which is the correct behaviour when resuming monitoring of an
    /// already-running agent on TUI restart.
    fn maybe_start_poller_with_time(&mut self, launch_time_ms: Option<f64>) {
        if !self.supports_session_poller() {
            return;
        }
        let tool = self.tool.as_str();

        let effective_launch_time = launch_time_ms.unwrap_or(0.0);

        let tmux_session_name = self
            .tmux_session()
            .map(|s| s.name().to_string())
            .unwrap_or_default();
        let mut poller = SessionPoller::new(tmux_session_name);
        let instance_id = self.id.clone();
        let initial_known = self.agent_session_id.clone();

        let poll_fn: Box<dyn Fn() -> Option<String> + Send + 'static> = match tool {
            "claude" => Box::new(claude_poll_fn()),
            "opencode" => Box::new(opencode_poll_fn(
                self.project_path.clone(),
                self.id.clone(),
                effective_launch_time,
            )),
            _ => return,
        };

        let cb_tmux_name = self
            .tmux_session()
            .map(|s| s.name().to_string())
            .unwrap_or_default();
        let cb_instance_id = self.id.clone();

        let on_change: Box<dyn Fn(&str) + Send + 'static> = Box::new(move |new_id: &str| {
            tracing::info!("Session ID changed for {}: {}", cb_instance_id, new_id);
            if !cb_tmux_name.is_empty() {
                if let Err(e) = publish_session_to_tmux_env(&cb_tmux_name, new_id) {
                    tracing::warn!("{}", e);
                }
            }
        });

        if poller.start(
            instance_id.clone(),
            poll_fn,
            on_change,
            initial_known,
            self.capture_gate.clone(),
        ) {
            self.session_id_poller = Some(Arc::new(Mutex::new(poller)));
        } else {
            tracing::warn!(
                "Failed to start session poller for instance {}, poller will not be stored",
                instance_id
            );
        }
    }

    fn stop_poller(&self) {
        if let Some(ref poller_arc) = self.session_id_poller {
            if let Ok(mut poller) = poller_arc.lock() {
                poller.stop();
            }
        }
    }

    pub fn restart(&mut self) -> Result<()> {
        self.restart_with_size(None)
    }

    pub fn restart_with_size(&mut self, size: Option<(u16, u16)>) -> Result<()> {
        self.restart_with_size_opts(size, false)
    }

    /// Restart the session, optionally skipping on_launch hooks (e.g. when
    /// they already ran in the background creation poller).
    pub fn restart_with_size_opts(
        &mut self,
        size: Option<(u16, u16)>,
        skip_on_launch: bool,
    ) -> Result<()> {
        self.stop_poller();
        self.session_id_poller = None;
        self.join_deferred_capture();
        self.deferred_capture_handle = None;
        self.capture_gate = None;

        let session = self.tmux_session()?;

        if session.exists() {
            session.kill()?;
        }

        // Small delay to ensure tmux cleanup
        std::thread::sleep(std::time::Duration::from_millis(100));

        self.start_with_size_opts(size, skip_on_launch)
    }

    pub fn kill(&self) -> Result<()> {
        self.stop_poller();
        self.join_deferred_capture();
        let session = self.tmux_session()?;
        if session.exists() {
            session.kill()?;
        }
        Ok(())
    }

    fn join_deferred_capture(&self) {
        if let Some(ref mtx) = self.deferred_capture_handle {
            if let Some(handle) = mtx.lock().ok().and_then(|mut guard| guard.take()) {
                let _ = handle.join();
            }
        }
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

        crate::hooks::cleanup_hook_status_dir(&self.id);

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

        // Check hook-based status first (more reliable than tmux pane parsing)
        if let Some(hook_status) = crate::hooks::read_hook_status(&self.id) {
            tracing::trace!("hook status detection '{}': {:?}", self.title, hook_status);
            self.status = if session.is_pane_dead() {
                Status::Error
            } else {
                hook_status
            };
            self.last_error = None;
            return;
        }

        // Fall back to tmux pane content detection
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
        let is_shell_stale = || !self.expects_shell() && session.is_pane_running_shell();
        self.status = match detected {
            Status::Idle if self.has_custom_command() => {
                if session.is_pane_dead() || is_shell_stale() {
                    Status::Error
                } else {
                    Status::Unknown
                }
            }
            Status::Idle if session.is_pane_dead() || is_shell_stale() => Status::Error,
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
    use serial_test::serial;

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

    #[test]
    fn test_opencode_acquire_returns_none_for_deferred_capture() {
        let mut inst = Instance::new("Test", "/nonexistent/opencode/test");
        inst.tool = "opencode".to_string();

        let (session_id, is_existing) = inst.acquire_session_id();

        // OpenCode never generates a pre-launch ID (unlike Claude).
        // Retroactive capture may still find an existing session via
        // fallback (opencode returns the most recent session regardless
        // of project path), so we assert the invariant: any returned
        // session must be flagged as existing, never generated.
        assert!(
            !is_existing || session_id.is_some(),
            "is_existing=true requires a session ID"
        );
        assert_eq!(inst.agent_session_id, session_id);
    }

    #[test]
    fn test_codex_acquire_returns_none_for_deferred_capture() {
        let mut inst = Instance::new("Test", "/nonexistent/path");
        inst.tool = "codex".to_string();

        let (session_id, is_existing) = inst.acquire_session_id();

        assert!(session_id.is_none());
        assert!(!is_existing);
        assert!(inst.agent_session_id.is_none());
    }

    #[test]
    fn test_persisted_opencode_session_id_reused() {
        let mut inst = Instance::new("Test", "/tmp/test");
        inst.tool = "opencode".to_string();
        inst.agent_session_id = Some("oc-session-42".to_string());

        let (session_id, is_existing) = inst.acquire_session_id();

        assert_eq!(session_id, Some("oc-session-42".to_string()));
        assert!(is_existing);
    }

    #[test]
    fn test_persisted_codex_session_id_reused() {
        let mut inst = Instance::new("Test", "/tmp/test");
        inst.tool = "codex".to_string();
        inst.agent_session_id = Some("codex-sess-99".to_string());

        let (session_id, is_existing) = inst.acquire_session_id();

        assert_eq!(session_id, Some("codex-sess-99".to_string()));
        assert!(is_existing);
    }

    #[test]
    fn test_resume_with_arbitrary_session_id() {
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

    #[test]
    fn test_is_valid_session_id() {
        assert!(is_valid_session_id("abc-123"));
        assert!(is_valid_session_id("session_id.v2"));
        assert!(is_valid_session_id("a"));
        assert!(is_valid_session_id("ABC-def_123.456"));

        assert!(!is_valid_session_id(""));
        assert!(!is_valid_session_id("bad id!@#"));
        assert!(!is_valid_session_id("has space"));
        assert!(!is_valid_session_id("semi;colon"));
        assert!(!is_valid_session_id("back`tick"));
        assert!(!is_valid_session_id("path/slash"));
        assert!(!is_valid_session_id(&"x".repeat(257)));
    }

    #[test]
    fn test_build_resume_flags_rejects_invalid_id() {
        let flags = build_resume_flags("claude", "$(rm -rf /)", true);
        assert_eq!(flags, "");

        let flags = build_resume_flags("opencode", "id; echo pwned", false);
        assert_eq!(flags, "");
    }

    #[test]
    fn test_codex_append_resume_flags_ordering() {
        let mut cmd = "codex --dangerously-auto-approve".to_string();
        append_resume_flags("codex", Some("ses-abc"), true, &mut cmd, "test");
        assert_eq!(cmd, "codex resume ses-abc --dangerously-auto-approve");
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
    fn test_build_gemini_resume_flags() {
        let session_id = "gemini-session-abc";
        let flags = build_resume_flags("gemini", session_id, true);
        assert_eq!(flags, "--resume gemini-session-abc");

        let flags_new = build_resume_flags("gemini", session_id, false);
        assert_eq!(flags_new, "--resume gemini-session-abc");
    }

    #[test]
    fn test_build_vibe_resume_flags() {
        let session_id = "vibe-session-xyz";
        let flags = build_resume_flags("vibe", session_id, true);
        assert_eq!(flags, "--resume vibe-session-xyz");

        let flags_new = build_resume_flags("vibe", session_id, false);
        assert_eq!(flags_new, "--resume vibe-session-xyz");
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
    fn test_expects_shell() {
        let mut inst = Instance::new("test", "/tmp/test");
        assert!(!inst.expects_shell());

        inst.tool = "unknown-tool".to_string();
        inst.command = String::new();
        assert!(inst.expects_shell());

        inst.tool = "claude".to_string();
        inst.command = "bash".to_string();
        assert!(inst.expects_shell());

        inst.command = "my-agent".to_string();
        assert!(!inst.expects_shell());
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
            {"id": "wrong-session", "directory": "/home/user/other-project", "updated": 1735689600000_u64},
            {"id": "correct-session", "directory": "/tmp/my-project", "updated": 1735776000000_u64},
            {"id": "older-match", "directory": "/tmp/my-project", "updated": 1735689600000_u64},
        ]);
        let session_entries: Vec<serde_json::Value> =
            serde_json::from_value(sessions_json).unwrap();

        let matching = filter_agent_sessions(
            &session_entries,
            Some("/tmp/my-project"),
            &HashSet::new(),
            None,
        );

        let session = matching
            .first()
            .copied()
            .or_else(|| session_entries.first());
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
    #[serial]
    fn test_codex_respects_codex_home_env() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions_dir = tmp.path().join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        let uuid = "cccccccc-cccc-cccc-cccc-cccccccccccc";
        let project_dir = tmp.path().join("test-project");
        std::fs::create_dir_all(&project_dir).unwrap();
        let jsonl_content = format!(
            r#"{{"type":"session_meta","payload":{{"cwd":"{}"}}}}"#,
            project_dir.display()
        );
        std::fs::write(
            sessions_dir.join(format!("rollout-2025-03-06T10-30-00-{}.jsonl", uuid)),
            jsonl_content,
        )
        .unwrap();

        let old_val = std::env::var("CODEX_HOME").ok();
        std::env::set_var("CODEX_HOME", tmp.path());

        let result = capture_codex_session_id(project_dir.to_str().unwrap(), &HashSet::new());
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
        // Non-UUID filenames should return None to prevent garbage session IDs
        let path = std::path::PathBuf::from("my-thread-name.jsonl");
        assert_eq!(extract_codex_uuid_from_filename(&path), None);
    }

    #[test]
    fn test_claude_poll_fn_extracts_uuid() {
        let tmp = tempfile::tempdir().unwrap();
        let uuid = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
        let sessions_dir = tmp.path().join("sessions").join(uuid);
        std::fs::create_dir_all(&sessions_dir).unwrap();
        let debug_log = sessions_dir.join("debug.log");
        std::fs::write(&debug_log, "").unwrap();

        let symlink_path = tmp.path().join("latest");
        std::os::unix::fs::symlink(&debug_log, &symlink_path).unwrap();

        let result = extract_uuid_from_symlink_target(&symlink_path);
        assert_eq!(result, Some(uuid.to_string()));
    }

    #[test]
    fn test_claude_poll_fn_invalid_target() {
        let tmp = tempfile::tempdir().unwrap();
        let target_dir = tmp.path().join("no-uuid-here");
        std::fs::create_dir_all(&target_dir).unwrap();
        let target_file = target_dir.join("somefile.log");
        std::fs::write(&target_file, "").unwrap();

        let symlink_path = tmp.path().join("latest");
        std::os::unix::fs::symlink(&target_file, &symlink_path).unwrap();

        assert_eq!(extract_uuid_from_symlink_target(&symlink_path), None);
    }

    #[test]
    fn test_claude_poll_fn_broken_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let dangling_target = tmp.path().join("sessions/dead-uuid/debug.log");
        let symlink_path = tmp.path().join("latest");
        std::os::unix::fs::symlink(&dangling_target, &symlink_path).unwrap();

        assert_eq!(extract_uuid_from_symlink_target(&symlink_path), None);
    }

    #[test]
    fn test_claude_poll_fn_extracts_uuid_from_dotted_filename() {
        let tmp = tempfile::tempdir().unwrap();
        let uuid = "a1b2c3d4-e5f6-7890-abcd-ef1234567890";
        let target_file = tmp.path().join(format!("{uuid}.txt"));
        std::fs::write(&target_file, "").unwrap();

        let symlink_path = tmp.path().join("latest");
        std::os::unix::fs::symlink(&target_file, &symlink_path).unwrap();

        let result = extract_uuid_from_symlink_target(&symlink_path);
        assert_eq!(result, Some(uuid.to_string()));
    }

    #[test]
    fn test_is_uuid_format_valid() {
        assert!(is_uuid_format("a1b2c3d4-e5f6-7890-abcd-ef1234567890"));
        assert!(is_uuid_format("00000000-0000-0000-0000-000000000000"));
        assert!(is_uuid_format("AAAAAAAA-BBBB-CCCC-DDDD-EEEEEEEEEEEE"));
    }

    #[test]
    fn test_is_uuid_format_invalid() {
        assert!(!is_uuid_format(""));
        assert!(!is_uuid_format("not-a-uuid"));
        assert!(!is_uuid_format("a1b2c3d4-e5f6-7890-abcd")); // too short
        assert!(!is_uuid_format("a1b2c3d4-e5f6-7890-abcd-ef1234567890.txt")); // has extension
        assert!(!is_uuid_format("a1b2c3d4e5f67890abcdef1234567890")); // no dashes
        assert!(!is_uuid_format("g1b2c3d4-e5f6-7890-abcd-ef1234567890")); // 'g' is not hex
    }

    #[test]
    fn test_extract_gemini_session_id_from_file_with_id() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session-42.json");
        std::fs::write(&path, r#"{"id": "abc-123", "cwd": "/tmp/project"}"#).unwrap();
        assert_eq!(
            extract_gemini_session_id_from_file(&path),
            Some("abc-123".to_string())
        );
    }

    #[test]
    fn test_extract_gemini_session_id_from_file_no_id_falls_back_to_stem() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session-42.json");
        std::fs::write(&path, r#"{"cwd": "/tmp/project"}"#).unwrap();
        assert_eq!(
            extract_gemini_session_id_from_file(&path),
            Some("session-42".to_string())
        );
    }

    #[test]
    fn test_extract_gemini_session_id_from_file_invalid_json() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session-42.json");
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(extract_gemini_session_id_from_file(&path), None);
    }

    #[test]
    fn test_extract_gemini_cwd_from_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session.json");
        std::fs::write(&path, r#"{"id": "s1", "cwd": "/home/user/project"}"#).unwrap();
        assert_eq!(
            extract_gemini_cwd_from_file(&path),
            Some("/home/user/project".to_string())
        );
    }

    #[test]
    fn test_extract_gemini_cwd_from_file_project_path_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session.json");
        std::fs::write(
            &path,
            r#"{"id": "s1", "projectPath": "/home/user/project"}"#,
        )
        .unwrap();
        assert_eq!(
            extract_gemini_cwd_from_file(&path),
            Some("/home/user/project".to_string())
        );
    }

    #[test]
    fn test_extract_vibe_cwd_from_meta() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("meta.json");
        std::fs::write(
            &path,
            r#"{"cwd": "/home/user/myrepo", "session_id": "abc"}"#,
        )
        .unwrap();
        assert_eq!(
            extract_vibe_cwd_from_meta(&path),
            Some("/home/user/myrepo".to_string())
        );
    }

    #[test]
    fn test_extract_vibe_cwd_from_meta_working_directory_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("meta.json");
        std::fs::write(&path, r#"{"working_directory": "/home/user/myrepo"}"#).unwrap();
        assert_eq!(
            extract_vibe_cwd_from_meta(&path),
            Some("/home/user/myrepo".to_string())
        );
    }

    #[test]
    fn test_extract_vibe_cwd_from_meta_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nonexistent.json");
        assert_eq!(extract_vibe_cwd_from_meta(&path), None);
    }

    #[test]
    fn test_build_exclusion_set_empty() {
        let result = build_exclusion_set("nonexistent-instance-id-12345");
        assert!(result.is_empty());
    }

    #[test]
    fn test_exclusion_filters_claimed_sessions() {
        let sessions_json = serde_json::json!([
            {"id": "A", "directory": "/tmp/my-project", "updated": 1735776000000_u64},
            {"id": "C", "directory": "/tmp/my-project", "updated": 1735775000000_u64},
            {"id": "B", "directory": "/tmp/my-project", "updated": 1735774000000_u64},
            {"id": "D", "directory": "/tmp/my-project", "updated": 1735773000000_u64},
        ]);
        let session_entries: Vec<serde_json::Value> =
            serde_json::from_value(sessions_json).unwrap();

        let mut exclusion = HashSet::new();
        exclusion.insert("A".to_string());
        exclusion.insert("B".to_string());

        let matching = filter_agent_sessions(
            &session_entries,
            Some("/tmp/my-project"),
            &exclusion,
            Some(0.0),
        );

        let best = matching.first().unwrap();
        assert_eq!(best["id"].as_str().unwrap(), "C");
    }

    #[test]
    fn test_opencode_capture_with_exclusion() {
        let sessions_json = serde_json::json!([
            {"id": "best-session", "directory": "/tmp/my-project", "updated": 1735776000000_u64},
            {"id": "second-best", "directory": "/tmp/my-project", "updated": 1735775000000_u64},
        ]);
        let session_entries: Vec<serde_json::Value> =
            serde_json::from_value(sessions_json).unwrap();

        let mut exclusion = HashSet::new();
        exclusion.insert("best-session".to_string());

        let matching =
            filter_agent_sessions(&session_entries, Some("/tmp/my-project"), &exclusion, None);

        let session = matching
            .first()
            .copied()
            .or_else(|| session_entries.first());
        let id = session.and_then(|s| s["id"].as_str()).unwrap();
        assert_eq!(id, "second-best");
    }

    #[test]
    fn test_opencode_capture_all_excluded() {
        let sessions_json = serde_json::json!([
            {"id": "sess-1", "directory": "/tmp/my-project", "updated": 1735776000000_u64},
            {"id": "sess-2", "directory": "/tmp/my-project", "updated": 1735775000000_u64},
        ]);
        let session_entries: Vec<serde_json::Value> =
            serde_json::from_value(sessions_json).unwrap();

        let mut exclusion = HashSet::new();
        exclusion.insert("sess-1".to_string());
        exclusion.insert("sess-2".to_string());

        let matching =
            filter_agent_sessions(&session_entries, Some("/tmp/my-project"), &exclusion, None);

        assert!(
            matching.is_empty(),
            "All sessions are excluded, matching should be empty"
        );
    }

    #[test]
    fn test_opencode_timestamp_guard() {
        let sessions_json = serde_json::json!([
            {"id": "old-session", "directory": "/tmp/my-project", "updated": 1000000000000_u64},
            {"id": "new-session", "directory": "/tmp/my-project", "updated": 1735776000000_u64},
            {"id": "stale-session", "directory": "/tmp/my-project", "updated": 1500000000000_u64},
        ]);
        let session_entries: Vec<serde_json::Value> =
            serde_json::from_value(sessions_json).unwrap();

        let launch_time_ms: f64 = 1735000000000.0;
        let exclusion: HashSet<String> = HashSet::new();

        let matching = filter_agent_sessions(
            &session_entries,
            Some("/tmp/my-project"),
            &exclusion,
            Some(launch_time_ms),
        );

        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0]["id"].as_str().unwrap(), "new-session");
    }

    struct TmuxSessionGuard {
        sessions: Vec<String>,
    }

    impl Drop for TmuxSessionGuard {
        fn drop(&mut self) {
            for name in &self.sessions {
                let _ = std::process::Command::new("tmux")
                    .args(["kill-session", "-t", name])
                    .output();
            }
        }
    }

    #[test]
    #[ignore]
    #[serial]
    fn test_parallel_multi_instance_capture() {
        if std::process::Command::new("tmux")
            .args(["-V"])
            .output()
            .is_err()
        {
            return;
        }

        let session_names = [
            "aoe_test_parallel_1",
            "aoe_test_parallel_2",
            "aoe_test_parallel_3",
        ];
        let instance_ids = ["instance-id-1", "instance-id-2", "instance-id-3"];
        let captured_sessions = [
            "opencode-session-AAA",
            "opencode-session-BBB",
            "opencode-session-CCC",
        ];

        let _guard = TmuxSessionGuard {
            sessions: session_names.iter().map(|s| s.to_string()).collect(),
        };

        for i in 0..3 {
            let output = std::process::Command::new("tmux")
                .args([
                    "new-session",
                    "-d",
                    "-s",
                    session_names[i],
                    "-x",
                    "200",
                    "-y",
                    "50",
                ])
                .output()
                .expect("Failed to create tmux session");
            assert!(
                output.status.success(),
                "Failed to create tmux session {}: {}",
                session_names[i],
                String::from_utf8_lossy(&output.stderr)
            );

            crate::tmux::env::set_hidden_env(
                session_names[i],
                crate::tmux::env::AOE_INSTANCE_ID_KEY,
                instance_ids[i],
            )
            .unwrap_or_else(|e| {
                panic!("Failed to set instance ID for {}: {}", session_names[i], e)
            });

            crate::tmux::env::set_hidden_env(
                session_names[i],
                crate::tmux::env::AOE_CAPTURED_SESSION_ID_KEY,
                captured_sessions[i],
            )
            .unwrap_or_else(|e| {
                panic!(
                    "Failed to set captured session for {}: {}",
                    session_names[i], e
                )
            });
        }

        let results: Vec<HashSet<String>> = std::thread::scope(|s| {
            let handles: Vec<_> = instance_ids
                .iter()
                .map(|id| s.spawn(move || build_exclusion_set(id)))
                .collect();

            handles
                .into_iter()
                .map(|h| h.join().expect("Thread panicked"))
                .collect()
        });

        // Each instance's exclusion set should contain only the other instances' captured sessions
        assert!(
            results[0].contains("opencode-session-BBB"),
            "Instance 1 exclusion set should contain BBB, got: {:?}",
            results[0]
        );
        assert!(
            results[0].contains("opencode-session-CCC"),
            "Instance 1 exclusion set should contain CCC, got: {:?}",
            results[0]
        );
        assert!(
            !results[0].contains("opencode-session-AAA"),
            "Instance 1 exclusion set must NOT contain its own AAA, got: {:?}",
            results[0]
        );

        assert!(
            results[1].contains("opencode-session-AAA"),
            "Instance 2 exclusion set should contain AAA, got: {:?}",
            results[1]
        );
        assert!(
            results[1].contains("opencode-session-CCC"),
            "Instance 2 exclusion set should contain CCC, got: {:?}",
            results[1]
        );
        assert!(
            !results[1].contains("opencode-session-BBB"),
            "Instance 2 exclusion set must NOT contain its own BBB, got: {:?}",
            results[1]
        );

        assert!(
            results[2].contains("opencode-session-AAA"),
            "Instance 3 exclusion set should contain AAA, got: {:?}",
            results[2]
        );
        assert!(
            results[2].contains("opencode-session-BBB"),
            "Instance 3 exclusion set should contain BBB, got: {:?}",
            results[2]
        );
        assert!(
            !results[2].contains("opencode-session-CCC"),
            "Instance 3 exclusion set must NOT contain its own CCC, got: {:?}",
            results[2]
        );

        for i in 0..3 {
            assert!(
                !results[i].contains(captured_sessions[i]),
                "Instance {} exclusion set must not contain its own captured session {}",
                i + 1,
                captured_sessions[i]
            );
        }
    }
}
