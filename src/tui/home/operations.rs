//! Session operations for HomeView (create, delete, rename, move)

use crate::session::builder::{self, InstanceParams};
use crate::session::{list_profiles, parent_path, GroupTree, Item, Status, Storage};
use crate::tui::deletion_poller::DeletionRequest;
use crate::tui::dialogs::{DeleteOptions, GroupDeleteOptions, NewSessionData};

use super::HomeView;

impl HomeView {
    pub(super) fn create_session(&mut self, data: NewSessionData) -> anyhow::Result<String> {
        let target_profile = data.profile.clone();

        // In unified mode, all instances are loaded, so use them for title dedup.
        // For the target profile, filter to that profile's instances.
        let existing_titles: Vec<&str> = self
            .instances()
            .iter()
            .filter(|i| i.source_profile == target_profile)
            .map(|i| i.title.as_str())
            .collect();

        let params = InstanceParams {
            title: data.title,
            path: data.path,
            group: data.group,
            tool: data.tool,
            profile: data.profile,
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

        self.add_instance(instance.clone());
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
            let ids_to_clear: Vec<String> = self
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
            for id in &ids_to_clear {
                self.mutate_instance(id, |inst| inst.group_path = String::new());
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
                .instances()
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
        self.instances().iter().any(|i| {
            (i.group_path == group_path || i.group_path.starts_with(prefix))
                && i.worktree_info.as_ref().is_some_and(|wt| wt.managed_by_aoe)
        })
    }

    pub(super) fn group_has_containers(&self, group_path: &str, prefix: &str) -> bool {
        self.instances().iter().any(|i| {
            (i.group_path == group_path || i.group_path.starts_with(prefix))
                && i.sandbox_info.as_ref().is_some_and(|s| s.enabled)
        })
    }

    // --- Move operations ---

    /// Unified move dispatcher. Returns early if search filter is active.
    pub(super) fn handle_move(&mut self, direction: i32) -> anyhow::Result<()> {
        if self.search_active {
            return Ok(());
        }

        if self.selected_group.is_some() {
            self.move_group(direction)?;
        } else if self.selected_session.is_some() && !self.move_session_in_group(direction)? {
            self.move_session_across_boundary(direction)?;
        }
        Ok(())
    }

    /// Move a group among its siblings. Swaps sort_order with adjacent sibling.
    fn move_group(&mut self, direction: i32) -> anyhow::Result<()> {
        let path = match &self.selected_group {
            Some(p) => p.clone(),
            None => return Ok(()),
        };

        let profile = match &self.selected_group_profile {
            Some(p) => p.clone(),
            None => return Ok(()),
        };

        let tree = match self.group_trees.get(&profile) {
            Some(t) => t,
            None => return Ok(()),
        };

        let siblings = tree.get_sibling_paths(&path);
        let pos = match siblings.iter().position(|p| *p == path) {
            Some(p) => p,
            None => return Ok(()),
        };

        let target_pos = if direction < 0 {
            if pos == 0 {
                return Ok(());
            }
            pos - 1
        } else {
            if pos + 1 >= siblings.len() {
                return Ok(());
            }
            pos + 1
        };

        if let Some(tree) = self.group_trees.get_mut(&profile) {
            tree.swap_group_order(&path, &siblings[target_pos]);
        }
        self.save_and_rebuild()?;
        self.select_group_by_path(&path);
        Ok(())
    }

    /// Move a session within its group.
    /// Returns Ok(true) if the move was handled (including no-ops like missing session),
    /// or Ok(false) if the session is at the group boundary and needs cross-group movement.
    fn move_session_in_group(&mut self, direction: i32) -> anyhow::Result<bool> {
        let session_id = match &self.selected_session {
            Some(id) => id.clone(),
            None => return Ok(true),
        };

        let group_path = match self.instance_map.get(&session_id) {
            Some(inst) => inst.group_path.clone(),
            None => return Ok(true),
        };

        // Collect indices of sessions in the same group, preserving Vec order
        let group_indices: Vec<usize> = self
            .instances
            .iter()
            .enumerate()
            .filter(|(_, inst)| inst.group_path == group_path)
            .map(|(i, _)| i)
            .collect();

        let pos_in_group = match group_indices
            .iter()
            .position(|&i| self.instances[i].id == session_id)
        {
            Some(p) => p,
            None => return Ok(true),
        };

        let at_boundary = if direction < 0 {
            pos_in_group == 0
        } else {
            pos_in_group + 1 >= group_indices.len()
        };

        if at_boundary {
            return Ok(false);
        }

        // Swap within the instances Vec
        let target_pos = if direction < 0 {
            pos_in_group - 1
        } else {
            pos_in_group + 1
        };

        let idx_a = group_indices[pos_in_group];
        let idx_b = group_indices[target_pos];
        self.instances.swap(idx_a, idx_b);

        self.save_and_rebuild()?;
        self.select_session_by_id(&session_id);
        Ok(true)
    }

    /// Move session across group boundary when at the edge of its current group.
    fn move_session_across_boundary(&mut self, direction: i32) -> anyhow::Result<()> {
        let session_id = match &self.selected_session {
            Some(id) => id.clone(),
            None => return Ok(()),
        };

        let current_group = match self.instance_map.get(&session_id) {
            Some(inst) => inst.group_path.clone(),
            None => return Ok(()),
        };

        if direction < 0 {
            // Moving UP from first session: move out to parent group
            let new_group = parent_path(&current_group);

            // Already at top level, can't move further up
            if new_group == current_group {
                return Ok(());
            }

            // Update group_path in instances
            if let Some(inst) = self.instances.iter_mut().find(|i| i.id == session_id) {
                inst.group_path = new_group.clone();
            }

            // Reposition: move to end of new group's sessions in the Vec
            self.reposition_session_in_vec(&session_id, &new_group, false);
        } else {
            // Moving DOWN from last session: look at what's next in flat_items
            let cursor_idx = self
                .flat_items
                .iter()
                .enumerate()
                .rev()
                .find_map(|(idx, item)| {
                    if let Item::Session { id, .. } = item {
                        if *id == session_id {
                            return Some(idx);
                        }
                    }
                    None
                });

            let cursor_idx = match cursor_idx {
                Some(i) => i,
                None => return Ok(()),
            };

            // Find the next group in flat_items after this session
            let target_group = self.flat_items[cursor_idx + 1..].iter().find_map(|item| {
                if let Item::Group { path, .. } = item {
                    Some(path.clone())
                } else {
                    None
                }
            });

            let target_group = match target_group {
                Some(g) => g,
                None => return Ok(()),
            };

            // Auto-expand collapsed target group
            let session_profile = self
                .instance_map
                .get(&session_id)
                .map(|i| i.source_profile.clone());
            if let Some(profile) = &session_profile {
                if let Some(tree) = self.group_trees.get_mut(profile) {
                    if let Some(g) = tree.get_group_mut(&target_group) {
                        if g.collapsed {
                            g.collapsed = false;
                        }
                    }
                }
            }

            // Update group_path in instances
            if let Some(inst) = self.instances.iter_mut().find(|i| i.id == session_id) {
                inst.group_path = target_group.clone();
            }

            // Reposition: move to beginning of target group's sessions
            self.reposition_session_in_vec(&session_id, &target_group, true);
        }

        self.save_and_rebuild()?;
        self.select_session_by_id(&session_id);
        Ok(())
    }

    /// Move a session in the instances Vec to appear at the start or end of its new group.
    fn reposition_session_in_vec(
        &mut self,
        session_id: &str,
        target_group: &str,
        at_beginning: bool,
    ) {
        // Remove the session from its current position
        let session_pos = match self.instances.iter().position(|i| i.id == session_id) {
            Some(p) => p,
            None => return,
        };
        let session = self.instances.remove(session_pos);

        if at_beginning {
            // Insert before the first session in the target group
            let insert_pos = self
                .instances
                .iter()
                .position(|i| i.group_path == target_group)
                .unwrap_or(self.instances.len());
            self.instances.insert(insert_pos, session);
        } else {
            // Insert after the last session in the target group
            let last_pos = self
                .instances
                .iter()
                .enumerate()
                .rev()
                .find_map(|(i, inst)| {
                    if inst.group_path == target_group {
                        Some(i)
                    } else {
                        None
                    }
                });
            match last_pos {
                Some(p) => self.instances.insert(p + 1, session),
                None => self.instances.push(session),
            }
        }
    }

    /// Rebuild all derived state from instances + group_trees and persist.
    fn save_and_rebuild(&mut self) -> anyhow::Result<()> {
        self.rebuild_group_trees();
        self.instance_map = self
            .instances
            .iter()
            .map(|i| (i.id.clone(), i.clone()))
            .collect();
        self.flat_items = self.build_flat_items();
        self.save()?;
        Ok(())
    }

    /// Find a group in flat_items by path and select it.
    fn select_group_by_path(&mut self, path: &str) {
        for (idx, item) in self.flat_items.iter().enumerate() {
            if let Item::Group {
                path: item_path, ..
            } = item
            {
                if item_path == path {
                    self.cursor = idx;
                    self.update_selected();
                    return;
                }
            }
        }
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
                        .instances()
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

            // Rename tmux session BEFORE mutating the instance, so we can
            // look up the session by its current (old) name.
            if current_title != effective_title {
                let old_tmux_session = crate::tmux::Session::new(&id, &current_title)?;
                if old_tmux_session.exists() {
                    let new_tmux_name = crate::tmux::Session::generate_name(&id, &effective_title);
                    if let Err(e) = old_tmux_session.rename(&new_tmux_name) {
                        tracing::warn!("Failed to rename tmux session: {}", e);
                    } else {
                        crate::tmux::refresh_session_cache();
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
