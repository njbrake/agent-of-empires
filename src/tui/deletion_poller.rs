//! Background deletion handler for TUI responsiveness

use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

use crate::docker::DockerContainer;
use crate::git::GitWorktree;
use crate::session::Instance;

pub struct DeletionRequest {
    pub session_id: String,
    pub instance: Instance,
    pub delete_worktree: bool,
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

        // Worktree cleanup (if user opted to delete it)
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
