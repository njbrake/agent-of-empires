//! Session operations for HomeView (create, delete, rename)

use crate::session::builder::{self, InstanceParams};
use crate::session::{list_profiles, GroupTree, Status, Storage};
use crate::tui::deletion_poller::DeletionRequest;
use crate::tui::dialogs::{DeleteOptions, GroupDeleteOptions, NewSessionData};

use super::HomeView;

impl HomeView {
    pub(super) fn create_session(&mut self, data: NewSessionData) -> anyhow::Result<String> {
        let target_profile = data.profile.clone();

        // In unified mode, all instances are loaded, so use them for title dedup.
        // For the target profile, filter to that profile's instances.
        let existing_titles: Vec<&str> = self
            .instances
            .iter()
            .filter(|i| i.source_profile == target_profile)
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
            extra_env: data.extra_env,
            extra_args: data.extra_args,
            command_override: data.command_override,
        };

        let build_result = builder::build_instance(params, &existing_titles, &target_profile)?;
        let mut instance = build_result.instance;
        instance.source_profile = target_profile.clone();
        let session_id = instance.id.clone();

        // Ensure target profile storage exists
        if !self.storages.contains_key(&target_profile) {
            self.storages
                .insert(target_profile.clone(), Storage::new(&target_profile)?);
        }

        self.instances.push(instance.clone());
        self.rebuild_group_trees();
        if !instance.group_path.is_empty() {
            if let Some(tree) = self.group_trees.get_mut(&target_profile) {
                tree.create_group(&instance.group_path);
            }
        }
        self.save()?;

        self.reload()?;
        Ok(session_id)
    }

    pub(super) fn delete_selected(&mut self, options: &DeleteOptions) -> anyhow::Result<()> {
        if let Some(id) = &self.selected_session {
            let id = id.clone();

            self.set_instance_status(&id, Status::Deleting);

            if let Some(inst) = self.get_instance(&id) {
                let request = DeletionRequest {
                    session_id: id.clone(),
                    instance: inst.clone(),
                    delete_worktree: options.delete_worktree,
                    delete_branch: options.delete_branch,
                    delete_sandbox: options.delete_sandbox,
                    force_delete: options.force_delete,
                };
                self.deletion_poller.request_deletion(request);
            }
        }
        Ok(())
    }

    pub(super) fn delete_selected_group(&mut self) -> anyhow::Result<()> {
        if let Some(group_path) = self.selected_group.take() {
            let owning_profile = self.selected_group_profile.take();
            let prefix = format!("{}/", group_path);

            // Only ungroup instances belonging to the owning profile
            for inst in &mut self.instances {
                if (inst.group_path == group_path || inst.group_path.starts_with(&prefix))
                    && owning_profile
                        .as_ref()
                        .map_or(true, |p| p == &inst.source_profile)
                {
                    inst.group_path = String::new();
                }
            }

            self.rebuild_group_trees();
            // Delete the group only from the owning profile's tree
            if let Some(profile) = &owning_profile {
                if let Some(tree) = self.group_trees.get_mut(profile) {
                    tree.delete_group(&group_path);
                }
            } else {
                for tree in self.group_trees.values_mut() {
                    tree.delete_group(&group_path);
                }
            }
            self.save()?;

            self.reload()?;
        }
        Ok(())
    }

    pub(super) fn delete_group_with_sessions(
        &mut self,
        options: &GroupDeleteOptions,
    ) -> anyhow::Result<()> {
        if let Some(group_path) = self.selected_group.take() {
            let owning_profile = self.selected_group_profile.take();
            let prefix = format!("{}/", group_path);

            let sessions_to_delete: Vec<String> = self
                .instances
                .iter()
                .filter(|i| {
                    (i.group_path == group_path || i.group_path.starts_with(&prefix))
                        && owning_profile
                            .as_ref()
                            .map_or(true, |p| p == &i.source_profile)
                })
                .map(|i| i.id.clone())
                .collect();

            for session_id in sessions_to_delete {
                self.mutate_instance(&session_id, |inst| {
                    inst.status = Status::Deleting;
                    inst.group_path = String::new();
                });

                if let Some(inst) = self.get_instance(&session_id) {
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
                        force_delete: options.force_delete_worktrees,
                    };
                    self.deletion_poller.request_deletion(request);
                }
            }

            if let Some(profile) = &owning_profile {
                if let Some(tree) = self.group_trees.get_mut(profile) {
                    tree.delete_group(&group_path);
                }
            } else {
                for tree in self.group_trees.values_mut() {
                    tree.delete_group(&group_path);
                }
            }
            self.save()?;
            self.flat_items = self.build_flat_items();
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
                .get_instance(&id)
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
                let current_profile = self
                    .get_instance(&id)
                    .map(|i| i.source_profile.clone())
                    .unwrap_or_else(|| {
                        self.active_profile
                            .clone()
                            .unwrap_or_else(|| "default".to_string())
                    });
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
                    if let Some(orig_inst) = self.get_instance(&id) {
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

                    // Ensure target profile storage exists
                    if !self.storages.contains_key(target_profile) {
                        self.storages
                            .insert(target_profile.to_string(), Storage::new(target_profile)?);
                    }

                    // Update source_profile and save (handles moving between profiles)
                    instance.source_profile = target_profile.to_string();
                    self.mutate_instance(&id, |inst| {
                        inst.title = instance.title.clone();
                        inst.group_path = instance.group_path.clone();
                        inst.source_profile = instance.source_profile.clone();
                    });

                    self.rebuild_group_trees();
                    if !effective_group.is_empty() {
                        // Ensure group tree exists for the target profile
                        if !self.group_trees.contains_key(target_profile) {
                            self.group_trees.insert(
                                target_profile.to_string(),
                                GroupTree::new_with_groups(&[], &[]),
                            );
                        }
                        if let Some(tree) = self.group_trees.get_mut(target_profile) {
                            tree.create_group(&effective_group);
                        }
                    }
                    self.save()?;
                    self.reload()?;
                    return Ok(());
                }
            }

            // No profile change - update in place
            // Rename tmux session BEFORE mutating the instance, so we can
            // look up the session by its current (old) name.
            let old_title = self.get_instance(&id).map(|i| i.title.clone());
            if let Some(ref old_t) = old_title {
                if *old_t != effective_title {
                    let old_tmux_session = crate::tmux::Session::new(&id, old_t)?;
                    if old_tmux_session.exists() {
                        let new_tmux_name =
                            crate::tmux::Session::generate_name(&id, &effective_title);
                        if let Err(e) = old_tmux_session.rename(&new_tmux_name) {
                            tracing::warn!("Failed to rename tmux session: {}", e);
                        } else {
                            crate::tmux::refresh_session_cache();
                        }
                    }
                }
            }

            self.mutate_instance(&id, |inst| {
                inst.title = effective_title.clone();
                inst.group_path = effective_group.clone();
            });

            // Rebuild group trees and create group if needed
            self.rebuild_group_trees();
            if !effective_group.is_empty() {
                let profile = self
                    .get_instance(&id)
                    .map(|i| i.source_profile.clone())
                    .unwrap_or_else(|| {
                        self.active_profile
                            .clone()
                            .unwrap_or_else(|| "default".to_string())
                    });
                if let Some(tree) = self.group_trees.get_mut(&profile) {
                    tree.create_group(&effective_group);
                }
            }
            self.save()?;

            self.reload()?;
        }
        Ok(())
    }
}
