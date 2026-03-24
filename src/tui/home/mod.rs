//! Home view - main session list and navigation

mod input;
mod operations;
mod render;

#[cfg(test)]
mod tests;

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use tui_input::Input;

use crate::session::{
    config::{load_config, save_config, SortOrder},
    flatten_tree, flatten_tree_all_profiles, resolve_config, DefaultTerminalMode, Group, GroupTree,
    Instance, Item, Storage,
};
use crate::tmux::AvailableTools;

use super::creation_poller::{CreationPoller, CreationRequest};
use super::deletion_poller::DeletionPoller;
use super::dialogs::{
    ChangelogDialog, ConfirmDialog, GroupDeleteOptionsDialog, HookTrustDialog, HooksInstallDialog,
    InfoDialog, NewSessionData, NewSessionDialog, ProfilePickerDialog, RenameDialog,
    UnifiedDeleteDialog, WelcomeDialog,
};
use super::diff::DiffView;
use super::settings::SettingsView;
use super::status_poller::StatusPoller;

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
}

impl Default for PreviewCache {
    fn default() -> Self {
        Self {
            session_id: None,
            content: String::new(),
            last_refresh: Instant::now(),
            dimensions: (0, 0),
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

pub(super) const ICON_RUNNING: &str = "●";
pub(super) const ICON_WAITING: &str = "◐";
pub(super) const ICON_IDLE: &str = "○";
pub(super) const ICON_ERROR: &str = "✕";
pub(super) const ICON_STARTING: &str = "◌";
pub(super) const ICON_UNKNOWN: &str = "?";
pub(super) const ICON_STOPPED: &str = "■";
pub(super) const ICON_DELETING: &str = "✗";
pub(super) const ICON_COLLAPSED: &str = "▶";
pub(super) const ICON_EXPANDED: &str = "▼";

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
    pub(super) changelog_dialog: Option<ChangelogDialog>,
    pub(super) info_dialog: Option<InfoDialog>,
    pub(super) profile_picker_dialog: Option<ProfilePickerDialog>,
    pub(super) send_message_dialog: Option<super::dialogs::SendMessageDialog>,
    /// Session to receive the message from the send dialog
    pub(super) pending_send_session: Option<String>,
    /// Session to attach after the custom instruction warning dialog is dismissed
    pub(super) pending_attach_after_warning: Option<String>,
    /// Session to stop after the confirmation dialog is accepted
    pub(super) pending_stop_session: Option<String>,
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

    // Performance: preview caching
    pub(super) preview_cache: PreviewCache,
    pub(super) terminal_preview_cache: PreviewCache,
    pub(super) container_terminal_preview_cache: PreviewCache,

    // Terminal mode for sandboxed sessions (per-session, ephemeral)
    pub(super) terminal_modes: HashMap<String, TerminalMode>,
    // Default terminal mode from config
    pub(super) default_terminal_mode: TerminalMode,

    // Sound config for state transition sounds
    pub(super) sound_config: crate::sound::SoundConfig,

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
        let user_config = load_config().ok().flatten();
        let sort_order = user_config
            .as_ref()
            .and_then(|c| c.app_state.sort_order)
            .unwrap_or_default();

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
            changelog_dialog: None,
            info_dialog: None,
            profile_picker_dialog: None,
            send_message_dialog: None,
            pending_send_session: None,
            pending_attach_after_warning: None,
            pending_stop_session: None,
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
            preview_cache: PreviewCache::default(),
            terminal_preview_cache: PreviewCache::default(),
            container_terminal_preview_cache: PreviewCache::default(),
            terminal_modes: HashMap::new(),
            default_terminal_mode,
            sound_config,
            settings_view: None,
            settings_close_confirm: false,
            diff_view: None,
            list_width: user_config
                .and_then(|c| c.app_state.home_list_width)
                .unwrap_or(35),
        };

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
        self.instance_map = self
            .instances
            .iter()
            .map(|i| (i.id.clone(), i.clone()))
            .collect();
        self.flat_items = self.build_flat_items();

        if self.cursor >= self.flat_items.len() && !self.flat_items.is_empty() {
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

    /// Request background session creation. Used for sandbox sessions to avoid blocking UI.
    pub fn request_creation(
        &mut self,
        data: NewSessionData,
        hooks: Option<crate::session::HooksConfig>,
    ) {
        let has_hooks = hooks
            .as_ref()
            .is_some_and(|h| !h.on_create.is_empty() || !h.on_launch.is_empty());
        if let Some(dialog) = &mut self.new_dialog {
            dialog.set_loading(true);
            dialog.set_has_hooks(has_hooks);
        }

        self.creation_cancelled = false;
        let request = CreationRequest {
            data,
            existing_instances: self.instances.clone(),
            hooks,
        };
        self.creation_poller.request_creation(request);
    }

    /// Mark the current creation operation as cancelled (user pressed Esc)
    pub fn cancel_creation(&mut self) {
        if self.creation_poller.is_pending() {
            self.creation_cancelled = true;
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

        // Check if the user cancelled while waiting
        if self.creation_cancelled {
            self.creation_cancelled = false;
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
            return None;
        }

        match result {
            CreationResult::Success {
                session_id,
                instance,
                on_launch_hooks_ran,
                ..
            } => {
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
                if let Some(dialog) = &mut self.new_dialog {
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

    /// Tick dialog animations/timers and drain hook progress.
    /// Returns true when a redraw is needed.
    pub fn tick_dialog(&mut self) -> bool {
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

        changed
    }

    pub fn has_dialog(&self) -> bool {
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
            || self.changelog_dialog.is_some()
            || self.info_dialog.is_some()
            || self.profile_picker_dialog.is_some()
            || self.send_message_dialog.is_some()
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

    pub fn show_changelog(&mut self, from_version: Option<String>) {
        self.changelog_dialog = Some(ChangelogDialog::new(from_version));
    }

    pub fn instances(&self) -> &[Instance] {
        &self.instances
    }

    pub fn get_instance(&self, id: &str) -> Option<&Instance> {
        self.instance_map.get(id)
    }

    pub(super) fn build_flat_items(&self) -> Vec<Item> {
        if let Some(profile) = &self.active_profile {
            // Filtered to a single profile -- only include that profile's instances
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
            // All-profiles mode with only one profile -- skip the profile header
            match self.group_trees.values().next() {
                Some(tree) => flatten_tree(tree, &self.instances, self.sort_order),
                None => Vec::new(),
            }
        } else {
            flatten_tree_all_profiles(&self.instances, &self.group_trees, self.sort_order)
        }
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
