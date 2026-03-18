//! Shared worktree cleanup utilities used by both CLI and TUI deletion paths.

use std::path::Path;

use crate::containers::DockerContainer;
use crate::session::Instance;

use super::GitWorktree;

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
    let container = DockerContainer::for_instance(instance);
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

/// Perform full worktree cleanup with automatic sandbox fallback.
///
/// Handles both cases:
/// - `.git` file missing: removes directory and prunes stale references
/// - `.git` file present: uses `git worktree remove`, falls back to
///   container cleanup for sandboxed sessions with permission errors
///
/// Returns `Ok(())` if the worktree was successfully removed, or
/// `Err(errors)` with error messages on failure.
pub fn remove_managed_worktree(
    git_wt: &GitWorktree,
    worktree_path: &Path,
    main_repo: &Path,
    instance: &Instance,
    force: bool,
) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    let has_dot_git = worktree_path.join(".git").exists();

    tracing::debug!(
        path = %worktree_path.display(),
        has_dot_git,
        is_sandboxed = instance.is_sandboxed(),
        force,
        "worktree cleanup starting"
    );

    if !has_dot_git {
        // .git is missing (manual deletion or other issue).
        // Remove the dir ourselves and prune stale references.
        if let Err(e) = remove_worktree_dir(worktree_path, main_repo, force) {
            tracing::debug!(error = %e, kind = ?e.kind(), "remove_worktree_dir failed (no .git)");
            if !(is_permission_error(&e.to_string())
                && try_sandbox_dir_cleanup(worktree_path, main_repo, instance))
            {
                errors.push(format!("Worktree: {}", e));
            }
        }
        if let Err(e) = git_wt.prune_worktrees() {
            errors.push(format!("Worktree: {}", e));
        }
    } else {
        match git_wt.remove_worktree(worktree_path, force) {
            Ok(()) => {}
            Err(e) => {
                let err_str = e.to_string();
                tracing::debug!(
                    error = %err_str,
                    is_perm = is_permission_error(&err_str),
                    "git worktree remove failed"
                );
                // Container cleanup deletes everything including .git, so
                // git worktree remove won't work afterward. Fall back to
                // removing the directory and pruning stale references.
                if is_permission_error(&err_str)
                    && try_sandbox_dir_cleanup(worktree_path, main_repo, instance)
                {
                    if let Err(e2) = git_wt.prune_worktrees() {
                        errors.push(format!("Worktree: {}", e2));
                    }
                } else {
                    errors.push(format!("Worktree: {}", e));
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Try to clean up a worktree directory using the sandbox container.
///
/// When worktree files are root-owned (from container execution), the host
/// can't delete them directly. This function:
/// 1. Runs `find . -mindepth 1 -delete` inside the container
/// 2. Force-removes the container to release the bind mount
/// 3. Retries directory removal (with VirtioFS delay handling)
fn try_sandbox_dir_cleanup(worktree_path: &Path, main_repo: &Path, instance: &Instance) -> bool {
    if !instance.is_sandboxed() {
        return false;
    }

    let cleaned = cleanup_sandbox_worktree(instance);
    tracing::debug!(cleaned, "container cleanup attempted");
    if !cleaned {
        return false;
    }

    let container = DockerContainer::for_instance(instance);
    let rm_result = container.remove(true);
    tracing::debug!(?rm_result, "container force-removed");

    match remove_worktree_dir(worktree_path, main_repo, true) {
        Ok(()) => true,
        Err(e) => {
            tracing::debug!(error = %e, kind = ?e.kind(), "remove_worktree_dir failed after cleanup");
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remove_worktree_dir_refuses_same_as_main_repo() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path();
        let result = remove_worktree_dir(path, path, false);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("refusing to delete"));
        assert!(path.exists());
    }

    #[test]
    fn test_remove_worktree_dir_removes_empty_dir() {
        let dir = tempfile::TempDir::new().unwrap();
        let main = dir.path().join("main");
        let wt = dir.path().join("worktree");
        std::fs::create_dir(&main).unwrap();
        std::fs::create_dir(&wt).unwrap();
        let result = remove_worktree_dir(&wt, &main, false);
        assert!(result.is_ok());
        assert!(!wt.exists());
    }

    #[test]
    fn test_is_permission_error_matches() {
        assert!(is_permission_error("Permission denied (os error 13)"));
        assert!(is_permission_error("operation not permitted"));
        assert!(is_permission_error("Access is denied"));
        assert!(!is_permission_error("file not found"));
    }
}
