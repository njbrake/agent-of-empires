//! Session operations for HomeView (create, delete, rename)

use crate::session::{flatten_tree, GroupTree, Instance, Status};
use crate::tui::deletion_poller::DeletionRequest;
use crate::tui::dialogs::{DeleteOptions, GroupDeleteOptions, NewSessionData};

use super::HomeView;

impl HomeView {
    pub(super) fn create_session(&mut self, data: NewSessionData) -> anyhow::Result<String> {
        use crate::git::GitWorktree;
        use crate::session::{Config, WorktreeInfo};
        use chrono::Utc;
        use std::path::PathBuf;

        if data.sandbox {
            if !crate::docker::is_docker_available() {
                anyhow::bail!(
                    "Docker is not installed. Please install Docker to use sandbox mode."
                );
            }
            if !crate::docker::is_daemon_running() {
                anyhow::bail!(
                    "Docker daemon is not running. Please start Docker to use sandbox mode."
                );
            }
        }

        let mut final_path = PathBuf::from(&data.path)
            .canonicalize()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| data.path.clone());
        let mut worktree_info_opt = None;

        if let Some(branch) = &data.worktree_branch {
            let path = PathBuf::from(&data.path);

            if !GitWorktree::is_git_repo(&path) {
                anyhow::bail!("Path is not in a git repository");
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
                    worktree_info_opt = Some(WorktreeInfo {
                        branch: branch.clone(),
                        main_repo_path: main_repo_path.to_string_lossy().to_string(),
                        managed_by_aoe: false,
                        created_at: Utc::now(),
                        cleanup_on_delete: false,
                    });
                } else {
                    let session_id = uuid::Uuid::new_v4().to_string();
                    let session_id_short = &session_id[..8];
                    let template = &config.worktree.path_template;
                    let worktree_path = git_wt.compute_path(branch, template, session_id_short)?;

                    git_wt.create_worktree(branch, &worktree_path, false)?;

                    final_path = worktree_path.to_string_lossy().to_string();
                    worktree_info_opt = Some(WorktreeInfo {
                        branch: branch.clone(),
                        main_repo_path: main_repo_path.to_string_lossy().to_string(),
                        managed_by_aoe: true,
                        created_at: Utc::now(),
                        cleanup_on_delete: true,
                    });
                }
            } else {
                let session_id = uuid::Uuid::new_v4().to_string();
                let session_id_short = &session_id[..8];
                let template = &config.worktree.path_template;
                let worktree_path = git_wt.compute_path(branch, template, session_id_short)?;

                if worktree_path.exists() {
                    anyhow::bail!("Worktree already exists at {}", worktree_path.display());
                }

                git_wt.create_worktree(branch, &worktree_path, true)?;

                final_path = worktree_path.to_string_lossy().to_string();
                worktree_info_opt = Some(WorktreeInfo {
                    branch: branch.clone(),
                    main_repo_path: main_repo_path.to_string_lossy().to_string(),
                    managed_by_aoe: true,
                    created_at: Utc::now(),
                    cleanup_on_delete: true,
                });
            }
        }

        let mut instance = Instance::new(&data.title, &final_path);
        instance.group_path = data.group;
        instance.tool = data.tool.clone();
        instance.command = if data.tool == "opencode" {
            "opencode".to_string()
        } else {
            String::new()
        };

        if let Some(worktree_info) = worktree_info_opt {
            instance.worktree_info = Some(worktree_info);
        }

        if data.sandbox {
            use crate::docker::DockerContainer;
            use crate::session::SandboxInfo;

            let container_name = DockerContainer::generate_name(&instance.id);
            instance.sandbox_info = Some(SandboxInfo {
                enabled: true,
                container_id: None,
                image: data.sandbox_image,
                container_name,
                created_at: None,
                yolo_mode: if data.yolo_mode { Some(true) } else { None },
            });
        }

        let session_id = instance.id.clone();
        self.instances.push(instance.clone());
        self.group_tree = GroupTree::new_with_groups(&self.instances, &self.groups);
        if !instance.group_path.is_empty() {
            self.group_tree.create_group(&instance.group_path);
        }
        self.storage
            .save_with_groups(&self.instances, &self.group_tree)?;

        self.reload()?;
        Ok(session_id)
    }

    pub(super) fn delete_selected(&mut self, options: &DeleteOptions) -> anyhow::Result<()> {
        if let Some(id) = &self.selected_session {
            let id = id.clone();

            if let Some(inst) = self.instance_map.get_mut(&id) {
                inst.status = Status::Deleting;
            }
            if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
                inst.status = Status::Deleting;
            }

            if let Some(inst) = self.instance_map.get(&id) {
                let request = DeletionRequest {
                    session_id: id.clone(),
                    instance: inst.clone(),
                    delete_worktree: options.delete_worktree,
                    delete_sandbox: options.delete_sandbox,
                };
                self.deletion_poller.request_deletion(request);
            }
        }
        Ok(())
    }

    pub(super) fn delete_selected_group(&mut self) -> anyhow::Result<()> {
        if let Some(group_path) = self.selected_group.take() {
            let prefix = format!("{}/", group_path);
            for inst in &mut self.instances {
                if inst.group_path == group_path || inst.group_path.starts_with(&prefix) {
                    inst.group_path = String::new();
                }
            }

            self.group_tree = GroupTree::new_with_groups(&self.instances, &self.groups);
            self.group_tree.delete_group(&group_path);
            self.storage
                .save_with_groups(&self.instances, &self.group_tree)?;

            self.reload()?;
        }
        Ok(())
    }

    pub(super) fn delete_group_with_sessions(
        &mut self,
        options: &GroupDeleteOptions,
    ) -> anyhow::Result<()> {
        if let Some(group_path) = self.selected_group.take() {
            let prefix = format!("{}/", group_path);

            let sessions_to_delete: Vec<String> = self
                .instances
                .iter()
                .filter(|i| i.group_path == group_path || i.group_path.starts_with(&prefix))
                .map(|i| i.id.clone())
                .collect();

            for session_id in sessions_to_delete {
                if let Some(inst) = self.instance_map.get_mut(&session_id) {
                    inst.status = Status::Deleting;
                }
                if let Some(inst) = self.instances.iter_mut().find(|i| i.id == session_id) {
                    inst.status = Status::Deleting;
                }

                if let Some(inst) = self.instance_map.get(&session_id) {
                    let delete_worktree = options.delete_worktrees
                        && inst
                            .worktree_info
                            .as_ref()
                            .is_some_and(|wt| wt.managed_by_aoe);
                    let delete_sandbox = inst.sandbox_info.as_ref().is_some_and(|s| s.enabled);
                    let request = DeletionRequest {
                        session_id: session_id.clone(),
                        instance: inst.clone(),
                        delete_worktree,
                        delete_sandbox,
                    };
                    self.deletion_poller.request_deletion(request);
                }
            }

            self.group_tree.delete_group(&group_path);
            self.storage
                .save_with_groups(&self.instances, &self.group_tree)?;
            self.flat_items = flatten_tree(&self.group_tree, &self.instances);
        }
        Ok(())
    }

    pub(super) fn group_has_managed_worktrees(&self, group_path: &str, prefix: &str) -> bool {
        self.instances.iter().any(|i| {
            (i.group_path == group_path || i.group_path.starts_with(prefix))
                && i.worktree_info.as_ref().is_some_and(|wt| wt.managed_by_aoe)
        })
    }

    pub(super) fn rename_selected(&mut self, new_title: &str) -> anyhow::Result<()> {
        if let Some(id) = &self.selected_session {
            let id = id.clone();

            if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
                inst.title = new_title.to_string();
            }

            if let Some(inst) = self.instance_map.get(&id) {
                if inst.title != new_title {
                    let tmux_session = inst.tmux_session()?;
                    if tmux_session.exists() {
                        let new_tmux_name = crate::tmux::Session::generate_name(&id, new_title);
                        if let Err(e) = tmux_session.rename(&new_tmux_name) {
                            tracing::warn!("Failed to rename tmux session: {}", e);
                        } else {
                            crate::tmux::refresh_session_cache();
                        }
                    }
                }
            }

            self.group_tree = GroupTree::new_with_groups(&self.instances, &self.groups);
            self.storage
                .save_with_groups(&self.instances, &self.group_tree)?;

            self.reload()?;
        }
        Ok(())
    }
}
