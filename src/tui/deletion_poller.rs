//! Background deletion handler for TUI responsiveness

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;

use crate::containers::DockerContainer;
use crate::git::GitWorktree;
use crate::session::Instance;

pub struct DeletionRequest {
    pub session_id: String,
    pub instance: Instance,
    pub delete_worktree: bool,
    pub delete_branch: bool,
    pub delete_sandbox: bool,
    pub force_delete: bool,
}

#[derive(Debug)]
pub struct DeletionResult {
    pub session_id: String,
    pub success: bool,
    pub error: Option<String>,
}

pub struct DeletionPoller {
    request_tx: mpsc::Sender<DeletionRequest>,
    result_rx: mpsc::Receiver<DeletionResult>,
    _handle: thread::JoinHandle<()>,
}

impl DeletionPoller {
    pub fn new() -> Self {
        let (request_tx, request_rx) = mpsc::channel::<DeletionRequest>();
        let (result_tx, result_rx) = mpsc::channel::<DeletionResult>();

        let handle = thread::spawn(move || {
            Self::deletion_loop(request_rx, result_tx);
        });

        Self {
            request_tx,
            result_rx,
            _handle: handle,
        }
    }

    fn deletion_loop(
        request_rx: mpsc::Receiver<DeletionRequest>,
        result_tx: mpsc::Sender<DeletionResult>,
    ) {
        while let Ok(request) = request_rx.recv() {
            let result = Self::perform_deletion(&request);
            if result_tx.send(result).is_err() {
                break;
            }
        }
    }

    fn perform_deletion(request: &DeletionRequest) -> DeletionResult {
        let mut errors = Vec::new();

        // Track branch info for potential deletion after worktree removal
        let branch_to_delete = if request.delete_branch {
            request
                .instance
                .worktree_info
                .as_ref()
                .filter(|wt| wt.managed_by_aoe)
                .map(|wt| (wt.branch.clone(), PathBuf::from(&wt.main_repo_path)))
        } else {
            None
        };

        // Worktree cleanup (if user opted to delete it)
        // Must happen before branch deletion since the worktree is using the branch
        if request.delete_worktree {
            if let Some(wt_info) = &request.instance.worktree_info {
                if wt_info.managed_by_aoe {
                    let worktree_path = PathBuf::from(&request.instance.project_path);
                    let main_repo = PathBuf::from(&wt_info.main_repo_path);

                    match GitWorktree::new(main_repo.clone()) {
                        Ok(git_wt) => {
                            let has_dot_git = worktree_path.join(".git").exists();
                            tracing::debug!(
                                path = %worktree_path.display(),
                                has_dot_git,
                                is_sandboxed = request.instance.is_sandboxed(),
                                force = request.force_delete,
                                "worktree cleanup starting"
                            );

                            if !has_dot_git {
                                // .git is missing (manual deletion or other
                                // issue). Remove the dir ourselves and prune.
                                if let Err(e) = remove_worktree_dir(
                                    &worktree_path,
                                    &main_repo,
                                    request.force_delete,
                                ) {
                                    tracing::debug!(
                                        error = %e,
                                        kind = ?e.kind(),
                                        "remove_worktree_dir failed (no .git path)"
                                    );
                                    if request.instance.is_sandboxed()
                                        && is_permission_error(&e.to_string())
                                    {
                                        let cleaned = cleanup_sandbox_worktree(&request.instance);
                                        tracing::debug!(cleaned, "container cleanup attempted");
                                        if cleaned {
                                            let container = DockerContainer::from_session_id(
                                                &request.instance.id,
                                            );
                                            let rm_result = container.remove(true);
                                            tracing::debug!(?rm_result, "container force-removed");
                                            if let Err(e2) = remove_worktree_dir(
                                                &worktree_path,
                                                &main_repo,
                                                true,
                                            ) {
                                                errors.push(format!("Worktree: {}", e2));
                                            }
                                        } else {
                                            errors.push(format!("Worktree: {}", e));
                                        }
                                    } else {
                                        errors.push(format!("Worktree: {}", e));
                                    }
                                }
                                if let Err(e) = git_wt.prune_worktrees() {
                                    errors.push(format!("Worktree: {}", e));
                                }
                            } else {
                                let result =
                                    git_wt.remove_worktree(&worktree_path, request.force_delete);
                                if let Err(e) = result {
                                    let err_str = e.to_string();
                                    tracing::debug!(
                                        error = %err_str,
                                        is_perm = is_permission_error(&err_str),
                                        "git worktree remove failed"
                                    );
                                    if request.instance.is_sandboxed()
                                        && is_permission_error(&err_str)
                                    {
                                        let cleaned = cleanup_sandbox_worktree(&request.instance);
                                        tracing::debug!(cleaned, "container cleanup attempted");
                                        if cleaned {
                                            let container = DockerContainer::from_session_id(
                                                &request.instance.id,
                                            );
                                            let rm_result = container.remove(true);
                                            tracing::debug!(?rm_result, "container force-removed");
                                            if let Err(e2) = remove_worktree_dir(
                                                &worktree_path,
                                                &main_repo,
                                                true,
                                            ) {
                                                tracing::debug!(
                                                    error = %e2,
                                                    kind = ?e2.kind(),
                                                    "remove_worktree_dir failed after cleanup"
                                                );
                                                errors.push(format!("Worktree: {}", e2));
                                            }
                                            if let Err(e2) = git_wt.prune_worktrees() {
                                                errors.push(format!("Worktree: {}", e2));
                                            }
                                        } else {
                                            errors.push(format!("Worktree: {}", e));
                                        }
                                    } else {
                                        errors.push(format!("Worktree: {}", e));
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            errors.push(format!("Worktree: {}", e));
                        }
                    }
                }
            }
        }

        // Branch cleanup (if user opted to delete it and worktree was successfully removed)
        if let Some((branch, main_repo)) = branch_to_delete {
            let worktree_ok =
                !request.delete_worktree || !errors.iter().any(|e| e.starts_with("Worktree:"));
            if worktree_ok {
                match GitWorktree::new(main_repo) {
                    Ok(git_wt) => {
                        if let Err(e) = git_wt.delete_branch(&branch) {
                            errors.push(format!("Branch: {}", e));
                        }
                    }
                    Err(e) => {
                        errors.push(format!("Branch: {}", e));
                    }
                }
            }
        }

        // Container cleanup (if user opted to delete it)
        if request.delete_sandbox {
            if let Some(sandbox) = &request.instance.sandbox_info {
                if sandbox.enabled {
                    let container = DockerContainer::from_session_id(&request.instance.id);
                    if container.exists().unwrap_or(false) {
                        if let Err(e) = container.remove(true) {
                            errors.push(format!("Container: {}", e));
                        }
                    }
                }
            }
        }

        // Tmux kill - non-fatal if session already gone
        let _ = request.instance.kill();

        // Kill paired terminal session if it exists
        let _ = request.instance.kill_terminal();

        // Clean up hook status files
        crate::hooks::cleanup_hook_status_dir(&request.instance.id);

        DeletionResult {
            session_id: request.session_id.clone(),
            success: errors.is_empty(),
            error: if errors.is_empty() {
                None
            } else {
                Some(errors.join("; "))
            },
        }
    }

    pub fn request_deletion(&self, request: DeletionRequest) {
        let _ = self.request_tx.send(request);
    }

    pub fn try_recv_result(&self) -> Option<DeletionResult> {
        self.result_rx.try_recv().ok()
    }
}

impl Default for DeletionPoller {
    fn default() -> Self {
        Self::new()
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
fn remove_worktree_dir(worktree_path: &Path, main_repo: &Path, force: bool) -> std::io::Result<()> {
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

    // Final attempt — return the error
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
fn is_permission_error(error: &str) -> bool {
    let lower = error.to_lowercase();
    lower.contains("permission denied")
        || lower.contains("operation not permitted")
        || lower.contains("access is denied")
}

/// Delete worktree contents from inside the sandbox container.
/// Returns true if the container successfully deleted the contents.
fn cleanup_sandbox_worktree(instance: &Instance) -> bool {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn create_test_instance() -> Instance {
        Instance::new("Test Session", "/tmp/test-project")
    }

    #[test]
    fn test_deletion_result_success_when_no_worktree_or_sandbox() {
        let instance = create_test_instance();
        let request = DeletionRequest {
            session_id: instance.id.clone(),
            instance,
            delete_worktree: false,
            delete_branch: false,
            delete_sandbox: false,
            force_delete: false,
        };

        let result = DeletionPoller::perform_deletion(&request);

        assert!(result.success);
        assert!(result.error.is_none());
        assert_eq!(result.session_id, request.session_id);
    }

    #[test]
    fn test_deletion_result_success_even_with_delete_worktree_flag_when_no_worktree() {
        let instance = create_test_instance();
        let request = DeletionRequest {
            session_id: instance.id.clone(),
            instance,
            delete_worktree: true,
            delete_branch: false,
            delete_sandbox: false,
            force_delete: false,
        };

        let result = DeletionPoller::perform_deletion(&request);

        assert!(result.success);
        assert!(result.error.is_none());
    }

    #[test]
    fn test_deletion_poller_channel_communication() {
        let poller = DeletionPoller::new();
        let instance = create_test_instance();
        let session_id = instance.id.clone();

        poller.request_deletion(DeletionRequest {
            session_id: session_id.clone(),
            instance,
            delete_worktree: false,
            delete_branch: false,
            delete_sandbox: false,
            force_delete: false,
        });

        let mut result = None;
        for _ in 0..50 {
            result = poller.try_recv_result();
            if result.is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(result.is_some(), "Timed out waiting for deletion result");

        let result = result.unwrap();
        assert_eq!(result.session_id, session_id);
        assert!(result.success);
    }

    #[test]
    fn test_deletion_poller_try_recv_returns_none_when_empty() {
        let poller = DeletionPoller::new();
        assert!(poller.try_recv_result().is_none());
    }

    #[test]
    fn test_deletion_request_preserves_session_id() {
        let instance = create_test_instance();
        let custom_id = "custom-session-id-123".to_string();

        let request = DeletionRequest {
            session_id: custom_id.clone(),
            instance,
            delete_worktree: false,
            delete_branch: false,
            delete_sandbox: false,
            force_delete: false,
        };

        let result = DeletionPoller::perform_deletion(&request);
        assert_eq!(result.session_id, custom_id);
    }
}
