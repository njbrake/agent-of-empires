//! Session operations for HomeView (create, delete, rename)

use crate::session::builder::{self, InstanceParams};
use crate::session::{flatten_tree, list_profiles, GroupTree, Status, Storage};
use crate::tui::deletion_poller::DeletionRequest;
use crate::tui::dialogs::{DeleteOptions, GroupDeleteOptions, NewSessionData};

use super::HomeView;

impl HomeView {
    pub(super) fn create_session(&mut self, data: NewSessionData) -> anyhow::Result<String> {
        let existing_titles: Vec<&str> = self.instances.iter().map(|i| i.title.as_str()).collect();

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

        let build_result = builder::build_instance(params, &existing_titles)?;
        let instance = build_result.instance;

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
                    delete_branch: options.delete_branch,
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
                // Clear group_path when marking for deletion so these instances
                // won't cause the group to be recreated during tree rebuilds
                if let Some(inst) = self.instance_map.get_mut(&session_id) {
                    inst.status = Status::Deleting;
                    inst.group_path = String::new();
                }
                if let Some(inst) = self.instances.iter_mut().find(|i| i.id == session_id) {
                    inst.status = Status::Deleting;
                    inst.group_path = String::new();
                }

                if let Some(inst) = self.instance_map.get(&session_id) {
                    let delete_worktree = options.delete_worktrees
                        && inst
                            .worktree_info
                            .as_ref()
                            .is_some_and(|wt| wt.managed_by_aoe);
                    let delete_branch = options.delete_branches
                        && inst
                            .worktree_info
                            .as_ref()
                            .is_some_and(|wt| wt.managed_by_aoe);
                    let delete_sandbox = options.delete_containers
                        && inst.sandbox_info.as_ref().is_some_and(|s| s.enabled);
                    let request = DeletionRequest {
                        session_id: session_id.clone(),
                        instance: inst.clone(),
                        delete_worktree,
                        delete_branch,
                        delete_sandbox,
                    };
                    self.deletion_poller.request_deletion(request);
                }
            }

            self.group_tree.delete_group(&group_path);
            self.groups = self.group_tree.get_all_groups();
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

    pub(super) fn group_has_containers(&self, group_path: &str, prefix: &str) -> bool {
        self.instances.iter().any(|i| {
            (i.group_path == group_path || i.group_path.starts_with(prefix))
                && i.sandbox_info.as_ref().is_some_and(|s| s.enabled)
        })
    }

    pub(super) fn rename_selected(
        &mut self,
        new_title: &str,
        new_group: Option<&str>,
        new_profile: Option<&str>,
    ) -> anyhow::Result<()> {
        if let Some(id) = &self.selected_session {
            let id = id.clone();

            // Get current values for comparison
            let (current_title, current_group) = self
                .instance_map
                .get(&id)
                .map(|i| (i.title.clone(), i.group_path.clone()))
                .unwrap_or_default();

            // Determine effective title (keep current if empty)
            let effective_title = if new_title.is_empty() {
                current_title.clone()
            } else {
                new_title.to_string()
            };

            // Determine effective group
            let effective_group = match new_group {
                None => current_group.clone(), // Keep current
                Some(g) => g.to_string(),      // Set new (empty string means ungroup)
            };

            // Handle profile change (move session to different profile)
            if let Some(target_profile) = new_profile {
                let current_profile = self.storage.profile();
                if target_profile != current_profile {
                    // Validate target profile exists
                    let profiles = list_profiles()?;
                    if !profiles.contains(&target_profile.to_string()) {
                        anyhow::bail!("Profile '{}' does not exist", target_profile);
                    }

                    // Get the instance to move
                    let mut instance = self
                        .instances
                        .iter()
                        .find(|i| i.id == id)
                        .cloned()
                        .ok_or_else(|| anyhow::anyhow!("Session not found"))?;

                    // Apply title and group changes to the instance
                    instance.title = effective_title.clone();
                    instance.group_path = effective_group.clone();

                    // Handle tmux rename if title changed
                    if let Some(orig_inst) = self.instance_map.get(&id) {
                        if orig_inst.title != effective_title {
                            let tmux_session = orig_inst.tmux_session()?;
                            if tmux_session.exists() {
                                let new_tmux_name =
                                    crate::tmux::Session::generate_name(&id, &effective_title);
                                if let Err(e) = tmux_session.rename(&new_tmux_name) {
                                    tracing::warn!("Failed to rename tmux session: {}", e);
                                } else {
                                    crate::tmux::refresh_session_cache();
                                }
                            }
                        }
                    }

                    // Remove from current profile
                    self.instances.retain(|i| i.id != id);
                    self.group_tree = GroupTree::new_with_groups(&self.instances, &self.groups);
                    self.storage
                        .save_with_groups(&self.instances, &self.group_tree)?;

                    // Add to target profile
                    let target_storage = Storage::new(target_profile)?;
                    let (mut target_instances, target_groups) =
                        target_storage.load_with_groups()?;
                    target_instances.push(instance);
                    let mut target_tree =
                        GroupTree::new_with_groups(&target_instances, &target_groups);
                    if !effective_group.is_empty() {
                        target_tree.create_group(&effective_group);
                    }
                    target_storage.save_with_groups(&target_instances, &target_tree)?;

                    // Clear selection since session is no longer in this profile
                    self.selected_session = None;

                    self.reload()?;
                    return Ok(());
                }
            }

            // No profile change - update in place
            if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
                inst.title = effective_title.clone();
                inst.group_path = effective_group.clone();
            }

            // Handle tmux rename if title changed
            if let Some(inst) = self.instance_map.get(&id) {
                if inst.title != effective_title {
                    let tmux_session = inst.tmux_session()?;
                    if tmux_session.exists() {
                        let new_tmux_name =
                            crate::tmux::Session::generate_name(&id, &effective_title);
                        if let Err(e) = tmux_session.rename(&new_tmux_name) {
                            tracing::warn!("Failed to rename tmux session: {}", e);
                        } else {
                            crate::tmux::refresh_session_cache();
                        }
                    }
                }
            }

            // Rebuild group tree and create group if needed
            self.group_tree = GroupTree::new_with_groups(&self.instances, &self.groups);
            if !effective_group.is_empty() {
                self.group_tree.create_group(&effective_group);
            }
            self.storage
                .save_with_groups(&self.instances, &self.group_tree)?;

            self.reload()?;
        }
        Ok(())
    }
}
