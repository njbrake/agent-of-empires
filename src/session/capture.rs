//! Session ID capture logic for all supported agent types.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
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
/// Single checkpoint at the capture boundary so that invalid IDs never
/// propagate into storage.
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
/// UUID-named `.jsonl` file.
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

/// Polling closure for Claude Code session tracking on the host filesystem.
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
    if Uuid::parse_str(id).is_err() {
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
        .filter(|name| name.starts_with(crate::tmux::SESSION_PREFIX))
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

/// Capture Gemini session ID from `~/.gemini/tmp/<dir>/chats/session-*.json`.
///
/// `<dir>` is a SHA-256 hash of the project path. We compute it locally and look
/// for a matching directory, then scan all subdirs as a fallback verifying via the
/// `projectHash` JSON field.
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

    // BUG FIX: exclusion check uses the JSON `sessionId` field (the actual
    // session ID), NOT the filename stem. The filename stem (e.g. "session-42")
    // differs from the JSON id (e.g. "abc-123-def"), so comparing filename
    // stems against the exclusion set would silently fail to isolate instances.
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

/// Extract the project hash from a Gemini session file for CWD matching.
///
/// Gemini stores a SHA-256 hash of the project root in `projectHash` rather than
/// a literal path.
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

pub(crate) fn is_valid_session_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 256
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
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
    fn test_capture_claude_session_in_container_returns_error_for_missing_container() {
        let result = capture_claude_session_id_in_container(
            "aoe-test-nonexistent-container-xyz",
            "/workspace/test",
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_extract_gemini_session_id_from_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session-42.json");
        std::fs::write(
            &path,
            r#"{"sessionId": "abc-123", "projectHash": "deadbeef"}"#,
        )
        .unwrap();
        assert_eq!(
            extract_gemini_session_id_from_file(&path),
            Some("abc-123".to_string())
        );
    }

    #[test]
    fn test_extract_gemini_session_id_from_file_falls_back_to_stem() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("session-42.json");
        std::fs::write(&path, r#"{"projectHash": "deadbeef"}"#).unwrap();
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

        // session-1.json has JSON id "json-id-AAA" (differs from stem "session-1")
        let file1 = chats_dir.join("session-1.json");
        std::fs::write(
            &file1,
            format!(r#"{{"sessionId": "json-id-AAA", "projectHash": "{hash}"}}"#),
        )
        .unwrap();

        // session-2.json has JSON id "json-id-BBB"
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

        // Exclude by JSON id "json-id-AAA" (the most recent).
        // If exclusion incorrectly used filename stem "session-1", it would NOT
        // exclude this file (regression).
        let mut exclusion = HashSet::new();
        exclusion.insert("json-id-AAA".to_string());

        let result = capture_gemini_session_id(project_path, &exclusion);
        assert_eq!(
            result.unwrap(),
            "json-id-BBB",
            "Exclusion must use JSON sessionId, not filename stem"
        );

        // Also verify that excluding by filename stem does NOT work (it shouldn't)
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
    fn test_build_exclusion_set_empty() {
        let result = build_exclusion_set("nonexistent-instance-id-12345");
        assert!(!result.contains("nonexistent-instance-id-12345"));
    }
}
