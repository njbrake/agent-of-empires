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

/// Try the first matching key from a JSON object as a string.
fn extract_cwd_from_json(parsed: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| parsed.get(*key).and_then(|v| v.as_str()))
        .map(String::from)
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
fn extract_vibe_meta(path: &Path) -> Option<(String, Option<String>)> {
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
fn extract_vibe_cwd_from_meta(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    parsed
        .get("environment")
        .and_then(|env| env.get("working_directory"))
        .and_then(|v| v.as_str())
        .map(String::from)
        .or_else(|| extract_cwd_from_json(&parsed, &["cwd", "working_directory", "project_path"]))
}

/// Extract the `session_id` from a Vibe `meta.json` file.
#[cfg(test)]
fn extract_vibe_session_id_from_meta(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&content).ok()?;
    parsed
        .get("session_id")
        .and_then(|v| v.as_str())
        .map(String::from)
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
        // No container with this name exists, so docker exec should fail and
        // we should get a clean error rather than a panic.
        let result = capture_claude_session_id_in_container(
            "aoe-test-nonexistent-container-xyz",
            "/workspace/test",
        );
        assert!(result.is_err());
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
    #[serial]
    fn test_vibe_capture_matches_by_cwd() {
        let tmp = tempfile::tempdir().unwrap();
        let project_dir = tmp.path().join("myproject");
        std::fs::create_dir_all(&project_dir).unwrap();

        let sessions_dir = tmp.path().join("logs").join("session");

        // Session 1: matches our project
        let s1_dir = sessions_dir.join("session-abc");
        std::fs::create_dir_all(&s1_dir).unwrap();
        let s1_meta = serde_json::json!({
            "session_id": "vibe-sess-match",
            "environment": {"working_directory": project_dir.to_str().unwrap()}
        });
        std::fs::write(s1_dir.join("meta.json"), s1_meta.to_string()).unwrap();

        // Session 2: different project
        let s2_dir = sessions_dir.join("session-def");
        std::fs::create_dir_all(&s2_dir).unwrap();
        let s2_meta = serde_json::json!({
            "session_id": "vibe-sess-other",
            "environment": {"working_directory": "/somewhere/else"}
        });
        std::fs::write(s2_dir.join("meta.json"), s2_meta.to_string()).unwrap();

        let old_val = std::env::var("VIBE_HOME").ok();
        std::env::set_var("VIBE_HOME", tmp.path());

        let exclusion = HashSet::new();
        let result = capture_vibe_session_id(project_dir.to_str().unwrap(), &exclusion);
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

        // CWD points to a directory that doesn't exist (so canonicalize won't match)
        let s1_meta = serde_json::json!({
            "session_id": "vibe-sess-stale",
            "environment": {"working_directory": "/nonexistent/path/that/wont/match"}
        });
        std::fs::write(s1_dir.join("meta.json"), s1_meta.to_string()).unwrap();

        let old_val = std::env::var("VIBE_HOME").ok();
        std::env::set_var("VIBE_HOME", tmp.path());

        let exclusion = HashSet::new();
        let result = capture_vibe_session_id(project_dir.to_str().unwrap(), &exclusion);
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
    fn test_build_exclusion_set_empty() {
        let result = build_exclusion_set("nonexistent-instance-id-12345");
        // The exclusion set should never contain our own instance ID
        // (it collects OTHER instances' captured session IDs).
        // On a machine with active AoE tmux sessions, the set may be
        // non-empty, so we verify our own ID isn't self-excluded.
        assert!(!result.contains("nonexistent-instance-id-12345"));
    }
}
