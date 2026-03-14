//! Session ID capture logic for all supported agent types.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Context, Result};
use nix::sys::signal::Signal;
use nix::unistd::Pid;
use uuid::Uuid;

use crate::containers::DockerContainer;

/// Iterate directory entries, silently skipping unreadable ones.
///
/// Wraps `std::fs::read_dir` and filters out individual entry errors (e.g.
/// broken symlinks, transient permission failures) so that one bad entry
/// doesn't abort the entire directory scan.
pub(crate) fn resilient_read_dir(
    dir: &std::path::Path,
) -> Result<impl Iterator<Item = std::fs::DirEntry> + '_> {
    Ok(std::fs::read_dir(dir)?.filter_map(move |entry| {
        entry
            .map_err(|e| tracing::debug!("Skipping unreadable entry in {}: {}", dir.display(), e))
            .ok()
    }))
}

/// Resolve an agent's home directory, checking an optional env var first.
fn resolve_agent_home(env_var: Option<&str>, default_subdir: &str) -> Result<PathBuf> {
    if let Some(var) = env_var {
        if let Ok(val) = std::env::var(var) {
            return Ok(PathBuf::from(val));
        }
    }
    Ok(dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine home directory"))?
        .join(default_subdir))
}

fn canonicalize_or_raw(path: &str) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path))
}

/// Validate a captured session ID, logging a warning if it fails.
///
/// Single checkpoint used at every capture boundary (host, container,
/// retroactive) so that invalid IDs never propagate into storage or tmux env.
pub(crate) fn validated_session_id(id: String) -> Option<String> {
    if is_valid_session_id(&id) {
        Some(id)
    } else {
        tracing::warn!("Captured session ID failed validation: {:?}", id);
        None
    }
}

/// Load session timing configuration from disk (or fall back to defaults).
pub(crate) fn session_timing() -> super::config::SessionConfig {
    super::config::Config::load()
        .map(|c| c.session)
        .unwrap_or_default()
}

/// Generate a new UUID v4 for a Claude Code session.
pub(crate) fn generate_claude_session_id() -> String {
    Uuid::new_v4().to_string()
}

/// Encode a project path into Claude Code's directory naming convention.
///
/// Claude stores per-project data under `~/.claude/projects/{encoded}/` where
/// non-alphanumeric characters (except `-`) are replaced with `-`.
/// For example: `/Users/foo/bar` becomes `-Users-foo-bar`.
fn encode_claude_project_path(project_path: &str) -> String {
    project_path
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Capture Claude Code session ID from the most recently active project directory,
/// falling back to `~/.claude.json` if the dir scan result is stale.
///
/// Used as a fallback when hooks don't fire (e.g. after `/clear` or `/new`).
pub(crate) fn capture_claude_session_id(
    project_path: &str,
    exclusion: &HashSet<String>,
) -> Result<String> {
    let claude_home = resolve_agent_home(Some("CLAUDE_CONFIG_DIR"), ".claude")?;
    let canonical = canonicalize_or_raw(project_path);

    // Source 1: most recently modified .jsonl in the project dir
    if let Some((id, modified)) = scan_claude_project_dir(&claude_home, &canonical, exclusion)? {
        let age = modified.elapsed().unwrap_or(Duration::from_secs(u64::MAX));
        if age <= Duration::from_secs(5 * 60) {
            return Ok(id);
        }
    }

    // Source 2: lastSessionId from ~/.claude.json
    if let Some(id) = read_claude_json_session_id(&canonical) {
        if Uuid::parse_str(&id).is_ok() && !exclusion.contains(&id) {
            return Ok(id);
        }
    }

    anyhow::bail!("No active Claude session found for {}", project_path)
}

/// Scan `~/.claude/projects/{encoded-path}/` for the most recently modified
/// UUID-named `.jsonl` file not owned by another AoE instance.
fn scan_claude_project_dir(
    claude_home: &Path,
    project_path: &Path,
    exclusion: &HashSet<String>,
) -> Result<Option<(String, std::time::SystemTime)>> {
    let dir_name = encode_claude_project_path(&project_path.to_string_lossy());
    let project_dir = claude_home.join("projects").join(&dir_name);

    if !project_dir.is_dir() {
        return Ok(None);
    }

    let mut best: Option<(String, std::time::SystemTime)> = None;

    for entry in resilient_read_dir(&project_dir)? {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let stem = match path.file_stem().and_then(|s| s.to_str()) {
            Some(s) => s,
            None => continue,
        };
        if Uuid::parse_str(stem).is_err() {
            continue;
        }
        if exclusion.contains(stem) {
            continue;
        }

        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);

        if best.as_ref().map_or(true, |(_, t)| modified > *t) {
            best = Some((stem.to_string(), modified));
        }
    }

    Ok(best)
}

/// Read `lastSessionId` from `~/.claude.json` for a given project path.
fn read_claude_json_session_id(project_path: &Path) -> Option<String> {
    let claude_json = dirs::home_dir()?.join(".claude.json");
    let content = std::fs::read_to_string(&claude_json).ok()?;
    let content = content.trim();
    if content.is_empty() {
        return None;
    }
    let parsed: serde_json::Value = serde_json::from_str(content).ok()?;

    let path_str = project_path.to_string_lossy();
    parsed
        .get("projects")?
        .get(path_str.as_ref())?
        .get("lastSessionId")?
        .as_str()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// Polling closure for Claude Code session tracking.
pub(crate) fn claude_poll_fn(
    project_path: String,
    instance_id: String,
) -> impl Fn() -> Option<String> + Send + 'static {
    move || {
        let exclusion = build_exclusion_set(&instance_id);
        capture_claude_session_id(&project_path, &exclusion)
            .map_err(|e| tracing::debug!("Claude disk scan failed: {}", e))
            .ok()
    }
}

/// Polling closure for Codex CLI session tracking.
pub(crate) fn codex_poll_fn(
    project_path: String,
    instance_id: String,
) -> impl Fn() -> Option<String> + Send + 'static {
    move || {
        let exclusion = build_exclusion_set(&instance_id);
        capture_codex_session_id(&project_path, &exclusion)
            .map_err(|e| tracing::debug!("Codex poll capture failed: {}", e))
            .ok()
    }
}

/// Polling closure for Gemini CLI session tracking.
pub(crate) fn gemini_poll_fn(
    project_path: String,
    instance_id: String,
) -> impl Fn() -> Option<String> + Send + 'static {
    move || {
        let exclusion = build_exclusion_set(&instance_id);
        capture_gemini_session_id(&project_path, &exclusion)
            .map_err(|e| tracing::debug!("Gemini poll capture failed: {}", e))
            .ok()
    }
}

/// Polling closure for Vibe (Mistral) session tracking.
pub(crate) fn vibe_poll_fn(
    project_path: String,
    instance_id: String,
) -> impl Fn() -> Option<String> + Send + 'static {
    move || {
        let exclusion = build_exclusion_set(&instance_id);
        capture_vibe_session_id(&project_path, &exclusion)
            .map_err(|e| tracing::debug!("Vibe poll capture failed: {}", e))
            .ok()
    }
}

/// Polling closure for OpenCode session tracking.
pub(crate) fn opencode_poll_fn(
    project_path: String,
    instance_id: String,
    launch_time_ms: f64,
) -> impl Fn() -> Option<String> + Send + 'static {
    move || {
        let timing = session_timing();
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
pub(crate) fn build_exclusion_set(current_instance_id: &str) -> HashSet<String> {
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
pub(crate) fn filter_agent_sessions<'a>(
    session_entries: &'a [serde_json::Value],
    project_path: Option<&str>,
    exclusion: &HashSet<String>,
    launch_time_ms: Option<f64>,
) -> Vec<&'a serde_json::Value> {
    let mut matching: Vec<&serde_json::Value> = if let Some(path) = project_path {
        let canonical_path = canonicalize_or_raw(path);
        let canonical_str = canonical_path.to_string_lossy();

        session_entries
            .iter()
            .filter(|s| {
                s.get("directory")
                    .and_then(|v| v.as_str())
                    .map(|dir| {
                        // Per-entry canonicalize is intentional: each session may
                        // reference a different directory that could be a symlink.
                        let session_path = canonicalize_or_raw(dir);
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
pub(crate) fn try_capture_opencode_session_id(
    project_path: &str,
    exclusion: &HashSet<String>,
    launch_time_ms: f64,
    timing: &super::config::SessionConfig,
) -> Result<String> {
    let child = std::process::Command::new("opencode")
        .args(["session", "list", "--format", "json"])
        .current_dir(project_path)
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
            // Safety: PIDs fit in i32 on all supported platforms (Linux/macOS).
            let _ = nix::sys::signal::kill(Pid::from_raw(child_id as i32), Signal::SIGKILL);
            return Err(anyhow::anyhow!("OpenCode session list timed out"));
        }
    };

    if !output.status.success() {
        anyhow::bail!("OpenCode session list command failed");
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        anyhow::bail!("No OpenCode sessions found");
    }
    let session_entries: Vec<serde_json::Value> =
        serde_json::from_str(trimmed).context("Failed to parse OpenCode session list JSON")?;

    let matching = filter_agent_sessions(
        &session_entries,
        Some(project_path),
        exclusion,
        Some(launch_time_ms),
    );

    matching
        .first()
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
pub(crate) fn capture_codex_session_id(
    project_path: &str,
    exclusion: &HashSet<String>,
) -> Result<String> {
    let codex_home = resolve_agent_home(Some("CODEX_HOME"), ".codex")?;
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

    let canonical_project = canonicalize_or_raw(project_path);

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
pub(crate) fn extract_codex_uuid_from_filename(path: &std::path::Path) -> Option<String> {
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
pub(crate) fn collect_codex_sessions(
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

/// Capture Gemini session ID from `~/.gemini/tmp/<dir>/chats/session-*.json`.
///
/// `<dir>` is a SHA-256 hash (legacy) or slug (current). We try the hash as a
/// fast path, then scan all subdirs and verify via the `projectHash` JSON field.
pub(crate) fn capture_gemini_session_id(
    project_path: &str,
    exclusion: &HashSet<String>,
) -> Result<String> {
    use sha2::{Digest, Sha256};

    let gemini_home = resolve_agent_home(Some("GEMINI_CLI_HOME"), ".gemini")?;
    let tmp_dir = gemini_home.join("tmp");

    if !tmp_dir.exists() {
        anyhow::bail!("Gemini tmp directory not found: {}", tmp_dir.display());
    }

    let canonical_project = canonicalize_or_raw(project_path);
    let expected_hash = format!(
        "{:x}",
        Sha256::digest(canonical_project.to_string_lossy().as_bytes())
    );

    // Hash-named dir (legacy) as fast path; fall back to scanning all subdirs.
    let project_dirs: Vec<std::path::PathBuf> = {
        let exact = tmp_dir.join(&expected_hash);
        if exact.is_dir() {
            vec![exact]
        } else {
            resilient_read_dir(&tmp_dir)?
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        }
    };

    let mut candidates: Vec<(std::path::PathBuf, std::time::SystemTime, Option<String>)> =
        Vec::new();

    for project_dir in &project_dirs {
        let chats_dir = project_dir.join("chats");
        if !chats_dir.is_dir() {
            continue;
        }

        let is_exact_match = project_dir
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == expected_hash);

        for chat_entry in resilient_read_dir(&chats_dir)? {
            let path = chat_entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json")
                || !path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("session-"))
            {
                continue;
            }

            let fields = extract_gemini_fields(&path);

            // For slug-named dirs, verify projectHash inside the file.
            if !is_exact_match {
                let file_hash = fields
                    .as_ref()
                    .and_then(|(_, h)| h.as_deref())
                    .unwrap_or_default();
                if file_hash != expected_hash {
                    continue;
                }
            }

            let session_id = fields.and_then(|(sid, _)| sid);

            let modified = chat_entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            candidates.push((path, modified, session_id));
        }
    }

    if candidates.is_empty() {
        anyhow::bail!("No Gemini session files found in {}", tmp_dir.display());
    }

    candidates.sort_by(|a, b| b.1.cmp(&a.1));

    candidates.retain(|(_, _, sid)| {
        let id = sid.as_deref().unwrap_or_default();
        !exclusion.contains(id)
    });

    candidates
        .first()
        .and_then(|(_, _, sid)| sid.clone())
        .ok_or_else(|| anyhow::anyhow!("No Gemini session found matching project path"))
}

/// Extract session ID from a Gemini session JSON file, falling back to filename stem.
#[cfg(test)]
pub(crate) fn extract_gemini_session_id_from_file(path: &std::path::Path) -> Option<String> {
    extract_gemini_fields(path).and_then(|(sid, _)| sid)
}

fn extract_cwd_from_json(parsed: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| parsed.get(*key).and_then(|v| v.as_str()))
        .map(String::from)
}

/// Extract the project hash from a Gemini session file for CWD matching.
///
/// Gemini stores a SHA-256 hash of the project root in `projectHash` rather than
/// a literal path. Returns the hash string so callers can compare against a
/// locally computed hash.
#[cfg(test)]
pub(crate) fn extract_gemini_project_hash_from_file(path: &std::path::Path) -> Option<String> {
    extract_gemini_fields(path).and_then(|(_, hash)| hash)
}

/// Read a Gemini session JSON file once and return both sessionId and projectHash.
fn extract_gemini_fields(path: &std::path::Path) -> Option<(Option<String>, Option<String>)> {
    let content = std::fs::read_to_string(path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    let session_id = parsed
        .get("sessionId")
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| path.file_stem().and_then(|s| s.to_str()).map(String::from));
    let project_hash = parsed
        .get("projectHash")
        .and_then(|v| v.as_str())
        .map(String::from);
    Some((session_id, project_hash))
}

/// Capture Vibe session ID from `meta.json` files in the session log directory.
///
/// Default path: `~/.vibe/logs/session/`; overridden by `VIBE_HOME` env var
/// (resolves to `$VIBE_HOME/logs/session/`).
/// Each session dir contains `meta.json` with `session_id` and
/// `environment.working_directory`. Returns the most recent match for the project.
pub(crate) fn capture_vibe_session_id(
    project_path: &str,
    exclusion: &HashSet<String>,
) -> Result<String> {
    let vibe_home = resolve_agent_home(Some("VIBE_HOME"), ".vibe")?;
    let sessions_dir = vibe_home.join("logs").join("session");

    if !sessions_dir.exists() {
        anyhow::bail!(
            "Vibe sessions directory not found: {}",
            sessions_dir.display()
        );
    }

    let mut candidates: Vec<(String, std::path::PathBuf, std::time::SystemTime)> = Vec::new();

    for entry in resilient_read_dir(&sessions_dir)? {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let meta_path = path.join("meta.json");
        if !meta_path.exists() {
            continue;
        }
        let session_id = match extract_vibe_session_id_from_meta(&meta_path) {
            Some(id) if !id.is_empty() && !exclusion.contains(&id) => id,
            _ => continue,
        };
        let modified = std::fs::metadata(&meta_path)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        candidates.push((session_id, meta_path, modified));
    }

    if candidates.is_empty() {
        anyhow::bail!(
            "No Vibe session directories found in {}",
            sessions_dir.display()
        );
    }

    candidates.sort_by(|a, b| b.2.cmp(&a.2));

    let canonical_project = canonicalize_or_raw(project_path);

    let project_match = candidates.iter().find(|(_, meta_path, _)| {
        extract_vibe_cwd_from_meta(meta_path)
            .and_then(|cwd| std::fs::canonicalize(&cwd).ok())
            .map(|cwd| cwd == canonical_project)
            .unwrap_or(false)
    });

    project_match
        .map(|(id, _, _)| id.clone())
        .ok_or_else(|| anyhow::anyhow!("No Vibe session found matching project path"))
}

/// Extract CWD from a Vibe `meta.json`.
///
/// The actual path lives at `environment.working_directory` (nested object).
/// Falls back to top-level `cwd` / `working_directory` for forward compatibility.
pub(crate) fn extract_vibe_cwd_from_meta(path: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    // Primary: nested under `environment`
    parsed
        .get("environment")
        .and_then(|env| env.get("working_directory"))
        .and_then(|v| v.as_str())
        .map(String::from)
        // Fallback: top-level keys for forward compatibility
        .or_else(|| extract_cwd_from_json(&parsed, &["cwd", "working_directory", "project_path"]))
}

/// Extract the `session_id` UUID from a Vibe `meta.json` file.
pub(crate) fn extract_vibe_session_id_from_meta(path: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    parsed
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(String::from)
}

pub(crate) fn is_valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
}

/// Dispatch agent-specific session ID capture from the host filesystem.
///
/// Tries each supported agent's capture strategy in order, returning the first
/// successfully captured session ID, or `None` if no agent produced a result.
pub(crate) fn capture_from_host(
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

pub(crate) fn capture_from_container(
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
            let trimmed = stdout.trim();
            if trimmed.is_empty() {
                return None;
            }
            let session_entries: Vec<serde_json::Value> = serde_json::from_str(trimmed)
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
                    "SESS_DIR=\"${CODEX_HOME:-$HOME/.codex}/sessions\"; find \"$SESS_DIR\" -name '*.jsonl' -printf '%T@ %p\\n' 2>/dev/null | sort -rn | head -1 | cut -d' ' -f2-",
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

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;

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
    fn test_extract_gemini_session_id_from_file_with_id() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session-42.json");
        std::fs::write(&path, r#"{"sessionId": "abc-123", "cwd": "/tmp/project"}"#).unwrap();
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
    fn test_extract_gemini_project_hash_from_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session.json");
        std::fs::write(
            &path,
            r#"{"sessionId": "s1", "projectHash": "abc123def456"}"#,
        )
        .unwrap();
        assert_eq!(
            extract_gemini_project_hash_from_file(&path),
            Some("abc123def456".to_string())
        );
    }

    #[test]
    fn test_extract_gemini_project_hash_from_file_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session.json");
        std::fs::write(&path, r#"{"sessionId": "s1"}"#).unwrap();
        assert_eq!(extract_gemini_project_hash_from_file(&path), None);
    }

    #[test]
    fn test_extract_vibe_cwd_from_meta_nested() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("meta.json");
        std::fs::write(
            &path,
            r#"{"session_id": "abc", "environment": {"working_directory": "/home/user/myrepo"}}"#,
        )
        .unwrap();
        assert_eq!(
            extract_vibe_cwd_from_meta(&path),
            Some("/home/user/myrepo".to_string())
        );
    }

    #[test]
    fn test_extract_vibe_cwd_from_meta_top_level_fallback() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("meta.json");
        // No `environment` object -- falls back to top-level keys
        std::fs::write(&path, r#"{"cwd": "/home/user/myrepo"}"#).unwrap();
        assert_eq!(
            extract_vibe_cwd_from_meta(&path),
            Some("/home/user/myrepo".to_string())
        );
    }

    #[test]
    fn test_extract_vibe_session_id_from_meta() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("meta.json");
        std::fs::write(
            &path,
            r#"{"session_id": "a1b2c3d4-e5f6-7890-abcd-ef1234567890", "environment": {"working_directory": "/tmp"}}"#,
        )
        .unwrap();
        assert_eq!(
            extract_vibe_session_id_from_meta(&path),
            Some("a1b2c3d4-e5f6-7890-abcd-ef1234567890".to_string())
        );
    }

    #[test]
    fn test_extract_vibe_session_id_from_meta_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("meta.json");
        std::fs::write(&path, r#"{"environment": {"working_directory": "/tmp"}}"#).unwrap();
        assert_eq!(extract_vibe_session_id_from_meta(&path), None);
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
        // May be non-empty if other AoE tmux sessions are running.
        assert!(!result.contains("nonexistent-instance-id-12345"));
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

    #[test]
    fn test_filter_agent_sessions_empty_input() {
        let empty: Vec<serde_json::Value> = Vec::new();
        let exclusion = HashSet::new();
        let result = filter_agent_sessions(&empty, Some("/tmp/project"), &exclusion, None);
        assert!(
            result.is_empty(),
            "Empty input should return empty result, not panic"
        );
    }

    #[test]
    fn test_gemini_extract_session_id_malformed_json() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session-bad.json");
        std::fs::write(&path, "this is not json at all {{{{").unwrap();
        assert_eq!(extract_gemini_session_id_from_file(&path), None);
    }

    #[test]
    #[serial]
    fn test_codex_capture_empty_sessions_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions_dir = tmp.path().join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();

        let old_val = std::env::var("CODEX_HOME").ok();
        std::env::set_var("CODEX_HOME", tmp.path());

        let result = capture_codex_session_id("/tmp/some-project", &HashSet::new());
        assert!(result.is_err(), "Empty sessions dir should return error");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("No Codex")
                || err_msg.contains("session")
                || err_msg.contains("found"),
            "Error message should mention sessions: {err_msg}"
        );

        match old_val {
            Some(v) => std::env::set_var("CODEX_HOME", v),
            None => std::env::remove_var("CODEX_HOME"),
        }
    }

    #[test]
    fn test_opencode_capture_respects_command_timeout() {
        use super::super::config::SessionConfig;
        let mut config = SessionConfig::default();
        config.opencode_command_timeout_secs = 1;
        config.opencode_max_retry_attempts = 1;

        let result = try_capture_opencode_session_id(
            "/tmp/nonexistent-project-xyz-12345",
            &HashSet::new(),
            0.0,
            &config,
        );
        let _ = result;
    }

    #[test]
    fn test_opencode_capture_deadline_exhaustion() {
        use super::super::config::SessionConfig;
        let mut config = SessionConfig::default();
        config.opencode_command_timeout_secs = 1;
        config.opencode_max_retry_attempts = 100;
        config.opencode_capture_deadline_secs = 1;

        let start = std::time::Instant::now();
        let result = capture_opencode_session_id(
            "/tmp/nonexistent-project-xyz-12345",
            &HashSet::new(),
            0.0,
            &config,
        );
        let elapsed = start.elapsed();
        assert!(
            elapsed.as_secs() < 10,
            "Capture should respect deadline, not exhaust all retries: elapsed={elapsed:?}"
        );
        let _ = result;
    }

    #[test]
    #[ignore]
    fn test_container_capture_not_running() {
        let result = capture_from_container(
            "nonexistent-container-id-xyz-12345",
            "claude",
            &HashSet::new(),
        );
        assert!(
            result.is_none(),
            "Non-existent container should return None"
        );
    }

    #[test]
    fn test_encode_claude_project_path_basic() {
        assert_eq!(
            encode_claude_project_path("/Users/foo/bar"),
            "-Users-foo-bar"
        );
    }

    #[test]
    fn test_encode_claude_project_path_preserves_alphanumeric_and_dash() {
        assert_eq!(
            encode_claude_project_path("my-project-123"),
            "my-project-123"
        );
    }

    #[test]
    fn test_encode_claude_project_path_replaces_special_chars() {
        assert_eq!(
            encode_claude_project_path("/home/user/my project (copy)"),
            "-home-user-my-project--copy-"
        );
    }

    #[test]
    #[serial]
    fn test_capture_claude_session_finds_most_recent() {
        let tmp = tempfile::tempdir().unwrap();
        let project_dir = tmp.path().join("projects").join("-tmp-myproject");
        std::fs::create_dir_all(&project_dir).unwrap();

        let uuid_old = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
        let uuid_new = "11111111-2222-3333-4444-555555555555";
        let old_file = project_dir.join(format!("{uuid_old}.jsonl"));
        let new_file = project_dir.join(format!("{uuid_new}.jsonl"));

        std::fs::write(&old_file, "old data\n").unwrap();
        // Set old file's mtime to 10 minutes ago
        let ten_min_ago = std::time::SystemTime::now() - Duration::from_secs(600);
        std::fs::File::options()
            .write(true)
            .open(&old_file)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(ten_min_ago))
            .unwrap();
        std::fs::write(&new_file, "new data\n").unwrap();

        let old_val = std::env::var("CLAUDE_CONFIG_DIR").ok();
        std::env::set_var("CLAUDE_CONFIG_DIR", tmp.path());

        let result = capture_claude_session_id("/tmp/myproject", &HashSet::new());
        assert_eq!(result.unwrap(), uuid_new);

        match old_val {
            Some(v) => std::env::set_var("CLAUDE_CONFIG_DIR", v),
            None => std::env::remove_var("CLAUDE_CONFIG_DIR"),
        }
    }

    #[test]
    #[serial]
    fn test_capture_claude_session_respects_exclusion() {
        let tmp = tempfile::tempdir().unwrap();
        let project_dir = tmp.path().join("projects").join("-tmp-myproject");
        std::fs::create_dir_all(&project_dir).unwrap();

        let uuid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
        std::fs::write(project_dir.join(format!("{uuid}.jsonl")), "data\n").unwrap();

        let old_val = std::env::var("CLAUDE_CONFIG_DIR").ok();
        std::env::set_var("CLAUDE_CONFIG_DIR", tmp.path());

        let mut exclusion = HashSet::new();
        exclusion.insert(uuid.to_string());
        let result = capture_claude_session_id("/tmp/myproject", &exclusion);
        assert!(result.is_err(), "Excluded session should not be returned");

        match old_val {
            Some(v) => std::env::set_var("CLAUDE_CONFIG_DIR", v),
            None => std::env::remove_var("CLAUDE_CONFIG_DIR"),
        }
    }

    #[test]
    #[serial]
    fn test_capture_claude_session_skips_agent_files() {
        let tmp = tempfile::tempdir().unwrap();
        let project_dir = tmp.path().join("projects").join("-tmp-myproject");
        std::fs::create_dir_all(&project_dir).unwrap();

        std::fs::write(
            project_dir.join("agent-aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee.jsonl"),
            "subagent data\n",
        )
        .unwrap();

        let old_val = std::env::var("CLAUDE_CONFIG_DIR").ok();
        std::env::set_var("CLAUDE_CONFIG_DIR", tmp.path());

        let result = capture_claude_session_id("/tmp/myproject", &HashSet::new());
        assert!(result.is_err(), "Agent files should not be picked up");

        match old_val {
            Some(v) => std::env::set_var("CLAUDE_CONFIG_DIR", v),
            None => std::env::remove_var("CLAUDE_CONFIG_DIR"),
        }
    }

    #[test]
    #[serial]
    fn test_capture_claude_session_rejects_stale() {
        let tmp = tempfile::tempdir().unwrap();
        let project_dir = tmp.path().join("projects").join("-tmp-myproject");
        std::fs::create_dir_all(&project_dir).unwrap();

        let uuid = "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee";
        let file = project_dir.join(format!("{uuid}.jsonl"));
        std::fs::write(&file, "old data\n").unwrap();

        // Set mtime to 10 minutes ago (beyond 5-minute threshold)
        let stale_time = std::time::SystemTime::now() - Duration::from_secs(600);
        std::fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(stale_time))
            .unwrap();

        let old_val = std::env::var("CLAUDE_CONFIG_DIR").ok();
        std::env::set_var("CLAUDE_CONFIG_DIR", tmp.path());

        let result = capture_claude_session_id("/tmp/myproject", &HashSet::new());
        assert!(result.is_err(), "Stale session file should be rejected");
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No active Claude session"),
            "Error should indicate no active session"
        );

        match old_val {
            Some(v) => std::env::set_var("CLAUDE_CONFIG_DIR", v),
            None => std::env::remove_var("CLAUDE_CONFIG_DIR"),
        }
    }

    #[test]
    #[serial]
    fn test_capture_claude_session_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let project_dir = tmp.path().join("projects").join("-tmp-myproject");
        std::fs::create_dir_all(&project_dir).unwrap();

        let old_val = std::env::var("CLAUDE_CONFIG_DIR").ok();
        std::env::set_var("CLAUDE_CONFIG_DIR", tmp.path());

        let result = capture_claude_session_id("/tmp/myproject", &HashSet::new());
        assert!(result.is_err(), "Empty dir should return error");

        match old_val {
            Some(v) => std::env::set_var("CLAUDE_CONFIG_DIR", v),
            None => std::env::remove_var("CLAUDE_CONFIG_DIR"),
        }
    }
}
