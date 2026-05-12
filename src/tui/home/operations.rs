//! Session operations for HomeView (create, delete, rename)

use chrono::Utc;

use crate::session::builder::{self, InstanceParams};
use crate::session::{list_profiles, GroupTree, Status, Storage};
use crate::tui::deletion_poller::DeletionRequest;
use crate::tui::dialogs::{DeleteOptions, GroupDeleteOptions, NewSessionData};

use super::HomeView;

/// Compact human-readable label for the snooze status line (`"30 min"`,
/// `"1 hr"`, `"24 hr"`, `"2 hr 30 min"`). The picker only ever submits
/// 30 / 60 / 1440, but formatting is kept general so arbitrary values
/// from other callers read cleanly too.
fn humanize_minutes(m: u32) -> String {
    let hours = m / 60;
    let mins = m % 60;
    match (hours, mins) {
        (0, _) => format!("{} min", mins),
        (_, 0) => format!("{} hr", hours),
        _ => format!("{} hr {} min", hours, mins),
    }
}

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
        let existing_branches: Vec<&str> = self
            .instances()
            .iter()
            .filter(|i| i.source_profile == target_profile)
            .filter_map(|i| i.worktree_info.as_ref().map(|w| w.branch.as_str()))
            .collect();

        let params = InstanceParams {
            title: data.title,
            path: data.path,
            group: data.group,
            tool: data.tool,
            worktree_enabled: data.worktree_enabled,
            worktree_branch: data.worktree_branch,
            create_new_branch: data.create_new_branch,
            sandbox: data.sandbox,
            sandbox_image: data.sandbox_image,
            yolo_mode: data.yolo_mode,
            extra_env: data.extra_env,
            extra_args: data.extra_args,
            command_override: data.command_override,
            extra_repo_paths: data.extra_repo_paths,
        };

        let build_result = builder::build_instance(
            params,
            &existing_titles,
            &existing_branches,
            &target_profile,
        )?;
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

    /// Restart the selected session — stops the current tmux/process and
    /// starts a fresh one. No-op if no session selected, or if the session
    /// is currently in a transient state (Creating/Deleting).
    pub(super) fn restart_selected_session(&mut self) -> anyhow::Result<()> {
        let id = match &self.selected_session {
            Some(id) => id.clone(),
            None => return Ok(()),
        };
        // Snapshot what we need; avoid holding a borrow while mutating.
        let (is_transient, _title) = match self.get_instance(&id) {
            Some(inst) => (
                matches!(inst.status, Status::Creating | Status::Deleting),
                inst.title.clone(),
            ),
            None => return Ok(()),
        };
        if is_transient {
            return Ok(());
        }

        // Mutate the persisted Instance via mutate_instance so storage is
        // updated atomically. restart_with_size operates on the live tmux
        // session by id internally — we need to call it on a clone if the
        // borrow checker fights us.
        let mut snapshot = match self.get_instance(&id) {
            Some(inst) => inst.clone(),
            None => return Ok(()),
        };
        snapshot.restart_with_size(crate::terminal::get_size())?;

        // Persist the (possibly updated) status by reflecting it back.
        self.mutate_instance(&id, |inst| {
            inst.status = snapshot.status;
        });
        self.save()?;

        // Mirror the CLI `aoe session restart` behavior: after the agent has
        // had a moment to come back up, send a wake-up prompt so it resumes
        // whatever it was doing without manual nudging.
        let title = snapshot.title.clone();
        let session_id = snapshot.id.clone();
        let tool = snapshot.tool.clone();
        std::thread::sleep(std::time::Duration::from_millis(2000));
        let tmux_session = crate::tmux::Session::new(&session_id, &title)?;
        if tmux_session.exists() {
            let delay = crate::agents::send_keys_enter_delay(&tool);
            let wake_msg = "wake up — pick up what you were doing";
            if let Err(e) = tmux_session.send_keys_with_delay(wake_msg, delay) {
                tracing::warn!("failed to send wake-up message after restart: {}", e);
            } else {
                self.mutate_instance(&session_id, |inst| {
                    inst.touch_last_accessed();
                });
            }
        }
        Ok(())
    }

    /// Toggle the archived state of the cursor's selection. Operates on a
    /// session OR a group. For groups, cascades to all child sessions
    /// (recursive). Returns Ok(Some(message)) on success with a status-line
    /// message, Ok(None) if no selection, or Err on failure.
    ///
    /// Sort behavior is handled by `attention_tier` (returns 99 for
    /// archived) — the rendered list will rebuild at the end and sink the
    /// archived rows to the bottom in italic+dim style.
    pub(super) fn toggle_archive_at_cursor(&mut self) -> anyhow::Result<Option<String>> {
        // Session takes precedence over group when both are set (the cursor
        // line is on a session row).
        if let Some(id) = self.selected_session.clone() {
            let mut new_state = false;
            let mut title = String::new();
            self.mutate_instance(&id, |inst| {
                if inst.archived_at.is_some() {
                    inst.archived_at = None;
                    new_state = false;
                } else {
                    inst.archived_at = Some(Utc::now());
                    new_state = true;
                }
                title = inst.title.clone();
            });
            self.save()?;
            self.flat_items = self.build_flat_items();
            // Jump cursor to the next attention item after archiving. Without
            // this, the cursor stays on the same index and lands on whatever
            // row happened to shift into that slot — effectively random. The
            // user's workflow is "press z to dismiss → work on the next thing
            // at top of Attention." Only fire on archive (new_state=true) and
            // only in Attention sort; unarchive is a deliberate "bring this
            // back" action where cursor-follows-item makes sense.
            if new_state && self.sort_order == crate::session::config::SortOrder::Attention {
                self.select_top_attention(None);
            }
            return Ok(Some(format!(
                "{}: {}",
                if new_state { "Archived" } else { "Unarchived" },
                title
            )));
        }

        if let Some(group_path) = self.selected_group.clone() {
            let owning_profile = self.selected_group_profile.clone();
            // Determine new state from the group itself (or its first member).
            let currently_archived = if let Some(profile) = &owning_profile {
                self.group_trees
                    .get(profile)
                    .and_then(|t| t.group_archived_at(&group_path))
                    .is_some()
            } else {
                self.group_trees
                    .values()
                    .any(|t| t.group_archived_at(&group_path).is_some())
            };
            let new_state = !currently_archived;
            let now = Some(Utc::now());

            // Set on the group tree(s).
            if let Some(profile) = &owning_profile {
                if let Some(tree) = self.group_trees.get_mut(profile) {
                    tree.set_archived(&group_path, new_state);
                }
            } else {
                for tree in self.group_trees.values_mut() {
                    if tree.group_exists(&group_path) {
                        tree.set_archived(&group_path, new_state);
                    }
                }
            }

            // Cascade to all child instances (direct + nested).
            let prefix = format!("{}/", group_path);
            let ids_to_update: Vec<String> = self
                .instances
                .iter()
                .filter(|i| {
                    (i.group_path == group_path || i.group_path.starts_with(&prefix))
                        && owning_profile
                            .as_ref()
                            .is_none_or(|p| p == &i.source_profile)
                })
                .map(|i| i.id.clone())
                .collect();
            for id in &ids_to_update {
                self.mutate_instance(id, |inst| {
                    inst.archived_at = if new_state { now } else { None };
                });
            }

            self.save()?;
            self.flat_items = self.build_flat_items();
            // Same rationale as the session path: after archiving a group,
            // jump the cursor to the top non-archived attention item so the
            // user can continue their Attention-view workflow without a
            // cursor-follows-sunk-folder detour.
            if new_state && self.sort_order == crate::session::config::SortOrder::Attention {
                self.select_top_attention(None);
            }
            return Ok(Some(format!(
                "{}: {} ({} session{})",
                if new_state { "Archived" } else { "Unarchived" },
                group_path,
                ids_to_update.len(),
                if ids_to_update.len() == 1 { "" } else { "s" }
            )));
        }

        Ok(None)
    }

    /// Handle `h`/`H`/`w`/`W` on the cursor's session. If already snoozed,
    /// wake it immediately (no picker — the user just wants it back).
    /// Otherwise open the duration picker (`SnoozeDurationDialog`) so they
    /// can choose 1-6 hours / 1 day / 1 week before the row sinks. The
    /// actual snooze runs in `snooze_session_for` once the dialog submits.
    ///
    /// Snooze semantics: "temporary archive" — sets `snoozed_until = now +
    /// minutes`, the row sinks to tier 99 alongside archived rows, renders
    /// italic+dim with a `z ` prefix and remaining-time in the age column,
    /// and wakes back up automatically when the timer elapses (lazy — no
    /// background task). Duration is resolved at snooze time; changing the
    /// config default does NOT extend in-flight snoozes.
    pub(super) fn toggle_snooze_at_cursor(&mut self) -> anyhow::Result<Option<String>> {
        let Some(id) = self.selected_session.clone() else {
            return Ok(None);
        };
        // Currently snoozed rows skip the picker — the only sensible "w on
        // a snoozed row" action is wake.
        let (is_snoozed, title) = {
            let inst = self.instances.iter().find(|i| i.id == id);
            match inst {
                Some(i) => (i.is_snoozed(), i.title.clone()),
                None => return Ok(None),
            }
        };
        if is_snoozed {
            self.mutate_instance(&id, |inst| inst.unsnooze());
            self.save()?;
            self.flat_items = self.build_flat_items();
            return Ok(Some(format!("Woke: {}", title)));
        }

        self.pending_snooze_session = Some(id);
        self.snooze_duration_dialog = Some(crate::tui::dialogs::SnoozeDurationDialog::new(&title));
        Ok(None)
    }

    /// Apply a snooze with an explicit duration. Called by the duration
    /// picker on submit; also the single place that actually mutates
    /// `snoozed_until` from the TUI. Mirrors the archive cursor-follow
    /// rule: after sinking the row in the Attention sort, jump to the next
    /// needs-attention item so the user can keep triaging.
    pub(super) fn snooze_session_for(
        &mut self,
        id: &str,
        minutes: u32,
    ) -> anyhow::Result<Option<String>> {
        let mut title = String::new();
        self.mutate_instance(id, |inst| {
            inst.snooze(minutes);
            title = inst.title.clone();
        });
        self.save()?;
        self.flat_items = self.build_flat_items();
        if self.sort_order == crate::session::config::SortOrder::Attention {
            self.select_top_attention(None);
        }
        Ok(Some(format!(
            "Snoozed for {}: {}",
            humanize_minutes(minutes),
            title
        )))
    }

    /// Toggle the favorite state of the cursor's session. Session-only for
    /// v1 (no group cascade — favorite is a "this one chat matters" signal,
    /// not a folder-level organizing tool). Pinning logic lives in
    /// `attention_session_key` — no list rebuild needed beyond what
    /// `save()` + `build_flat_items()` already do.
    pub(super) fn toggle_favorite_at_cursor(&mut self) -> anyhow::Result<Option<String>> {
        let Some(id) = self.selected_session.clone() else {
            return Ok(None);
        };
        let mut new_state = false;
        let mut title = String::new();
        self.mutate_instance(&id, |inst| {
            if inst.favorited_at.is_some() {
                inst.favorited_at = None;
                new_state = false;
            } else {
                inst.favorited_at = Some(Utc::now());
                new_state = true;
            }
            title = inst.title.clone();
        });
        self.save()?;
        self.flat_items = self.build_flat_items();
        Ok(Some(format!(
            "{}: {}",
            if new_state {
                "Favorited"
            } else {
                "Unfavorited"
            },
            title
        )))
    }

    pub(super) fn delete_selected(&mut self, options: &DeleteOptions) -> anyhow::Result<()> {
        if let Some(id) = &self.selected_session {
            let id = id.clone();

            // delete-as-archive: when the user picks `d` without checking any
            // destructive cleanup boxes, treat it as archive+kill. The entry
            // persists in sessions.json with `archived_at` set, the pane dies,
            // and the row sinks to tier 99 italic+dim. Recoverable via
            // unarchive (`u`) or `aoe session unarchive`. Hard purge — actual
            // worktree/branch/sandbox tear-down — happens only when the user
            // explicitly opts in via the dialog checkboxes.
            //
            // User directive 2026-05-11: "I want the delete function in aoe
            // to really be our archive."
            let is_hard_purge =
                options.delete_worktree || options.delete_branch || options.delete_sandbox;

            if !is_hard_purge {
                let kill_result = self
                    .get_instance(&id)
                    .map(|inst| inst.kill())
                    .unwrap_or(Ok(()));
                if let Err(e) = kill_result {
                    tracing::warn!("delete-as-archive: failed to kill pane for {}: {}", id, e);
                }
                self.mutate_instance(&id, |inst| {
                    if !inst.is_archived() {
                        inst.archive();
                    }
                });
                self.save()?;
                self.flat_items = self.build_flat_items();
                return Ok(());
            }

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
                            .is_none_or(|p| p == &i.source_profile)
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
                            .is_none_or(|p| p == &i.source_profile)
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
                        && (inst
                            .worktree_info
                            .as_ref()
                            .is_some_and(|wt| wt.managed_by_aoe)
                            || inst
                                .workspace_info
                                .as_ref()
                                .is_some_and(|ws| ws.cleanup_on_delete));
                    let delete_branch = options.delete_branches
                        && (inst
                            .worktree_info
                            .as_ref()
                            .is_some_and(|wt| wt.managed_by_aoe)
                            || inst
                                .workspace_info
                                .as_ref()
                                .is_some_and(|ws| ws.cleanup_on_delete));
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

    /// Force-remove a session from storage without any cleanup.
    /// Used for sessions stuck in the Deleting state where the background
    /// deletion thread never returned a result.
    pub(super) fn force_remove_session(&mut self, session_id: &str) -> anyhow::Result<()> {
        self.remove_instance(session_id);
        self.rebuild_group_trees();
        self.save()?;
        self.reload()?;
        Ok(())
    }

    pub(super) fn group_has_managed_worktrees(&self, group_path: &str, prefix: &str) -> bool {
        self.instances().iter().any(|i| {
            (i.group_path == group_path || i.group_path.starts_with(prefix))
                && (i.worktree_info.as_ref().is_some_and(|wt| wt.managed_by_aoe)
                    || i.workspace_info
                        .as_ref()
                        .is_some_and(|ws| ws.cleanup_on_delete))
        })
    }

    pub(super) fn group_has_containers(&self, group_path: &str, prefix: &str) -> bool {
        self.instances().iter().any(|i| {
            (i.group_path == group_path || i.group_path.starts_with(prefix))
                && i.sandbox_info.as_ref().is_some_and(|s| s.enabled)
        })
    }

    /// Rename a group in-place: the old group path is removed and all sessions and
    /// sub-groups follow the new name. Re-sorting happens automatically on reload.
    pub(super) fn rename_selected_group(
        &mut self,
        new_group: Option<&str>,
        new_profile: Option<&str>,
    ) -> anyhow::Result<()> {
        let ctx = match self.group_rename_context.take() {
            Some(ctx) => ctx,
            None => return Ok(()),
        };

        let new_path = match new_group {
            Some(g) if !g.is_empty() && g != ctx.old_path => g,
            _ if new_profile.is_none() => return Ok(()), // nothing changed
            _ => &ctx.old_path,                          // profile-only change
        };

        // Defense-in-depth: reject duplicate names (dialog validates inline, but guard here too)
        let target_profile = new_profile.unwrap_or(&ctx.old_profile);
        if new_path != ctx.old_path {
            if let Some(tree) = self.group_trees.get(target_profile) {
                if tree.group_exists(new_path) {
                    anyhow::bail!(
                        "A group named '{}' already exists in profile '{}'",
                        new_path,
                        target_profile
                    );
                }
            }
        }

        // Validate target profile exists when moving across profiles
        if let Some(target) = new_profile {
            if target != ctx.old_profile {
                let profiles = list_profiles()?;
                if !profiles.contains(&target.to_string()) {
                    anyhow::bail!("Profile '{}' does not exist", target);
                }
            }
        }

        let old_prefix = format!("{}/", ctx.old_path);

        // Collect sessions belonging to this group and its descendants
        let affected_ids: Vec<String> = self
            .instances
            .iter()
            .filter(|i| {
                (i.group_path == ctx.old_path || i.group_path.starts_with(&old_prefix))
                    && i.source_profile == ctx.old_profile
            })
            .map(|i| i.id.clone())
            .collect();

        // Update group_path (and optionally source_profile) for all affected sessions
        for id in &affected_ids {
            let new_group_path = if new_path != ctx.old_path {
                let inst = self.get_instance(id);
                match inst {
                    Some(i) if i.group_path == ctx.old_path => new_path.to_string(),
                    Some(i) => format!("{}{}", new_path, &i.group_path[ctx.old_path.len()..]),
                    None => continue,
                }
            } else {
                match self.get_instance(id) {
                    Some(i) => i.group_path.clone(),
                    None => continue,
                }
            };

            if let Some(tp) = new_profile {
                self.mutate_instance(id, |inst| {
                    inst.group_path = new_group_path.clone();
                    inst.source_profile = tp.to_string();
                });
            } else {
                self.mutate_instance(id, |inst| {
                    inst.group_path = new_group_path.clone();
                });
            }
        }

        // Ensure target profile storage exists when moving across profiles
        if let Some(tp) = new_profile {
            if tp != ctx.old_profile && !self.storages.contains_key(tp) {
                self.storages.insert(tp.to_string(), Storage::new(tp)?);
            }
        }

        // Rebuild trees from the updated instance list
        self.rebuild_group_trees();

        // Rename the group node in the source tree so the old path is removed
        // and the new path is established (including all descendant nodes).
        if new_path != ctx.old_path {
            if let Some(tree) = self.group_trees.get_mut(&ctx.old_profile) {
                tree.rename_group(&ctx.old_path, new_path);
            }
        }

        // When moving to a different profile, ensure the new path exists in the target tree
        if let Some(tp) = new_profile {
            if let Some(tree) = self.group_trees.get_mut(tp) {
                tree.create_group(new_path);
            }
        }

        self.save()?;
        self.reload()?;
        Ok(())
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
