// Integration tests for git worktree functionality
// These tests verify end-to-end worktree workflows

use agent_of_empires::git::error::GitError;
use agent_of_empires::git::GitWorktree;
use agent_of_empires::session::{Instance, Storage, WorktreeInfo};
use chrono::Utc;
use tempfile::TempDir;

fn setup_test_environment() -> (TempDir, git2::Repository, TempDir) {
    let repo_dir = TempDir::new().unwrap();
    let repo = git2::Repository::init(repo_dir.path()).unwrap();

    let sig = git2::Signature::now("Test", "test@example.com").unwrap();
    let tree_id = {
        let mut index = repo.index().unwrap();
        index.write_tree().unwrap()
    };
    {
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial", &tree, &[])
            .unwrap();

        let head = repo.head().unwrap();
        let commit = head.peel_to_commit().unwrap();
        repo.branch("test-feature", &commit, false).unwrap();
    }

    let config_dir = TempDir::new().unwrap();

    (repo_dir, repo, config_dir)
}

#[test]
fn test_add_session_with_worktree_flag() {
    let (repo_dir, _repo, _config_dir) = setup_test_environment();

    let git_wt = GitWorktree::new(repo_dir.path().to_path_buf()).unwrap();
    let wt_path = repo_dir.path().join("worktree-test-feature");

    git_wt
        .create_worktree("test-feature", &wt_path, false)
        .unwrap();

    let mut instance = Instance::new("Test Session", wt_path.to_str().unwrap());
    instance.worktree_info = Some(WorktreeInfo {
        branch: "test-feature".to_string(),
        main_repo_path: repo_dir.path().to_string_lossy().to_string(),
        managed_by_aoe: true,
        created_at: Utc::now(),
        cleanup_on_delete: true,
    });

    assert!(wt_path.exists());
    assert!(instance.worktree_info.is_some());
    let info = instance.worktree_info.as_ref().unwrap();
    assert_eq!(info.branch, "test-feature");
    assert!(info.managed_by_aoe);
}

#[test]
fn test_session_has_worktree_info_after_creation() {
    let (repo_dir, _repo, _config_dir) = setup_test_environment();

    let mut instance = Instance::new("Test Session", repo_dir.path().to_str().unwrap());
    let now = Utc::now();

    instance.worktree_info = Some(WorktreeInfo {
        branch: "test-feature".to_string(),
        main_repo_path: repo_dir.path().to_string_lossy().to_string(),
        managed_by_aoe: true,
        created_at: now,
        cleanup_on_delete: true,
    });

    let info = instance.worktree_info.as_ref().unwrap();
    assert_eq!(info.branch, "test-feature");
    assert_eq!(
        info.main_repo_path,
        repo_dir.path().to_string_lossy().to_string()
    );
    assert!(info.managed_by_aoe);
    assert_eq!(info.created_at, now);
    assert!(info.cleanup_on_delete);
}

#[test]
fn test_worktree_info_persists_across_save_load() {
    let temp_home = TempDir::new().unwrap();
    std::env::set_var("HOME", temp_home.path());

    let storage = Storage::new("worktree-test-profile").unwrap();

    let mut instance = Instance::new("Worktree Session", "/tmp/test");
    instance.worktree_info = Some(WorktreeInfo {
        branch: "feature-branch".to_string(),
        main_repo_path: "/original/repo".to_string(),
        managed_by_aoe: true,
        created_at: Utc::now(),
        cleanup_on_delete: false,
    });

    storage.save(&[instance.clone()]).unwrap();

    let loaded = storage.load().unwrap();
    assert_eq!(loaded.len(), 1);

    let loaded_info = loaded[0].worktree_info.as_ref().unwrap();
    assert_eq!(loaded_info.branch, "feature-branch");
    assert_eq!(loaded_info.main_repo_path, "/original/repo");
    assert!(loaded_info.managed_by_aoe);
    assert!(!loaded_info.cleanup_on_delete);
}

#[test]
fn test_session_without_worktree_has_none_worktree_info() {
    let instance = Instance::new("Regular Session", "/tmp/project");

    assert!(instance.worktree_info.is_none());
}

#[test]
fn test_manual_worktree_detection() {
    let (repo_dir, _repo, _config_dir) = setup_test_environment();

    let git_wt = GitWorktree::new(repo_dir.path().to_path_buf()).unwrap();
    let wt_path = repo_dir.path().join("detected-worktree");

    git_wt
        .create_worktree("test-feature", &wt_path, false)
        .unwrap();

    let worktrees = git_wt.list_worktrees().unwrap();

    assert!(worktrees.len() >= 2);

    let main_wt = worktrees.iter().find(|w| w.path == repo_dir.path());
    assert!(main_wt.is_some());

    let added_wt = worktrees.iter().find(|w| {
        w.branch
            .as_ref()
            .map(|b| b == "test-feature")
            .unwrap_or(false)
    });
    assert!(added_wt.is_some());
}

#[test]
fn test_worktree_cleanup_on_session_removal() {
    let (repo_dir, _repo, _config_dir) = setup_test_environment();
    let worktree_container = TempDir::new().unwrap();

    let git_wt = GitWorktree::new(repo_dir.path().to_path_buf()).unwrap();
    let wt_path = worktree_container.path().join("cleanup-worktree");

    git_wt
        .create_worktree("test-feature", &wt_path, false)
        .unwrap();
    assert!(wt_path.exists());

    let mut instance = Instance::new("Cleanup Session", wt_path.to_str().unwrap());
    instance.worktree_info = Some(WorktreeInfo {
        branch: "test-feature".to_string(),
        main_repo_path: repo_dir.path().to_string_lossy().to_string(),
        managed_by_aoe: true,
        created_at: Utc::now(),
        cleanup_on_delete: true,
    });

    git_wt.remove_worktree(&wt_path).unwrap();

    assert!(!wt_path.exists());
}

#[test]
fn test_worktree_preserved_when_keep_flag_used() {
    let (repo_dir, _repo, _config_dir) = setup_test_environment();
    let worktree_container = TempDir::new().unwrap();

    let git_wt = GitWorktree::new(repo_dir.path().to_path_buf()).unwrap();
    let wt_path = worktree_container.path().join("keep-worktree");

    git_wt
        .create_worktree("test-feature", &wt_path, false)
        .unwrap();
    assert!(wt_path.exists());

    let mut instance = Instance::new("Keep Session", wt_path.to_str().unwrap());
    instance.worktree_info = Some(WorktreeInfo {
        branch: "test-feature".to_string(),
        main_repo_path: repo_dir.path().to_string_lossy().to_string(),
        managed_by_aoe: true,
        created_at: Utc::now(),
        cleanup_on_delete: false,
    });

    assert!(wt_path.exists());
    assert!(!instance.worktree_info.as_ref().unwrap().cleanup_on_delete);
}

#[test]
fn test_error_when_worktree_already_exists() {
    let (repo_dir, _repo, _config_dir) = setup_test_environment();

    let git_wt = GitWorktree::new(repo_dir.path().to_path_buf()).unwrap();
    let wt_path = repo_dir.path().join("duplicate-worktree");

    git_wt
        .create_worktree("test-feature", &wt_path, false)
        .unwrap();

    let result = git_wt.create_worktree("test-feature", &wt_path, false);

    assert!(result.is_err());
    match result.unwrap_err() {
        GitError::WorktreeAlreadyExists(path) => {
            assert_eq!(path, wt_path);
        }
        other => panic!("Expected WorktreeAlreadyExists, got {:?}", other),
    }
}

#[test]
fn test_error_when_branch_does_not_exist() {
    let (repo_dir, _repo, _config_dir) = setup_test_environment();

    let git_wt = GitWorktree::new(repo_dir.path().to_path_buf()).unwrap();
    let wt_path = repo_dir.path().join("nonexistent-branch-worktree");

    let result = git_wt.create_worktree("nonexistent-branch", &wt_path, false);

    assert!(result.is_err());
    match result.unwrap_err() {
        GitError::BranchNotFound(branch) => {
            assert_eq!(branch, "nonexistent-branch");
        }
        other => panic!("Expected BranchNotFound, got {:?}", other),
    }
}

#[test]
fn test_create_new_branch_with_b_flag() {
    let (repo_dir, repo, _config_dir) = setup_test_environment();

    let git_wt = GitWorktree::new(repo_dir.path().to_path_buf()).unwrap();
    let wt_path = repo_dir.path().join("new-branch-worktree");

    let branch_exists_before = repo
        .find_branch("brand-new-branch", git2::BranchType::Local)
        .is_ok();
    assert!(!branch_exists_before);

    git_wt
        .create_worktree("brand-new-branch", &wt_path, true)
        .unwrap();

    assert!(wt_path.exists());

    let branch_exists_after = repo
        .find_branch("brand-new-branch", git2::BranchType::Local)
        .is_ok();
    assert!(branch_exists_after);
}
