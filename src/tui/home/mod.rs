//! Home view - main session list and navigation

mod input;
mod operations;
mod render;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use ratatui::prelude::Rect;
use tui_input::Input;

use crate::session::{
    config::{load_config, save_config, GroupByMode, SortOrder},
    flatten_tree, flatten_tree_all_profiles, resolve_config, DefaultTerminalMode, Group, GroupTree,
    Instance, Item, Storage,
};
use crate::tmux::AvailableTools;

use super::creation_poller::{CreationPoller, CreationRequest};
use super::deletion_poller::DeletionPoller;
#[cfg(feature = "serve")]
use super::dialogs::ServeView;
use super::dialogs::{
    ChangelogDialog, ConfirmDialog, GroupDeleteOptionsDialog, HookTrustDialog, HooksInstallDialog,
    InfoDialog, NewSessionData, NewSessionDialog, NoAgentsDialog, ProfilePickerDialog,
    RenameDialog, UnifiedDeleteDialog, WelcomeDialog,
};
use super::diff::DiffView;
use super::settings::SettingsView;
use super::status_poller::StatusPoller;

/// Extract a project group name from a session instance.
/// Uses `worktree_info.main_repo_path` for worktree sessions (so all branches of the
/// same repo group together), otherwise uses `project_path`. Returns the last path segment.
fn project_group_name(inst: &Instance) -> String {
    let base_path = inst
        .worktree_info
        .as_ref()
        .map(|wt| wt.main_repo_path.as_str())
        .unwrap_or(&inst.project_path);

    let path = std::path::Path::new(base_path);
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| {
            // For root paths like "/", use a readable fallback
            if base_path == "/" || base_path.is_empty() {
                "(root)".to_string()
            } else {
                base_path.to_string()
            }
        })
}

pub(super) struct GroupRenameContext {
    pub(super) old_path: String,
    pub(super) old_profile: String,
}

/// View mode for the home screen
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    #[default]
    Agent,
    Terminal,
}

/// Terminal mode for sandboxed sessions (container vs host)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TerminalMode {
    #[default]
    Host,
    Container,
}

/// Cached preview content to avoid subprocess calls on every frame
pub(super) struct PreviewCache {
    pub(super) session_id: Option<String>,
    pub(super) content: String,
    pub(super) last_refresh: Instant,
    pub(super) dimensions: (u16, u16),
    /// Number of lines that were captured into `content`. Used together with
    /// the BUFFER reserve so consecutive wheel ticks don't trigger a fresh
    /// `tmux capture-pane` subprocess while the cached window still covers
    /// the requested scroll.
    pub(super) captured_lines: usize,
}

impl Default for PreviewCache {
    fn default() -> Self {
        Self {
            session_id: None,
            content: String::new(),
            last_refresh: Instant::now(),
            dimensions: (0, 0),
            captured_lines: 0,
        }
    }
}

pub(super) const INDENTS: [&str; 10] = [
    "",
    " ",
    "  ",
    "   ",
    "    ",
    "     ",
    "      ",
    "       ",
    "        ",
    "         ",
];

pub(super) fn get_indent(depth: usize) -> &'static str {
    INDENTS.get(depth).copied().unwrap_or(INDENTS[9])
}

pub(super) const ICON_IDLE: &str = "⠒";
pub(super) const ICON_ERROR: &str = "✕";
pub(super) const ICON_UNKNOWN: &str = "⠤";
pub(super) const ICON_STOPPED: &str = "⠒";
pub(super) const ICON_DELETING: &str = "✕";
pub(super) const ICON_COLLAPSED: &str = "▶";
pub(super) const ICON_EXPANDED: &str = "▼";

/// Hook progress for a session being created in the background
pub(super) struct CreatingHookProgress {
    pub(super) hook_output: Vec<String>,
    pub(super) current_hook: Option<String>,
}

pub struct HomeView {
    pub(super) storages: HashMap<String, Storage>,
    pub(super) active_profile: Option<String>,
    instances: Vec<Instance>,
    instance_map: HashMap<String, Instance>,
    pub(super) group_trees: HashMap<String, GroupTree>,
    pub(super) flat_items: Vec<Item>,

    // UI state
    pub(super) cursor: usize,
    pub(super) selected_session: Option<String>,
    pub(super) selected_group: Option<String>,
    /// Which profile the selected group belongs to (for scoped group operations)
    pub(super) selected_group_profile: Option<String>,
    pub(super) view_mode: ViewMode,
    pub(super) sort_order: SortOrder,
    pub(super) group_by: GroupByMode,
    /// Collapsed state for project-mode groups (persists across rebuilds)
    pub(super) project_group_collapsed: HashMap<String, bool>,

    // Dialogs
    pub(super) show_help: bool,
    pub(super) new_dialog: Option<NewSessionDialog>,
    pub(super) confirm_dialog: Option<ConfirmDialog>,
    pub(super) unified_delete_dialog: Option<UnifiedDeleteDialog>,
    pub(super) group_delete_options_dialog: Option<GroupDeleteOptionsDialog>,
    pub(super) rename_dialog: Option<RenameDialog>,
    pub(super) group_rename_context: Option<GroupRenameContext>,
    pub(super) hook_trust_dialog: Option<HookTrustDialog>,
    /// Session data pending hook trust approval
    pub(super) pending_hook_trust_data: Option<NewSessionData>,
    pub(super) hooks_install_dialog: Option<HooksInstallDialog>,
    /// Session data pending agent hooks acknowledgment
    pub(super) pending_hooks_install_data: Option<NewSessionData>,
    pub(super) welcome_dialog: Option<WelcomeDialog>,
    pub(super) no_agents_dialog: Option<NoAgentsDialog>,
    pub(super) changelog_dialog: Option<ChangelogDialog>,
    pub(super) info_dialog: Option<InfoDialog>,
    pub(super) profile_picker_dialog: Option<ProfilePickerDialog>,
    #[cfg(feature = "serve")]
    pub(super) serve_view: Option<ServeView>,
    pub(super) send_message_dialog: Option<super::dialogs::SendMessageDialog>,
    /// Session to receive the message from the send dialog
    pub(super) pending_send_session: Option<String>,
    /// Session to attach after the custom instruction warning dialog is dismissed
    pub(super) pending_attach_after_warning: Option<String>,
    /// Session to stop after the confirmation dialog is accepted
    pub(super) pending_stop_session: Option<String>,
    /// Session to force-remove after the confirmation dialog is accepted
    pub(super) pending_force_remove_session: Option<String>,
    // Search
    pub(super) search_active: bool,
    pub(super) search_query: Input,
    pub(super) search_matches: Vec<usize>,
    pub(super) search_match_index: usize,

    // Tool availability
    pub(super) available_tools: AvailableTools,

    // Performance: background status polling
    pub(super) status_poller: StatusPoller,
    pub(super) pending_status_refresh: bool,

    // Performance: background deletion
    pub(super) deletion_poller: DeletionPoller,

    // Performance: background session creation (for sandbox)
    pub(super) creation_poller: CreationPoller,
    /// Set to true if user cancelled while creation was pending
    pub(super) creation_cancelled: bool,
    /// Sessions whose on_launch hooks already ran in the creation poller
    pub(super) on_launch_hooks_ran: HashSet<String>,

    /// Hook progress for sessions in Creating state, keyed by stub instance ID
    pub(super) creating_hook_progress: HashMap<String, CreatingHookProgress>,
    /// The stub instance ID for the current background creation
    pub(super) creating_stub_id: Option<String>,

    // Performance: preview caching
    pub(super) preview_cache: PreviewCache,
    pub(super) terminal_preview_cache: PreviewCache,
    pub(super) container_terminal_preview_cache: PreviewCache,

    /// Mouse wheel offset for the preview pane, in lines back from the bottom.
    /// Reset to 0 whenever the selected session changes.
    pub(super) preview_scroll_offset: u16,
    pub(super) preview_area: Rect,
    pub(super) diff_area: Rect,

    // Terminal mode for sandboxed sessions (per-session, ephemeral)
    pub(super) terminal_modes: HashMap<String, TerminalMode>,
    // Default terminal mode from config
    pub(super) default_terminal_mode: TerminalMode,

    // Sound config for state transition sounds
    pub(super) sound_config: crate::sound::SoundConfig,

    // When true, letter-based action hotkeys require SHIFT (guard against
    // dictation / stray keystrokes triggering destructive actions).
    pub(super) strict_hotkeys: bool,

    // Settings view
    pub(super) settings_view: Option<SettingsView>,
    /// Flag to indicate we're confirming settings close (unsaved changes)
    pub(super) settings_close_confirm: bool,

    // Diff view
    pub(super) diff_view: Option<DiffView>,

    // Resizable list column width (percentage-like units)
    pub(super) list_width: u16,
}

impl HomeView {
    pub fn new(
        active_profile: Option<String>,
        available_tools: AvailableTools,
    ) -> anyhow::Result<Self> {
        use crate::session::list_profiles;

        let mut storages = HashMap::new();
        let mut all_instances = Vec::new();
        let mut group_trees = HashMap::new();

        let profile_names = match &active_profile {
            Some(name) => vec![name.clone()],
            None => list_profiles()?.into_iter().collect(),
        };

        for profile_name in &profile_names {
            let storage = Storage::new(profile_name)?;
            let (mut instances, groups) = storage.load_with_groups()?;
            for inst in &mut instances {
                inst.source_profile = profile_name.clone();
            }
            let tree = GroupTree::new_with_groups(&instances, &groups);
            group_trees.insert(profile_name.clone(), tree);
            all_instances.extend(instances);
            storages.insert(profile_name.clone(), storage);
        }

        let instance_map: HashMap<String, Instance> = all_instances
            .iter()
            .map(|i| (i.id.clone(), i.clone()))
            .collect();

        // In unified mode, config comes from "default" profile
        let config_profile = active_profile.as_deref().unwrap_or("default");
        let resolved = resolve_config(config_profile);
        let default_terminal_mode = resolved
            .as_ref()
            .map(|config| match config.sandbox.default_terminal_mode {
                DefaultTerminalMode::Host => TerminalMode::Host,
                DefaultTerminalMode::Container => TerminalMode::Container,
            })
            .unwrap_or_default();
        let sound_config = resolved
            .as_ref()
            .map(|config| config.sound.clone())
            .unwrap_or_default();
        let strict_hotkeys = resolved
            .as_ref()
            .map(|config| config.session.strict_hotkeys)
            .unwrap_or(false);
        let user_config = load_config().ok().flatten();
        let sort_order = user_config
            .as_ref()
            .and_then(|c| c.app_state.sort_order)
            .unwrap_or_default();
        // New users (haven't dismissed the welcome screen) default to Project
        // grouping so they see the same layout as the web dashboard. Existing
        // users keep Manual (the existing behavior) unless they explicitly
        // toggle to Project with `g`.
        let is_new_user = user_config
            .as_ref()
            .is_none_or(|c| !c.app_state.has_seen_welcome);
        let default_group_by = if is_new_user {
            GroupByMode::Project
        } else {
            GroupByMode::Manual
        };
        let group_by = user_config
            .as_ref()
            .and_then(|c| c.app_state.group_by)
            .unwrap_or(default_group_by);

        let mut view = Self {
            storages,
            active_profile,
            instances: all_instances,
            instance_map,
            group_trees,
            flat_items: Vec::new(),
            cursor: 0,
            selected_session: None,
            selected_group: None,
            selected_group_profile: None,
            view_mode: ViewMode::default(),
            sort_order,
            group_by,
            project_group_collapsed: HashMap::new(),
            show_help: false,
            new_dialog: None,
            confirm_dialog: None,
            unified_delete_dialog: None,
            group_delete_options_dialog: None,
            rename_dialog: None,
            group_rename_context: None,
            hook_trust_dialog: None,
            pending_hook_trust_data: None,
            hooks_install_dialog: None,
            pending_hooks_install_data: None,
            welcome_dialog: None,
            no_agents_dialog: None,
            changelog_dialog: None,
            info_dialog: None,
            profile_picker_dialog: None,
            #[cfg(feature = "serve")]
            serve_view: None,
            send_message_dialog: None,
            pending_send_session: None,
            pending_attach_after_warning: None,
            pending_stop_session: None,
            pending_force_remove_session: None,
            search_active: false,
            search_query: Input::default(),
            search_matches: Vec::new(),
            search_match_index: 0,
            available_tools,
            status_poller: StatusPoller::new(),
            pending_status_refresh: false,
            deletion_poller: DeletionPoller::new(),
            creation_poller: CreationPoller::new(),
            creation_cancelled: false,
            on_launch_hooks_ran: HashSet::new(),
            creating_hook_progress: HashMap::new(),
            creating_stub_id: None,
            preview_cache: PreviewCache::default(),
            terminal_preview_cache: PreviewCache::default(),
            container_terminal_preview_cache: PreviewCache::default(),
            preview_scroll_offset: 0,
            preview_area: Rect::default(),
            diff_area: Rect::default(),
            terminal_modes: HashMap::new(),
            default_terminal_mode,
            sound_config,
            strict_hotkeys,
            settings_view: None,
            settings_close_confirm: false,
            diff_view: None,
            list_width: user_config
                .and_then(|c| c.app_state.home_list_width)
                .unwrap_or(35),
        };

        // Clean up orphaned Creating instances from a prior crash
        let orphan_ids: Vec<String> = view
            .instances
            .iter()
            .filter(|i| i.status == crate::session::Status::Creating)
            .map(|i| i.id.clone())
            .collect();
        for id in &orphan_ids {
            view.remove_instance(id);
        }
        if !orphan_ids.is_empty() {
            tracing::info!("Cleaned up {} orphaned creating sessions", orphan_ids.len());
            let _ = view.save();
        }

        // Recover session IDs for pre-existing sessions via pollers.
        for inst in &mut view.instances {
            let has_live_tmux = inst
                .tmux_session()
                .map(|s| s.exists() && !s.is_pane_dead())
                .unwrap_or(false);
            if !has_live_tmux {
                continue;
            }

            if inst.supports_session_poller() && inst.session_id_poller.is_none() {
                inst.maybe_start_poller();
            }
        }
        view.instance_map = view
            .instances
            .iter()
            .map(|i| (i.id.clone(), i.clone()))
            .collect();

        view.flat_items = view.build_flat_items();
        view.update_selected();
        Ok(view)
    }

    pub fn reload(&mut self) -> anyhow::Result<()> {
        use crate::session::list_profiles;

        let mut all_instances = Vec::new();

        // Re-discover profiles in "all" mode
        if self.active_profile.is_none() {
            let current_profiles = list_profiles()?;
            for name in &current_profiles {
                if !self.storages.contains_key(name) {
                    self.storages.insert(name.clone(), Storage::new(name)?);
                }
            }
            self.storages.retain(|k, _| current_profiles.contains(k));
        }

        for (profile_name, storage) in &self.storages {
            let (mut instances, groups) = storage.load_with_groups()?;
            for inst in &mut instances {
                inst.source_profile = profile_name.clone();
                if let Some(prev) = self.instance_map.get(&inst.id) {
                    inst.status = prev.status;
                    inst.last_error = prev.last_error.clone();
                    inst.last_error_check = prev.last_error_check;
                    inst.last_start_time = prev.last_start_time;
                    inst.session_id_poller = prev.session_id_poller.clone();
                    // Use in-memory session_id if present; fallback to disk.
                    // In-memory state takes priority over disk: the poller
                    // may have updated the ID since last save.
                    inst.agent_session_id = prev
                        .agent_session_id
                        .clone()
                        .or(inst.agent_session_id.take());
                }
            }
            // Rebuild this profile's tree from disk, preserving any collapsed
            // state that was toggled in-memory but not yet on disk
            let mut new_tree = GroupTree::new_with_groups(&instances, &groups);
            if let Some(old_tree) = self.group_trees.get(profile_name) {
                for g in old_tree.get_all_groups() {
                    if g.collapsed {
                        new_tree.set_collapsed(&g.path, true);
                    }
                }
            }
            self.group_trees.insert(profile_name.clone(), new_tree);
            all_instances.extend(instances);
        }

        // Remove trees for profiles that no longer exist
        let storage_keys: Vec<String> = self.storages.keys().cloned().collect();
        self.group_trees.retain(|k, _| storage_keys.contains(k));

        self.instances = all_instances;

        // Re-inject any in-flight Creating stub that won't be on disk
        if let Some(ref stub_id) = self.creating_stub_id {
            if !self.instances.iter().any(|i| i.id == *stub_id) {
                if let Some(stub) = self.instance_map.get(stub_id).cloned() {
                    self.instances.push(stub);
                }
            }
        }

        self.instance_map = self
            .instances
            .iter()
            .map(|i| (i.id.clone(), i.clone()))
            .collect();
        // Remember what the cursor was pointing at so we can follow it
        let prev_selected_session = self.selected_session.clone();
        let prev_selected_group = self.selected_group.clone();

        self.flat_items = self.build_flat_items();

        // Try to restore cursor to the same session/group after rebuild
        let mut restored = false;
        if let Some(ref sid) = prev_selected_session {
            for (idx, item) in self.flat_items.iter().enumerate() {
                if let Item::Session { id, .. } = item {
                    if id == sid {
                        self.cursor = idx;
                        restored = true;
                        break;
                    }
                }
            }
        } else if let Some(ref gpath) = prev_selected_group {
            for (idx, item) in self.flat_items.iter().enumerate() {
                if let Item::Group { path, .. } = item {
                    if path == gpath {
                        self.cursor = idx;
                        restored = true;
                        break;
                    }
                }
            }
        }
        if !restored && self.cursor >= self.flat_items.len() && !self.flat_items.is_empty() {
            self.cursor = self.flat_items.len() - 1;
        }

        if self.search_active && !self.search_query.value().is_empty() {
            self.update_search();
        } else if !self.search_matches.is_empty() {
            // Recalculate match indices without moving the cursor
            self.refresh_search_matches();
        }

        self.update_selected();
        Ok(())
    }

    /// Request a status refresh in the background (non-blocking).
    /// Call `apply_status_updates` to check for and apply results.
    pub fn request_status_refresh(&mut self) {
        if !self.pending_status_refresh {
            let instances: Vec<Instance> = self.instances.clone();
            self.status_poller.request_refresh(instances);
            self.pending_status_refresh = true;
        }
    }

    /// Apply any pending status updates from the background poller.
    /// Returns true if updates were applied.
    pub fn apply_status_updates(&mut self) -> bool {
        use crate::session::Status;

        if let Some(updates) = self.status_poller.try_recv_updates() {
            for update in updates {
                let old_status = self.get_instance(&update.id).map(|i| i.status);

                let should_update = old_status.is_some_and(|s| {
                    s != Status::Deleting
                        && s != Status::Creating
                        && s != Status::Stopped
                        && update.status != Status::Stopped
                });

                if should_update {
                    let new_status = update.status;
                    let new_error = update.last_error;
                    self.mutate_instance(&update.id, |inst| {
                        inst.status = new_status;
                        inst.last_error = new_error;
                    });

                    if let Some(old) = old_status {
                        if old != new_status {
                            crate::sound::play_for_transition(old, new_status, &self.sound_config);
                        }
                    }
                }
            }
            self.pending_status_refresh = false;
            return true;
        }
        false
    }

    pub fn apply_deletion_results(&mut self) -> bool {
        use crate::session::Status;

        if let Some(result) = self.deletion_poller.try_recv_result() {
            if result.success {
                self.remove_instance(&result.session_id);
                self.rebuild_group_trees();

                if let Err(e) = self.save() {
                    tracing::error!("Failed to save after deletion: {}", e);
                }
                let _ = self.reload();
            } else {
                let error = result.error;
                self.mutate_instance(&result.session_id, |inst| {
                    inst.status = Status::Error;
                    inst.last_error = error;
                });
            }
            return true;
        }
        false
    }

    /// Apply any pending session ID updates from background pollers.
    /// Returns true if any instance was updated.
    pub fn apply_session_id_updates(&mut self) -> bool {
        let mut updates: Vec<(String, String)> = Vec::new();

        for inst in &self.instances {
            if let Some((_id, session_id)) = inst
                .session_id_poller
                .as_ref()
                .and_then(|p| p.lock().ok())
                .and_then(|p| p.try_recv_session_update())
            {
                let Some(session_id) = crate::session::capture::validated_session_id(session_id)
                else {
                    continue;
                };
                if inst.agent_session_id.as_deref() != Some(session_id.as_str()) {
                    updates.push((inst.id.clone(), session_id));
                }
                continue;
            }
        }

        if !updates.is_empty() {
            let prev: Vec<(String, Option<String>)> = updates
                .iter()
                .filter_map(|(id, _)| {
                    self.get_instance(id)
                        .map(|inst| (id.clone(), inst.agent_session_id.clone()))
                })
                .collect();

            for (id, session_id) in &updates {
                self.mutate_instance(id, |inst| {
                    inst.agent_session_id = Some(session_id.clone());
                });
            }
            if let Err(e) = self.save() {
                tracing::error!("Failed to save after session ID update: {}", e);
                for (id, old_val) in &prev {
                    self.mutate_instance(id, |inst| {
                        inst.agent_session_id = old_val.clone();
                    });
                }
                return false;
            }
        }
        !updates.is_empty()
    }

    /// Request background session creation. Used for sandbox sessions to avoid blocking UI.
    /// Creates a stub instance in the session list with Status::Creating so the user
    /// can see progress in the preview pane while continuing to use the TUI.
    pub fn request_creation(
        &mut self,
        data: NewSessionData,
        hooks: Option<crate::session::HooksConfig>,
    ) {
        // Build a stub instance to show in the list while creation runs
        let stub_title = if data.title.is_empty() {
            data.worktree_branch
                .as_deref()
                .filter(|b| !b.is_empty())
                .map(|b| b.to_string())
                .unwrap_or_else(|| {
                    std::path::Path::new(&data.path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "New session".to_string())
                })
        } else {
            data.title.clone()
        };
        let mut stub = Instance::new(&stub_title, &data.path);
        stub.tool = if data.tool.is_empty() {
            "claude".to_string()
        } else {
            data.tool.clone()
        };
        stub.group_path = data.group.clone();
        stub.status = crate::session::Status::Creating;
        stub.yolo_mode = data.yolo_mode;
        stub.source_profile = data.profile.clone();

        // Set stub worktree_info so project-mode grouping works during creation.
        // The real worktree_info (with resolved main_repo_path) replaces this
        // once build_instance completes.
        if let Some(ref branch) = data.worktree_branch {
            if !branch.is_empty() {
                stub.worktree_info = Some(crate::session::WorktreeInfo {
                    branch: branch.clone(),
                    main_repo_path: data.path.clone(),
                    managed_by_aoe: false,
                    created_at: chrono::Utc::now(),
                });
            }
        }

        let stub_id = stub.id.clone();
        let target_profile = data.profile.clone();

        // Add stub to instance list
        self.add_instance(stub);
        self.rebuild_group_trees();
        if !data.group.is_empty() {
            if let Some(tree) = self.group_trees.get_mut(&target_profile) {
                tree.create_group(&data.group);
            }
        }

        // Initialize progress tracking and select the stub
        self.creating_hook_progress.insert(
            stub_id.clone(),
            CreatingHookProgress {
                hook_output: Vec::new(),
                current_hook: None,
            },
        );
        self.creating_stub_id = Some(stub_id.clone());
        self.flat_items = self.build_flat_items();

        // Move cursor to the new stub
        if let Some(pos) = self
            .flat_items
            .iter()
            .position(|item| matches!(item, Item::Session { id, .. } if id == &stub_id))
        {
            self.cursor = pos;
            self.update_selected();
        }

        // Close the dialog
        self.new_dialog = None;

        self.creation_cancelled = false;
        // Filter out the stub from existing instances so the builder doesn't
        // treat its placeholder title as a duplicate to auto-increment.
        let existing_instances: Vec<Instance> = self
            .instances
            .iter()
            .filter(|i| i.id != stub_id)
            .cloned()
            .collect();
        let request = CreationRequest {
            data,
            existing_instances,
            hooks,
        };
        self.creation_poller.request_creation(request);
    }

    /// Mark the current creation operation as cancelled
    pub fn cancel_creation(&mut self) {
        if self.creation_poller.is_pending() {
            self.creation_cancelled = true;
        }
        // Remove the stub instance
        if let Some(stub_id) = self.creating_stub_id.take() {
            self.remove_instance(&stub_id);
            self.creating_hook_progress.remove(&stub_id);
            self.rebuild_group_trees();
            self.flat_items = self.build_flat_items();
            self.update_selected();
        }
        self.new_dialog = None;
    }

    /// Apply any pending creation results from the background poller.
    /// Returns Some(session_id) if creation succeeded and we should attach.
    pub fn apply_creation_results(&mut self) -> Option<String> {
        use super::creation_poller::CreationResult;
        use crate::session::builder::{self, CreatedWorktree};
        use std::path::PathBuf;

        let result = self.creation_poller.try_recv_result()?;

        // Clean up the stub and progress tracking
        let stub_id = self.creating_stub_id.take();
        if let Some(ref id) = stub_id {
            self.creating_hook_progress.remove(id);
        }

        // Check if the user cancelled while waiting
        if self.creation_cancelled {
            self.creation_cancelled = false;
            if let Some(id) = &stub_id {
                self.remove_instance(id);
            }
            if let CreationResult::Success {
                ref instance,
                ref created_worktree,
                ..
            } = result
            {
                let worktree = created_worktree.as_ref().map(|wt| CreatedWorktree {
                    path: PathBuf::from(&wt.path),
                    main_repo_path: PathBuf::from(&wt.main_repo_path),
                });
                builder::cleanup_instance(instance, worktree.as_ref(), &[]);
            }
            self.rebuild_group_trees();
            self.flat_items = self.build_flat_items();
            self.update_selected();
            return None;
        }

        match result {
            CreationResult::Success {
                session_id,
                instance,
                on_launch_hooks_ran,
                ..
            } => {
                // Remove the stub instance
                if let Some(id) = &stub_id {
                    self.remove_instance(id);
                }

                let mut instance = *instance;
                let target_profile = self.creation_poller.last_profile().unwrap_or_else(|| {
                    self.active_profile
                        .clone()
                        .unwrap_or_else(|| "default".to_string())
                });
                instance.source_profile = target_profile.clone();

                // Ensure target profile storage exists
                if !self.storages.contains_key(&target_profile) {
                    if let Ok(s) = Storage::new(&target_profile) {
                        self.storages.insert(target_profile.clone(), s);
                    }
                }

                self.add_instance(instance.clone());
                self.rebuild_group_trees();
                if !instance.group_path.is_empty() {
                    if let Some(tree) = self.group_trees.get_mut(&target_profile) {
                        tree.create_group(&instance.group_path);
                    }
                }

                if let Err(e) = self.save() {
                    tracing::error!("Failed to save after creation: {}", e);
                }

                if on_launch_hooks_ran {
                    self.on_launch_hooks_ran.insert(session_id.clone());
                }

                let _ = self.reload();
                self.new_dialog = None;

                Some(session_id)
            }
            CreationResult::Error(error) => {
                // Remove the stub and show the error in an info dialog
                if let Some(id) = &stub_id {
                    self.remove_instance(id);
                    self.rebuild_group_trees();
                    self.flat_items = self.build_flat_items();
                    self.update_selected();
                    self.info_dialog = Some(InfoDialog::new("Creation Failed", &error));
                } else if let Some(dialog) = &mut self.new_dialog {
                    dialog.set_loading(false);
                    dialog.set_error(error);
                }
                None
            }
        }
    }

    /// Check if on_launch hooks already ran for this session (and consume the flag).
    pub fn take_on_launch_hooks_ran(&mut self, session_id: &str) -> bool {
        self.on_launch_hooks_ran.remove(session_id)
    }

    /// Check if there's a pending creation operation
    pub fn is_creation_pending(&self) -> bool {
        self.creation_poller.is_pending()
    }

    /// Check if the currently selected session is the in-flight creating stub
    pub fn is_creating_stub_selected(&self) -> bool {
        match (&self.creating_stub_id, &self.selected_session) {
            (Some(stub_id), Some(selected)) => stub_id == selected,
            _ => false,
        }
    }

    /// Show a confirmation dialog warning that a session is being created.
    pub fn show_quit_during_creation_confirm(&mut self) {
        self.confirm_dialog = Some(ConfirmDialog::new(
            "Session Creating",
            "A session is still being created. Quit anyway? The hook will be cancelled.",
            "quit_during_creation",
        ));
    }

    /// Clean up a pending creation on TUI shutdown. Waits briefly for the
    /// background thread to finish so we can clean up worktrees/instances.
    /// If the thread doesn't finish in time, the hook subprocess will
    /// complete on its own and orphaned Creating stubs are cleaned up on
    /// next launch.
    pub fn cleanup_pending_creation(&mut self) {
        if !self.creation_poller.is_pending() {
            return;
        }
        self.creation_cancelled = true;
        if let Some(stub_id) = self.creating_stub_id.take() {
            self.remove_instance(&stub_id);
            self.creating_hook_progress.remove(&stub_id);
        }

        // Wait briefly for the background thread to finish
        let result = self
            .creation_poller
            .recv_result_timeout(std::time::Duration::from_secs(2));

        if let Some(crate::tui::creation_poller::CreationResult::Success {
            ref instance,
            ref created_worktree,
            ..
        }) = result
        {
            let worktree =
                created_worktree
                    .as_ref()
                    .map(|wt| crate::session::builder::CreatedWorktree {
                        path: std::path::PathBuf::from(&wt.path),
                        main_repo_path: std::path::PathBuf::from(&wt.main_repo_path),
                    });
            crate::session::builder::cleanup_instance(instance, worktree.as_ref(), &[]);
            tracing::info!("Cleaned up cancelled session on exit");
        }
    }

    /// Tick dialog animations/timers and drain hook progress.
    /// Returns true when a redraw is needed.
    pub fn tick_dialog(&mut self) -> bool {
        use crate::session::repo_config::HookProgress;

        let mut changed = false;

        if let Some(dialog) = &mut self.new_dialog {
            if dialog.tick() {
                changed = true;
            }

            if dialog.is_loading() {
                // Drain all pending hook progress messages
                while let Some(progress) = self.creation_poller.try_recv_progress() {
                    dialog.push_hook_progress(progress);
                    changed = true;
                }
            }
        }

        // Poll serve dialog for subprocess startup events.
        #[cfg(feature = "serve")]
        if let Some(view) = &mut self.serve_view {
            if view.tick() {
                changed = true;
            }
        }

        // Drain hook progress into the creating buffer when no dialog is open
        if self.new_dialog.is_none() {
            if let Some(ref stub_id) = self.creating_stub_id {
                let stub_id = stub_id.clone();
                if let Some(progress_buf) = self.creating_hook_progress.get_mut(&stub_id) {
                    while let Some(progress) = self.creation_poller.try_recv_progress() {
                        match progress {
                            HookProgress::Started(cmd) => {
                                progress_buf.current_hook = Some(cmd);
                            }
                            HookProgress::Output(line) => {
                                progress_buf.hook_output.push(line);
                                // Cap buffer to prevent unbounded memory growth
                                if progress_buf.hook_output.len() > 1000 {
                                    progress_buf.hook_output.drain(..500);
                                }
                            }
                        }
                        changed = true;
                    }
                }
            }
        }

        changed
    }

    /// Whether the user is currently looking at a surface where they're
    /// likely to want to copy text (URLs, error messages, release notes).
    /// The App uses this to release xterm mouse capture so the terminal's
    /// native drag-to-select works without a modifier; mouse capture comes
    /// back as soon as the surface is dismissed.
    ///
    /// Add new dialogs here only when their content is meant to be copied,
    /// not for every modal: capture toggling has a small but visible cost
    /// (the wheel-scroll on the dashboard preview won't work while it's
    /// off).
    pub fn wants_text_selection(&self) -> bool {
        #[cfg(feature = "serve")]
        let serve_open = self.serve_view.is_some();
        #[cfg(not(feature = "serve"))]
        let serve_open = false;

        serve_open || self.info_dialog.is_some() || self.changelog_dialog.is_some()
    }

    pub fn has_dialog(&self) -> bool {
        #[cfg(feature = "serve")]
        let serve_open = self.serve_view.is_some();
        #[cfg(not(feature = "serve"))]
        let serve_open = false;

        self.show_help
            || self.search_active
            || self.new_dialog.is_some()
            || self.confirm_dialog.is_some()
            || self.unified_delete_dialog.is_some()
            || self.group_delete_options_dialog.is_some()
            || self.rename_dialog.is_some()
            || self.hook_trust_dialog.is_some()
            || self.hooks_install_dialog.is_some()
            || self.welcome_dialog.is_some()
            || self.no_agents_dialog.is_some()
            || self.changelog_dialog.is_some()
            || self.info_dialog.is_some()
            || self.profile_picker_dialog.is_some()
            || self.send_message_dialog.is_some()
            || serve_open
            || self.settings_view.is_some()
            || self.diff_view.is_some()
    }

    pub fn shrink_list(&mut self) {
        self.list_width = self.list_width.saturating_sub(5).max(10);
        self.save_list_width();
    }

    pub fn grow_list(&mut self) {
        self.list_width = (self.list_width + 5).min(80);
        self.save_list_width();
    }

    fn save_list_width(&self) {
        if let Ok(mut config) = load_config().map(|c| c.unwrap_or_default()) {
            config.app_state.home_list_width = Some(self.list_width);
            let _ = save_config(&config);
        }
    }

    pub fn show_welcome(&mut self) {
        self.welcome_dialog = Some(WelcomeDialog::new());
    }

    pub fn show_no_agents(&mut self) {
        self.no_agents_dialog = Some(NoAgentsDialog::new());
    }

    /// Replace available tools (used after re-check from no-agents dialog).
    pub fn set_available_tools(&mut self, tools: AvailableTools) {
        self.available_tools = tools;
    }

    pub fn show_changelog(&mut self, from_version: Option<String>) {
        self.changelog_dialog = Some(ChangelogDialog::new(from_version));
    }

    pub fn instances(&self) -> &[Instance] {
        &self.instances
    }

    pub fn get_instance(&self, id: &str) -> Option<&Instance> {
        self.instance_map.get(id)
    }

    /// Returns true if any session has an animated status (Running, Waiting, Starting,
    /// Creating), which means the TUI needs periodic redraws for spinner animation.
    pub fn has_animated_sessions(&self) -> bool {
        use crate::session::Status;
        self.instances.iter().any(|inst| {
            matches!(
                inst.status,
                Status::Running | Status::Waiting | Status::Starting | Status::Creating
            )
        })
    }

    pub(super) fn build_flat_items(&self) -> Vec<Item> {
        if self.group_by == GroupByMode::Project {
            return self.build_flat_items_by_project();
        }

        if let Some(profile) = &self.active_profile {
            let filtered: Vec<Instance> = self
                .instances
                .iter()
                .filter(|i| i.source_profile == *profile)
                .cloned()
                .collect();
            match self.group_trees.get(profile) {
                Some(tree) => flatten_tree(tree, &filtered, self.sort_order),
                None => Vec::new(),
            }
        } else if self.storages.len() <= 1 {
            match self.group_trees.values().next() {
                Some(tree) => flatten_tree(tree, &self.instances, self.sort_order),
                None => Vec::new(),
            }
        } else {
            flatten_tree_all_profiles(&self.instances, &self.group_trees, self.sort_order)
        }
    }

    fn build_flat_items_by_project(&self) -> Vec<Item> {
        // In project mode, always merge all sessions into one tree regardless of
        // profile count. Project grouping unifies by repo across profiles.
        let base_instances: Vec<Instance> = if let Some(profile) = &self.active_profile {
            self.instances
                .iter()
                .filter(|i| i.source_profile == *profile)
                .cloned()
                .collect()
        } else {
            self.instances.clone()
        };

        let grouped: Vec<Instance> = base_instances
            .into_iter()
            .map(|mut inst| {
                inst.group_path = project_group_name(&inst);
                inst
            })
            .collect();

        let mut tree = GroupTree::new_with_groups(&grouped, &[]);
        for (path, &collapsed) in &self.project_group_collapsed {
            if collapsed {
                tree.set_collapsed(path, true);
            }
        }
        flatten_tree(&tree, &grouped, self.sort_order)
    }

    pub fn active_profile_display(&self) -> &str {
        self.active_profile.as_deref().unwrap_or("all")
    }

    /// Switch the active profile filter in-place without destroying the view.
    /// Pass `None` for all-profiles mode, or `Some(name)` to filter to one profile.
    pub fn switch_profile(&mut self, new_profile: Option<String>) -> anyhow::Result<()> {
        self.active_profile = new_profile;
        // Clear selection before reload so stale session/group refs don't linger
        self.selected_session = None;
        self.selected_group = None;
        self.selected_group_profile = None;
        self.reload()?;
        self.refresh_from_config();
        // Invalidate preview caches since the visible sessions changed
        self.preview_cache = PreviewCache::default();
        self.terminal_preview_cache = PreviewCache::default();
        self.container_terminal_preview_cache = PreviewCache::default();
        self.preview_scroll_offset = 0;
        // Clear search since match indices are invalid with new flat_items
        if self.search_active {
            self.search_active = false;
            self.search_query = Input::default();
            self.search_matches.clear();
            self.search_match_index = 0;
        }
        Ok(())
    }

    /// Show the profile picker dialog with fresh data from disk.
    pub(super) fn show_profile_picker(&mut self) {
        use crate::session::list_profiles;
        use crate::tui::dialogs::{ProfileEntry, ProfilePickerDialog};

        let current_profile = self
            .active_profile
            .clone()
            .unwrap_or_else(|| "all".to_string());
        let profiles = list_profiles().unwrap_or_else(|_| vec!["default".to_string()]);
        let mut entries: Vec<ProfileEntry> = profiles
            .iter()
            .map(|name| {
                let session_count = Storage::new(name)
                    .and_then(|s| s.load())
                    .map(|instances| instances.len())
                    .unwrap_or(0);
                ProfileEntry {
                    name: name.clone(),
                    session_count,
                    is_active: self.active_profile.as_deref() == Some(name.as_str()),
                }
            })
            .collect();

        // In filtered mode, add "all" entry at top
        if self.active_profile.is_some() {
            let total: usize = entries.iter().map(|e| e.session_count).sum();
            entries.insert(
                0,
                ProfileEntry {
                    name: "all".to_string(),
                    session_count: total,
                    is_active: false,
                },
            );
        }

        self.profile_picker_dialog = Some(ProfilePickerDialog::new(entries, &current_profile));
    }

    pub fn set_instance_status(&mut self, id: &str, status: crate::session::Status) {
        self.mutate_instance(id, |inst| inst.status = status);
    }

    /// Stamp `last_accessed_at` on a session (user-initiated interaction).
    pub fn stamp_last_accessed(&mut self, id: &str) {
        self.mutate_instance(id, |inst| inst.touch_last_accessed());
    }

    pub fn save(&self) -> anyhow::Result<()> {
        for (profile_name, storage) in &self.storages {
            let profile_instances: Vec<Instance> = self
                .instances
                .iter()
                .filter(|i| i.source_profile == *profile_name)
                .cloned()
                .collect();
            // Each profile has its own GroupTree with correct collapsed state
            let tree = self
                .group_trees
                .get(profile_name)
                .cloned()
                .unwrap_or_else(|| GroupTree::new_with_groups(&profile_instances, &[]));
            storage.save_with_groups(&profile_instances, &tree)?;
        }
        Ok(())
    }

    /// Rebuild all per-profile GroupTrees from the current instances,
    /// preserving each tree's collapsed state.
    pub(super) fn rebuild_group_trees(&mut self) {
        for (profile_name, tree) in &mut self.group_trees {
            let existing_groups = tree.get_all_groups();
            let profile_instances: Vec<Instance> = self
                .instances
                .iter()
                .filter(|i| i.source_profile == *profile_name)
                .cloned()
                .collect();
            *tree = GroupTree::new_with_groups(&profile_instances, &existing_groups);
        }
    }

    /// Determine which profile the item at the given cursor position belongs to.
    pub(super) fn profile_for_cursor(&self, cursor: usize) -> Option<String> {
        if let Some(profile) = &self.active_profile {
            return Some(profile.clone());
        }
        if let Some(item) = self.flat_items.get(cursor) {
            match item {
                crate::session::Item::Session { id, .. } => {
                    return self
                        .get_instance(id.as_str())
                        .map(|i| i.source_profile.clone());
                }
                crate::session::Item::Group { profile, path, .. } => {
                    if let Some(p) = profile {
                        return Some(p.clone());
                    }
                    // Fallback for single-profile mode: find any instance in this group
                    return self
                        .instances
                        .iter()
                        .find(|i| {
                            i.group_path == *path || i.group_path.starts_with(&format!("{}/", path))
                        })
                        .map(|i| i.source_profile.clone());
                }
            }
        }
        None
    }

    /// Collect all groups from all per-profile GroupTrees.
    pub(super) fn all_groups(&self) -> Vec<Group> {
        self.group_trees
            .values()
            .flat_map(|t| t.get_all_groups())
            .collect()
    }

    /// Check if any profile has groups, without collecting them all.
    pub(super) fn has_any_groups(&self) -> bool {
        self.group_trees
            .values()
            .any(|t| !t.get_all_groups().is_empty())
    }

    /// Centralized instance addition: adds to both the `instances` vec
    /// and `instance_map` to keep both collections in sync.
    pub(super) fn add_instance(&mut self, instance: Instance) {
        self.instance_map
            .insert(instance.id.clone(), instance.clone());
        self.instances.push(instance);
    }

    /// Centralized instance removal: removes from both the `instances` vec
    /// and `instance_map` to keep both collections in sync.
    pub(super) fn remove_instance(&mut self, id: &str) {
        self.instances.retain(|i| i.id != id);
        self.instance_map.remove(id);
    }

    /// Centralized instance mutation: applies `f` once to the `instances` vec
    /// entry, then clones the result into `instance_map`. This guarantees both
    /// collections stay in sync even for non-idempotent closures.
    pub(super) fn mutate_instance(&mut self, id: &str, f: impl FnOnce(&mut Instance)) {
        if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
            f(inst);
            self.instance_map.insert(id.to_string(), inst.clone());
        }
    }

    /// Like `mutate_instance`, but for fallible operations. Clones the entry,
    /// applies `f` to the clone, and writes back to both collections only on
    /// success -- neither collection is modified on error.
    pub(super) fn try_mutate_instance(
        &mut self,
        id: &str,
        f: impl FnOnce(&mut Instance) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
            let mut updated = inst.clone();
            f(&mut updated)?;
            *inst = updated.clone();
            self.instance_map.insert(id.to_string(), updated);
        }
        Ok(())
    }

    pub fn set_instance_error(&mut self, id: &str, error: Option<String>) {
        self.mutate_instance(id, |inst| inst.last_error = error);
    }

    pub fn start_terminal_for_instance_with_size(
        &mut self,
        id: &str,
        size: Option<(u16, u16)>,
    ) -> anyhow::Result<()> {
        self.try_mutate_instance(id, |inst| inst.start_terminal_with_size(size))?;
        self.save()?;
        Ok(())
    }

    pub fn restart_instance_with_size_opts(
        &mut self,
        id: &str,
        size: Option<(u16, u16)>,
        skip_on_launch: bool,
    ) -> anyhow::Result<()> {
        self.try_mutate_instance(id, |inst| inst.restart_with_size_opts(size, skip_on_launch))
    }

    pub fn select_session_by_id(&mut self, session_id: &str) {
        for (idx, item) in self.flat_items.iter().enumerate() {
            if let Item::Session { id, .. } = item {
                if id == session_id {
                    self.cursor = idx;
                    self.update_selected();
                    return;
                }
            }
        }
    }

    /// Get the terminal mode for a session (uses config default if not set)
    pub fn get_terminal_mode(&self, session_id: &str) -> TerminalMode {
        self.terminal_modes
            .get(session_id)
            .copied()
            .unwrap_or(self.default_terminal_mode)
    }

    /// Refresh all config-dependent state from the current profile's config.
    /// Call this after settings are saved to pick up any changes.
    pub fn refresh_from_config(&mut self) {
        let profile = self.active_profile.as_deref().unwrap_or("default");
        if let Ok(config) = resolve_config(profile) {
            // Refresh default terminal mode for sandboxed sessions
            self.default_terminal_mode = match config.sandbox.default_terminal_mode {
                DefaultTerminalMode::Host => TerminalMode::Host,
                DefaultTerminalMode::Container => TerminalMode::Container,
            };

            // Refresh sound config
            self.sound_config = config.sound.clone();

            // Refresh strict-hotkeys mode
            self.strict_hotkeys = config.session.strict_hotkeys;
        }
    }

    /// Toggle terminal mode between Container and Host for a session
    pub fn toggle_terminal_mode(&mut self, session_id: &str) {
        let current = self.get_terminal_mode(session_id);
        let new_mode = match current {
            TerminalMode::Container => TerminalMode::Host,
            TerminalMode::Host => TerminalMode::Container,
        };
        self.terminal_modes.insert(session_id.to_string(), new_mode);
    }

    pub fn start_container_terminal_for_instance_with_size(
        &mut self,
        id: &str,
        size: Option<(u16, u16)>,
    ) -> anyhow::Result<()> {
        self.try_mutate_instance(id, |inst| inst.start_container_terminal_with_size(size))
    }
}
