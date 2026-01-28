//! Background session creation handler for TUI responsiveness
//!
//! This handles the potentially slow Docker operations (image pull, container creation)
//! in a background thread so the UI remains responsive.

use std::sync::mpsc;
use std::thread;

use crate::session::builder::{self, CreatedWorktree, InstanceParams};
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
        /// Worktree created during build, needed for cleanup if cancelled
        created_worktree: Option<CreatedWorktreeInfo>,
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
                let result = Self::create_instance(request);
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

    fn create_instance(request: CreationRequest) -> CreationResult {
        let data = request.data;

        let existing_titles: Vec<&str> = request
            .existing_instances
            .iter()
            .map(|i| i.title.as_str())
            .collect();

        let params = InstanceParams {
            title: data.title,
            path: data.path,
            group: data.group,
            tool: data.tool,
            worktree_branch: data.worktree_branch,
            create_new_branch: data.create_new_branch,
            sandbox: data.sandbox,
            sandbox_image: data.sandbox_image,
            yolo_mode: data.yolo_mode,
            extra_env_keys: data.extra_env_keys,
        };

        let build_result = match builder::build_instance(params, &existing_titles) {
            Ok(r) => r,
            Err(e) => return CreationResult::Error(e.to_string()),
        };

        let mut instance = build_result.instance;
        let created_worktree = build_result.created_worktree;

        if data.sandbox {
            if let Err(e) = instance.start() {
                builder::cleanup_instance(&instance, created_worktree.as_ref());
                return CreationResult::Error(e.to_string());
            }
        }

        let created_worktree_info = created_worktree.as_ref().map(CreatedWorktreeInfo::from);

        CreationResult::Success {
            session_id: instance.id.clone(),
            instance: Box::new(instance),
            created_worktree: created_worktree_info,
        }
    }

    pub fn request_creation(&mut self, request: CreationRequest) {
        self.pending = true;
        if self.request_tx.send(request).is_err() {
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

    pub fn is_pending(&self) -> bool {
        self.pending
    }
}

impl Default for CreationPoller {
    fn default() -> Self {
        Self::new()
    }
}
