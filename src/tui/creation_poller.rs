//! Background session creation handler for TUI responsiveness
//!
//! This handles the potentially slow Docker operations (image pull, container creation)
//! in a background thread so the UI remains responsive.

use std::sync::mpsc;
use std::thread;

use crate::session::builder::{self, CreatedWorktree, InstanceParams};
use crate::session::repo_config::{self, HookProgress, HooksConfig};
use crate::session::Instance;
use crate::tui::dialogs::NewSessionData;

pub struct CreationRequest {
    pub data: NewSessionData,
    /// Existing instances, used for generating unique titles
    pub existing_instances: Vec<Instance>,
    /// Trusted hooks to execute after instance creation (already approved by user).
    pub hooks: Option<HooksConfig>,
}

#[derive(Debug)]
pub enum CreationResult {
    Success {
        session_id: String,
        instance: Box<Instance>,
        /// Worktree created during build, needed for cleanup if cancelled
        created_worktree: Option<CreatedWorktreeInfo>,
        /// Whether on_launch hooks were already executed in the background
        on_launch_hooks_ran: bool,
    },
    Error(String),
}

/// Serializable worktree info for passing across thread boundary
#[derive(Debug, Clone)]
pub struct CreatedWorktreeInfo {
    pub path: String,
    pub main_repo_path: String,
}

impl From<&CreatedWorktree> for CreatedWorktreeInfo {
    fn from(wt: &CreatedWorktree) -> Self {
        Self {
            path: wt.path.to_string_lossy().to_string(),
            main_repo_path: wt.main_repo_path.to_string_lossy().to_string(),
        }
    }
}

pub struct CreationPoller {
    request_tx: mpsc::Sender<(CreationRequest, mpsc::Sender<HookProgress>)>,
    result_rx: mpsc::Receiver<CreationResult>,
    progress_rx: mpsc::Receiver<HookProgress>,
    progress_tx: mpsc::Sender<HookProgress>,
    _handle: thread::JoinHandle<()>,
    pending: bool,
}

impl CreationPoller {
    pub fn new() -> Self {
        let (request_tx, request_rx) =
            mpsc::channel::<(CreationRequest, mpsc::Sender<HookProgress>)>();
        let (result_tx, result_rx) = mpsc::channel::<CreationResult>();
        let (progress_tx, progress_rx) = mpsc::channel::<HookProgress>();

        let handle = thread::spawn(move || {
            while let Ok((request, prog_tx)) = request_rx.recv() {
                let result = Self::create_instance(request, &prog_tx);
                if result_tx.send(result).is_err() {
                    break;
                }
            }
        });

        Self {
            request_tx,
            result_rx,
            progress_rx,
            progress_tx,
            _handle: handle,
            pending: false,
        }
    }

    fn create_instance(
        request: CreationRequest,
        progress_tx: &mpsc::Sender<HookProgress>,
    ) -> CreationResult {
        let data = request.data;
        let hooks = request.hooks;

        let existing_titles: Vec<&str> = request
            .existing_instances
            .iter()
            .map(|i| i.title.as_str())
            .collect();

        let params = InstanceParams {
            title: data.title,
            path: data.path.clone(),
            group: data.group,
            tool: data.tool,
            worktree_branch: data.worktree_branch,
            create_new_branch: data.create_new_branch,
            sandbox: data.sandbox,
            sandbox_image: data.sandbox_image,
            yolo_mode: data.yolo_mode,
            extra_env_keys: data.extra_env_keys,
            extra_env_values: data.extra_env_values,
        };

        let build_result = match builder::build_instance(params, &existing_titles) {
            Ok(r) => r,
            Err(e) => return CreationResult::Error(e.to_string()),
        };

        let mut instance = build_result.instance;
        let created_worktree = build_result.created_worktree;

        let has_on_create = hooks.as_ref().is_some_and(|h| !h.on_create.is_empty());
        let has_on_launch = hooks.as_ref().is_some_and(|h| !h.on_launch.is_empty());

        // Execute on_create hooks after worktree setup, before starting
        if has_on_create {
            let hooks = hooks.as_ref().unwrap();
            if data.sandbox {
                // Ensure the container is running so we can exec hooks inside it.
                // Don't create the tmux session yet -- that happens at attach time
                // where the terminal size is available.
                if let Err(e) = instance.get_container_for_instance() {
                    builder::cleanup_instance(&instance, created_worktree.as_ref());
                    return CreationResult::Error(e.to_string());
                }
                if let Some(ref sandbox) = instance.sandbox_info {
                    let workdir = instance.container_workdir();
                    if let Err(e) = repo_config::execute_hooks_in_container_streamed(
                        &hooks.on_create,
                        &sandbox.container_name,
                        &workdir,
                        progress_tx,
                    ) {
                        tracing::warn!("on_create hook failed in container: {}", e);
                        return CreationResult::Error(format!("on_create hook failed: {}", e));
                    }
                }
            } else if let Err(e) = repo_config::execute_hooks_streamed(
                &hooks.on_create,
                std::path::Path::new(&instance.project_path),
                progress_tx,
            ) {
                builder::cleanup_instance(&instance, created_worktree.as_ref());
                return CreationResult::Error(format!("on_create hook failed: {}", e));
            }
        }

        // Execute on_launch hooks in background too (non-fatal, like start_with_size).
        // This prevents blocking the UI thread when the session is first attached.
        if has_on_launch {
            let hooks = hooks.as_ref().unwrap();
            if data.sandbox {
                // For sandbox, ensure container is running (may already be from on_create above).
                if instance.get_container_for_instance().is_ok() {
                    if let Some(ref sandbox) = instance.sandbox_info {
                        let workdir = instance.container_workdir();
                        if let Err(e) = repo_config::execute_hooks_in_container_streamed(
                            &hooks.on_launch,
                            &sandbox.container_name,
                            &workdir,
                            progress_tx,
                        ) {
                            tracing::warn!("on_launch hook failed in container: {}", e);
                        }
                    }
                } else {
                    tracing::warn!("Skipping on_launch hooks: container not running");
                }
            } else if let Err(e) = repo_config::execute_hooks_streamed(
                &hooks.on_launch,
                std::path::Path::new(&instance.project_path),
                progress_tx,
            ) {
                tracing::warn!("on_launch hook failed: {}", e);
            }
        }

        if data.sandbox {
            // Only ensure the Docker container is running here. Don't create the tmux
            // session yet -- that happens at attach time where the terminal size is
            // available, avoiding a race that creates the session at 80x24 default.
            if let Err(e) = instance.get_container_for_instance() {
                builder::cleanup_instance(&instance, created_worktree.as_ref());
                return CreationResult::Error(e.to_string());
            }
        }

        let created_worktree_info = created_worktree.as_ref().map(CreatedWorktreeInfo::from);

        CreationResult::Success {
            session_id: instance.id.clone(),
            instance: Box::new(instance),
            created_worktree: created_worktree_info,
            on_launch_hooks_ran: has_on_launch,
        }
    }

    pub fn request_creation(&mut self, request: CreationRequest) {
        self.pending = true;
        if self
            .request_tx
            .send((request, self.progress_tx.clone()))
            .is_err()
        {
            tracing::error!("Failed to send creation request: receiver thread died");
            self.pending = false;
        }
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

    pub fn try_recv_progress(&self) -> Option<HookProgress> {
        self.progress_rx.try_recv().ok()
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
