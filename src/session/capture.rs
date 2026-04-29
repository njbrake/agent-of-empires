//! Session ID capture logic for all supported agent types.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use anyhow::{Context, Result};
use uuid::Uuid;

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
pub(crate) fn capture_claude_session_id(project_path: &str) -> Result<String> {
    let claude_home = resolve_agent_home(Some("CLAUDE_CONFIG_DIR"), ".claude")?;
    let canonical = canonicalize_or_raw(project_path);

    // Source 1: most recently modified .jsonl in the project dir
    if let Some((id, modified)) = scan_claude_project_dir(&claude_home, &canonical)? {
        let age = modified.elapsed().unwrap_or(Duration::from_secs(u64::MAX));
        if age <= Duration::from_secs(5 * 60) {
            return Ok(id);
        }
    }

    // Source 2: lastSessionId from ~/.claude.json (same staleness threshold)
    if let Some(id) = read_claude_json_session_id(&canonical) {
        let claude_json = dirs::home_dir()
            .map(|h| h.join(".claude.json"))
            .and_then(|p| std::fs::metadata(&p).ok())
            .and_then(|m| m.modified().ok());
        let is_fresh = claude_json
            .and_then(|t| t.elapsed().ok())
            .is_some_and(|age| age <= Duration::from_secs(5 * 60));
        if is_fresh && Uuid::parse_str(&id).is_ok() {
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

        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);

        if best.as_ref().is_none_or(|(_, t)| modified > *t) {
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
pub(crate) fn claude_poll_fn(project_path: String) -> impl Fn() -> Option<String> + Send + 'static {
    move || {
        capture_claude_session_id(&project_path)
            .map_err(|e| tracing::debug!("Claude disk scan failed: {}", e))
            .ok()
            .and_then(validated_session_id)
    }
}

/// Capture Claude Code session ID inside a Docker container.
///
/// Claude in a sandboxed AoE session writes its `.jsonl` files to the
/// container's `~/.claude/projects/{encoded-cwd}/` directory, not the host's.
/// This shells `docker exec` into the running container to find the most
/// recently modified UUID-named jsonl in that directory, with a 5-minute
/// staleness guard.
pub(crate) fn capture_claude_session_id_in_container(
    container_name: &str,
    container_cwd: &str,
) -> Result<String> {
    let dir_name = encode_claude_project_path(container_cwd);

    // Shell snippet:
    //   - resolve $CLAUDE_CONFIG_DIR or $HOME/.claude
    //   - walk projects/<encoded>/ for *.jsonl files
    //   - keep ones with mtime within 5 minutes
    //   - emit basename (without .jsonl) of the most recent
    //
    // Using POSIX `find -mmin -5` and `ls -t` to avoid GNU-only `printf '%T@ %f'`.
    let snippet = format!(
        r#"
CLAUDE_HOME="${{CLAUDE_CONFIG_DIR:-$HOME/.claude}}"
DIR="$CLAUDE_HOME/projects/{dir_name}"
[ -d "$DIR" ] || exit 0
NEWEST=$(ls -t "$DIR"/*.jsonl 2>/dev/null | head -1)
[ -z "$NEWEST" ] && exit 0
[ -n "$(find "$NEWEST" -mmin -5 2>/dev/null)" ] || exit 0
basename "$NEWEST" .jsonl
"#
    );

    let output = std::process::Command::new("docker")
        .args(["exec", container_name, "sh", "-c", &snippet])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| anyhow::anyhow!("docker exec failed: {}", e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("docker exec returned non-zero: {}", stderr.trim());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let id = stdout.trim();
    if id.is_empty() {
        anyhow::bail!(
            "No active Claude session found in container {}",
            container_name
        );
    }
    if uuid::Uuid::parse_str(id).is_err() {
        anyhow::bail!("Container returned non-UUID session ID: {:?}", id);
    }

    Ok(id.to_string())
}

/// Polling closure for sandboxed (Docker) Claude Code session tracking.
pub(crate) fn claude_poll_fn_sandboxed(
    container_name: String,
    container_cwd: String,
) -> impl Fn() -> Option<String> + Send + 'static {
    move || {
        capture_claude_session_id_in_container(&container_name, &container_cwd)
            .map_err(|e| tracing::debug!("Claude container scan failed: {}", e))
            .ok()
            .and_then(validated_session_id)
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
            .and_then(validated_session_id)
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
            .and_then(validated_session_id)
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
            .and_then(validated_session_id)
    }
}

const VIBE_COMMAND_TIMEOUT_SECS: u64 = 5;

/// Shell snippet executed via `docker exec` to enumerate Vibe `meta.json` files
/// inside the container. Each file is emitted as a `===VIBE:<unix-mtime>===`
/// header followed by the JSON body and a `===END===` trailer; the host parses
/// this stream rather than spawning one `docker exec cat` per file.
const VIBE_CONTAINER_LIST_SCRIPT: &str = r#"SESS_DIR="${VIBE_HOME:-$HOME/.vibe}/logs/session"
[ -d "$SESS_DIR" ] || exit 0
for d in "$SESS_DIR"/*/; do
  m="$d/meta.json"
  [ -f "$m" ] || continue
  ts=$(stat -c %Y "$m" 2>/dev/null || stat -f %m "$m" 2>/dev/null || echo 0)
  printf '===VIBE:%s===\n' "$ts"
  cat "$m"
  printf '\n===END===\n'
done
"#;

pub(crate) fn try_capture_vibe_session_id_in_container(
    container_name: &str,
    container_cwd: &str,
    exclusion: &HashSet<String>,
) -> Result<String> {
    let mut cmd = std::process::Command::new("docker");
    cmd.args([
        "exec",
        container_name,
        "sh",
        "-c",
        VIBE_CONTAINER_LIST_SCRIPT,
    ]);

    let stdout_bytes = run_with_timeout(
        cmd,
        Duration::from_secs(VIBE_COMMAND_TIMEOUT_SECS),
        "docker exec sh (vibe meta scan)",
    )?;
    select_vibe_session_in_container(&stdout_bytes, container_cwd, exclusion)
}

fn select_vibe_session_in_container(
    stdout_bytes: &[u8],
    container_cwd: &str,
    exclusion: &HashSet<String>,
) -> Result<String> {
    let text = String::from_utf8_lossy(stdout_bytes);
    let mut candidates: Vec<(String, Option<String>, u64)> = Vec::new();

    for chunk in text.split("===VIBE:").skip(1) {
        let (ts_str, rest) = match chunk.split_once("===\n") {
            Some(p) => p,
            None => continue,
        };
        let ts: u64 = ts_str.trim().parse().unwrap_or(0);
        let json_part = match rest.split_once("\n===END===") {
            Some((j, _)) => j,
            None => rest,
        };
        let (session_id, cwd) = match parse_vibe_meta_json(json_part.trim()) {
            Some(pair) if !pair.0.is_empty() && !exclusion.contains(&pair.0) => pair,
            _ => continue,
        };
        candidates.push((session_id, cwd, ts));
    }

    if candidates.is_empty() {
        anyhow::bail!("No Vibe sessions found in container");
    }

    candidates.sort_by_key(|c| std::cmp::Reverse(c.2));

    let project_match = candidates
        .iter()
        .find(|(_, cwd, _)| cwd.as_deref() == Some(container_cwd));

    project_match
        .map(|(id, _, _)| id.clone())
        .ok_or_else(|| anyhow::anyhow!("No Vibe session matching container CWD"))
}

fn parse_vibe_meta_json(content: &str) -> Option<(String, Option<String>)> {
    let parsed: serde_json::Value = serde_json::from_str(content).ok()?;
    let session_id = parsed
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(String::from)?;
    let cwd = parsed
        .get("environment")
        .and_then(|env| env.get("working_directory"))
        .and_then(|v| v.as_str())
        .map(String::from);
    Some((session_id, cwd))
}

pub(crate) fn vibe_poll_fn_sandboxed(
    container_name: String,
    container_cwd: String,
    instance_id: String,
) -> impl Fn() -> Option<String> + Send + 'static {
    move || {
        let exclusion = build_exclusion_set(&instance_id);
        try_capture_vibe_session_id_in_container(&container_name, &container_cwd, &exclusion)
            .map_err(|e| tracing::debug!("Vibe container poll capture failed: {}", e))
            .ok()
            .and_then(validated_session_id)
    }
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

    let chosen = session_entries.iter().find_map(|(path, _)| {
        let cwd_matches = extract_codex_cwd_from_file(path)
            .and_then(|cwd| std::fs::canonicalize(&cwd).ok())
            .map(|cwd| cwd == canonical_project)
            .unwrap_or(false);
        if cwd_matches {
            extract_codex_uuid_from_filename(path)
        } else {
            None
        }
    });

    chosen.ok_or_else(|| anyhow::anyhow!("No Codex session found matching project path"))
}

/// Extract UUID from a Codex rollout filename.
///
/// Codex filenames follow the pattern `rollout-YYYY-MM-DDThh-mm-ss-<uuid>.jsonl`.
/// The UUID is the last hyphen-delimited segment before `.jsonl`, comprising 5 groups
/// (e.g. `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`).
pub(crate) fn extract_codex_uuid_from_filename(path: &std::path::Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    if stem.len() >= 36 {
        let candidate = &stem[stem.len() - 36..];
        if Uuid::parse_str(candidate).is_ok() {
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
    let digest = Sha256::digest(canonical_project.to_string_lossy().as_bytes());
    let expected_hash = digest
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>();

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
        sid.as_deref()
            .map(|id| !id.is_empty() && !exclusion.contains(id))
            .unwrap_or(false)
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

    let mut candidates: Vec<(String, Option<String>, std::time::SystemTime)> = Vec::new();

    for entry in resilient_read_dir(&sessions_dir)? {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let meta_path = path.join("meta.json");
        if !meta_path.exists() {
            continue;
        }
        let (session_id, cwd) = match extract_vibe_meta(&meta_path) {
            Some(pair) if !pair.0.is_empty() && !exclusion.contains(&pair.0) => pair,
            _ => continue,
        };
        let modified = std::fs::metadata(&meta_path)
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
        candidates.push((session_id, cwd, modified));
    }

    if candidates.is_empty() {
        anyhow::bail!(
            "No Vibe session directories found in {}",
            sessions_dir.display()
        );
    }

    candidates.sort_by(|a, b| b.2.cmp(&a.2));

    let canonical_project = canonicalize_or_raw(project_path);

    let project_match = candidates.iter().find(|(_, cwd, _)| {
        cwd.as_ref()
            .and_then(|cwd| std::fs::canonicalize(cwd).ok())
            .map(|cwd| cwd == canonical_project)
            .unwrap_or(false)
    });

    project_match
        .map(|(id, _, _)| id.clone())
        .ok_or_else(|| anyhow::anyhow!("No Vibe session found matching project path"))
}

/// Parse a Vibe `meta.json` once, returning both `session_id` and the working directory.
fn extract_vibe_meta(path: &std::path::Path) -> Option<(String, Option<String>)> {
    let content = std::fs::read_to_string(path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    let session_id = parsed
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(String::from)?;
    let cwd = parsed
        .get("environment")
        .and_then(|env| env.get("working_directory"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| extract_cwd_from_json(&parsed, &["cwd", "working_directory", "project_path"]));
    Some((session_id, cwd))
}

/// Extract CWD from a Vibe `meta.json`.
///
/// The actual path lives at `environment.working_directory` (nested object).
/// Falls back to top-level `cwd` / `working_directory` for forward compatibility.
#[cfg(test)]
pub(crate) fn extract_vibe_cwd_from_meta(path: &std::path::Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    parsed
        .get("environment")
        .and_then(|env| env.get("working_directory"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| extract_cwd_from_json(&parsed, &["cwd", "working_directory", "project_path"]))
}

/// Extract the `session_id` UUID from a Vibe `meta.json` file.
#[cfg(test)]
pub(crate) fn extract_vibe_session_id_from_meta(path: &std::path::Path) -> Option<String> {
    extract_vibe_meta(path).map(|(id, _)| id)
}

/// Encode a project path into Pi's directory naming convention.
///
/// Replicates Pi's `getDefaultSessionDir` encoding from
/// [`session-manager.ts#L428-L435`](https://github.com/badlogic/pi-mono/blob/main/packages/pi/src/session-manager.ts#L428-L435):
/// ```typescript
/// const safePath = `--${cwd.replace(/^[/\\]/, "").replace(/[/\\:]/g, "-")}--`;
/// ```
pub(crate) fn encode_pi_project_path(cwd: &str) -> String {
    let stripped = cwd
        .strip_prefix('/')
        .or_else(|| cwd.strip_prefix('\\'))
        .unwrap_or(cwd);

    let encoded: String = stripped
        .chars()
        .map(|c| match c {
            '/' | '\\' | ':' => '-',
            _ => c,
        })
        .collect();

    format!("--{encoded}--")
}

/// Read a Pi session JSONL header once and return both session ID and CWD.
fn extract_pi_header_fields(path: &Path) -> Option<(Option<String>, Option<String>)> {
    let file = std::fs::File::open(path).ok()?;
    let reader = std::io::BufReader::new(file);
    let first_line = std::io::BufRead::lines(reader).next()?.ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&first_line).ok()?;
    if parsed.get("type")?.as_str()? != "session" {
        return None;
    }
    let session_id = parsed.get("id").and_then(|v| v.as_str()).map(String::from);
    let cwd = parsed
        .get("cwd")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);
    Some((session_id, cwd))
}

/// Extract the session ID from the first line of a Pi `.jsonl` session file.
///
/// The first line must be a JSON object with `"type": "session"` and an `"id"` field.
/// Returns `None` if the file is empty, unreadable, or the header doesn't match.
pub(crate) fn extract_pi_session_id_from_header(path: &Path) -> Option<String> {
    extract_pi_header_fields(path).and_then(|(id, _)| id)
}

/// Extract the working directory from the first line of a Pi `.jsonl` session file.
///
/// The first line must be a JSON object with `"type": "session"` and a `"cwd"` field.
/// Returns `None` if missing or empty.
#[cfg(test)]
pub(crate) fn extract_pi_cwd_from_header(path: &Path) -> Option<String> {
    extract_pi_header_fields(path).and_then(|(_, cwd)| cwd)
}

/// Extract a UUID from a Pi session filename.
///
/// Pi filenames follow the pattern `2024-12-03T14-00-00-000Z_<uuid>.jsonl`.
/// The UUID is the substring after the last `_`, before `.jsonl`.
pub(crate) fn extract_pi_uuid_from_filename(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let uuid_part = stem.rsplit('_').next()?;
    Uuid::parse_str(uuid_part).ok()?;
    Some(uuid_part.to_string())
}

/// Capture Pi session ID by scanning the Pi agent sessions directory.
///
/// Looks for `.jsonl` session files under `~/.pi/agent/sessions/` (or
/// `$PI_CODING_AGENT_DIR/sessions/`). The primary lookup uses the encoded
/// project path as a directory name. Falls back to scanning all session
/// directories and matching via the `cwd` header field. Session IDs present
/// in `exclusion` are skipped.
pub(crate) fn capture_pi_session_id(
    project_path: &str,
    exclusion: &HashSet<String>,
) -> Result<String> {
    let pi_home = resolve_agent_home(Some("PI_CODING_AGENT_DIR"), ".pi/agent")?;
    let sessions_dir = pi_home.join("sessions");

    if !sessions_dir.exists() {
        anyhow::bail!(
            "Pi sessions directory not found: {}",
            sessions_dir.display()
        );
    }

    let encoded_name = encode_pi_project_path(project_path);
    let project_dir = sessions_dir.join(&encoded_name);

    // Primary path: scan the encoded project directory
    if project_dir.is_dir() {
        let mut candidates: Vec<(String, std::time::SystemTime)> = Vec::new();

        for entry in resilient_read_dir(&project_dir)? {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let session_id = match extract_pi_session_id_from_header(&path) {
                Some(id) if !id.is_empty() && !exclusion.contains(&id) => id,
                _ => continue,
            };
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            candidates.push((session_id, modified));
        }

        candidates.sort_by(|a, b| b.1.cmp(&a.1));

        if let Some((id, _)) = candidates.first() {
            return Ok(id.clone());
        }

        anyhow::bail!("No Pi session found matching project path");
    }

    // Fallback: scan all subdirectories and match via CWD header
    let canonical_project = canonicalize_or_raw(project_path);
    let mut fallback_candidates: Vec<(String, std::time::SystemTime)> = Vec::new();

    for subdir_entry in resilient_read_dir(&sessions_dir)? {
        let subdir_path = subdir_entry.path();
        if !subdir_path.is_dir() {
            continue;
        }
        for file_entry in resilient_read_dir(&subdir_path)? {
            let file_path = file_entry.path();
            if file_path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let fields = match extract_pi_header_fields(&file_path) {
                Some(f) => f,
                None => continue,
            };
            let cwd = match fields.1 {
                Some(c) if !c.is_empty() => c,
                _ => continue,
            };
            let canonical_cwd = canonicalize_or_raw(&cwd);
            if canonical_cwd != canonical_project {
                continue;
            }
            let session_id = match fields.0 {
                Some(id) if !id.is_empty() && !exclusion.contains(&id) => id,
                _ => continue,
            };
            let modified = file_entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            fallback_candidates.push((session_id, modified));
        }
    }

    fallback_candidates.sort_by(|a, b| b.1.cmp(&a.1));

    if let Some((id, _)) = fallback_candidates.first() {
        return Ok(id.clone());
    }

    // Tier 3: when all JSONL headers fail to parse, pick the most
    // recently modified session directory and extract a UUID from its files.
    let mut dirs_by_mtime: Vec<(PathBuf, std::time::SystemTime)> = Vec::new();
    if let Ok(entries) = resilient_read_dir(&sessions_dir) {
        for entry in entries {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let mtime = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
            dirs_by_mtime.push((path, mtime));
        }
    }
    dirs_by_mtime.sort_by(|a, b| b.1.cmp(&a.1));

    for (dir, _) in &dirs_by_mtime {
        if let Ok(entries) = resilient_read_dir(dir) {
            let mut file_candidates: Vec<(String, std::time::SystemTime)> = Vec::new();
            for entry in entries {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                    continue;
                }
                if let Some(uuid) = extract_pi_uuid_from_filename(&path) {
                    if !exclusion.contains(&uuid) {
                        let mtime = entry
                            .metadata()
                            .and_then(|m| m.modified())
                            .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                        file_candidates.push((uuid, mtime));
                    }
                }
            }
            file_candidates.sort_by(|a, b| b.1.cmp(&a.1));
            if let Some((id, _)) = file_candidates.first() {
                return Ok(id.clone());
            }
        }
    }

    anyhow::bail!("No Pi session found matching project path")
}

/// Polling closure for Pi session tracking.
pub(crate) fn pi_poll_fn(
    project_path: String,
    instance_id: String,
) -> impl Fn() -> Option<String> + Send + 'static {
    move || {
        let exclusion = build_exclusion_set(&instance_id);
        capture_pi_session_id(&project_path, &exclusion)
            .map_err(|e| tracing::debug!("Pi poll capture failed: {}", e))
            .ok()
            .and_then(validated_session_id)
    }
}

const PI_COMMAND_TIMEOUT_SECS: u64 = 5;

/// Shell snippet executed via `docker exec` to enumerate Pi `.jsonl` session
/// files inside the container. Each file is emitted as a `===PI:<unix-mtime>===`
/// header followed by the first line of the file (the session header) and a
/// `===END===` trailer; the host parses this stream rather than spawning one
/// `docker exec head` per file.
const PI_CONTAINER_LIST_SCRIPT: &str = r#"SESS_DIR="${PI_CODING_AGENT_DIR:-$HOME/.pi/agent}/sessions"
[ -d "$SESS_DIR" ] || exit 0
for d in "$SESS_DIR"/*/; do
  for f in "$d"*.jsonl; do
    [ -f "$f" ] || continue
    ts=$(stat -c %Y "$f" 2>/dev/null || stat -f %m "$f" 2>/dev/null || echo 0)
    printf '===PI:%s===\n' "$ts"
    head -n 1 "$f"
    printf '\n===END===\n'
  done
done
"#;

pub(crate) fn try_capture_pi_session_id_in_container(
    container_name: &str,
    container_cwd: &str,
    exclusion: &HashSet<String>,
) -> Result<String> {
    let mut cmd = std::process::Command::new("docker");
    cmd.args(["exec", container_name, "sh", "-c", PI_CONTAINER_LIST_SCRIPT]);

    let stdout_bytes = run_with_timeout(
        cmd,
        Duration::from_secs(PI_COMMAND_TIMEOUT_SECS),
        "docker exec sh (pi session scan)",
    )?;
    select_pi_session_in_container(&stdout_bytes, container_cwd, exclusion)
}

fn select_pi_session_in_container(
    stdout_bytes: &[u8],
    container_cwd: &str,
    exclusion: &HashSet<String>,
) -> Result<String> {
    let text = String::from_utf8_lossy(stdout_bytes);
    let mut candidates: Vec<(String, Option<String>, u64)> = Vec::new();

    for chunk in text.split("===PI:").skip(1) {
        let (ts_str, rest) = match chunk.split_once("===\n") {
            Some(p) => p,
            None => continue,
        };
        let ts: u64 = ts_str.trim().parse().unwrap_or(0);
        let json_part = match rest.split_once("\n===END===") {
            Some((j, _)) => j,
            None => rest,
        };
        let (id_opt, cwd) = match parse_pi_header_json(json_part.trim()) {
            Some(p) => p,
            None => continue,
        };
        let session_id = match id_opt {
            Some(id) if !id.is_empty() && !exclusion.contains(&id) => id,
            _ => continue,
        };
        candidates.push((session_id, cwd, ts));
    }

    if candidates.is_empty() {
        anyhow::bail!("No Pi sessions found in container");
    }

    candidates.sort_by_key(|c| std::cmp::Reverse(c.2));

    let project_match = candidates
        .iter()
        .find(|(_, cwd, _)| cwd.as_deref() == Some(container_cwd));

    project_match
        .map(|(id, _, _)| id.clone())
        .ok_or_else(|| anyhow::anyhow!("No Pi session matching container CWD"))
}

fn parse_pi_header_json(line: &str) -> Option<(Option<String>, Option<String>)> {
    let parsed: serde_json::Value = serde_json::from_str(line).ok()?;
    if parsed.get("type")?.as_str()? != "session" {
        return None;
    }
    let session_id = parsed.get("id").and_then(|v| v.as_str()).map(String::from);
    let cwd = parsed
        .get("cwd")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(String::from);
    Some((session_id, cwd))
}

pub(crate) fn pi_poll_fn_sandboxed(
    container_name: String,
    container_cwd: String,
    instance_id: String,
) -> impl Fn() -> Option<String> + Send + 'static {
    move || {
        let exclusion = build_exclusion_set(&instance_id);
        try_capture_pi_session_id_in_container(&container_name, &container_cwd, &exclusion)
            .map_err(|e| tracing::debug!("Pi container poll capture failed: {}", e))
            .ok()
            .and_then(validated_session_id)
    }
}

pub(crate) fn is_valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
}

/// Build a set of session IDs already claimed by other AoE instances.
///
/// Lists all tmux sessions with the AoE prefix, reads each one's hidden env vars
/// to find its instance ID and captured session ID, and collects all captured IDs
/// from instances other than `current_instance_id`.
pub(crate) fn build_exclusion_set(current_instance_id: &str) -> HashSet<String> {
    let output = match std::process::Command::new("tmux")
        .args(["list-sessions", "-F", "#{session_name}"])
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => return HashSet::new(),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let aoe_sessions: Vec<&str> = stdout
        .lines()
        .filter(|name| {
            name.starts_with(crate::tmux::SESSION_PREFIX)
                && !name.starts_with(crate::tmux::TERMINAL_PREFIX)
                && !name.starts_with(crate::tmux::CONTAINER_TERMINAL_PREFIX)
        })
        .collect();

    if aoe_sessions.is_empty() {
        return HashSet::new();
    }

    let instance_ids = crate::tmux::env::get_hidden_env_batch(
        &aoe_sessions,
        crate::tmux::env::AOE_INSTANCE_ID_KEY,
    );

    let other_sessions: Vec<&str> = instance_ids
        .iter()
        .filter(|(_, owner)| owner.as_deref() != Some(current_instance_id))
        .map(|(name, _)| name.as_str())
        .collect();

    if other_sessions.is_empty() {
        return HashSet::new();
    }

    let captured_ids = crate::tmux::env::get_hidden_env_batch(
        &other_sessions,
        crate::tmux::env::AOE_CAPTURED_SESSION_ID_KEY,
    );

    captured_ids.into_iter().filter_map(|(_, id)| id).collect()
}

/// Filter, sort, and deduplicate agent sessions by project directory.
///
/// Given a list of parsed session JSON values:
/// 1. Filters to sessions matching `project_path` (canonicalized comparison on `directory`)
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

const OPENCODE_COMMAND_TIMEOUT_SECS: u64 = 5;

/// Spawn `cmd`, read stdout to EOF on a worker thread, and wait for the
/// process to exit. Kills the child if `timeout` elapses first.
fn run_with_timeout(
    mut cmd: std::process::Command,
    timeout: Duration,
    label: &str,
) -> Result<Vec<u8>> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::null());
    let mut child = cmd
        .spawn()
        .with_context(|| format!("Failed to spawn '{}'", label))?;

    let stdout_pipe = child.stdout.take();
    let stdout_handle = std::thread::spawn(move || {
        stdout_pipe.map(|mut r| {
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut r, &mut buf).ok();
            buf
        })
    });

    let deadline = std::time::Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break s,
            Ok(None) => {
                if std::time::Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(anyhow::anyhow!("{} timed out", label));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(anyhow::anyhow!("Failed to wait on {}: {}", label, e)),
        }
    };

    let stdout_bytes = stdout_handle.join().ok().flatten().unwrap_or_default();

    if !status.success() {
        anyhow::bail!("{} command failed", label);
    }

    Ok(stdout_bytes)
}

/// Parse `opencode session list --format json` output and pick the best match.
///
/// `match_path` is the directory the session's `directory` field is compared
/// against. For host capture this is the host project path; for sandboxed
/// capture this is the container CWD (since opencode records its own CWD).
fn select_opencode_session(
    stdout_bytes: &[u8],
    match_path: &str,
    exclusion: &HashSet<String>,
    launch_time_ms: Option<f64>,
) -> Result<String> {
    let stdout = String::from_utf8_lossy(stdout_bytes);
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        anyhow::bail!("No OpenCode sessions found");
    }
    let session_entries: Vec<serde_json::Value> =
        serde_json::from_str(trimmed).context("Failed to parse OpenCode session list JSON")?;

    let matching = filter_agent_sessions(
        &session_entries,
        Some(match_path),
        exclusion,
        launch_time_ms,
    );

    matching
        .first()
        .and_then(|s| s["id"].as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow::anyhow!("No OpenCode sessions found matching project path"))
}

/// Capture an OpenCode session ID by running `opencode session list --format json`,
/// parsing the output, and matching by CWD. Returns error (not fallback) when
/// no unexcluded session matches.
///
/// `launch_time_ms` is the lower bound on the session's `updated` timestamp,
/// used to ignore stale sessions left over from prior runs. Pass `None` for
/// retroactive capture on TUI startup, when the launch time isn't known.
pub(crate) fn try_capture_opencode_session_id(
    project_path: &str,
    exclusion: &HashSet<String>,
    launch_time_ms: Option<f64>,
) -> Result<String> {
    let mut cmd = std::process::Command::new("opencode");
    cmd.args(["session", "list", "--format", "json"])
        .current_dir(project_path);

    let stdout_bytes = run_with_timeout(
        cmd,
        Duration::from_secs(OPENCODE_COMMAND_TIMEOUT_SECS),
        "opencode session list",
    )?;
    select_opencode_session(&stdout_bytes, project_path, exclusion, launch_time_ms)
}

/// Capture an OpenCode session ID from inside a Docker container.
///
/// Mirrors `try_capture_opencode_session_id` but runs `opencode session list`
/// via `docker exec -w <cwd>`. Matching is done against `container_cwd` (the
/// path opencode-in-container records as its working directory), not the host
/// project path.
pub(crate) fn try_capture_opencode_session_id_in_container(
    container_name: &str,
    container_cwd: &str,
    exclusion: &HashSet<String>,
    launch_time_ms: Option<f64>,
) -> Result<String> {
    let mut cmd = std::process::Command::new("docker");
    cmd.args([
        "exec",
        "-w",
        container_cwd,
        container_name,
        "opencode",
        "session",
        "list",
        "--format",
        "json",
    ]);

    let stdout_bytes = run_with_timeout(
        cmd,
        Duration::from_secs(OPENCODE_COMMAND_TIMEOUT_SECS),
        "opencode session list (container)",
    )?;
    select_opencode_session(&stdout_bytes, container_cwd, exclusion, launch_time_ms)
}

/// Polling closure for OpenCode session tracking.
pub(crate) fn opencode_poll_fn(
    project_path: String,
    instance_id: String,
    launch_time_ms: f64,
) -> impl Fn() -> Option<String> + Send + 'static {
    move || {
        let exclusion = build_exclusion_set(&instance_id);
        try_capture_opencode_session_id(&project_path, &exclusion, Some(launch_time_ms))
            .map_err(|e| tracing::debug!("OpenCode poll capture failed: {}", e))
            .ok()
            .and_then(validated_session_id)
    }
}

/// Polling closure for sandboxed (Docker) OpenCode session tracking.
pub(crate) fn opencode_poll_fn_sandboxed(
    container_name: String,
    container_cwd: String,
    instance_id: String,
    launch_time_ms: f64,
) -> impl Fn() -> Option<String> + Send + 'static {
    move || {
        let exclusion = build_exclusion_set(&instance_id);
        try_capture_opencode_session_id_in_container(
            &container_name,
            &container_cwd,
            &exclusion,
            Some(launch_time_ms),
        )
        .map_err(|e| tracing::debug!("OpenCode container poll capture failed: {}", e))
        .ok()
        .and_then(validated_session_id)
    }
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

        let session = matching.first().copied();
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
        let old_file = sessions_dir.join(format!("rollout-2025-01-01T00-00-00-{}.jsonl", uuid_old));
        std::fs::write(&old_file, "{}").unwrap();
        let old_time = std::time::SystemTime::now() - Duration::from_secs(600);
        std::fs::File::options()
            .write(true)
            .open(&old_file)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(old_time))
            .unwrap();
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

        let session = matching.first().copied();
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
        let result = try_capture_opencode_session_id(
            "/tmp/nonexistent-project-xyz-12345",
            &HashSet::new(),
            None,
        );
        let _ = result;
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

        let result = capture_claude_session_id("/tmp/myproject");
        assert_eq!(result.unwrap(), uuid_new);

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

        let result = capture_claude_session_id("/tmp/myproject");
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

        let result = capture_claude_session_id("/tmp/myproject");
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

        let result = capture_claude_session_id("/tmp/myproject");
        assert!(result.is_err(), "Empty dir should return error");

        match old_val {
            Some(v) => std::env::set_var("CLAUDE_CONFIG_DIR", v),
            None => std::env::remove_var("CLAUDE_CONFIG_DIR"),
        }
    }

    #[test]
    fn test_encode_pi_project_path_basic() {
        assert_eq!(
            encode_pi_project_path("/home/user/project"),
            "--home-user-project--"
        );
    }

    #[test]
    fn test_encode_pi_project_path_with_dashes() {
        assert_eq!(
            encode_pi_project_path("/home/user/my-project"),
            "--home-user-my-project--"
        );
    }

    #[test]
    fn test_encode_pi_project_path_trailing_slash() {
        assert_eq!(
            encode_pi_project_path("/home/user/project/"),
            "--home-user-project---"
        );
    }

    #[test]
    fn test_encode_pi_project_path_double_slash() {
        assert_eq!(
            encode_pi_project_path("/a//double/slash"),
            "--a--double-slash--"
        );
    }

    #[test]
    fn test_encode_pi_project_path_spaces() {
        assert_eq!(
            encode_pi_project_path("/path/with spaces"),
            "--path-with spaces--"
        );
    }

    #[test]
    fn test_encode_pi_project_path_windows_backslash() {
        assert_eq!(
            encode_pi_project_path("C:\\Users\\bob\\proj"),
            "--C--Users-bob-proj--"
        );
    }

    #[test]
    fn test_encode_pi_project_path_colon() {
        assert_eq!(encode_pi_project_path("C:/Users/bob"), "--C--Users-bob--");
    }

    #[test]
    fn test_encode_pi_project_path_root() {
        assert_eq!(encode_pi_project_path("/"), "----");
    }

    #[test]
    fn test_extract_pi_session_id_from_header_valid() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session.jsonl");
        std::fs::write(
            &path,
            r#"{"type":"session","id":"019342ab-1234-7def-8901-abcdef012345","cwd":"/tmp"}"#,
        )
        .unwrap();
        assert_eq!(
            extract_pi_session_id_from_header(&path),
            Some("019342ab-1234-7def-8901-abcdef012345".to_string())
        );
    }

    #[test]
    fn test_extract_pi_session_id_from_header_missing_id() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session.jsonl");
        std::fs::write(&path, r#"{"type":"session","cwd":"/tmp"}"#).unwrap();
        assert_eq!(extract_pi_session_id_from_header(&path), None);
    }

    #[test]
    fn test_extract_pi_session_id_from_header_invalid_json() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session.jsonl");
        std::fs::write(&path, "not valid json at all").unwrap();
        assert_eq!(extract_pi_session_id_from_header(&path), None);
    }

    #[test]
    fn test_extract_pi_session_id_from_header_empty_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session.jsonl");
        std::fs::write(&path, "").unwrap();
        assert_eq!(extract_pi_session_id_from_header(&path), None);
    }

    #[test]
    fn test_extract_pi_cwd_from_header() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session.jsonl");
        std::fs::write(
            &path,
            r#"{"type":"session","id":"aaa","cwd":"/home/user/project"}"#,
        )
        .unwrap();
        assert_eq!(
            extract_pi_cwd_from_header(&path),
            Some("/home/user/project".to_string())
        );
    }

    #[test]
    fn test_extract_pi_uuid_from_filename() {
        let path =
            PathBuf::from("2024-12-03T14-00-00-000Z_019342ab-1234-7def-8901-abcdef012345.jsonl");
        assert_eq!(
            extract_pi_uuid_from_filename(&path),
            Some("019342ab-1234-7def-8901-abcdef012345".to_string())
        );
    }

    #[test]
    #[serial]
    fn test_capture_pi_session_id_basic() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions_dir = tmp.path().join("sessions");
        let project_encoded = encode_pi_project_path("/home/user/project");
        let project_dir = sessions_dir.join(&project_encoded);
        std::fs::create_dir_all(&project_dir).unwrap();

        let uuid = "019342ab-1234-7def-8901-abcdef012345";
        std::fs::write(
            project_dir.join(format!("2024-12-03T14-00-00-000Z_{uuid}.jsonl")),
            format!(r#"{{"type":"session","id":"{uuid}","cwd":"/home/user/project"}}"#),
        )
        .unwrap();

        let old_val = std::env::var("PI_CODING_AGENT_DIR").ok();
        std::env::set_var("PI_CODING_AGENT_DIR", tmp.path());

        let result = capture_pi_session_id("/home/user/project", &HashSet::new());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), uuid);

        match old_val {
            Some(v) => std::env::set_var("PI_CODING_AGENT_DIR", v),
            None => std::env::remove_var("PI_CODING_AGENT_DIR"),
        }
    }

    #[test]
    #[serial]
    fn test_capture_pi_session_id_most_recent_wins() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions_dir = tmp.path().join("sessions");
        let project_encoded = encode_pi_project_path("/home/user/project");
        let project_dir = sessions_dir.join(&project_encoded);
        std::fs::create_dir_all(&project_dir).unwrap();

        let uuid_old = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
        let uuid_new = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";

        std::fs::write(
            project_dir.join(format!("2024-12-01T10-00-00-000Z_{uuid_old}.jsonl")),
            format!(r#"{{"type":"session","id":"{uuid_old}","cwd":"/home/user/project"}}"#),
        )
        .unwrap();
        let old_time = std::time::SystemTime::now() - Duration::from_secs(600);
        std::fs::File::options()
            .write(true)
            .open(project_dir.join(format!("2024-12-01T10-00-00-000Z_{uuid_old}.jsonl")))
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(old_time))
            .unwrap();
        std::fs::write(
            project_dir.join(format!("2024-12-03T14-00-00-000Z_{uuid_new}.jsonl")),
            format!(r#"{{"type":"session","id":"{uuid_new}","cwd":"/home/user/project"}}"#),
        )
        .unwrap();

        let old_val = std::env::var("PI_CODING_AGENT_DIR").ok();
        std::env::set_var("PI_CODING_AGENT_DIR", tmp.path());

        let result = capture_pi_session_id("/home/user/project", &HashSet::new());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), uuid_new);

        match old_val {
            Some(v) => std::env::set_var("PI_CODING_AGENT_DIR", v),
            None => std::env::remove_var("PI_CODING_AGENT_DIR"),
        }
    }

    #[test]
    #[serial]
    fn test_capture_pi_session_id_exclusion() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions_dir = tmp.path().join("sessions");
        let project_encoded = encode_pi_project_path("/home/user/project");
        let project_dir = sessions_dir.join(&project_encoded);
        std::fs::create_dir_all(&project_dir).unwrap();

        let uuid_excluded = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
        let uuid_kept = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";

        std::fs::write(
            project_dir.join(format!("2024-12-01T10-00-00-000Z_{uuid_excluded}.jsonl")),
            format!(r#"{{"type":"session","id":"{uuid_excluded}","cwd":"/home/user/project"}}"#),
        )
        .unwrap();
        std::fs::write(
            project_dir.join(format!("2024-12-03T14-00-00-000Z_{uuid_kept}.jsonl")),
            format!(r#"{{"type":"session","id":"{uuid_kept}","cwd":"/home/user/project"}}"#),
        )
        .unwrap();

        let old_val = std::env::var("PI_CODING_AGENT_DIR").ok();
        std::env::set_var("PI_CODING_AGENT_DIR", tmp.path());

        let mut exclusion = HashSet::new();
        exclusion.insert(uuid_excluded.to_string());

        let result = capture_pi_session_id("/home/user/project", &exclusion);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), uuid_kept);

        match old_val {
            Some(v) => std::env::set_var("PI_CODING_AGENT_DIR", v),
            None => std::env::remove_var("PI_CODING_AGENT_DIR"),
        }
    }

    #[test]
    #[serial]
    fn test_capture_pi_session_id_all_excluded() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions_dir = tmp.path().join("sessions");
        let project_encoded = encode_pi_project_path("/home/user/project");
        let project_dir = sessions_dir.join(&project_encoded);
        std::fs::create_dir_all(&project_dir).unwrap();

        let uuid = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
        std::fs::write(
            project_dir.join(format!("2024-12-03T14-00-00-000Z_{uuid}.jsonl")),
            format!(r#"{{"type":"session","id":"{uuid}","cwd":"/home/user/project"}}"#),
        )
        .unwrap();

        let old_val = std::env::var("PI_CODING_AGENT_DIR").ok();
        std::env::set_var("PI_CODING_AGENT_DIR", tmp.path());

        let mut exclusion = HashSet::new();
        exclusion.insert(uuid.to_string());

        let result = capture_pi_session_id("/home/user/project", &exclusion);
        assert!(result.is_err());

        match old_val {
            Some(v) => std::env::set_var("PI_CODING_AGENT_DIR", v),
            None => std::env::remove_var("PI_CODING_AGENT_DIR"),
        }
    }

    #[test]
    #[serial]
    fn test_capture_pi_session_id_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions_dir = tmp.path().join("sessions");
        let project_encoded = encode_pi_project_path("/home/user/project");
        let project_dir = sessions_dir.join(&project_encoded);
        std::fs::create_dir_all(&project_dir).unwrap();

        let old_val = std::env::var("PI_CODING_AGENT_DIR").ok();
        std::env::set_var("PI_CODING_AGENT_DIR", tmp.path());

        let result = capture_pi_session_id("/home/user/project", &HashSet::new());
        assert!(result.is_err());

        match old_val {
            Some(v) => std::env::set_var("PI_CODING_AGENT_DIR", v),
            None => std::env::remove_var("PI_CODING_AGENT_DIR"),
        }
    }

    #[test]
    #[serial]
    fn test_capture_pi_session_id_no_matching_project() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions_dir = tmp.path().join("sessions");
        let other_encoded = encode_pi_project_path("/home/user/other-project");
        let other_dir = sessions_dir.join(&other_encoded);
        std::fs::create_dir_all(&other_dir).unwrap();

        // Use a filename without a parseable UUID so tier 3 also fails
        std::fs::write(
            other_dir.join("session-no-uuid.jsonl"),
            r#"{"type":"session","id":"not-a-uuid","cwd":"/home/user/other-project"}"#,
        )
        .unwrap();

        let old_val = std::env::var("PI_CODING_AGENT_DIR").ok();
        std::env::set_var("PI_CODING_AGENT_DIR", tmp.path());

        let result = capture_pi_session_id("/home/user/my-project", &HashSet::new());
        assert!(result.is_err());

        match old_val {
            Some(v) => std::env::set_var("PI_CODING_AGENT_DIR", v),
            None => std::env::remove_var("PI_CODING_AGENT_DIR"),
        }
    }

    #[test]
    #[serial]
    fn test_capture_pi_session_id_invalid_jsonl_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions_dir = tmp.path().join("sessions");
        let project_encoded = encode_pi_project_path("/home/user/project");
        let project_dir = sessions_dir.join(&project_encoded);
        std::fs::create_dir_all(&project_dir).unwrap();

        std::fs::write(
            project_dir.join("2024-12-01T10-00-00-000Z_aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa.jsonl"),
            "this is not valid json",
        )
        .unwrap();

        let uuid_valid = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";
        std::fs::write(
            project_dir.join(format!("2024-12-03T14-00-00-000Z_{uuid_valid}.jsonl")),
            format!(r#"{{"type":"session","id":"{uuid_valid}","cwd":"/home/user/project"}}"#),
        )
        .unwrap();

        let old_val = std::env::var("PI_CODING_AGENT_DIR").ok();
        std::env::set_var("PI_CODING_AGENT_DIR", tmp.path());

        let result = capture_pi_session_id("/home/user/project", &HashSet::new());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), uuid_valid);

        match old_val {
            Some(v) => std::env::set_var("PI_CODING_AGENT_DIR", v),
            None => std::env::remove_var("PI_CODING_AGENT_DIR"),
        }
    }

    #[test]
    #[serial]
    fn test_capture_pi_session_id_respects_env_var() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions_dir = tmp.path().join("sessions");
        let project_encoded = encode_pi_project_path("/home/user/project");
        let project_dir = sessions_dir.join(&project_encoded);
        std::fs::create_dir_all(&project_dir).unwrap();

        let uuid = "cccccccc-cccc-cccc-cccc-cccccccccccc";
        std::fs::write(
            project_dir.join(format!("2024-12-03T14-00-00-000Z_{uuid}.jsonl")),
            format!(r#"{{"type":"session","id":"{uuid}","cwd":"/home/user/project"}}"#),
        )
        .unwrap();

        let old_val = std::env::var("PI_CODING_AGENT_DIR").ok();
        std::env::set_var("PI_CODING_AGENT_DIR", tmp.path());

        let result = capture_pi_session_id("/home/user/project", &HashSet::new());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), uuid);

        std::env::set_var("PI_CODING_AGENT_DIR", "/nonexistent-aoe-test-dir-xyz");
        let result_without_env = capture_pi_session_id("/home/user/project", &HashSet::new());
        assert!(result_without_env.is_err());

        match old_val {
            Some(v) => std::env::set_var("PI_CODING_AGENT_DIR", v),
            None => std::env::remove_var("PI_CODING_AGENT_DIR"),
        }
    }

    #[test]
    #[serial]
    fn test_capture_pi_session_id_cwd_fallback_most_recent_wins() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions_dir = tmp.path().join("sessions");

        let uuid_old = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
        let uuid_new = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";

        // Two sessions in DIFFERENT wrongly-encoded subdirs, both with CWD
        // matching our project. The fallback must pick the most recent.
        let dir_a = sessions_dir.join("--wrong-name-a--");
        std::fs::create_dir_all(&dir_a).unwrap();
        let old_file_a = dir_a.join(format!("2024-12-01T10-00-00-000Z_{uuid_old}.jsonl"));
        std::fs::write(
            &old_file_a,
            format!(r#"{{"type":"session","id":"{uuid_old}","cwd":"/home/user/project"}}"#),
        )
        .unwrap();
        let old_time = std::time::SystemTime::now() - Duration::from_secs(600);
        std::fs::File::options()
            .write(true)
            .open(&old_file_a)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(old_time))
            .unwrap();

        let dir_b = sessions_dir.join("--wrong-name-b--");
        std::fs::create_dir_all(&dir_b).unwrap();
        std::fs::write(
            dir_b.join(format!("2024-12-03T14-00-00-000Z_{uuid_new}.jsonl")),
            format!(r#"{{"type":"session","id":"{uuid_new}","cwd":"/home/user/project"}}"#),
        )
        .unwrap();

        let old_val = std::env::var("PI_CODING_AGENT_DIR").ok();
        std::env::set_var("PI_CODING_AGENT_DIR", tmp.path());

        let result = capture_pi_session_id("/home/user/project", &HashSet::new());
        assert!(
            result.is_ok(),
            "Fallback should find sessions via CWD header"
        );
        assert_eq!(result.unwrap(), uuid_new);

        match old_val {
            Some(v) => std::env::set_var("PI_CODING_AGENT_DIR", v),
            None => std::env::remove_var("PI_CODING_AGENT_DIR"),
        }
    }

    #[test]
    #[serial]
    fn test_capture_pi_session_id_cwd_fallback_succeeds() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions_dir = tmp.path().join("sessions");

        let wrong_encoded = "--some-other-name--";
        let wrong_dir = sessions_dir.join(wrong_encoded);
        std::fs::create_dir_all(&wrong_dir).unwrap();

        let uuid = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
        std::fs::write(
            wrong_dir.join(format!("2024-12-03T14-00-00-000Z_{uuid}.jsonl")),
            format!(r#"{{"type":"session","id":"{uuid}","cwd":"/home/user/project"}}"#),
        )
        .unwrap();

        let old_val = std::env::var("PI_CODING_AGENT_DIR").ok();
        std::env::set_var("PI_CODING_AGENT_DIR", tmp.path());

        let result = capture_pi_session_id("/home/user/project", &HashSet::new());
        assert!(result.is_ok(), "Fallback CWD scan should find the session");
        assert_eq!(result.unwrap(), uuid);

        match old_val {
            Some(v) => std::env::set_var("PI_CODING_AGENT_DIR", v),
            None => std::env::remove_var("PI_CODING_AGENT_DIR"),
        }
    }

    #[test]
    #[serial]
    fn test_capture_pi_session_id_fallback_by_dir_mtime() {
        let tmp = tempfile::tempdir().unwrap();
        let sessions_dir = tmp.path().join("sessions");

        let uuid_old = "aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa";
        let uuid_new = "bbbbbbbb-bbbb-bbbb-bbbb-bbbbbbbbbbbb";

        let dir_old = sessions_dir.join("--old-dir--");
        std::fs::create_dir_all(&dir_old).unwrap();
        std::fs::write(
            dir_old.join(format!("2024-12-01T10-00-00-000Z_{uuid_old}.jsonl")),
            "not valid json\n",
        )
        .unwrap();

        let dir_new = sessions_dir.join("--new-dir--");
        std::fs::create_dir_all(&dir_new).unwrap();
        std::fs::write(
            dir_new.join(format!("2024-12-03T14-00-00-000Z_{uuid_new}.jsonl")),
            "also not valid json\n",
        )
        .unwrap();

        // Set old dir file mtime to the past
        let old_time = std::time::SystemTime::now() - Duration::from_secs(600);
        std::fs::File::options()
            .write(true)
            .open(dir_old.join(format!("2024-12-01T10-00-00-000Z_{uuid_old}.jsonl")))
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(old_time))
            .unwrap();

        let old_val = std::env::var("PI_CODING_AGENT_DIR").ok();
        std::env::set_var("PI_CODING_AGENT_DIR", tmp.path());

        let result = capture_pi_session_id("/nonexistent/path/for/test", &HashSet::new());
        assert!(
            result.is_ok(),
            "Dir-mtime fallback should find session: {:?}",
            result
        );
        assert_eq!(result.unwrap(), uuid_new);

        match old_val {
            Some(v) => std::env::set_var("PI_CODING_AGENT_DIR", v),
            None => std::env::remove_var("PI_CODING_AGENT_DIR"),
        }
    }

    #[test]
    fn test_capture_claude_session_in_container_returns_error_for_missing_container() {
        let result = capture_claude_session_id_in_container(
            "aoe-test-nonexistent-container-xyz",
            "/workspace/test",
        );
        assert!(result.is_err());
    }

    #[test]
    #[serial]
    fn test_gemini_exclusion_uses_json_id_not_stem() {
        use sha2::{Digest, Sha256};

        let tmp = tempfile::tempdir().unwrap();
        let project_path = "/tmp/gemini-exclusion-test";
        let digest = Sha256::digest(project_path.as_bytes());
        let hash = digest
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();

        let chats_dir = tmp.path().join("tmp").join(&hash).join("chats");
        std::fs::create_dir_all(&chats_dir).unwrap();

        let file1 = chats_dir.join("session-1.json");
        std::fs::write(
            &file1,
            format!(r#"{{"sessionId": "json-id-AAA", "projectHash": "{hash}"}}"#),
        )
        .unwrap();

        let file2 = chats_dir.join("session-2.json");
        std::fs::write(
            &file2,
            format!(r#"{{"sessionId": "json-id-BBB", "projectHash": "{hash}"}}"#),
        )
        .unwrap();
        let older = std::time::SystemTime::now() - Duration::from_secs(10);
        std::fs::File::options()
            .write(true)
            .open(&file2)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(older))
            .unwrap();

        let old_val = std::env::var("GEMINI_CLI_HOME").ok();
        std::env::set_var("GEMINI_CLI_HOME", tmp.path());

        let mut exclusion = HashSet::new();
        exclusion.insert("json-id-AAA".to_string());

        let result = capture_gemini_session_id(project_path, &exclusion);
        assert_eq!(
            result.unwrap(),
            "json-id-BBB",
            "Exclusion must use JSON sessionId, not filename stem"
        );

        let mut wrong_exclusion = HashSet::new();
        wrong_exclusion.insert("session-1".to_string());

        let result2 = capture_gemini_session_id(project_path, &wrong_exclusion);
        assert_eq!(
            result2.unwrap(),
            "json-id-AAA",
            "Filename stem in exclusion should have no effect"
        );

        match old_val {
            Some(v) => std::env::set_var("GEMINI_CLI_HOME", v),
            None => std::env::remove_var("GEMINI_CLI_HOME"),
        }
    }

    #[test]
    #[serial]
    fn test_gemini_capture_returns_most_recent_by_cwd() {
        use sha2::{Digest, Sha256};

        let tmp = tempfile::tempdir().unwrap();
        let project_path = "/tmp/gemini-test-project";
        let digest = Sha256::digest(project_path.as_bytes());
        let hash = digest
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();

        let chats_dir = tmp.path().join("tmp").join(&hash).join("chats");
        std::fs::create_dir_all(&chats_dir).unwrap();

        let old_file = chats_dir.join("session-1.json");
        std::fs::write(
            &old_file,
            format!(r#"{{"sessionId": "old-id-111", "projectHash": "{hash}"}}"#),
        )
        .unwrap();
        let ten_min_ago = std::time::SystemTime::now() - Duration::from_secs(600);
        std::fs::File::options()
            .write(true)
            .open(&old_file)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(ten_min_ago))
            .unwrap();

        let new_file = chats_dir.join("session-2.json");
        std::fs::write(
            &new_file,
            format!(r#"{{"sessionId": "new-id-222", "projectHash": "{hash}"}}"#),
        )
        .unwrap();

        let old_val = std::env::var("GEMINI_CLI_HOME").ok();
        std::env::set_var("GEMINI_CLI_HOME", tmp.path());

        let result = capture_gemini_session_id(project_path, &HashSet::new());
        assert_eq!(result.unwrap(), "new-id-222");

        match old_val {
            Some(v) => std::env::set_var("GEMINI_CLI_HOME", v),
            None => std::env::remove_var("GEMINI_CLI_HOME"),
        }
    }

    #[test]
    #[serial]
    fn test_vibe_capture_matches_by_cwd() {
        let tmp = tempfile::tempdir().unwrap();
        let project_dir = tmp.path().join("myproject");
        std::fs::create_dir_all(&project_dir).unwrap();

        let sessions_dir = tmp.path().join("logs").join("session");

        let s1_dir = sessions_dir.join("session-abc");
        std::fs::create_dir_all(&s1_dir).unwrap();
        let s1_meta = serde_json::json!({
            "session_id": "vibe-sess-match",
            "environment": {"working_directory": project_dir.to_str().unwrap()}
        });
        std::fs::write(s1_dir.join("meta.json"), s1_meta.to_string()).unwrap();

        let s2_dir = sessions_dir.join("session-def");
        std::fs::create_dir_all(&s2_dir).unwrap();
        let s2_meta = serde_json::json!({
            "session_id": "vibe-sess-other",
            "environment": {"working_directory": "/somewhere/else"}
        });
        std::fs::write(s2_dir.join("meta.json"), s2_meta.to_string()).unwrap();

        let old_val = std::env::var("VIBE_HOME").ok();
        std::env::set_var("VIBE_HOME", tmp.path());

        let result = capture_vibe_session_id(project_dir.to_str().unwrap(), &HashSet::new());
        assert_eq!(result.unwrap(), "vibe-sess-match");

        match old_val {
            Some(v) => std::env::set_var("VIBE_HOME", v),
            None => std::env::remove_var("VIBE_HOME"),
        }
    }

    #[test]
    #[serial]
    fn test_vibe_stale_session_rejected() {
        let tmp = tempfile::tempdir().unwrap();
        let project_dir = tmp.path().join("myproject");
        std::fs::create_dir_all(&project_dir).unwrap();

        let sessions_dir = tmp.path().join("logs").join("session");
        let s1_dir = sessions_dir.join("session-stale");
        std::fs::create_dir_all(&s1_dir).unwrap();

        let s1_meta = serde_json::json!({
            "session_id": "vibe-sess-stale",
            "environment": {"working_directory": "/nonexistent/path/that/wont/match"}
        });
        std::fs::write(s1_dir.join("meta.json"), s1_meta.to_string()).unwrap();

        let old_val = std::env::var("VIBE_HOME").ok();
        std::env::set_var("VIBE_HOME", tmp.path());

        let result = capture_vibe_session_id(project_dir.to_str().unwrap(), &HashSet::new());
        assert!(
            result.is_err(),
            "Session with non-matching CWD should not be returned"
        );

        match old_val {
            Some(v) => std::env::set_var("VIBE_HOME", v),
            None => std::env::remove_var("VIBE_HOME"),
        }
    }

    #[test]
    fn test_extract_codex_cwd_from_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("rollout.jsonl");
        std::fs::write(
            &path,
            r#"{"type":"session_meta","payload":{"cwd":"/home/user/myproject"}}"#,
        )
        .unwrap();
        assert_eq!(
            extract_codex_cwd_from_file(&path),
            Some("/home/user/myproject".to_string())
        );
    }

    #[test]
    fn test_extract_codex_cwd_from_file_missing_field() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("rollout.jsonl");
        std::fs::write(&path, r#"{"type":"session_meta","payload":{}}"#).unwrap();
        assert_eq!(extract_codex_cwd_from_file(&path), None);
    }

    #[test]
    fn test_extract_codex_cwd_from_file_invalid_json() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("rollout.jsonl");
        std::fs::write(&path, "not json at all").unwrap();
        assert_eq!(extract_codex_cwd_from_file(&path), None);
    }
}
