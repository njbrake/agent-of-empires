//! Status file I/O for hooks-based agent status detection.
//!
//! Agent hooks write `running`, `waiting`, or `idle` to a well-known
//! file path so AoE can detect agent status without parsing tmux pane content.

use std::path::PathBuf;

use crate::session::Status;

use super::HOOK_STATUS_BASE;

/// Return the directory for a given instance's hook status file.
pub fn hook_status_dir(instance_id: &str) -> PathBuf {
    PathBuf::from(HOOK_STATUS_BASE).join(instance_id)
}

/// Read the hook-written status file for the given instance.
///
/// Returns `None` if the file doesn't exist. Callers are responsible for
/// detecting crashed/abandoned sessions via process liveness checks
/// (e.g. `is_pane_dead()`, `is_pane_running_shell()`).
pub fn read_hook_status(instance_id: &str) -> Option<Status> {
    let status_path = hook_status_dir(instance_id).join("status");

    let content = std::fs::read_to_string(&status_path).ok()?;
    match content.trim() {
        "running" => Some(Status::Running),
        "waiting" => Some(Status::Waiting),
        "idle" => Some(Status::Idle),
        other => {
            tracing::warn!("Unexpected hook status value: {:?}", other);
            None
        }
    }
}

/// Remove the hook status directory for a given instance (cleanup on stop/delete).
pub fn cleanup_hook_status_dir(instance_id: &str) {
    let dir = hook_status_dir(instance_id);
    if dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            tracing::warn!("Failed to cleanup hook status dir {}: {}", dir.display(), e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn setup_status_file(instance_id: &str, content: &str) -> PathBuf {
        let dir = hook_status_dir(instance_id);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("status");
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        dir
    }

    #[test]
    fn test_read_running_status() {
        let id = "test_read_running";
        let dir = setup_status_file(id, "running");
        assert_eq!(read_hook_status(id), Some(Status::Running));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_read_waiting_status() {
        let id = "test_read_waiting";
        let dir = setup_status_file(id, "waiting");
        assert_eq!(read_hook_status(id), Some(Status::Waiting));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_read_idle_status() {
        let id = "test_read_idle";
        let dir = setup_status_file(id, "idle");
        assert_eq!(read_hook_status(id), Some(Status::Idle));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_read_waiting_with_newline() {
        let id = "test_read_newline";
        let dir = setup_status_file(id, "waiting\n");
        assert_eq!(read_hook_status(id), Some(Status::Waiting));
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_read_missing_file() {
        assert_eq!(read_hook_status("nonexistent_instance_id"), None);
    }

    #[test]
    fn test_read_dangling_symlink() {
        let id = "test_dangling_symlink";
        let dir = hook_status_dir(id);
        fs::create_dir_all(&dir).unwrap();
        std::os::unix::fs::symlink("/nonexistent/target", dir.join("status")).unwrap();
        assert_eq!(read_hook_status(id), None);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_read_unexpected_content() {
        let id = "test_read_unexpected";
        let dir = setup_status_file(id, "something_else");
        assert_eq!(read_hook_status(id), None);
        fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_cleanup_existing_dir() {
        let id = "test_cleanup_existing";
        let dir = setup_status_file(id, "running");
        assert!(dir.exists());
        cleanup_hook_status_dir(id);
        assert!(!dir.exists());
    }

    #[test]
    fn test_cleanup_nonexistent_dir() {
        // Should not panic
        cleanup_hook_status_dir("nonexistent_cleanup_test");
    }

    #[test]
    fn test_hook_status_dir_path() {
        let dir = hook_status_dir("abc123");
        assert_eq!(dir, PathBuf::from("/tmp/aoe-hooks/abc123"));
    }
}
