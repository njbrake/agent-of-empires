//! Shared worktree cleanup utilities used by both CLI and TUI deletion paths.

use std::path::Path;

use crate::containers::DockerContainer;
use crate::session::Instance;

/// Remove a worktree directory from the filesystem.
///
/// Always tries `remove_dir` first (fast path for empty dirs). When `force`
/// is true, falls back to `remove_dir_all` for non-empty directories.
/// Refuses to delete the directory if it is the main repo itself.
///
/// On failure, retries a few times with short delays to handle macOS
/// Docker Desktop VirtioFS propagation delays after container removal.
pub fn remove_worktree_dir(
    worktree_path: &Path,
    main_repo: &Path,
    force: bool,
) -> std::io::Result<()> {
    let wt = worktree_path
        .canonicalize()
        .unwrap_or(worktree_path.to_path_buf());
    let mr = main_repo.canonicalize().unwrap_or(main_repo.to_path_buf());
    if wt == mr {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "worktree path is the same as the main repo -- refusing to delete",
        ));
    }

    for attempt in 0..5 {
        if !worktree_path.exists() {
            return Ok(());
        }
        let result = std::fs::remove_dir(worktree_path);
        if result.is_ok() {
            return Ok(());
        }
        if force {
            let result = std::fs::remove_dir_all(worktree_path);
            if result.is_ok() {
                return Ok(());
            }
        }
        if attempt < 4 {
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }

    // Final attempt -- return the error
    if !worktree_path.exists() {
        return Ok(());
    }
    let result = std::fs::remove_dir(worktree_path);
    if result.is_ok() || !force {
        return result;
    }
    std::fs::remove_dir_all(worktree_path)
}

/// Check if a git error message indicates a permission problem.
pub fn is_permission_error(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("permission denied")
        || lower.contains("operation not permitted")
        || lower.contains("access is denied")
}

/// Delete worktree contents from inside the sandbox container.
///
/// Starts the container if it exists but is stopped, then runs
/// `find . -mindepth 1 -delete` to remove all contents (including
/// root-owned files that the host user cannot delete directly).
///
/// Returns true if the container successfully deleted the contents.
pub fn cleanup_sandbox_worktree(instance: &Instance) -> bool {
    let container = DockerContainer::from_session_id(&instance.id);
    if !container.exists().unwrap_or(false) {
        return false;
    }
    if !container.is_running().unwrap_or(false) && container.start().is_err() {
        return false;
    }
    match container.exec(&["find", ".", "-mindepth", "1", "-delete"]) {
        Ok(output) => output.status.success(),
        Err(_) => false,
    }
}
