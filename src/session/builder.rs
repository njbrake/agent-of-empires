//! Instance creation and cleanup utilities.
//!
//! This module provides shared logic for building new session instances,
//! used by both synchronous (TUI operations) and asynchronous (background poller) code paths.

use std::path::PathBuf;

use anyhow::{bail, Result};
use chrono::Utc;

use crate::docker::DockerContainer;
use crate::git::GitWorktree;

use super::{civilizations, Config, Instance, SandboxInfo, WorktreeInfo};

/// Parameters for creating a new session instance.
#[derive(Debug, Clone)]
pub struct InstanceParams {
    pub title: String,
    pub path: String,
    pub group: String,
    pub tool: String,
    pub worktree_branch: Option<String>,
    pub create_new_branch: bool,
    pub sandbox: bool,
    /// The sandbox image to use. Required when sandbox is true.
    pub sandbox_image: String,
    pub yolo_mode: bool,
    /// Additional environment variable keys to pass from host to container.
    pub extra_env_keys: Vec<String>,
}

/// Result of building an instance, tracking what was created for cleanup purposes.
pub struct BuildResult {
    pub instance: Instance,
    /// Path to worktree if one was created and managed by aoe
    pub created_worktree: Option<CreatedWorktree>,
}

/// Info about a worktree created during instance building.
pub struct CreatedWorktree {
    pub path: PathBuf,
    pub main_repo_path: PathBuf,
}

/// Build an instance with all setup (worktree resolution, sandbox config).
///
/// This does NOT start the instance or create Docker containers - that happens
/// separately via `instance.start()`. This separation allows for proper cleanup
/// if starting fails.
pub fn build_instance(params: InstanceParams, existing_titles: &[&str]) -> Result<BuildResult> {
    if params.sandbox {
        if !crate::docker::is_docker_available() {
            bail!("Docker is not installed. Please install Docker to use sandbox mode.");
        }
        if !crate::docker::is_daemon_running() {
            bail!("Docker daemon is not running. Please start Docker to use sandbox mode.");
        }
    }

    let mut final_path = PathBuf::from(&params.path)
        .canonicalize()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| params.path.clone());

    let mut worktree_info = None;
    let mut created_worktree = None;

    if let Some(branch) = &params.worktree_branch {
        let path = PathBuf::from(&params.path);

        if !GitWorktree::is_git_repo(&path) {
            bail!("Path is not in a git repository");
        }

        let config = Config::load()?;
        let main_repo_path = GitWorktree::find_main_repo(&path)?;
        let git_wt = GitWorktree::new(main_repo_path.clone())?;

        // Choose appropriate template based on repo type (bare vs regular)
        // Use main_repo_path (not path) to correctly detect bare repos when running from a worktree
        let is_bare = GitWorktree::is_bare_repo(&main_repo_path);
        let template = if is_bare {
            &config.worktree.bare_repo_path_template
        } else {
            &config.worktree.path_template
        };

        if !params.create_new_branch {
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
                let worktree_path = git_wt.compute_path(branch, template, &session_id[..8])?;

                git_wt.create_worktree(branch, &worktree_path, false)?;

                final_path = worktree_path.to_string_lossy().to_string();
                created_worktree = Some(CreatedWorktree {
                    path: worktree_path,
                    main_repo_path: main_repo_path.clone(),
                });
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
            let worktree_path = git_wt.compute_path(branch, template, &session_id[..8])?;

            if worktree_path.exists() {
                bail!("Worktree already exists at {}", worktree_path.display());
            }

            git_wt.create_worktree(branch, &worktree_path, true)?;

            final_path = worktree_path.to_string_lossy().to_string();
            created_worktree = Some(CreatedWorktree {
                path: worktree_path,
                main_repo_path: main_repo_path.clone(),
            });
            worktree_info = Some(WorktreeInfo {
                branch: branch.clone(),
                main_repo_path: main_repo_path.to_string_lossy().to_string(),
                managed_by_aoe: true,
                created_at: Utc::now(),
                cleanup_on_delete: true,
            });
        }
    }

    let final_title = if params.title.is_empty() {
        civilizations::generate_random_title(existing_titles)
    } else {
        params.title.clone()
    };

    let mut instance = Instance::new(&final_title, &final_path);
    instance.group_path = params.group;
    instance.tool = params.tool.clone();
    instance.command = match params.tool.as_str() {
        "opencode" => "opencode".to_string(),
        "codex" => "codex".to_string(),
        _ => String::new(),
    };
    instance.worktree_info = worktree_info;

    if params.sandbox {
        instance.sandbox_info = Some(SandboxInfo {
            enabled: true,
            container_id: None,
            image: params.sandbox_image.clone(),
            container_name: DockerContainer::generate_name(&instance.id),
            created_at: None,
            yolo_mode: if params.yolo_mode { Some(true) } else { None },
            extra_env_keys: if params.extra_env_keys.is_empty() {
                None
            } else {
                Some(params.extra_env_keys.clone())
            },
        });
    }

    Ok(BuildResult {
        instance,
        created_worktree,
    })
}

/// Clean up resources created during a failed or cancelled instance build.
///
/// This handles:
/// - Removing worktrees created by aoe
/// - Removing Docker containers
/// - Killing tmux sessions
pub fn cleanup_instance(instance: &Instance, created_worktree: Option<&CreatedWorktree>) {
    if let Some(wt) = created_worktree {
        if let Ok(git_wt) = GitWorktree::new(wt.main_repo_path.clone()) {
            if let Err(e) = git_wt.remove_worktree(&wt.path) {
                tracing::warn!("Failed to clean up worktree: {}", e);
            }
        }
    }

    if let Some(sandbox) = &instance.sandbox_info {
        if sandbox.enabled {
            let container = DockerContainer::from_session_id(&instance.id);
            if container.exists().unwrap_or(false) {
                if let Err(e) = container.remove(true) {
                    tracing::warn!("Failed to clean up container: {}", e);
                }
            }
        }
    }

    let _ = instance.kill();
}
