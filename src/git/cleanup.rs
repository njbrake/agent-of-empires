//! Shared worktree cleanup utilities used by both CLI and TUI deletion paths.

use std::path::Path;

use crate::containers::DockerContainer;
use crate::session::Instance;

use super::open_repo_at;
use super::GitWorktree;

/// Cap on the number of dirty file entries we list inline in error messages so
/// the TUI output pane does not get blown out on a worktree with thousands of
/// changes (e.g., `target/` accidentally tracked).
const MAX_DIRTY_FILES_LISTED: usize = 30;

/// Cap on how many empty parent directories `prune_empty_parent_dirs` will
/// climb after a worktree removal. Shallow templates need 0-1 hops; deeper
/// nested templates like `../{repo-name}-worktrees/{branch}/{repo-name}` need
/// 2. Higher than that suggests a pathological template and we'd rather stop
/// than walk too far up the user's filesystem.
const MAX_PARENT_PRUNE_HOPS: usize = 4;

/// Walk up from a removed worktree path, deleting empty wrapper directories
/// that `git worktree add` created as a side effect of a nested path template.
///
/// Empty-only by design: uses `remove_dir`, never `remove_dir_all`. Anything
/// non-empty (e.g., a sibling repo cloned by an `on_create` hook) keeps the
/// wrapper alive so the user can decide what to do with the orphan.
///
/// Stops on:
/// - First non-empty / inaccessible parent
/// - Any directory that is `main_repo` itself or an ancestor of it
/// - The user's home directory or any of its ancestors
/// - Filesystem root
/// - `MAX_PARENT_PRUNE_HOPS` levels climbed
///
/// Best-effort: failures are logged and swallowed. The caller's worktree
/// removal already succeeded; an orphaned wrapper is a cosmetic leak, not a
/// reason to fail the deletion.
fn prune_empty_parent_dirs(worktree_path: &Path, main_repo: &Path) {
    let main_canonical = main_repo
        .canonicalize()
        .unwrap_or_else(|_| main_repo.to_path_buf());
    let home = dirs::home_dir();

    let mut current = worktree_path.parent().map(|p| p.to_path_buf());
    let mut hops = 0;

    while let Some(parent) = current {
        if hops >= MAX_PARENT_PRUNE_HOPS {
            break;
        }

        // Filesystem root has no parent; never try to remove it.
        if parent.parent().is_none() {
            break;
        }

        let parent_canonical = parent
            .canonicalize()
            .unwrap_or_else(|_| parent.to_path_buf());

        // Refuse to touch the main repo or any of its ancestors.
        if main_canonical.starts_with(&parent_canonical) {
            break;
        }

        // Refuse to touch the user's home dir or any of its ancestors.
        if let Some(h) = &home {
            if h.starts_with(&parent_canonical) {
                break;
            }
        }

        match std::fs::remove_dir(&parent) {
            Ok(()) => {
                tracing::debug!(
                    path = %parent.display(),
                    "removed empty worktree wrapper dir"
                );
                current = parent.parent().map(|p| p.to_path_buf());
                hops += 1;
            }
            Err(e) => {
                tracing::debug!(
                    path = %parent.display(),
                    error = %e,
                    "stopped pruning at non-empty or inaccessible parent"
                );
                break;
            }
        }
    }
}

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

/// Returns true if a `git worktree remove` stderr indicates the failure was
/// caused by modified or untracked files (i.e., re-running with `--force` would
/// resolve it). Matches the wording git itself uses: "contains modified or
/// untracked files, use --force to delete it".
pub fn is_dirty_worktree_error(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("modified or untracked files")
        || (lower.contains("--force") && lower.contains("contains"))
}

/// Enumerate modified, staged, and untracked files inside a worktree using
/// libgit2. Returns a vec of `"<status> <path>"` entries (e.g.
/// `"modified src/foo.rs"`, `"untracked debug.log"`).
///
/// Returns an empty vec if the path is not a git repo or the status walk fails;
/// the caller treats this as "no list available" and falls back to the bare
/// stderr.
pub fn list_dirty_files(worktree_path: &Path) -> Vec<String> {
    let Ok(repo) = open_repo_at(worktree_path) else {
        return Vec::new();
    };

    let mut opts = git2::StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(false);

    let Ok(statuses) = repo.statuses(Some(&mut opts)) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for entry in statuses.iter() {
        let path = entry.path().unwrap_or("<unreadable path>").to_string();
        let label = describe_status(entry.status());
        out.push(format!("{} {}", label, path));
    }
    out
}

fn describe_status(status: git2::Status) -> &'static str {
    if status.contains(git2::Status::CONFLICTED) {
        "conflicted"
    } else if status.intersects(git2::Status::WT_NEW) {
        "untracked"
    } else if status.intersects(git2::Status::INDEX_NEW) {
        "added   "
    } else if status.intersects(git2::Status::WT_DELETED | git2::Status::INDEX_DELETED) {
        "deleted "
    } else if status.intersects(git2::Status::WT_RENAMED | git2::Status::INDEX_RENAMED) {
        "renamed "
    } else if status.intersects(git2::Status::WT_TYPECHANGE | git2::Status::INDEX_TYPECHANGE) {
        "typechg "
    } else if status.intersects(git2::Status::WT_MODIFIED | git2::Status::INDEX_MODIFIED) {
        "modified"
    } else {
        "changed "
    }
}

/// Build an enriched error message for a failed worktree removal. When the
/// failure is caused by uncommitted/untracked files, list the offending paths
/// (capped at `MAX_DIRTY_FILES_LISTED`) so the user can decide whether
/// re-running with "force delete" is safe.
pub fn enrich_worktree_remove_error(stderr: &str, worktree_path: &Path) -> String {
    if !is_dirty_worktree_error(stderr) {
        return stderr.to_string();
    }

    let dirty = list_dirty_files(worktree_path);
    if dirty.is_empty() {
        return stderr.to_string();
    }

    let total = dirty.len();
    let mut out = String::with_capacity(stderr.len() + 64 + total * 32);
    out.push_str(stderr);
    out.push('\n');
    out.push('\n');
    out.push_str(&format!(
        "Uncommitted changes ({}; force delete will discard these):",
        total
    ));
    for entry in dirty.iter().take(MAX_DIRTY_FILES_LISTED) {
        out.push('\n');
        out.push_str("  ");
        out.push_str(entry);
    }
    if total > MAX_DIRTY_FILES_LISTED {
        out.push('\n');
        out.push_str(&format!(
            "  ... and {} more",
            total - MAX_DIRTY_FILES_LISTED
        ));
    }
    out
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

    let mut worktree_removed = false;

    if !has_dot_git {
        // .git is missing (manual deletion or other issue).
        // Remove the dir ourselves and prune stale references.
        match remove_worktree_dir(worktree_path, main_repo, force) {
            Ok(()) => {
                worktree_removed = true;
            }
            Err(e) => {
                tracing::debug!(error = %e, kind = ?e.kind(), "remove_worktree_dir failed (no .git)");
                if is_permission_error(&e.to_string())
                    && try_sandbox_dir_cleanup(worktree_path, main_repo, instance)
                {
                    worktree_removed = true;
                } else {
                    errors.push(format!("Worktree: {}", e));
                }
            }
        }
        if let Err(e) = git_wt.prune_worktrees() {
            errors.push(format!("Worktree: {}", e));
        }
    } else {
        match git_wt.remove_worktree(worktree_path, force) {
            Ok(()) => {
                worktree_removed = true;
            }
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
                    worktree_removed = true;
                    if let Err(e2) = git_wt.prune_worktrees() {
                        errors.push(format!("Worktree: {}", e2));
                    }
                } else {
                    errors.push(format!(
                        "Worktree: {}",
                        enrich_worktree_remove_error(&err_str, worktree_path)
                    ));
                }
            }
        }
    }

    // Clean up empty wrapper directories created by nested path templates
    // (e.g., `../{repo-name}-worktrees/{branch}/{repo-name}` leaves an empty
    // `{branch}/` behind once the leaf is gone). Best-effort, never fails
    // deletion.
    if worktree_removed {
        prune_empty_parent_dirs(worktree_path, main_repo);
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

    let container = DockerContainer::from_session_id(&instance.id);
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

    #[test]
    fn test_is_dirty_worktree_error_matches_git_message() {
        assert!(is_dirty_worktree_error(
            "fatal: '/tmp/wt' contains modified or untracked files, use --force to delete it"
        ));
        assert!(!is_dirty_worktree_error("permission denied"));
        assert!(!is_dirty_worktree_error("file not found"));
    }

    fn init_repo_with_commit() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let tree_id = {
            let mut index = repo.index().unwrap();
            index.write_tree().unwrap()
        };
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
            .unwrap();
        let path = dir.path().to_path_buf();
        (dir, path)
    }

    #[test]
    fn test_list_dirty_files_returns_untracked_and_modified() {
        let (_dir, repo_path) = init_repo_with_commit();

        // Untracked file
        std::fs::write(repo_path.join("new.txt"), "hello").unwrap();

        // Tracked + modified file: commit it first, then modify.
        std::fs::write(repo_path.join("tracked.txt"), "v1").unwrap();
        let repo = git2::Repository::open(&repo_path).unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(std::path::Path::new("tracked.txt")).unwrap();
        index.write().unwrap();
        let tree_id = index.write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let parent = repo.head().unwrap().peel_to_commit().unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "add tracked", &tree, &[&parent])
            .unwrap();
        std::fs::write(repo_path.join("tracked.txt"), "v2-modified").unwrap();

        let dirty = list_dirty_files(&repo_path);
        assert!(
            dirty.iter().any(|s| s.contains("new.txt")),
            "expected untracked new.txt in {:?}",
            dirty
        );
        assert!(
            dirty.iter().any(|s| s.contains("tracked.txt")),
            "expected modified tracked.txt in {:?}",
            dirty
        );
        assert!(dirty.iter().any(|s| s.starts_with("untracked ")));
        assert!(dirty.iter().any(|s| s.starts_with("modified ")));
    }

    #[test]
    fn test_list_dirty_files_returns_empty_for_non_repo() {
        let dir = tempfile::TempDir::new().unwrap();
        assert!(list_dirty_files(dir.path()).is_empty());
    }

    #[test]
    fn test_enrich_worktree_remove_error_appends_file_list() {
        let (_dir, repo_path) = init_repo_with_commit();
        std::fs::write(repo_path.join("scratch.log"), "data").unwrap();

        let stderr =
            "fatal: '/some/path' contains modified or untracked files, use --force to delete it";
        let enriched = enrich_worktree_remove_error(stderr, &repo_path);

        assert!(enriched.contains(stderr));
        assert!(enriched.contains("Uncommitted changes"));
        assert!(enriched.contains("scratch.log"));
    }

    #[test]
    fn test_enrich_worktree_remove_error_passes_through_unrelated_errors() {
        let (_dir, repo_path) = init_repo_with_commit();
        std::fs::write(repo_path.join("scratch.log"), "data").unwrap();

        let stderr = "fatal: permission denied";
        let enriched = enrich_worktree_remove_error(stderr, &repo_path);
        assert_eq!(enriched, stderr);
    }

    #[test]
    fn test_enrich_worktree_remove_error_caps_long_lists() {
        let (_dir, repo_path) = init_repo_with_commit();
        for i in 0..(MAX_DIRTY_FILES_LISTED + 5) {
            std::fs::write(repo_path.join(format!("f{}.txt", i)), "x").unwrap();
        }
        let stderr =
            "fatal: '/some/path' contains modified or untracked files, use --force to delete it";
        let enriched = enrich_worktree_remove_error(stderr, &repo_path);
        assert!(enriched.contains("and 5 more"));
    }

    /// Mirrors the user's nested template `../{repo-name}-worktrees/{branch}/{repo-name}`
    /// where the worktree leaf is two levels below a `<repo>-worktrees` base.
    /// After removing the leaf, both intermediate dirs should also be cleaned.
    #[test]
    fn test_prune_empty_parent_dirs_climbs_through_nested_template() {
        let dir = tempfile::TempDir::new().unwrap();
        let main_repo = dir.path().join("clawbolt-premium");
        let base = dir.path().join("clawbolt-premium-worktrees");
        let branch_dir = base.join("feature-foo");
        let worktree = branch_dir.join("clawbolt-premium");
        std::fs::create_dir_all(&main_repo).unwrap();
        std::fs::create_dir_all(&worktree).unwrap();

        // Simulate the leaf having just been removed by `git worktree remove`.
        std::fs::remove_dir(&worktree).unwrap();
        assert!(branch_dir.exists());

        prune_empty_parent_dirs(&worktree, &main_repo);

        assert!(!branch_dir.exists(), "branch wrapper dir should be gone");
        assert!(!base.exists(), "worktrees base dir should be gone");
        assert!(main_repo.exists(), "main repo must be untouched");
    }

    /// `on_create` hooks sometimes drop a sibling repo next to the worktree
    /// (e.g. an OSS pin clone). After deleting the worktree, that sibling
    /// keeps the wrapper non-empty and we MUST leave it alone.
    #[test]
    fn test_prune_empty_parent_dirs_preserves_non_empty_wrapper() {
        let dir = tempfile::TempDir::new().unwrap();
        let main_repo = dir.path().join("clawbolt-premium");
        let base = dir.path().join("clawbolt-premium-worktrees");
        let branch_dir = base.join("feature-foo");
        let worktree = branch_dir.join("clawbolt-premium");
        let sibling = branch_dir.join("clawbolt"); // orphan from on_create hook
        std::fs::create_dir_all(&main_repo).unwrap();
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        std::fs::write(sibling.join("README.md"), "oss pin").unwrap();

        std::fs::remove_dir_all(&worktree).unwrap();

        prune_empty_parent_dirs(&worktree, &main_repo);

        assert!(
            branch_dir.exists(),
            "wrapper must survive non-empty sibling"
        );
        assert!(sibling.exists(), "sibling repo must not be touched");
    }

    /// Default template `../{repo-name}-worktrees/{branch}` keeps the
    /// `<repo>-worktrees` base shared across multiple sessions. If another
    /// branch's worktree is still there, we must stop at the base.
    #[test]
    fn test_prune_empty_parent_dirs_stops_at_shared_base() {
        let dir = tempfile::TempDir::new().unwrap();
        let main_repo = dir.path().join("clawbolt-premium");
        let base = dir.path().join("clawbolt-premium-worktrees");
        let deleted_wt = base.join("feature-foo");
        let other_wt = base.join("feature-bar");
        std::fs::create_dir_all(&main_repo).unwrap();
        std::fs::create_dir_all(&deleted_wt).unwrap();
        std::fs::create_dir_all(&other_wt).unwrap();

        std::fs::remove_dir(&deleted_wt).unwrap();

        prune_empty_parent_dirs(&deleted_wt, &main_repo);

        assert!(base.exists(), "shared base must survive other worktrees");
        assert!(other_wt.exists(), "other worktree must be untouched");
    }

    /// Bare-repo template `./{branch}` puts the worktree inside the main repo.
    /// We must never remove the main repo or any of its ancestors.
    #[test]
    fn test_prune_empty_parent_dirs_refuses_to_climb_into_main_repo() {
        let dir = tempfile::TempDir::new().unwrap();
        let main_repo = dir.path().join("bare-repo");
        let worktree = main_repo.join("feature-foo");
        std::fs::create_dir_all(&worktree).unwrap();

        std::fs::remove_dir(&worktree).unwrap();

        prune_empty_parent_dirs(&worktree, &main_repo);

        assert!(main_repo.exists(), "main repo must be untouched");
    }

    /// If the wrapper isn't actually empty for any reason (race, leftover
    /// metadata file, FS quirk), `remove_dir` returns ENOTEMPTY and we stop.
    /// Don't ever fall through to recursive deletion.
    #[test]
    fn test_prune_empty_parent_dirs_never_recurses() {
        let dir = tempfile::TempDir::new().unwrap();
        let main_repo = dir.path().join("repo");
        let wrapper = dir.path().join("wrapper");
        let worktree = wrapper.join("wt");
        let stray = wrapper.join("DS_Store_or_similar");
        std::fs::create_dir_all(&main_repo).unwrap();
        std::fs::create_dir_all(&worktree).unwrap();
        std::fs::write(&stray, "junk").unwrap();

        std::fs::remove_dir(&worktree).unwrap();

        prune_empty_parent_dirs(&worktree, &main_repo);

        assert!(wrapper.exists(), "wrapper with stray file must survive");
        assert!(stray.exists(), "stray file must not be touched");
    }
}
