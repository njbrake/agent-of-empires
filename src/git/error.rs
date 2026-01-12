// Git error types

use std::path::PathBuf;

#[derive(Debug)]
pub enum GitError {
    NotAGitRepo,
    WorktreeAlreadyExists(PathBuf),
    WorktreeNotFound(PathBuf),
    BranchNotFound(String),
    Git2Error(git2::Error),
    IoError(std::io::Error),
}

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GitError::NotAGitRepo => write!(f, "Path is not in a git repository"),
            GitError::WorktreeAlreadyExists(path) => {
                write!(f, "Worktree already exists at {}", path.display())
            }
            GitError::WorktreeNotFound(path) => {
                write!(f, "Worktree not found at {}", path.display())
            }
            GitError::BranchNotFound(branch) => write!(f, "Branch '{}' not found", branch),
            GitError::Git2Error(err) => write!(f, "Git error: {}", err),
            GitError::IoError(err) => write!(f, "IO error: {}", err),
        }
    }
}

impl std::error::Error for GitError {}

impl From<git2::Error> for GitError {
    fn from(err: git2::Error) -> Self {
        GitError::Git2Error(err)
    }
}

impl From<std::io::Error> for GitError {
    fn from(err: std::io::Error) -> Self {
        GitError::IoError(err)
    }
}

pub type Result<T> = std::result::Result<T, GitError>;
