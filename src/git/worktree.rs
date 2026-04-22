//! `GitWorktree` — worktree lifecycle, branch detection, and template-based
//! path computation. Split out from `mod.rs` as part of the code-quality
//! consolidation pass; the public API is unchanged.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

use super::error::{GitError, Result};
use super::open_repo_at;
use super::template::{resolve_template, TemplateVars};

/// Default remote name used for fetch-before-worktree-create.
/// Hardcoded for now; if multi-remote support is needed (e.g., "upstream"
/// vs personal fork), this is the place to parameterize.
const FETCH_REMOTE: &str = "origin";

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
        open_repo_at(path).is_ok()
            || Self::find_main_repo_from_linked_worktree_gitfile(path).is_some()
    }

    /// Returns true if the repository is a bare repo (including linked worktree bare repo setups).
    /// This is useful for choosing appropriate worktree path templates.
    pub fn is_bare_repo(path: &Path) -> bool {
        open_repo_at(path)
            .map(|repo| repo.is_bare())
            .unwrap_or(false)
    }

    pub fn find_main_repo(path: &Path) -> Result<PathBuf> {
        if let Ok(repo) = open_repo_at(path) {
            if let Some(main_repo) = Self::find_main_repo_from_worktree_gitdir(repo.path()) {
                return Ok(main_repo);
            }

            // For regular repos with a working directory, return it
            if let Some(workdir) = repo.workdir() {
                return Ok(workdir.to_path_buf());
            }

            let bare_repo_path = repo.path().to_path_buf();
            let parent_dir = bare_repo_path.parent().ok_or(GitError::NotAGitRepo)?;

            // For linked setups where the parent has a `.git` file (not a directory)
            // pointing to the bare repo (e.g. `/repo/.git` containing `gitdir: ./.bare`),
            // use the parent as the main repo path.  We check `is_file()` rather than
            // `exists()` to avoid being fooled by spurious `.git/` directories created by
            // external tools (e.g. opencode drops a state file in `.git/` wherever it runs,
            // including inside bare-repo parent directories).
            if parent_dir.join(".git").is_file() {
                return Ok(parent_dir.to_path_buf());
            }

            // For direct bare repos (e.g. `/repo/foo.git`), use the bare repo itself.
            return Ok(bare_repo_path);
        }

        // Fallback for linked worktree layouts that open_repo_at doesn't handle.
        Self::find_main_repo_from_linked_worktree_gitfile(path).ok_or(GitError::NotAGitRepo)
    }

    /// For linked worktrees, `.git` is a file containing `gitdir: <path>`.
    /// If that path points to `.../worktrees/<name>`, return the repository root.
    ///
    /// Only checks the given path directly (does not walk up parent directories).
    fn find_main_repo_from_linked_worktree_gitfile(path: &Path) -> Option<PathBuf> {
        let dir = if path.is_file() { path.parent()? } else { path };
        let git_entry = dir.join(".git");
        if git_entry.is_file() {
            let gitdir = Self::read_gitdir_from_file(&git_entry)?;
            return Self::find_main_repo_from_worktree_gitdir(&gitdir);
        }
        None
    }

    fn read_gitdir_from_file(git_file: &Path) -> Option<PathBuf> {
        let content = std::fs::read_to_string(git_file).ok()?;
        let gitdir = content
            .lines()
            .find_map(|line| line.strip_prefix("gitdir:").map(str::trim))?;
        let gitdir_path = PathBuf::from(gitdir);
        let resolved = if gitdir_path.is_absolute() {
            gitdir_path
        } else {
            git_file.parent()?.join(gitdir_path)
        };
        resolved.canonicalize().ok()
    }

    fn find_main_repo_from_worktree_gitdir(gitdir: &Path) -> Option<PathBuf> {
        let worktrees_dir = gitdir.parent()?;
        if worktrees_dir.file_name() != Some(OsStr::new("worktrees")) {
            return None;
        }

        let git_or_bare_dir = worktrees_dir.parent()?;
        let parent_dir = git_or_bare_dir.parent()?;
        if git_or_bare_dir.file_name() == Some(OsStr::new(".git"))
            || parent_dir.join(".git").is_file()
        {
            return Some(parent_dir.to_path_buf());
        }

        Some(git_or_bare_dir.to_path_buf())
    }

    /// Fetch a specific branch from a remote. Fails silently on network errors
    /// or missing remotes (logs a warning), so callers can fall back to local state.
    /// Stdin is piped to null to prevent SSH passphrase prompts from hanging.
    /// Times out after 10 seconds.
    pub fn fetch_branch(&self, remote: &str, branch: &str) -> Result<()> {
        let mut child = match std::process::Command::new("git")
            .args(["fetch", remote, branch])
            .current_dir(&self.repo_path)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                tracing::warn!("git fetch {remote}/{branch} spawn failed: {e}");
                return Ok(());
            }
        };

        let timeout = std::time::Duration::from_secs(10);
        let start = std::time::Instant::now();
        let poll_interval = std::time::Duration::from_millis(100);

        loop {
            match child.try_wait() {
                Ok(Some(status)) => {
                    if !status.success() {
                        if let Some(mut stderr) = child.stderr.take() {
                            let mut msg = String::new();
                            let _ = std::io::Read::read_to_string(&mut stderr, &mut msg);
                            tracing::warn!("git fetch {remote}/{branch} failed: {}", msg.trim());
                        }
                    }
                    return Ok(());
                }
                Ok(None) => {
                    if start.elapsed() > timeout {
                        let _ = child.kill();
                        let _ = child.wait();
                        tracing::warn!(
                            "git fetch {remote}/{branch} timed out after {}s",
                            timeout.as_secs()
                        );
                        return Ok(());
                    }
                    std::thread::sleep(poll_interval);
                }
                Err(e) => {
                    tracing::warn!("git fetch {remote}/{branch} error: {e}");
                    return Ok(());
                }
            }
        }
    }

    /// Detect the default branch name by checking local branches and remote
    /// tracking refs for "main" or "master". Falls back to the first local
    /// branch if neither exists.
    pub fn detect_default_branch(&self) -> Result<String> {
        let repo = open_repo_at(&self.repo_path)?;

        for name in &["main", "master"] {
            if repo.find_branch(name, git2::BranchType::Local).is_ok() {
                return Ok(name.to_string());
            }
        }

        for name in &["main", "master"] {
            let remote_ref = format!("{FETCH_REMOTE}/{name}");
            if repo
                .find_branch(&remote_ref, git2::BranchType::Remote)
                .is_ok()
            {
                return Ok(name.to_string());
            }
        }

        if let Some(Ok((branch, _))) = repo.branches(Some(git2::BranchType::Local))?.next() {
            if let Ok(Some(name)) = branch.name() {
                return Ok(name.to_string());
            }
        }

        Err(GitError::BranchNotFound(
            "No default branch found".to_string(),
        ))
    }

    pub fn create_worktree(&self, branch: &str, path: &Path, create_branch: bool) -> Result<()> {
        if path.exists() {
            return Err(GitError::WorktreeAlreadyExists(path.to_path_buf()));
        }

        // Prune stale worktree entries so git doesn't reject a path that was
        // previously used by a now-deleted worktree directory.
        self.prune_worktrees()?;

        // Fetch from remote so the worktree starts from the latest state.
        // For new branches, fetch the default branch (main/master) to use as
        // the base. For existing branches, fetch that specific branch.
        // Fails silently on network errors, falling back to local refs.
        let default_branch = if create_branch {
            let db = self
                .detect_default_branch()
                .unwrap_or_else(|_| "main".to_string());
            self.fetch_branch(FETCH_REMOTE, &db)?;
            Some(db)
        } else {
            self.fetch_branch(FETCH_REMOTE, branch)?;
            None
        };

        let repo = open_repo_at(&self.repo_path)?;

        if let Some(default_branch) = default_branch {
            // Branch from origin/<default> so new branches start from the
            // latest remote state. Falls back to local HEAD if no remote
            // exists, then to any local branch (bare repo with broken HEAD).
            let remote_ref = format!("{FETCH_REMOTE}/{default_branch}");
            let commit_oid = repo
                .find_branch(&remote_ref, git2::BranchType::Remote)
                .ok()
                .and_then(|b| b.get().target())
                .or_else(|| {
                    repo.head()
                        .ok()
                        .and_then(|h| h.peel_to_commit().ok())
                        .map(|c| c.id())
                })
                .or_else(|| {
                    repo.branches(Some(git2::BranchType::Local))
                        .ok()
                        .and_then(|mut branches| {
                            branches.find_map(|b| b.ok().and_then(|(b, _)| b.get().target()))
                        })
                })
                .ok_or_else(|| {
                    GitError::WorktreeCommandFailed("No commits found to branch from".to_string())
                })?;
            let commit = repo.find_commit(commit_oid)?;
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
    pub fn prune_worktrees(&self) -> Result<()> {
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
    pub(crate) fn diff_paths(target: &Path, base: &Path) -> Option<PathBuf> {
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
        let repo = open_repo_at(&self.repo_path)?;
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

    pub fn remove_worktree(&self, path: &Path, force: bool) -> Result<()> {
        if !path.exists() {
            return Err(GitError::WorktreeNotFound(path.to_path_buf()));
        }

        let path_str = path
            .to_str()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid path"))?;

        let mut args = vec!["worktree", "remove"];
        if force {
            args.push("--force");
        }
        args.push(path_str);

        let output = std::process::Command::new("git")
            .args(&args)
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
        let repo = open_repo_at(path)?;
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

        git_wt.remove_worktree(&wt_path, false).unwrap();
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

    fn convert_worktree_gitfile_to_relative(worktree_path: &Path) {
        let git_file = worktree_path.join(".git");
        let content = std::fs::read_to_string(&git_file).unwrap();
        let gitdir_line = content
            .lines()
            .find_map(|line| line.strip_prefix("gitdir:").map(str::trim))
            .unwrap();
        let gitdir_path = Path::new(gitdir_line);

        if gitdir_path.is_relative() {
            return;
        }

        let worktree_canonical = worktree_path.canonicalize().unwrap();
        let gitdir_canonical = gitdir_path.canonicalize().unwrap();
        let relative = GitWorktree::diff_paths(&gitdir_canonical, &worktree_canonical).unwrap();
        std::fs::write(git_file, format!("gitdir: {}\n", relative.display())).unwrap();
    }

    fn setup_linked_worktree_with_worktrees_gitfile() -> Option<(TempDir, PathBuf)> {
        let dir = setup_linked_worktree_bare_repo();
        let worktree_path = dir.path().join("main");
        if !worktree_path.exists() {
            return None;
        }

        convert_worktree_gitfile_to_relative(&worktree_path);
        let git_file_content = std::fs::read_to_string(worktree_path.join(".git")).unwrap();
        assert!(
            git_file_content.contains("worktrees"),
            ".git file should point to worktrees/<name>, got: {git_file_content}"
        );

        Some((dir, worktree_path))
    }

    fn setup_sibling_bare_repo_worktree() -> Option<(TempDir, PathBuf, PathBuf)> {
        let dir = TempDir::new().unwrap();
        let repo_root = dir.path().join("fe");
        std::fs::create_dir_all(&repo_root).unwrap();
        let bare_repo_path = repo_root.join("foo.git");

        let init = std::process::Command::new("git")
            .args(["init", "--bare", bare_repo_path.to_str().unwrap()])
            .output()
            .ok()?;
        if !init.status.success() {
            return None;
        }

        let seed_path = repo_root.join("seed");
        let clone = std::process::Command::new("git")
            .args([
                "clone",
                bare_repo_path.to_str().unwrap(),
                seed_path.to_str().unwrap(),
            ])
            .output()
            .ok()?;
        if !clone.status.success() {
            return None;
        }

        let config_name = std::process::Command::new("git")
            .args(["config", "user.name", "Test User"])
            .current_dir(&seed_path)
            .output()
            .ok()?;
        if !config_name.status.success() {
            return None;
        }
        let config_email = std::process::Command::new("git")
            .args(["config", "user.email", "test@example.com"])
            .current_dir(&seed_path)
            .output()
            .ok()?;
        if !config_email.status.success() {
            return None;
        }

        std::fs::write(seed_path.join("README.md"), "hello\n").ok()?;
        let add = std::process::Command::new("git")
            .args(["add", "README.md"])
            .current_dir(&seed_path)
            .output()
            .ok()?;
        if !add.status.success() {
            return None;
        }
        let commit = std::process::Command::new("git")
            .args(["commit", "-m", "init"])
            .current_dir(&seed_path)
            .output()
            .ok()?;
        if !commit.status.success() {
            return None;
        }
        let push = std::process::Command::new("git")
            .args(["push", "origin", "HEAD:main"])
            .current_dir(&seed_path)
            .output()
            .ok()?;
        if !push.status.success() {
            return None;
        }

        std::fs::remove_dir_all(&seed_path).ok()?;

        let worktree_path = repo_root.join("master");
        let add_worktree = std::process::Command::new("git")
            .args([
                "--git-dir",
                bare_repo_path.to_str().unwrap(),
                "worktree",
                "add",
                worktree_path.to_str().unwrap(),
                "main",
            ])
            .output()
            .ok()?;
        if !add_worktree.status.success() || !worktree_path.exists() {
            return None;
        }

        Some((dir, bare_repo_path, worktree_path))
    }

    #[test]
    fn test_is_git_repo_recognizes_worktree_gitfile_pointing_to_worktrees() {
        let Some((_dir, worktree_path)) = setup_linked_worktree_with_worktrees_gitfile() else {
            return;
        };

        assert!(
            GitWorktree::is_git_repo(&worktree_path),
            "Worktree path with .git -> <bare>/worktrees/<name> should be recognized"
        );

        // Nested subdirectories should NOT be recognized as git repos (no walk-up)
        let nested_path = worktree_path.join("nested");
        std::fs::create_dir_all(&nested_path).unwrap();
        assert!(
            !GitWorktree::is_git_repo(&nested_path),
            "Nested paths should not walk up to find ancestor repos"
        );
    }

    #[test]
    fn test_find_main_repo_from_worktree_gitfile_pointing_to_worktrees_returns_root() {
        let Some((dir, worktree_path)) = setup_linked_worktree_with_worktrees_gitfile() else {
            return;
        };

        let expected = dir.path().canonicalize().unwrap();
        assert_eq!(
            GitWorktree::find_main_repo(&worktree_path).unwrap(),
            expected,
            "find_main_repo should resolve linked worktree path back to bare repo root"
        );

        // Nested subdirectories should NOT resolve (no walk-up)
        let nested_path = worktree_path.join("nested");
        std::fs::create_dir_all(&nested_path).unwrap();
        assert!(
            GitWorktree::find_main_repo(&nested_path).is_err(),
            "find_main_repo should not walk up from nested paths"
        );
    }

    #[test]
    fn test_find_main_repo_from_sibling_bare_repo_worktree_returns_bare_repo_path() {
        let Some((_dir, bare_repo_path, worktree_path)) = setup_sibling_bare_repo_worktree() else {
            return;
        };

        let expected = bare_repo_path.canonicalize().unwrap();
        assert_eq!(
            GitWorktree::find_main_repo(&worktree_path).unwrap(),
            expected,
            "find_main_repo should resolve sibling bare-repo worktree to bare repo path"
        );
        assert!(
            GitWorktree::new(expected).is_ok(),
            "resolved bare repo path should be accepted by GitWorktree::new"
        );

        // Nested subdirectories should NOT resolve (no walk-up)
        let nested_path = worktree_path.join("nested");
        std::fs::create_dir_all(&nested_path).unwrap();
        assert!(
            GitWorktree::find_main_repo(&nested_path).is_err(),
            "find_main_repo should not walk up from nested paths"
        );
    }

    #[test]
    fn test_find_main_repo_from_direct_bare_repo_path_returns_bare_repo_path() {
        let Some((_dir, bare_repo_path, _worktree_path)) = setup_sibling_bare_repo_worktree()
        else {
            return;
        };

        let expected = bare_repo_path.canonicalize().unwrap();
        assert_eq!(
            GitWorktree::find_main_repo(&bare_repo_path).unwrap(),
            expected,
            "find_main_repo should keep direct bare repo paths instead of returning their parent"
        );
        assert!(
            GitWorktree::new(expected).is_ok(),
            "direct bare repo path should be accepted by GitWorktree::new"
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

    // ---- Full worktree creation flow tests for regular repos ----

    #[test]
    fn test_regular_repo_create_worktree_existing_branch() {
        let (dir, repo) = setup_test_repo();
        let repo_path = repo.path().parent().unwrap();

        let head = repo.head().unwrap();
        let commit = head.peel_to_commit().unwrap();
        repo.branch("feature-a", &commit, false).unwrap();

        let main_repo = GitWorktree::find_main_repo(repo_path).unwrap();
        assert!(!GitWorktree::is_bare_repo(&main_repo));
        let git_wt = GitWorktree::new(main_repo).unwrap();

        let wt_path = dir.path().join("feature-a-wt");
        git_wt
            .create_worktree("feature-a", &wt_path, false)
            .unwrap();

        assert!(wt_path.exists());
        assert!(wt_path.join(".git").exists());
    }

    #[test]
    fn test_regular_repo_create_worktree_new_branch() {
        let (dir, repo) = setup_test_repo();
        let repo_path = repo.path().parent().unwrap();

        let main_repo = GitWorktree::find_main_repo(repo_path).unwrap();
        let git_wt = GitWorktree::new(main_repo).unwrap();

        let wt_path = dir.path().join("new-feat-wt");
        git_wt.create_worktree("new-feat", &wt_path, true).unwrap();

        assert!(wt_path.exists());
        assert!(repo
            .find_branch("new-feat", git2::BranchType::Local)
            .is_ok());
    }

    #[test]
    fn test_regular_repo_full_builder_flow() {
        let (_dir, repo) = setup_test_repo();
        let repo_path = repo.path().parent().unwrap();

        // Mimic what builder.rs does
        assert!(GitWorktree::is_git_repo(repo_path));

        let main_repo_path = GitWorktree::find_main_repo(repo_path).unwrap();
        let git_wt = GitWorktree::new(main_repo_path.clone()).unwrap();

        let is_bare = GitWorktree::is_bare_repo(&main_repo_path);
        assert!(!is_bare);

        let template = if is_bare {
            "./{branch}"
        } else {
            "../{repo-name}-worktrees/{branch}"
        };
        let wt_path = git_wt
            .compute_path("my-branch", template, "abc12345")
            .unwrap();

        git_wt.create_worktree("my-branch", &wt_path, true).unwrap();

        assert!(wt_path.exists());
        assert!(wt_path.join(".git").exists());

        // Cleanup
        let _ = std::fs::remove_dir_all(&wt_path);
    }

    // ---- Full worktree creation flow tests for linked bare repos ----

    #[test]
    fn test_linked_bare_repo_create_worktree_new_branch() {
        let dir = setup_linked_worktree_bare_repo();
        let main_wt = dir.path().join("main");
        if !main_wt.exists() {
            return; // git not available
        }

        let main_repo_path = GitWorktree::find_main_repo(dir.path()).unwrap();
        let git_wt = GitWorktree::new(main_repo_path).unwrap();

        let wt_path = dir.path().join("new-feature");
        git_wt
            .create_worktree("new-feature", &wt_path, true)
            .unwrap();

        assert!(wt_path.exists(), "Worktree directory should be created");
        assert!(
            wt_path.join(".git").exists(),
            "Worktree should have .git file"
        );
    }

    #[test]
    fn test_linked_bare_repo_create_worktree_existing_branch() {
        let dir = setup_linked_worktree_bare_repo();
        let main_wt = dir.path().join("main");
        if !main_wt.exists() {
            return;
        }

        let main_repo_path = GitWorktree::find_main_repo(dir.path()).unwrap();
        let git_wt = GitWorktree::new(main_repo_path.clone()).unwrap();

        // Create a branch first
        {
            let repo = open_repo_at(&main_repo_path).unwrap();
            let head = repo.head().unwrap();
            let commit = head.peel_to_commit().unwrap();
            repo.branch("existing-branch", &commit, false).unwrap();
        }

        let wt_path = dir.path().join("existing-branch-wt");
        git_wt
            .create_worktree("existing-branch", &wt_path, false)
            .unwrap();

        assert!(wt_path.exists());
        assert!(wt_path.join(".git").exists());
    }

    #[test]
    fn test_linked_bare_repo_create_worktree_from_worktree_dir() {
        let dir = setup_linked_worktree_bare_repo();
        let main_wt = dir.path().join("main");
        if !main_wt.exists() {
            return;
        }

        // Start from the worktree directory (as a user would)
        let main_repo_path = GitWorktree::find_main_repo(&main_wt).unwrap();
        let git_wt = GitWorktree::new(main_repo_path.clone()).unwrap();

        assert!(
            GitWorktree::is_bare_repo(&main_repo_path),
            "Should detect bare repo when resolved from worktree"
        );

        let wt_path = dir.path().join("from-wt-branch");
        git_wt
            .create_worktree("from-wt-branch", &wt_path, true)
            .unwrap();

        assert!(wt_path.exists());
        assert!(wt_path.join(".git").exists());
    }

    #[test]
    fn test_linked_bare_repo_full_builder_flow() {
        let dir = setup_linked_worktree_bare_repo();
        let main_wt = dir.path().join("main");
        if !main_wt.exists() {
            return;
        }

        // Mimic the exact flow from builder.rs, starting from a worktree dir
        let path = &main_wt;
        assert!(
            GitWorktree::is_git_repo(path),
            "Worktree should be recognized as git repo"
        );

        let main_repo_path = GitWorktree::find_main_repo(path).unwrap();
        let git_wt = GitWorktree::new(main_repo_path.clone()).unwrap();

        let is_bare = GitWorktree::is_bare_repo(&main_repo_path);
        assert!(is_bare, "Should detect bare repo");

        let template = if is_bare {
            "./{branch}"
        } else {
            "../{repo-name}-worktrees/{branch}"
        };
        let wt_path = git_wt
            .compute_path("builder-branch", template, "abc12345")
            .unwrap();

        git_wt
            .create_worktree("builder-branch", &wt_path, true)
            .unwrap();

        assert!(
            wt_path.exists(),
            "Worktree should be created at computed path"
        );
        assert!(wt_path.join(".git").exists());

        // Verify the worktree is a sibling inside the project dir (bare repo template)
        assert_eq!(
            wt_path.parent().unwrap().canonicalize().unwrap(),
            dir.path().canonicalize().unwrap(),
            "Worktree should be a sibling directory in bare repo layout"
        );
    }

    #[test]
    fn test_linked_bare_repo_full_builder_flow_from_root() {
        let dir = setup_linked_worktree_bare_repo();
        let main_wt = dir.path().join("main");
        if !main_wt.exists() {
            return;
        }

        // Same as above, but starting from the root directory (not a worktree)
        let path = dir.path();
        assert!(
            GitWorktree::is_git_repo(path),
            "Bare repo root should be recognized as git repo"
        );

        let main_repo_path = GitWorktree::find_main_repo(path).unwrap();
        let git_wt = GitWorktree::new(main_repo_path.clone()).unwrap();

        let is_bare = GitWorktree::is_bare_repo(&main_repo_path);
        assert!(is_bare, "Should detect bare repo from root");

        let template = "./{branch}";
        let wt_path = git_wt
            .compute_path("root-branch", template, "xyz99999")
            .unwrap();

        git_wt
            .create_worktree("root-branch", &wt_path, true)
            .unwrap();

        assert!(wt_path.exists());
        assert!(wt_path.join(".git").exists());
    }

    // ---- Sibling bare repo worktree creation tests ----

    #[test]
    fn test_sibling_bare_repo_create_worktree_new_branch() {
        let Some((_dir, bare_repo_path, _worktree_path)) = setup_sibling_bare_repo_worktree()
        else {
            return;
        };

        let main_repo_path = GitWorktree::find_main_repo(&bare_repo_path).unwrap();
        let git_wt = GitWorktree::new(main_repo_path).unwrap();

        let wt_path = bare_repo_path.parent().unwrap().join("new-sibling-wt");
        git_wt
            .create_worktree("new-sibling", &wt_path, true)
            .unwrap();

        assert!(wt_path.exists());
        assert!(wt_path.join(".git").exists());
    }

    #[test]
    fn test_sibling_bare_repo_create_worktree_existing_branch() {
        let Some((_dir, bare_repo_path, _worktree_path)) = setup_sibling_bare_repo_worktree()
        else {
            return;
        };

        let main_repo_path = GitWorktree::find_main_repo(&bare_repo_path).unwrap();
        let git_wt = GitWorktree::new(main_repo_path.clone()).unwrap();

        // Create a branch to check out
        {
            let repo = open_repo_at(&main_repo_path).unwrap();
            // Find any existing commit to branch from
            let oid = repo
                .reflog("HEAD")
                .ok()
                .and_then(|reflog| reflog.get(0).map(|e| e.id_new()))
                .or_else(|| {
                    repo.branches(Some(git2::BranchType::Local))
                        .ok()
                        .and_then(|mut branches| {
                            branches.find_map(|b| b.ok().and_then(|(b, _)| b.get().target()))
                        })
                })
                .expect("bare repo should have at least one commit");
            let commit = repo.find_commit(oid).unwrap();
            repo.branch("existing-sibling", &commit, false).unwrap();
        }

        let wt_path = bare_repo_path.parent().unwrap().join("existing-sibling-wt");
        git_wt
            .create_worktree("existing-sibling", &wt_path, false)
            .unwrap();

        assert!(wt_path.exists());
        assert!(wt_path.join(".git").exists());
    }

    #[test]
    fn test_sibling_bare_repo_create_worktree_from_worktree_dir() {
        let Some((_dir, _bare_repo_path, worktree_path)) = setup_sibling_bare_repo_worktree()
        else {
            return;
        };

        // Start from an existing worktree
        let main_repo_path = GitWorktree::find_main_repo(&worktree_path).unwrap();
        let git_wt = GitWorktree::new(main_repo_path).unwrap();

        let wt_path = worktree_path.parent().unwrap().join("from-wt-sibling");
        git_wt
            .create_worktree("from-wt-sibling", &wt_path, true)
            .unwrap();

        assert!(wt_path.exists());
        assert!(wt_path.join(".git").exists());
    }

    /// Sets up a bare repo whose parent directory contains a spurious `.git/` directory
    /// (e.g. created by an external tool storing state files there).
    fn setup_bare_repo_whose_parent_has_spurious_git_dir() -> Option<(TempDir, PathBuf)> {
        let dir = TempDir::new().unwrap();

        // Simulate what opencode does: create a `.git/` directory in the parent and
        // drop a state file into it.  This is the exact scenario seen in production.
        let spurious_git_dir = dir.path().join(".git");
        std::fs::create_dir_all(&spurious_git_dir).unwrap();
        std::fs::write(spurious_git_dir.join("opencode"), "some-sha\n").unwrap();

        // Create the actual bare repo as a subdirectory of the parent.
        let bare_path = dir.path().join("bare");
        let init = std::process::Command::new("git")
            .args(["init", "--bare", bare_path.to_str().unwrap()])
            .output()
            .ok()?;
        if !init.status.success() {
            return None;
        }

        // Give the bare repo an initial commit so HEAD resolves.
        {
            let sig = git2::Signature::now("Test", "test@example.com").unwrap();
            let repo = git2::Repository::open_bare(&bare_path).unwrap();
            let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
                .unwrap();
        }

        // Create a linked worktree (this is what creates bare/worktrees/ and bare/main/).
        let main_wt = bare_path.join("main");
        let add = std::process::Command::new("git")
            .args([
                "--git-dir",
                bare_path.to_str().unwrap(),
                "worktree",
                "add",
                main_wt.to_str().unwrap(),
                "HEAD",
            ])
            .output()
            .ok()?;
        if !add.status.success() || !main_wt.exists() {
            return None;
        }

        Some((dir, bare_path))
    }

    #[test]
    fn test_find_main_repo_for_bare_repo_whose_parent_has_spurious_git_dir() {
        let Some((_dir, bare_path)) = setup_bare_repo_whose_parent_has_spurious_git_dir() else {
            return;
        };

        let result = GitWorktree::find_main_repo(&bare_path);
        assert!(
            result.is_ok(),
            "find_main_repo should succeed for a bare repo whose parent has a \
             spurious .git/ directory, got: {:?}",
            result.err()
        );
        assert_eq!(
            result.unwrap().canonicalize().unwrap(),
            bare_path.canonicalize().unwrap(),
            "find_main_repo should return the bare repo root itself, not its parent"
        );
    }

    #[test]
    fn test_find_main_repo_from_worktree_with_spurious_parent_git_dir() {
        let Some((_dir, bare_path)) = setup_bare_repo_whose_parent_has_spurious_git_dir() else {
            return;
        };

        // find_main_repo from a linked worktree inside the bare repo should resolve
        // back to the bare repo, not to the parent with the spurious .git/ dir.
        let worktree_path = bare_path.join("main");
        let result = GitWorktree::find_main_repo(&worktree_path);
        assert!(
            result.is_ok(),
            "find_main_repo from worktree should succeed, got: {:?}",
            result.err()
        );
        let resolved = result.unwrap().canonicalize().unwrap();
        let expected = bare_path.canonicalize().unwrap();
        assert_eq!(
            resolved, expected,
            "should resolve to bare repo, not parent"
        );
    }

    #[test]
    fn test_full_builder_flow_for_bare_repo_whose_parent_has_spurious_git_dir() {
        let Some((_dir, bare_path)) = setup_bare_repo_whose_parent_has_spurious_git_dir() else {
            return;
        };

        // Mimic the exact flow executed by builder.rs / cli/add.rs.
        assert!(
            GitWorktree::is_git_repo(&bare_path),
            "is_git_repo must return true before the builder proceeds"
        );

        let main_repo_path = GitWorktree::find_main_repo(&bare_path).unwrap();

        // This is the call that fails before the fix: find_main_repo returns the
        // parent directory, and GitWorktree::new then rejects it as NotAGitRepo.
        let git_wt = GitWorktree::new(main_repo_path.clone())
            .expect("GitWorktree::new should succeed with the resolved bare repo path");

        assert!(
            GitWorktree::is_bare_repo(&main_repo_path),
            "Should be detected as a bare repo"
        );

        let wt_path = bare_path.join("new-feature");
        git_wt
            .create_worktree("new-feature", &wt_path, true)
            .unwrap();

        assert!(wt_path.exists(), "Worktree directory should be created");
        assert!(
            wt_path.join(".git").is_file(),
            "Worktree should have a .git pointer file"
        );
    }

    // --- detect_default_branch tests ---

    #[test]
    fn test_detect_default_branch_returns_main() {
        // setup_test_repo creates a repo with HEAD on main (git2 default)
        let (dir, _repo) = setup_test_repo();
        let git_wt = GitWorktree::new(dir.path().to_path_buf()).unwrap();
        // git2::Repository::init creates an initial branch based on init.defaultBranch
        // or "master". Either way, it should be detected.
        let result = git_wt.detect_default_branch().unwrap();
        assert!(
            result == "main" || result == "master",
            "expected main or master, got: {result}"
        );
    }

    #[test]
    fn test_detect_default_branch_finds_master_when_no_main() {
        let dir = TempDir::new().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();

        // Create a commit on a "master" branch explicitly
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("refs/heads/master"), &sig, &sig, "init", &tree, &[])
            .unwrap();

        // If there's also a "main" from init, remove it
        if let Ok(mut main_branch) = repo.find_branch("main", git2::BranchType::Local) {
            let _ = main_branch.delete();
        }

        let git_wt = GitWorktree::new(dir.path().to_path_buf()).unwrap();
        assert_eq!(git_wt.detect_default_branch().unwrap(), "master");
    }

    #[test]
    fn test_detect_default_branch_falls_back_to_first_branch() {
        let dir = TempDir::new().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();

        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("refs/heads/develop"), &sig, &sig, "init", &tree, &[])
            .unwrap();

        // Remove main and master if they exist
        if let Ok(mut b) = repo.find_branch("main", git2::BranchType::Local) {
            let _ = b.delete();
        }
        if let Ok(mut b) = repo.find_branch("master", git2::BranchType::Local) {
            let _ = b.delete();
        }

        let git_wt = GitWorktree::new(dir.path().to_path_buf()).unwrap();
        let result = git_wt.detect_default_branch().unwrap();
        assert_eq!(result, "develop");
    }

    // --- fetch_branch tests ---

    #[test]
    fn test_fetch_branch_ok_when_no_remote() {
        let (dir, _repo) = setup_test_repo();
        let git_wt = GitWorktree::new(dir.path().to_path_buf()).unwrap();
        // No remote configured, fetch should return Ok (silent failure)
        assert!(git_wt.fetch_branch("origin", "main").is_ok());
    }

    #[test]
    fn test_fetch_branch_ok_when_branch_missing_on_remote() {
        let remote_dir = TempDir::new().unwrap();
        let _remote = git2::Repository::init_bare(remote_dir.path()).unwrap();

        // Clone to get a local repo with an "origin" remote
        let local_dir = TempDir::new().unwrap();
        let _local =
            git2::Repository::clone(remote_dir.path().to_str().unwrap(), local_dir.path()).unwrap();

        let git_wt = GitWorktree::new(local_dir.path().to_path_buf()).unwrap();
        // Fetching a branch that doesn't exist on the remote should succeed silently
        assert!(git_wt.fetch_branch("origin", "nonexistent-branch").is_ok());
    }

    // --- create_worktree fetch integration test ---

    #[test]
    fn test_create_worktree_branches_from_remote_after_fetch() {
        // Create a bare "remote" repo with an initial commit on main
        let remote_dir = TempDir::new().unwrap();
        let remote = git2::Repository::init_bare(remote_dir.path()).unwrap();

        // Point HEAD at refs/heads/main so clone creates a local "main" branch
        remote.set_head("refs/heads/main").unwrap();

        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let tree_id = {
            let blob_oid = remote.blob(b"hello").unwrap();
            let mut tb = remote.treebuilder(None).unwrap();
            tb.insert("file.txt", blob_oid, 0o100644).unwrap();
            tb.write().unwrap()
        };
        let tree = remote.find_tree(tree_id).unwrap();
        let initial_oid = remote
            .commit(Some("refs/heads/main"), &sig, &sig, "initial", &tree, &[])
            .unwrap();

        // Clone it locally
        let local_dir = TempDir::new().unwrap();
        git2::Repository::clone(remote_dir.path().to_str().unwrap(), local_dir.path()).unwrap();

        // Add a second commit to the remote (local is now 1 commit behind)
        let blob2 = remote.blob(b"world").unwrap();
        let mut tb2 = remote.treebuilder(Some(&tree)).unwrap();
        tb2.insert("file2.txt", blob2, 0o100644).unwrap();
        let tree2_id = tb2.write().unwrap();
        let tree2 = remote.find_tree(tree2_id).unwrap();
        let initial_commit = remote.find_commit(initial_oid).unwrap();
        let remote_head_oid = remote
            .commit(
                Some("refs/heads/main"),
                &sig,
                &sig,
                "second commit",
                &tree2,
                &[&initial_commit],
            )
            .unwrap();

        // Verify local is behind: local main should still be at initial_oid
        let local_repo = git2::Repository::open(local_dir.path()).unwrap();
        let local_main = local_repo
            .find_branch("main", git2::BranchType::Local)
            .unwrap();
        assert_eq!(local_main.get().peel_to_commit().unwrap().id(), initial_oid);

        // Create a new worktree branch via create_worktree
        let wt_parent = TempDir::new().unwrap();
        let wt_path = wt_parent.path().join("test-fetch-wt");
        let git_wt = GitWorktree::new(local_dir.path().to_path_buf()).unwrap();
        git_wt
            .create_worktree("new-feature", &wt_path, true)
            .unwrap();

        // The new branch should be based on the remote's latest commit,
        // not the stale local HEAD
        let new_branch = local_repo
            .find_branch("new-feature", git2::BranchType::Local)
            .unwrap();
        let branch_commit_id = new_branch.get().peel_to_commit().unwrap().id();
        assert_eq!(
            branch_commit_id, remote_head_oid,
            "new branch should be based on remote HEAD ({remote_head_oid}), \
             not stale local HEAD ({initial_oid})"
        );
    }
}
