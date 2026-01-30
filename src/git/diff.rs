//! Git diff computation module
//!
//! Provides functionality for computing diffs between branches/commits
//! and the working directory.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use similar::{ChangeTag, TextDiff};

use super::error::{GitError, Result};

/// Status of a file in the diff
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileStatus {
    Added,
    Modified,
    Deleted,
    Renamed,
    Copied,
    Untracked,
}

impl FileStatus {
    /// Returns a single character indicator for the status
    pub fn indicator(&self) -> char {
        match self {
            FileStatus::Added => 'A',
            FileStatus::Modified => 'M',
            FileStatus::Deleted => 'D',
            FileStatus::Renamed => 'R',
            FileStatus::Copied => 'C',
            FileStatus::Untracked => '?',
        }
    }

    /// Returns a human-readable label
    pub fn label(&self) -> &'static str {
        match self {
            FileStatus::Added => "added",
            FileStatus::Modified => "modified",
            FileStatus::Deleted => "deleted",
            FileStatus::Renamed => "renamed",
            FileStatus::Copied => "copied",
            FileStatus::Untracked => "untracked",
        }
    }
}

/// Represents a file that has changed
#[derive(Debug, Clone)]
pub struct DiffFile {
    /// Path to the file (relative to repo root)
    pub path: PathBuf,
    /// Previous path if renamed
    pub old_path: Option<PathBuf>,
    /// Status of the change
    pub status: FileStatus,
    /// Number of lines added
    pub additions: usize,
    /// Number of lines deleted
    pub deletions: usize,
}

/// A single line in a diff with change information
#[derive(Debug, Clone)]
pub struct DiffLine {
    /// The type of change
    pub tag: ChangeTag,
    /// Line number in old file (None for insertions)
    pub old_line_num: Option<usize>,
    /// Line number in new file (None for deletions)
    pub new_line_num: Option<usize>,
    /// The actual content of the line
    pub content: String,
}

/// A hunk (group of changes) in a diff
#[derive(Debug, Clone)]
pub struct DiffHunk {
    /// Starting line in old file
    pub old_start: usize,
    /// Number of lines in old file
    pub old_lines: usize,
    /// Starting line in new file
    pub new_start: usize,
    /// Number of lines in new file
    pub new_lines: usize,
    /// Lines in this hunk
    pub lines: Vec<DiffLine>,
}

/// Complete diff for a single file
#[derive(Debug, Clone)]
pub struct FileDiff {
    /// The file being diffed
    pub file: DiffFile,
    /// Hunks of changes
    pub hunks: Vec<DiffHunk>,
    /// Whether this is a binary file
    pub is_binary: bool,
}

/// Compute the list of changed files between a base branch and the working directory
pub fn compute_changed_files(repo_path: &Path, base_branch: &str) -> Result<Vec<DiffFile>> {
    let repo = git2::Repository::discover(repo_path)?;

    // Get the tree from the base branch
    let base_tree = get_tree_from_ref(&repo, base_branch)?;

    // Create diff options
    let mut opts = git2::DiffOptions::new();
    opts.include_untracked(true);
    opts.recurse_untracked_dirs(true);

    // Get diff from base tree to working directory (includes index)
    let diff = repo.diff_tree_to_workdir_with_index(Some(&base_tree), Some(&mut opts))?;

    // Find renames/copies
    let mut find_opts = git2::DiffFindOptions::new();
    find_opts.renames(true);
    find_opts.copies(true);
    let mut diff = diff;
    diff.find_similar(Some(&mut find_opts))?;

    let mut files = Vec::new();
    let mut stats_map: HashMap<PathBuf, (usize, usize)> = HashMap::new();

    // First pass: collect stats
    diff.print(git2::DiffFormat::Patch, |delta, _hunk, line| {
        if let Some(path) = delta.new_file().path().or(delta.old_file().path()) {
            let entry = stats_map.entry(path.to_path_buf()).or_insert((0, 0));
            match line.origin() {
                '+' => entry.0 += 1,
                '-' => entry.1 += 1,
                _ => {}
            }
        }
        true
    })?;

    // Second pass: collect files
    for delta in diff.deltas() {
        let status = match delta.status() {
            git2::Delta::Added => FileStatus::Added,
            git2::Delta::Deleted => FileStatus::Deleted,
            git2::Delta::Modified => FileStatus::Modified,
            git2::Delta::Renamed => FileStatus::Renamed,
            git2::Delta::Copied => FileStatus::Copied,
            git2::Delta::Untracked => FileStatus::Untracked,
            _ => continue,
        };

        let path = delta
            .new_file()
            .path()
            .or(delta.old_file().path())
            .map(|p| p.to_path_buf())
            .unwrap_or_default();

        let old_path = if status == FileStatus::Renamed || status == FileStatus::Copied {
            delta.old_file().path().map(|p| p.to_path_buf())
        } else {
            None
        };

        let (additions, deletions) = stats_map.get(&path).copied().unwrap_or((0, 0));

        files.push(DiffFile {
            path,
            old_path,
            status,
            additions,
            deletions,
        });
    }

    // Sort by path for consistent ordering
    files.sort_by(|a, b| a.path.cmp(&b.path));

    Ok(files)
}

/// Get a git tree from a reference (branch name, tag, or commit)
fn get_tree_from_ref<'a>(repo: &'a git2::Repository, reference: &str) -> Result<git2::Tree<'a>> {
    // Try as a branch first
    if let Ok(branch) = repo.find_branch(reference, git2::BranchType::Local) {
        let commit = branch.get().peel_to_commit()?;
        return Ok(commit.tree()?);
    }

    // Try as a remote branch
    let remote_ref = format!("origin/{}", reference);
    if let Ok(branch) = repo.find_branch(&remote_ref, git2::BranchType::Remote) {
        let commit = branch.get().peel_to_commit()?;
        return Ok(commit.tree()?);
    }

    // Try as a reference/commit
    let obj = repo.revparse_single(reference)?;
    let commit = obj
        .peel_to_commit()
        .map_err(|_| GitError::BranchNotFound(reference.to_string()))?;
    Ok(commit.tree()?)
}

/// Compute the full diff for a specific file
pub fn compute_file_diff(
    repo_path: &Path,
    file_path: &Path,
    base_branch: &str,
    context_lines: usize,
) -> Result<FileDiff> {
    let repo = git2::Repository::discover(repo_path)?;
    let workdir = repo.workdir().ok_or(GitError::NotAGitRepo)?;

    // Get the base tree
    let base_tree = get_tree_from_ref(&repo, base_branch)?;

    // Get old content from base tree (as bytes first to check for binary)
    let old_bytes = get_blob_bytes(&repo, &base_tree, file_path);
    let old_is_binary = old_bytes
        .as_ref()
        .map(|b| is_binary_bytes(b))
        .unwrap_or(false);

    // Get new content from working directory (as bytes first to check for binary)
    let full_path = workdir.join(file_path);
    let new_bytes = if full_path.exists() {
        std::fs::read(&full_path).ok()
    } else {
        None
    };
    let new_is_binary = new_bytes
        .as_ref()
        .map(|b| is_binary_bytes(b))
        .unwrap_or(false);

    let is_binary = old_is_binary || new_is_binary;

    // Convert to strings (safe now that we've checked for binary)
    let old_content = old_bytes
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_default();
    let new_content = new_bytes
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_default();

    // Determine file status
    let status = if old_content.is_empty() && !new_content.is_empty() {
        FileStatus::Added
    } else if !old_content.is_empty() && new_content.is_empty() && !full_path.exists() {
        FileStatus::Deleted
    } else {
        FileStatus::Modified
    };

    if is_binary {
        return Ok(FileDiff {
            file: DiffFile {
                path: file_path.to_path_buf(),
                old_path: None,
                status,
                additions: 0,
                deletions: 0,
            },
            hunks: Vec::new(),
            is_binary: true,
        });
    }

    // Compute diff using similar
    let text_diff = TextDiff::from_lines(&old_content, &new_content);
    let mut hunks = Vec::new();
    let mut additions = 0;
    let mut deletions = 0;

    for group in text_diff.grouped_ops(context_lines) {
        let mut hunk_lines = Vec::new();
        let mut old_start = None;
        let mut new_start = None;
        let mut old_count = 0;
        let mut new_count = 0;

        for op in &group {
            for change in text_diff.iter_changes(op) {
                let tag = change.tag();
                let content = change.value().to_string();

                // Track line counts
                match tag {
                    ChangeTag::Delete => {
                        deletions += 1;
                        old_count += 1;
                    }
                    ChangeTag::Insert => {
                        additions += 1;
                        new_count += 1;
                    }
                    ChangeTag::Equal => {
                        old_count += 1;
                        new_count += 1;
                    }
                }

                // Track start lines
                if old_start.is_none() {
                    old_start = change.old_index();
                }
                if new_start.is_none() {
                    new_start = change.new_index();
                }

                hunk_lines.push(DiffLine {
                    tag,
                    old_line_num: change.old_index().map(|i| i + 1),
                    new_line_num: change.new_index().map(|i| i + 1),
                    content,
                });
            }
        }

        if !hunk_lines.is_empty() {
            hunks.push(DiffHunk {
                old_start: old_start.map(|i| i + 1).unwrap_or(1),
                old_lines: old_count,
                new_start: new_start.map(|i| i + 1).unwrap_or(1),
                new_lines: new_count,
                lines: hunk_lines,
            });
        }
    }

    Ok(FileDiff {
        file: DiffFile {
            path: file_path.to_path_buf(),
            old_path: None,
            status,
            additions,
            deletions,
        },
        hunks,
        is_binary: false,
    })
}

/// Get raw bytes of a blob from a tree by path
fn get_blob_bytes(repo: &git2::Repository, tree: &git2::Tree, path: &Path) -> Option<Vec<u8>> {
    let entry = tree.get_path(path).ok()?;
    let obj = entry.to_object(repo).ok()?;
    let blob = obj.as_blob()?;
    Some(blob.content().to_vec())
}

/// Check if raw bytes appear to be binary (null byte heuristic)
fn is_binary_bytes(content: &[u8]) -> bool {
    content.iter().take(8000).any(|&b| b == 0)
}

/// Get the content of a file from the working directory
pub fn get_working_file_content(repo_path: &Path, file_path: &Path) -> Result<String> {
    let repo = git2::Repository::discover(repo_path)?;
    let workdir = repo.workdir().ok_or(GitError::NotAGitRepo)?;
    let full_path = workdir.join(file_path);

    std::fs::read_to_string(&full_path).map_err(GitError::IoError)
}

/// Save content to a file in the working directory
pub fn save_working_file_content(repo_path: &Path, file_path: &Path, content: &str) -> Result<()> {
    let repo = git2::Repository::discover(repo_path)?;
    let workdir = repo.workdir().ok_or(GitError::NotAGitRepo)?;
    let full_path = workdir.join(file_path);

    // Create parent directories if needed
    if let Some(parent) = full_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    std::fs::write(&full_path, content).map_err(GitError::IoError)
}

/// List available branches in the repository
pub fn list_branches(repo_path: &Path) -> Result<Vec<String>> {
    let repo = git2::Repository::discover(repo_path)?;
    let mut branches = Vec::new();

    // Local branches
    for branch in repo.branches(Some(git2::BranchType::Local))? {
        let (branch, _) = branch?;
        if let Some(name) = branch.name()? {
            branches.push(name.to_string());
        }
    }

    // Sort alphabetically, but put main/master first
    branches.sort_by(|a, b| {
        let a_is_main = a == "main" || a == "master";
        let b_is_main = b == "main" || b == "master";
        match (a_is_main, b_is_main) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.cmp(b),
        }
    });

    Ok(branches)
}

/// Get the default branch name (main or master)
pub fn get_default_branch(repo_path: &Path) -> Result<String> {
    let repo = git2::Repository::discover(repo_path)?;

    // Try to find main first, then master
    for name in &["main", "master"] {
        if repo.find_branch(name, git2::BranchType::Local).is_ok() {
            return Ok(name.to_string());
        }
    }

    // Fall back to first branch
    if let Some(branch) = repo.branches(Some(git2::BranchType::Local))?.next() {
        let (branch, _) = branch?;
        if let Some(name) = branch.name()? {
            return Ok(name.to_string());
        }
    }

    Err(GitError::BranchNotFound("No branches found".to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_repo() -> (TempDir, git2::Repository) {
        let dir = TempDir::new().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();

        // Create initial commit
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();

        // Create a test file
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "line 1\nline 2\nline 3\n").unwrap();

        // Add and commit
        {
            let mut index = repo.index().unwrap();
            index.add_path(Path::new("test.txt")).unwrap();
            index.write().unwrap();
            let tree_id = index.write_tree().unwrap();
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "Initial commit", &tree, &[])
                .unwrap();
        }

        (dir, repo)
    }

    #[test]
    fn test_file_status_indicator() {
        assert_eq!(FileStatus::Added.indicator(), 'A');
        assert_eq!(FileStatus::Modified.indicator(), 'M');
        assert_eq!(FileStatus::Deleted.indicator(), 'D');
        assert_eq!(FileStatus::Renamed.indicator(), 'R');
    }

    #[test]
    fn test_compute_changed_files_no_changes() {
        let (dir, _repo) = setup_test_repo();
        let files = compute_changed_files(dir.path(), "HEAD").unwrap();
        assert!(files.is_empty());
    }

    #[test]
    fn test_compute_changed_files_with_modification() {
        let (dir, _repo) = setup_test_repo();

        // Modify the file
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "line 1 modified\nline 2\nline 3\n").unwrap();

        let files = compute_changed_files(dir.path(), "HEAD").unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].status, FileStatus::Modified);
        assert_eq!(files[0].path, Path::new("test.txt"));
    }

    #[test]
    fn test_compute_changed_files_with_addition() {
        let (dir, _repo) = setup_test_repo();

        // Add a new file
        let new_file = dir.path().join("new.txt");
        fs::write(&new_file, "new content\n").unwrap();

        let files = compute_changed_files(dir.path(), "HEAD").unwrap();
        assert!(files.iter().any(|f| f.status == FileStatus::Untracked));
    }

    #[test]
    fn test_compute_file_diff() {
        let (dir, _repo) = setup_test_repo();

        // Modify the file
        let file_path = dir.path().join("test.txt");
        fs::write(&file_path, "line 1 modified\nline 2\nline 3\nnew line 4\n").unwrap();

        let diff = compute_file_diff(dir.path(), Path::new("test.txt"), "HEAD", 3).unwrap();

        assert!(!diff.is_binary);
        assert!(!diff.hunks.is_empty());
        assert!(diff.file.additions > 0);
    }

    #[test]
    fn test_list_branches() {
        let (dir, repo) = setup_test_repo();

        // Create another branch
        let head = repo.head().unwrap();
        let commit = head.peel_to_commit().unwrap();
        repo.branch("feature", &commit, false).unwrap();

        let branches = list_branches(dir.path()).unwrap();
        assert!(!branches.is_empty());
    }

    #[test]
    fn test_get_default_branch() {
        let (dir, _repo) = setup_test_repo();
        // Should return the current branch (usually "master" for git init)
        let branch = get_default_branch(dir.path());
        assert!(branch.is_ok());
    }

    #[test]
    fn test_is_binary_bytes() {
        assert!(!is_binary_bytes(b"hello world"));
        assert!(!is_binary_bytes(b"line 1\nline 2"));
        assert!(is_binary_bytes(b"hello\0world"));
    }

    #[test]
    fn test_save_and_get_working_file() {
        let (dir, _repo) = setup_test_repo();

        let content = "new content here\n";
        save_working_file_content(dir.path(), Path::new("test.txt"), content).unwrap();

        let loaded = get_working_file_content(dir.path(), Path::new("test.txt")).unwrap();
        assert_eq!(loaded, content);
    }
}
