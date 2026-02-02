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
    config::{load_config, save_config},
    flatten_tree, resolve_config, DefaultTerminalMode, Group, GroupTree, Instance, Item, Storage,
};
use crate::tmux::AvailableTools;

use super::creation_poller::{CreationPoller, CreationRequest};
use super::deletion_poller::DeletionPoller;
use super::dialogs::{
    ChangelogDialog, ConfirmDialog, GroupDeleteOptionsDialog, HookTrustDialog, InfoDialog,
    NewSessionData, NewSessionDialog, RenameDialog, UnifiedDeleteDialog, WelcomeDialog,
};
use super::diff::DiffView;
use super::settings::SettingsView;
use super::status_poller::StatusPoller;

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
    "  ",
    "    ",
    "      ",
    "        ",
    "          ",
    "            ",
    "              ",
    "                ",
    "                  ",
];

pub(super) fn get_indent(depth: usize) -> &'static str {
    INDENTS.get(depth).copied().unwrap_or(INDENTS[9])
}

pub(super) const ICON_RUNNING: &str = "●";
pub(super) const ICON_WAITING: &str = "◐";
pub(super) const ICON_IDLE: &str = "○";
pub(super) const ICON_ERROR: &str = "✕";
pub(super) const ICON_STARTING: &str = "◌";
pub(super) const ICON_DELETING: &str = "✗";
pub(super) const ICON_COLLAPSED: &str = "▶";
pub(super) const ICON_EXPANDED: &str = "▼";

pub struct HomeView {
    pub(super) storage: Storage,
    pub(super) instances: Vec<Instance>,
    pub(super) instance_map: HashMap<String, Instance>,
    pub(super) groups: Vec<Group>,
    pub(super) group_tree: GroupTree,
    pub(super) flat_items: Vec<Item>,

    // UI state
    pub(super) cursor: usize,
    pub(super) selected_session: Option<String>,
    pub(super) selected_group: Option<String>,
    pub(super) view_mode: ViewMode,

    // Dialogs
    pub(super) show_help: bool,
    pub(super) new_dialog: Option<NewSessionDialog>,
    pub(super) confirm_dialog: Option<ConfirmDialog>,
    pub(super) unified_delete_dialog: Option<UnifiedDeleteDialog>,
    pub(super) group_delete_options_dialog: Option<GroupDeleteOptionsDialog>,
    pub(super) rename_dialog: Option<RenameDialog>,
    pub(super) hook_trust_dialog: Option<HookTrustDialog>,
    /// Session data pending hook trust approval
    pub(super) pending_hook_trust_data: Option<NewSessionData>,
    pub(super) welcome_dialog: Option<WelcomeDialog>,
    pub(super) changelog_dialog: Option<ChangelogDialog>,
    pub(super) info_dialog: Option<InfoDialog>,

    // Search
    pub(super) search_active: bool,
    pub(super) search_query: Input,
    pub(super) filtered_items: Option<Vec<usize>>,

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
    pub fn new(storage: Storage, available_tools: AvailableTools) -> anyhow::Result<Self> {
        let (mut instances, groups) = storage.load_with_groups()?;

        for inst in &mut instances {
            inst.update_search_cache();
        }

        let instance_map: HashMap<String, Instance> = instances
            .iter()
            .map(|i| (i.id.clone(), i.clone()))
            .collect();
        let group_tree = GroupTree::new_with_groups(&instances, &groups);
        let flat_items = flatten_tree(&group_tree, &instances);

        // Load the resolved config to get the default terminal mode and sound config
        let resolved = resolve_config(storage.profile());
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

        let mut view = Self {
            storage,
            instances,
            instance_map,
            groups,
            group_tree,
            flat_items,
            cursor: 0,
            selected_session: None,
            selected_group: None,
            view_mode: ViewMode::default(),
            show_help: false,
            new_dialog: None,
            confirm_dialog: None,
            unified_delete_dialog: None,
            group_delete_options_dialog: None,
            rename_dialog: None,
            hook_trust_dialog: None,
            pending_hook_trust_data: None,
            welcome_dialog: None,
            changelog_dialog: None,
            info_dialog: None,
            search_active: false,
            search_query: Input::default(),
            filtered_items: None,
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
            list_width: load_config()
                .ok()
                .flatten()
                .and_then(|c| c.app_state.home_list_width)
                .unwrap_or(35),
        };

        view.update_selected();
        Ok(view)
    }

    pub fn reload(&mut self) -> anyhow::Result<()> {
        let (mut instances, groups) = self.storage.load_with_groups()?;

        for inst in &mut instances {
            if let Some(prev) = self.instance_map.get(&inst.id) {
                inst.status = prev.status;
                inst.last_error = prev.last_error.clone();
                inst.last_error_check = prev.last_error_check;
                inst.last_start_time = prev.last_start_time;
            }
            inst.update_search_cache();
        }

        self.instances = instances;
        self.instance_map = self
            .instances
            .iter()
            .map(|i| (i.id.clone(), i.clone()))
            .collect();
        self.groups = groups;
        self.group_tree = GroupTree::new_with_groups(&self.instances, &self.groups);
        self.flat_items = flatten_tree(&self.group_tree, &self.instances);

        if self.cursor >= self.flat_items.len() && !self.flat_items.is_empty() {
            self.cursor = self.flat_items.len() - 1;
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
                if let Some(inst) = self.instances.iter_mut().find(|i| i.id == update.id) {
                    if inst.status != Status::Deleting {
                        let old_status = inst.status;
                        inst.status = update.status;
                        inst.last_error = update.last_error.clone();
                        if old_status != update.status {
                            crate::sound::play_for_transition(
                                old_status,
                                update.status,
                                &self.sound_config,
                            );
                        }
                    }
                }
                if let Some(inst) = self.instance_map.get_mut(&update.id) {
                    if inst.status != Status::Deleting {
                        inst.status = update.status;
                        inst.last_error = update.last_error;
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
                self.instances.retain(|i| i.id != result.session_id);
                self.instance_map.remove(&result.session_id);
                self.group_tree = GroupTree::new_with_groups(&self.instances, &self.groups);

                if let Err(e) = self
                    .storage
                    .save_with_groups(&self.instances, &self.group_tree)
                {
                    tracing::error!("Failed to save after deletion: {}", e);
                }
                let _ = self.reload();
            } else {
                if let Some(inst) = self
                    .instances
                    .iter_mut()
                    .find(|i| i.id == result.session_id)
                {
                    inst.status = Status::Error;
                    inst.last_error = result.error.clone();
                }
                if let Some(inst) = self.instance_map.get_mut(&result.session_id) {
                    inst.status = Status::Error;
                    inst.last_error = result.error;
                }
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
                builder::cleanup_instance(instance, worktree.as_ref());
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
                let instance = *instance;
                self.instances.push(instance.clone());
                self.group_tree = GroupTree::new_with_groups(&self.instances, &self.groups);
                if !instance.group_path.is_empty() {
                    self.group_tree.create_group(&instance.group_path);
                }

                if let Err(e) = self
                    .storage
                    .save_with_groups(&self.instances, &self.group_tree)
                {
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

    /// Tick the dialog spinner animation if loading, and drain hook progress
    pub fn tick_dialog(&mut self) {
        if let Some(dialog) = &mut self.new_dialog {
            if dialog.is_loading() {
                dialog.tick();
                // Drain all pending hook progress messages
                while let Some(progress) = self.creation_poller.try_recv_progress() {
                    dialog.push_hook_progress(progress);
                }
            }
        }
    }

    pub fn has_dialog(&self) -> bool {
        self.show_help
            || self.new_dialog.is_some()
            || self.confirm_dialog.is_some()
            || self.unified_delete_dialog.is_some()
            || self.group_delete_options_dialog.is_some()
            || self.rename_dialog.is_some()
            || self.hook_trust_dialog.is_some()
            || self.welcome_dialog.is_some()
            || self.changelog_dialog.is_some()
            || self.info_dialog.is_some()
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

    pub fn get_instance(&self, id: &str) -> Option<&Instance> {
        self.instance_map.get(id)
    }

    pub fn available_tools(&self) -> AvailableTools {
        self.available_tools.clone()
    }

    pub(super) fn get_next_profile(&self) -> Option<String> {
        use crate::session::list_profiles;

        let profiles = list_profiles().ok()?;
        if profiles.len() <= 1 {
            return None;
        }
        let current = self.storage.profile();
        let current_idx = profiles.iter().position(|p| p == current).unwrap_or(0);
        let next_idx = (current_idx + 1) % profiles.len();
        Some(profiles[next_idx].clone())
    }

    pub fn set_instance_error(&mut self, id: &str, error: Option<String>) {
        if let Some(inst) = self.instance_map.get_mut(id) {
            inst.last_error = error.clone();
        }
        if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
            inst.last_error = error;
        }
    }

    pub fn start_terminal_for_instance_with_size(
        &mut self,
        id: &str,
        size: Option<(u16, u16)>,
    ) -> anyhow::Result<()> {
        if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
            inst.start_terminal_with_size(size)?;
        }
        if let Some(inst) = self.instance_map.get_mut(id) {
            inst.start_terminal_with_size(size)?;
        }
        self.storage
            .save_with_groups(&self.instances, &self.group_tree)?;
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
        if let Ok(config) = resolve_config(self.storage.profile()) {
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
        if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
            inst.start_container_terminal_with_size(size)?;
        }
        if let Some(inst) = self.instance_map.get_mut(id) {
            inst.start_container_terminal_with_size(size)?;
        }
        // Don't save terminal info for container terminals - it's ephemeral
        Ok(())
    }
}
