//! Session operations for HomeView (create, delete, rename, move)

use crate::session::builder::{self, InstanceParams};
use crate::session::{flatten_tree, list_profiles, parent_path, GroupTree, Item, Status, Storage};
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
            profile: data.profile,
            worktree_branch: data.worktree_branch,
            create_new_branch: data.create_new_branch,
            sandbox: data.sandbox,
            sandbox_image: data.sandbox_image,
            yolo_mode: data.yolo_mode,
            extra_env_keys: data.extra_env_keys,
            extra_env_values: data.extra_env_values,
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
                    force_delete: options.force_delete,
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
                        force_delete: options.force_delete_worktrees,
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

    // --- Move operations ---

    /// Unified move dispatcher. Returns early if search filter is active.
    pub(super) fn handle_move(&mut self, direction: i32) -> anyhow::Result<()> {
        if self.filtered_items.is_some() {
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

        let siblings = self.group_tree.get_sibling_paths(&path);
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

        self.group_tree
            .swap_group_order(&path, &siblings[target_pos]);
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
            if let Some(g) = self.group_tree.get_group_mut(&target_group) {
                if g.collapsed {
                    // toggle_collapsed rebuilds the tree, but we'll rebuild after anyway
                    g.collapsed = false;
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

    /// Rebuild all derived state from instances + group_tree and persist.
    fn save_and_rebuild(&mut self) -> anyhow::Result<()> {
        // Phase 1: Snapshot current sort_orders so new_with_groups preserves them
        self.groups = self.group_tree.get_all_groups();
        self.group_tree = GroupTree::new_with_groups(&self.instances, &self.groups);
        self.flat_items = flatten_tree(&self.group_tree, &self.instances);
        self.instance_map = self
            .instances
            .iter()
            .map(|i| (i.id.clone(), i.clone()))
            .collect();
        // Phase 2: Re-snapshot after rebuild (initialize_sort_orders may have normalized)
        self.groups = self.group_tree.get_all_groups();
        self.storage
            .save_with_groups(&self.instances, &self.group_tree)?;
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
