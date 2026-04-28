//! Session ID capture logic for all supported agent types.

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
}
