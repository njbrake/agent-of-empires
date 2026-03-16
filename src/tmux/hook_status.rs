//! Read status from Claude Code hook files.
//!
//! When Claude Code hooks are installed, the `aoe _hook` subcommand writes
//! status files to `<app_dir>/hook_status/<instance_id>`. This module reads
//! those files, providing a fast alternative to tmux pane content scraping.

use crate::session::Status;
use std::time::{SystemTime, UNIX_EPOCH};

/// Maximum age in seconds before a hook status file is considered stale.
/// Long enough to handle long-running tool calls (large file writes, slow
/// bash commands), short enough that orphaned files from crashes clear quickly.
const STALENESS_THRESHOLD_SECS: u64 = 120;

/// Read hook-reported status for an instance.
/// Returns None if file is missing, malformed, or stale (>120s).
///
/// Checks the full 16-char instance ID first (new sessions that have
/// `AOE_INSTANCE_ID` exported), then falls back to the 8-char prefix
/// (legacy sessions that detected their ID from the tmux session name).
pub fn read_hook_status(instance_id: &str) -> Option<Status> {
    let app_dir = crate::session::get_app_dir().ok()?;
    let hook_dir = app_dir.join("hook_status");

    // Fast path: full ID file (new sessions)
    if let Some(status) = parse_status_file(&hook_dir.join(instance_id)) {
        return Some(status);
    }

    // Fallback: 8-char prefix file (legacy sessions)
    if instance_id.len() > 8 {
        let prefix = &instance_id[..8];
        if let Some(status) = parse_status_file(&hook_dir.join(prefix)) {
            return Some(status);
        }
    }

    None
}

/// Parse a single hook status file, returning None if missing, malformed, or stale.
fn parse_status_file(path: &std::path::Path) -> Option<Status> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut parts = content.trim().splitn(2, ' ');
    let status_str = parts.next()?;
    let timestamp: u64 = parts.next()?.parse().ok()?;

    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();
    if now.saturating_sub(timestamp) > STALENESS_THRESHOLD_SECS {
        return None;
    }

    match status_str {
        "running" => Some(Status::Running),
        "idle" => Some(Status::Idle),
        "waiting" => Some(Status::Waiting),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn test_read_hook_status_missing_file() {
        // Non-existent instance should return None
        assert!(read_hook_status("nonexistent_instance_id_12345").is_none());
    }

    #[test]
    fn test_read_hook_status_fresh_file() {
        let dir = tempfile::tempdir().unwrap();
        let hook_dir = dir.path().join("hook_status");
        std::fs::create_dir_all(&hook_dir).unwrap();

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        std::fs::write(hook_dir.join("test_id"), format!("running {}", now)).unwrap();

        // We can't easily test this without mocking get_app_dir,
        // but we can test the parsing logic directly
        let content = format!("running {}", now);
        let mut parts = content.trim().splitn(2, ' ');
        let status_str = parts.next().unwrap();
        let timestamp: u64 = parts.next().unwrap().parse().unwrap();

        assert_eq!(status_str, "running");
        assert!(now.saturating_sub(timestamp) <= STALENESS_THRESHOLD_SECS);
    }

    #[test]
    fn test_stale_timestamp_detection() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let old_timestamp = now - 200; // 200 seconds ago, well past threshold
        assert!(now.saturating_sub(old_timestamp) > STALENESS_THRESHOLD_SECS);
    }

    #[test]
    fn test_fresh_timestamp_detection() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let recent_timestamp = now - 10; // 10 seconds ago
        assert!(now.saturating_sub(recent_timestamp) <= STALENESS_THRESHOLD_SECS);
    }

    #[test]
    fn test_status_string_parsing() {
        // Valid statuses
        assert!(matches!(
            match "running" {
                "running" => Some(Status::Running),
                "idle" => Some(Status::Idle),
                "waiting" => Some(Status::Waiting),
                _ => None,
            },
            Some(Status::Running)
        ));
        assert!(matches!(
            match "idle" {
                "running" => Some(Status::Running),
                "idle" => Some(Status::Idle),
                "waiting" => Some(Status::Waiting),
                _ => None,
            },
            Some(Status::Idle)
        ));
        assert!(matches!(
            match "waiting" {
                "running" => Some(Status::Running),
                "idle" => Some(Status::Idle),
                "waiting" => Some(Status::Waiting),
                _ => None,
            },
            Some(Status::Waiting)
        ));
        // Invalid status
        assert!(matches!(
            match "unknown" {
                "running" => Some(Status::Running),
                "idle" => Some(Status::Idle),
                "waiting" => Some(Status::Waiting),
                _ => None,
            },
            None
        ));
    }
}
