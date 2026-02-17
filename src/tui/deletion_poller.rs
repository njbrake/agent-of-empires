//! Background deletion handler for TUI responsiveness

use std::path::PathBuf;
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

                    if let Ok(git_wt) = GitWorktree::new(main_repo) {
                        if let Err(e) = git_wt.remove_worktree(&worktree_path) {
                            errors.push(format!("Worktree: {}", e));
                        }
                    }
                }
            }
        }

        // Branch cleanup (if user opted to delete it and worktree was successfully removed)
        if let Some((branch, main_repo)) = branch_to_delete {
            // Only delete branch if worktree deletion succeeded (or wasn't requested)
            let worktree_ok =
                !request.delete_worktree || !errors.iter().any(|e| e.starts_with("Worktree:"));
            if worktree_ok {
                if let Ok(git_wt) = GitWorktree::new(main_repo) {
                    if let Err(e) = git_wt.delete_branch(&branch) {
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
        };

        let result = DeletionPoller::perform_deletion(&request);
        assert_eq!(result.session_id, custom_id);
    }
}
