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

    pub fn find_main_repo(path: &Path) -> Result<PathBuf> {
        let repo = git2::Repository::discover(path)?;
        let workdir = repo.workdir().ok_or(GitError::NotAGitRepo)?.to_path_buf();
        Ok(workdir)
    }

    pub fn create_worktree(&self, branch: &str, path: &Path, create_branch: bool) -> Result<()> {
        if path.exists() {
            return Err(GitError::WorktreeAlreadyExists(path.to_path_buf()));
        }

        let repo = git2::Repository::open(&self.repo_path)?;

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
        let repo = git2::Repository::open(&self.repo_path)?;
        let worktrees = repo.worktrees()?;

        let mut entries = vec![];

        entries.push(WorktreeEntry {
            path: self.repo_path.clone(),
            branch: Self::get_current_branch(&self.repo_path).ok(),
            is_detached: repo.head_detached()?,
        });

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
        let repo = git2::Repository::open(path)?;
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
}
