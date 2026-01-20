//! Background session creation handler for TUI responsiveness
//!
//! This handles the potentially slow Docker operations (image pull, container creation)
//! in a background thread so the UI remains responsive.

use std::sync::mpsc;
use std::thread;

use anyhow::{bail, Result};

use crate::session::Instance;
use crate::tui::dialogs::NewSessionData;

pub struct CreationRequest {
    pub data: NewSessionData,
    /// Existing instances, used for generating unique titles
    pub existing_instances: Vec<Instance>,
}

#[derive(Debug)]
pub enum CreationResult {
    Success {
        session_id: String,
        instance: Box<Instance>,
    },
    Error(String),
}

pub struct CreationPoller {
    request_tx: mpsc::Sender<CreationRequest>,
    result_rx: mpsc::Receiver<CreationResult>,
    _handle: thread::JoinHandle<()>,
    pending: bool,
}

impl CreationPoller {
    pub fn new() -> Self {
        let (request_tx, request_rx) = mpsc::channel::<CreationRequest>();
        let (result_tx, result_rx) = mpsc::channel::<CreationResult>();

        let handle = thread::spawn(move || {
            while let Ok(request) = request_rx.recv() {
                let result = match Self::create_instance(request) {
                    Ok(instance) => CreationResult::Success {
                        session_id: instance.id.clone(),
                        instance: Box::new(instance),
                    },
                    Err(e) => CreationResult::Error(e.to_string()),
                };
                if result_tx.send(result).is_err() {
                    break;
                }
            }
        });

        Self {
            request_tx,
            result_rx,
            _handle: handle,
            pending: false,
        }
    }

    /// Create an instance with all setup (worktree, sandbox container, etc.)
    fn create_instance(request: CreationRequest) -> Result<Instance> {
        use crate::docker::DockerContainer;
        use crate::git::GitWorktree;
        use crate::session::{civilizations, Config, SandboxInfo, WorktreeInfo};
        use chrono::Utc;
        use std::path::PathBuf;

        let data = request.data;

        // Docker availability check
        if data.sandbox {
            if !crate::docker::is_docker_available() {
                bail!("Docker is not installed. Please install Docker to use sandbox mode.");
            }
            if !crate::docker::is_daemon_running() {
                bail!("Docker daemon is not running. Please start Docker to use sandbox mode.");
            }
        }

        let mut final_path = PathBuf::from(&data.path)
            .canonicalize()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| data.path.clone());

        let mut worktree_info = None;

        // Worktree setup
        if let Some(branch) = &data.worktree_branch {
            let path = PathBuf::from(&data.path);

            if !GitWorktree::is_git_repo(&path) {
                bail!("Path is not in a git repository");
            }

            let config = Config::load()?;
            let main_repo_path = GitWorktree::find_main_repo(&path)?;
            let git_wt = GitWorktree::new(main_repo_path.clone())?;

            if !data.create_new_branch {
                let existing_worktrees = git_wt.list_worktrees()?;
                if let Some(existing) = existing_worktrees
                    .iter()
                    .find(|wt| wt.branch.as_deref() == Some(branch))
                {
                    final_path = existing.path.to_string_lossy().to_string();
                    worktree_info = Some(WorktreeInfo {
                        branch: branch.clone(),
                        main_repo_path: main_repo_path.to_string_lossy().to_string(),
                        managed_by_aoe: false,
                        created_at: Utc::now(),
                        cleanup_on_delete: false,
                    });
                } else {
                    let session_id = uuid::Uuid::new_v4().to_string();
                    let template = &config.worktree.path_template;
                    let worktree_path = git_wt.compute_path(branch, template, &session_id[..8])?;

                    git_wt.create_worktree(branch, &worktree_path, false)?;

                    final_path = worktree_path.to_string_lossy().to_string();
                    worktree_info = Some(WorktreeInfo {
                        branch: branch.clone(),
                        main_repo_path: main_repo_path.to_string_lossy().to_string(),
                        managed_by_aoe: true,
                        created_at: Utc::now(),
                        cleanup_on_delete: true,
                    });
                }
            } else {
                let session_id = uuid::Uuid::new_v4().to_string();
                let template = &config.worktree.path_template;
                let worktree_path = git_wt.compute_path(branch, template, &session_id[..8])?;

                if worktree_path.exists() {
                    bail!("Worktree already exists at {}", worktree_path.display());
                }

                git_wt.create_worktree(branch, &worktree_path, true)?;

                final_path = worktree_path.to_string_lossy().to_string();
                worktree_info = Some(WorktreeInfo {
                    branch: branch.clone(),
                    main_repo_path: main_repo_path.to_string_lossy().to_string(),
                    managed_by_aoe: true,
                    created_at: Utc::now(),
                    cleanup_on_delete: true,
                });
            }
        }

        // Generate title if empty
        let existing_titles: Vec<&str> = request
            .existing_instances
            .iter()
            .map(|i| i.title.as_str())
            .collect();
        let final_title = if data.title.is_empty() {
            civilizations::generate_random_title(&existing_titles)
        } else {
            data.title.clone()
        };

        // Create instance
        let mut instance = Instance::new(&final_title, &final_path);
        instance.group_path = data.group;
        instance.tool = data.tool.clone();
        instance.command = if data.tool == "opencode" {
            "opencode".to_string()
        } else {
            String::new()
        };
        instance.worktree_info = worktree_info;

        // Sandbox setup - this does the slow Docker operations (image pull, container creation)
        if data.sandbox {
            instance.sandbox_info = Some(SandboxInfo {
                enabled: true,
                container_id: None,
                image: data.sandbox_image.clone(),
                container_name: DockerContainer::generate_name(&instance.id),
                created_at: None,
                yolo_mode: if data.yolo_mode { Some(true) } else { None },
            });

            // Start the session - pulls image and creates container
            instance.start()?;
        }

        Ok(instance)
    }

    pub fn request_creation(&mut self, request: CreationRequest) {
        self.pending = true;
        let _ = self.request_tx.send(request);
    }

    pub fn try_recv_result(&mut self) -> Option<CreationResult> {
        match self.result_rx.try_recv() {
            Ok(result) => {
                self.pending = false;
                Some(result)
            }
            Err(_) => None,
        }
    }

    pub fn is_pending(&self) -> bool {
        self.pending
    }
}

impl Default for CreationPoller {
    fn default() -> Self {
        Self::new()
    }
}
