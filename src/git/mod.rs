// Git worktree operations module

use std::path::{Path, PathBuf};

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

    pub fn create_worktree(&self, branch: &str, path: &Path, create_branch: bool) -> Result<()> {
        if path.exists() {
            return Err(GitError::WorktreeAlreadyExists(path.to_path_buf()));
        }

        let repo = git2::Repository::discover(&self.repo_path)?;

        if create_branch {
            let head = repo.head()?;
            let commit = head.peel_to_commit()?;
            repo.branch(branch, &commit, false)?;
        } else {
            repo.find_branch(branch, git2::BranchType::Local)
                .map_err(|_| GitError::BranchNotFound(branch.to_string()))?;
        }

        let path_str = path
            .to_str()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "Invalid path"))?;

        std::process::Command::new("git")
            .args(["worktree", "add", path_str, branch])
            .current_dir(&self.repo_path)
            .output()?;

        Ok(())
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

        std::process::Command::new("git")
            .args(["worktree", "remove", path_str])
            .current_dir(&self.repo_path)
            .output()?;

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
}
