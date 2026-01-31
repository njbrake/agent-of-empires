// Git worktree operations module

use std::path::{Path, PathBuf};

pub mod diff;
pub mod error;
pub mod template;

use error::{GitError, Result};
use template::{resolve_template, TemplateVars};

pub struct WorktreeEntry {
    pub path: PathBuf,
    pub branch: Option<String>,
    pub is_detached: bool,
}

pub struct GitWorktree {
    pub repo_path: PathBuf,
}

impl GitWorktree {
    pub fn new(repo_path: PathBuf) -> Result<Self> {
        if !Self::is_git_repo(&repo_path) {
            return Err(GitError::NotAGitRepo);
        }
        Ok(Self { repo_path })
    }

    pub fn is_git_repo(path: &Path) -> bool {
        git2::Repository::discover(path).is_ok()
    }

    /// Returns true if the repository is a bare repo (including linked worktree bare repo setups).
    /// This is useful for choosing appropriate worktree path templates.
    pub fn is_bare_repo(path: &Path) -> bool {
        git2::Repository::discover(path)
            .map(|repo| repo.is_bare())
            .unwrap_or(false)
    }

    pub fn find_main_repo(path: &Path) -> Result<PathBuf> {
        let repo = git2::Repository::discover(path)?;

        // Check if this is a worktree of a bare repo setup.
        // For worktrees of bare repos, repo.path() returns something like:
        // /project/.bare/worktrees/main/
        // We want to return /project/ (the parent of .bare).
        if let Some(bare_repo_root) = Self::find_bare_repo_root_from_gitdir(repo.path()) {
            return Ok(bare_repo_root);
        }

        // For regular repos with a working directory, return it
        if let Some(workdir) = repo.workdir() {
            return Ok(workdir.to_path_buf());
        }

        // For bare repos (including linked worktree setups where .git file points to a bare repo),
        // return the parent of the git directory.
        // Example: /repo/.git file -> /repo/.bare/, repo.path() returns /repo/.bare/,
        // so we return /repo/ as the main repo path.
        repo.path()
            .parent()
            .map(|p| p.to_path_buf())
            .ok_or(GitError::NotAGitRepo)
    }

    /// If gitdir is inside a worktrees/ directory of a bare repo (e.g., /project/.bare/worktrees/main/),
    /// returns the parent of the bare repo (e.g., /project/).
    /// Returns None if not a bare repo worktree setup.
    fn find_bare_repo_root_from_gitdir(gitdir: &Path) -> Option<PathBuf> {
        // Look for a "worktrees" ancestor directory
        let mut current = gitdir;
        while let Some(parent) = current.parent() {
            if current
                .file_name()
                .map(|n| n == "worktrees")
                .unwrap_or(false)
            {
                // current is the "worktrees" directory
                // parent is the bare repo directory (e.g., .bare)
                // parent.parent() is the main repo root
                return parent.parent().map(|p| p.to_path_buf());
            }
            current = parent;
        }
        None
    }

    pub fn create_worktree(&self, branch: &str, path: &Path, create_branch: bool) -> Result<()> {
        if path.exists() {
            return Err(GitError::WorktreeAlreadyExists(path.to_path_buf()));
        }

        // Prune stale worktree entries so git doesn't reject a path that was
        // previously used by a now-deleted worktree directory.
        self.prune_worktrees()?;

        let repo = git2::Repository::discover(&self.repo_path)?;

        if create_branch {
            let head = repo.head()?;
            let commit = head.peel_to_commit()?;
            repo.branch(branch, &commit, false)?;
        } else {
            let has_local = repo.find_branch(branch, git2::BranchType::Local).is_ok();
            if !has_local {
                let has_remote = repo
                    .branches(Some(git2::BranchType::Remote))
                    .ok()
                    .map(|branches| {
                        branches.filter_map(|b| b.ok()).any(|(b, _)| {
                            b.name()
                                .ok()
                                .flatten()
                                .map(|name| {
                                    name.ends_with(&format!("/{}", branch)) || name == branch
                                })
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false);
                if !has_remote {
                    return Err(GitError::BranchNotFound(branch.to_string()));
                }
            }
        }

        let path_str = path
            .to_str()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid path"))?;

        let output = std::process::Command::new("git")
            .args(["worktree", "add", path_str, branch])
            .current_dir(&self.repo_path)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(GitError::WorktreeCommandFailed(stderr));
        }

        // Convert the .git file from absolute to relative path.
        // Git always writes absolute paths, but relative paths work better when
        // the repo is mounted at different locations (e.g., in Docker containers).
        Self::convert_git_file_to_relative(path)?;

        Ok(())
    }

    /// Prune stale worktree entries whose directories no longer exist on disk.
    fn prune_worktrees(&self) -> Result<()> {
        let output = std::process::Command::new("git")
            .args(["worktree", "prune"])
            .current_dir(&self.repo_path)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(GitError::WorktreeCommandFailed(stderr));
        }

        Ok(())
    }

    /// Convert a worktree's .git file from absolute to relative path.
    ///
    /// Git worktrees contain a `.git` file (not directory) with content like:
    /// `gitdir: /absolute/path/to/.bare/worktrees/name`
    ///
    /// This converts it to a relative path like:
    /// `gitdir: ../.bare/worktrees/name`
    ///
    /// Relative paths work when the repo is mounted at different locations.
    fn convert_git_file_to_relative(worktree_path: &Path) -> Result<()> {
        let git_file = worktree_path.join(".git");
        if !git_file.exists() || !git_file.is_file() {
            return Ok(()); // Not a worktree or already a directory
        }

        let content = std::fs::read_to_string(&git_file)?;
        let Some(gitdir_line) = content.lines().find(|l| l.starts_with("gitdir:")) else {
            return Ok(()); // No gitdir line found
        };

        let absolute_path = gitdir_line.trim_start_matches("gitdir:").trim();
        let absolute_path = Path::new(absolute_path);

        if absolute_path.is_relative() {
            return Ok(()); // Already relative
        }

        // Calculate relative path from worktree to gitdir
        let worktree_canonical = worktree_path.canonicalize()?;
        let gitdir_canonical = absolute_path.canonicalize()?;

        if let Some(relative) = Self::diff_paths(&gitdir_canonical, &worktree_canonical) {
            let new_content = format!("gitdir: {}\n", relative.display());
            std::fs::write(&git_file, new_content)?;
        }

        Ok(())
    }

    /// Calculate a relative path from `base` to `target`.
    /// Returns None if the paths have no common ancestor.
    fn diff_paths(target: &Path, base: &Path) -> Option<PathBuf> {
        let mut target_components = target.components().peekable();
        let mut base_components = base.components().peekable();

        // Skip common prefix
        while let (Some(t), Some(b)) = (target_components.peek(), base_components.peek()) {
            if t != b {
                break;
            }
            target_components.next();
            base_components.next();
        }

        // Count remaining base components (need ".." for each)
        let up_count = base_components.count();

        // Build relative path: "../" for each remaining base component + remaining target
        let mut result = PathBuf::new();
        for _ in 0..up_count {
            result.push("..");
        }
        for component in target_components {
            result.push(component);
        }

        Some(result)
    }

    pub fn list_worktrees(&self) -> Result<Vec<WorktreeEntry>> {
        let repo = git2::Repository::discover(&self.repo_path)?;
        let worktrees = repo.worktrees()?;

        let mut entries = vec![];

        // For non-bare repos, add the main worktree entry
        // Bare repos don't have a main worktree, only linked worktrees
        if !repo.is_bare() {
            entries.push(WorktreeEntry {
                path: self.repo_path.clone(),
                branch: Self::get_current_branch(&self.repo_path).ok(),
                is_detached: repo.head_detached()?,
            });
        }

        for name_str in worktrees.iter().flatten() {
            if let Ok(wt) = repo.find_worktree(name_str) {
                if let Ok(path) = wt.path().canonicalize() {
                    entries.push(WorktreeEntry {
                        path: path.clone(),
                        branch: Self::get_current_branch(&path).ok(),
                        is_detached: false,
                    });
                }
            }
        }

        Ok(entries)
    }

    pub fn remove_worktree(&self, path: &Path) -> Result<()> {
        if !path.exists() {
            return Err(GitError::WorktreeNotFound(path.to_path_buf()));
        }

        let path_str = path
            .to_str()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid path"))?;

        let output = std::process::Command::new("git")
            .args(["worktree", "remove", path_str])
            .current_dir(&self.repo_path)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(GitError::WorktreeCommandFailed(stderr));
        }

        Ok(())
    }

    /// Delete a local git branch.
    /// Returns an error if the branch doesn't exist or is currently checked out.
    pub fn delete_branch(&self, branch: &str) -> Result<()> {
        let output = std::process::Command::new("git")
            .args(["branch", "-d", branch])
            .current_dir(&self.repo_path)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // If the branch has unmerged changes, try force delete
            if stderr.contains("not fully merged") {
                let force_output = std::process::Command::new("git")
                    .args(["branch", "-D", branch])
                    .current_dir(&self.repo_path)
                    .output()?;

                if !force_output.status.success() {
                    return Err(GitError::BranchNotFound(branch.to_string()));
                }
            } else {
                return Err(GitError::BranchNotFound(branch.to_string()));
            }
        }

        Ok(())
    }

    pub fn compute_path(&self, branch: &str, template: &str, session_id: &str) -> Result<PathBuf> {
        let repo_name = self
            .repo_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("repo")
            .to_string();

        let vars = TemplateVars {
            repo_name,
            branch: branch.to_string(),
            session_id: session_id.to_string(),
            base_path: self.repo_path.clone(),
        };

        resolve_template(template, &vars)
    }

    pub fn get_current_branch(path: &Path) -> Result<String> {
        let repo = git2::Repository::discover(path)?;
        let head = repo.head()?;

        if let Some(branch_name) = head.shorthand() {
            Ok(branch_name.to_string())
        } else {
            Err(GitError::NotAGitRepo)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn setup_test_repo() -> (TempDir, git2::Repository) {
        let dir = TempDir::new().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();

        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let tree_id = {
            let mut index = repo.index().unwrap();
            index.write_tree().unwrap()
        };
        {
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
                .unwrap();
        }

        (dir, repo)
    }

    #[test]
    fn test_is_git_repo_returns_true_for_git_directory() {
        let (_dir, repo) = setup_test_repo();
        assert!(GitWorktree::is_git_repo(repo.path().parent().unwrap()));
    }

    #[test]
    fn test_is_git_repo_returns_false_for_non_git_directory() {
        let dir = TempDir::new().unwrap();
        assert!(!GitWorktree::is_git_repo(dir.path()));
    }

    #[test]
    fn test_find_main_repo_returns_repo_root() {
        let (_dir, repo) = setup_test_repo();
        let repo_path = repo.path().parent().unwrap();
        let result = GitWorktree::find_main_repo(repo_path).unwrap();
        assert_eq!(result, repo_path);
    }

    #[test]
    fn test_find_main_repo_fails_for_non_git_directory() {
        let dir = TempDir::new().unwrap();
        assert!(GitWorktree::find_main_repo(dir.path()).is_err());
    }

    #[test]
    fn test_create_worktree_creates_new_worktree() {
        let (dir, repo) = setup_test_repo();
        let repo_path = repo.path().parent().unwrap();

        let head = repo.head().unwrap();
        let commit = head.peel_to_commit().unwrap();
        repo.branch("test-branch", &commit, false).unwrap();

        let wt_path = dir.path().join("test-worktree");
        let git_wt = GitWorktree::new(repo_path.to_path_buf()).unwrap();
        git_wt
            .create_worktree("test-branch", &wt_path, false)
            .unwrap();

        assert!(wt_path.exists());
        assert!(wt_path.join(".git").exists());
    }

    #[test]
    fn test_create_worktree_with_new_branch() {
        let (dir, repo) = setup_test_repo();
        let repo_path = repo.path().parent().unwrap();

        let wt_path = dir.path().join("new-branch-worktree");
        let git_wt = GitWorktree::new(repo_path.to_path_buf()).unwrap();
        git_wt
            .create_worktree("new-branch", &wt_path, true)
            .unwrap();

        assert!(wt_path.exists());
        assert!(repo
            .find_branch("new-branch", git2::BranchType::Local)
            .is_ok());
    }

    #[test]
    fn test_list_worktrees_returns_main_and_additional() {
        let (dir, repo) = setup_test_repo();
        let repo_path = repo.path().parent().unwrap();

        let head = repo.head().unwrap();
        let commit = head.peel_to_commit().unwrap();
        repo.branch("feature", &commit, false).unwrap();

        let wt_path = dir.path().join("feature-worktree");
        let git_wt = GitWorktree::new(repo_path.to_path_buf()).unwrap();
        git_wt.create_worktree("feature", &wt_path, false).unwrap();

        let worktrees = git_wt.list_worktrees().unwrap();
        assert!(worktrees.len() >= 2);
    }

    #[test]
    fn test_remove_worktree_deletes_worktree() {
        let (_dir, repo) = setup_test_repo();
        let repo_path = repo.path().parent().unwrap();

        let head = repo.head().unwrap();
        let commit = head.peel_to_commit().unwrap();
        repo.branch("removable", &commit, false).unwrap();

        let wt_path = repo_path.parent().unwrap().join("removable-wt");
        let git_wt = GitWorktree::new(repo_path.to_path_buf()).unwrap();
        git_wt
            .create_worktree("removable", &wt_path, false)
            .unwrap();

        assert!(wt_path.exists());

        git_wt.remove_worktree(&wt_path).unwrap();
        assert!(!wt_path.exists());
    }

    #[test]
    fn test_compute_path_with_template() {
        let (_dir, repo) = setup_test_repo();
        let repo_path = repo.path().parent().unwrap();
        let git_wt = GitWorktree::new(repo_path.to_path_buf()).unwrap();

        let template = "../{repo-name}-worktrees/{branch}";
        let path = git_wt
            .compute_path("feat/test", template, "abc123")
            .unwrap();

        assert!(path.to_string_lossy().contains("feat-test"));
        assert!(path.to_string_lossy().contains("-worktrees"));
    }

    /// Sets up a linked worktree bare repo structure:
    /// /tmp/xxx/
    ///   .bare/           <- bare git repository
    ///   .git             <- file containing "gitdir: ./.bare"
    ///   main/            <- worktree for main branch
    fn setup_linked_worktree_bare_repo() -> TempDir {
        let dir = TempDir::new().unwrap();
        let bare_path = dir.path().join(".bare");

        // Create a bare repository
        let repo = git2::Repository::init_bare(&bare_path).unwrap();

        // Create initial commit so we have a valid HEAD
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .unwrap();

        // Create the .git file pointing to the bare repo
        let git_file_path = dir.path().join(".git");
        std::fs::write(&git_file_path, "gitdir: ./.bare\n").unwrap();

        // Create a worktree using git command (git2 worktree API is limited for bare repos)
        let main_wt_path = dir.path().join("main");
        std::process::Command::new("git")
            .args(["worktree", "add", main_wt_path.to_str().unwrap(), "HEAD"])
            .current_dir(&bare_path)
            .output()
            .unwrap();

        dir
    }

    #[test]
    fn test_is_git_repo_recognizes_linked_worktree_bare_repo() {
        let dir = setup_linked_worktree_bare_repo();

        // The root directory with .git file should be recognized as a git repo
        assert!(
            GitWorktree::is_git_repo(dir.path()),
            "Root of linked worktree setup should be recognized as git repo"
        );

        // The worktree should also be recognized
        let main_wt = dir.path().join("main");
        if main_wt.exists() {
            assert!(
                GitWorktree::is_git_repo(&main_wt),
                "Worktree should be recognized as git repo"
            );
        }
    }

    #[test]
    fn test_is_bare_repo_returns_true_for_bare_repo() {
        let dir = setup_linked_worktree_bare_repo();

        // Root of linked worktree setup should be detected as bare
        assert!(
            GitWorktree::is_bare_repo(dir.path()),
            "Linked worktree bare repo setup should be detected as bare"
        );
    }

    #[test]
    fn test_is_bare_repo_from_worktree_uses_main_repo_path() {
        let dir = setup_linked_worktree_bare_repo();
        let worktree_path = dir.path().join("main");

        // Skip if worktree wasn't created (git command might not be available)
        if !worktree_path.exists() {
            return;
        }

        // When checking from a worktree directory, is_bare_repo may return false
        // because git2 sees the worktree as having a working directory.
        // This documents the current behavior - callers should use find_main_repo first.
        let from_worktree = GitWorktree::is_bare_repo(&worktree_path);

        // The correct approach: find the main repo first, then check if it's bare
        let main_repo_path = GitWorktree::find_main_repo(&worktree_path).unwrap();
        let from_main_repo = GitWorktree::is_bare_repo(&main_repo_path);

        // Main repo path detection should always work correctly
        assert!(
            from_main_repo,
            "is_bare_repo should return true when called with main_repo_path from find_main_repo"
        );

        // Document that direct worktree check may differ (this is why we use main_repo_path)
        if !from_worktree {
            // This is expected behavior - git2's is_bare() returns false for worktrees
            // because they have a working directory. The fix is to always use
            // find_main_repo() first, then check is_bare_repo() on that path.
        }
    }

    #[test]
    fn test_is_bare_repo_returns_false_for_regular_repo() {
        let (_dir, repo) = setup_test_repo();
        let repo_path = repo.path().parent().unwrap();

        assert!(
            !GitWorktree::is_bare_repo(repo_path),
            "Regular repo should not be detected as bare"
        );
    }

    #[test]
    fn test_find_main_repo_works_with_linked_worktree_bare_repo() {
        let dir = setup_linked_worktree_bare_repo();

        // find_main_repo from root should return the root path
        let result = GitWorktree::find_main_repo(dir.path());
        assert!(
            result.is_ok(),
            "find_main_repo should succeed for linked worktree setup"
        );

        let main_repo = result.unwrap();
        // Canonicalize both paths to handle macOS /var -> /private/var symlink
        let expected = dir.path().canonicalize().unwrap();
        assert_eq!(
            main_repo, expected,
            "find_main_repo should return the root directory for bare repo setup"
        );
    }

    #[test]
    fn test_find_main_repo_from_worktree_returns_root() {
        let dir = setup_linked_worktree_bare_repo();
        let worktree_path = dir.path().join("main");

        // Skip if worktree wasn't created (git command might not be available)
        if !worktree_path.exists() {
            return;
        }

        // find_main_repo from a worktree should return the root path, not the worktree path
        let result = GitWorktree::find_main_repo(&worktree_path);
        assert!(
            result.is_ok(),
            "find_main_repo should succeed when called from a worktree"
        );

        let main_repo = result.unwrap();
        let expected = dir.path().canonicalize().unwrap();
        assert_eq!(
            main_repo, expected,
            "find_main_repo from worktree should return the root directory, not the worktree directory"
        );
    }

    #[test]
    fn test_git_worktree_new_works_with_linked_worktree_bare_repo() {
        let dir = setup_linked_worktree_bare_repo();

        let main_repo_path = GitWorktree::find_main_repo(dir.path()).unwrap();
        let result = GitWorktree::new(main_repo_path);
        assert!(
            result.is_ok(),
            "GitWorktree::new should succeed for linked worktree setup"
        );
    }

    #[test]
    fn test_list_worktrees_works_with_linked_worktree_bare_repo() {
        let dir = setup_linked_worktree_bare_repo();

        let main_repo_path = GitWorktree::find_main_repo(dir.path()).unwrap();
        let git_wt = GitWorktree::new(main_repo_path).unwrap();

        let worktrees = git_wt.list_worktrees();
        assert!(
            worktrees.is_ok(),
            "list_worktrees should succeed for linked worktree setup"
        );

        let worktrees = worktrees.unwrap();
        // Should have at least the main worktree
        assert!(!worktrees.is_empty(), "Should list at least one worktree");
    }

    #[test]
    fn test_delete_branch_deletes_local_branch() {
        let (_dir, repo) = setup_test_repo();
        let repo_path = repo.path().parent().unwrap();

        // Create a new branch
        let head = repo.head().unwrap();
        let commit = head.peel_to_commit().unwrap();
        repo.branch("to-delete", &commit, false).unwrap();

        // Verify branch exists
        assert!(repo
            .find_branch("to-delete", git2::BranchType::Local)
            .is_ok());

        let git_wt = GitWorktree::new(repo_path.to_path_buf()).unwrap();
        git_wt.delete_branch("to-delete").unwrap();

        // Verify branch no longer exists
        assert!(repo
            .find_branch("to-delete", git2::BranchType::Local)
            .is_err());
    }

    #[test]
    fn test_create_worktree_from_remote_branch() {
        let dir = TempDir::new().unwrap();

        // Create the "remote" repo with a branch
        let remote_path = dir.path().join("remote");
        std::fs::create_dir(&remote_path).unwrap();
        let remote_repo = git2::Repository::init(&remote_path).unwrap();
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let tree_id = remote_repo.index().unwrap().write_tree().unwrap();
        let tree = remote_repo.find_tree(tree_id).unwrap();
        let commit_oid = remote_repo
            .commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
            .unwrap();
        let commit = remote_repo.find_commit(commit_oid).unwrap();
        remote_repo
            .branch("remote-only-branch", &commit, false)
            .unwrap();

        // Clone it as the "local" repo
        let local_path = dir.path().join("local");
        std::process::Command::new("git")
            .args([
                "clone",
                remote_path.to_str().unwrap(),
                local_path.to_str().unwrap(),
            ])
            .output()
            .unwrap();

        // Verify the branch is not local but is remote
        let local_repo = git2::Repository::open(&local_path).unwrap();
        assert!(local_repo
            .find_branch("remote-only-branch", git2::BranchType::Local)
            .is_err());
        assert!(local_repo
            .find_branch("origin/remote-only-branch", git2::BranchType::Remote)
            .is_ok());

        // Create a worktree from the remote branch
        let wt_path = dir.path().join("remote-wt");
        let git_wt = GitWorktree::new(local_path).unwrap();
        git_wt
            .create_worktree("remote-only-branch", &wt_path, false)
            .unwrap();

        assert!(wt_path.exists());
        assert!(wt_path.join(".git").exists());
    }

    #[test]
    fn test_delete_branch_fails_for_nonexistent_branch() {
        let (_dir, repo) = setup_test_repo();
        let repo_path = repo.path().parent().unwrap();

        let git_wt = GitWorktree::new(repo_path.to_path_buf()).unwrap();
        let result = git_wt.delete_branch("nonexistent");

        assert!(result.is_err());
    }

    #[test]
    fn test_create_worktree_succeeds_after_stale_directory_deleted() {
        let (dir, repo) = setup_test_repo();
        let repo_path = repo.path().parent().unwrap();

        let head = repo.head().unwrap();
        let commit = head.peel_to_commit().unwrap();
        repo.branch("stale-branch", &commit, false).unwrap();

        let wt_path = dir.path().join("stale-worktree");
        let git_wt = GitWorktree::new(repo_path.to_path_buf()).unwrap();
        git_wt
            .create_worktree("stale-branch", &wt_path, false)
            .unwrap();
        assert!(wt_path.exists());

        // Simulate external deletion (e.g., container rebuild) by removing the
        // worktree directory without going through `git worktree remove`.
        std::fs::remove_dir_all(&wt_path).unwrap();
        assert!(!wt_path.exists());

        // Creating a worktree at the same path should succeed because
        // create_worktree prunes stale entries first.
        git_wt
            .create_worktree("stale-branch", &wt_path, false)
            .unwrap();
        assert!(wt_path.exists());
    }

    #[test]
    fn test_create_worktree_returns_error_on_git_failure() {
        let (dir, repo) = setup_test_repo();
        let repo_path = repo.path().parent().unwrap();

        let head = repo.head().unwrap();
        let commit = head.peel_to_commit().unwrap();
        repo.branch("fail-branch", &commit, false).unwrap();

        let wt_path = dir.path().join("fail-worktree");
        let git_wt = GitWorktree::new(repo_path.to_path_buf()).unwrap();
        git_wt
            .create_worktree("fail-branch", &wt_path, false)
            .unwrap();

        // Try creating again at a different path but same branch - git won't
        // allow two worktrees to check out the same branch.
        let wt_path2 = dir.path().join("fail-worktree-2");
        let result = git_wt.create_worktree("fail-branch", &wt_path2, false);
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("worktree command failed"),
            "Expected WorktreeCommandFailed error, got: {err_msg}"
        );
    }
}
