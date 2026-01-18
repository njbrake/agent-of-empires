//! Home view - main session list and navigation

mod input;
mod operations;
mod render;

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::time::Instant;

use crate::session::{flatten_tree, Group, GroupTree, Instance, Item, Storage};
use crate::tmux::AvailableTools;

use super::deletion_poller::DeletionPoller;
use super::dialogs::{
    ChangelogDialog, ConfirmDialog, GroupDeleteOptionsDialog, InfoDialog, NewSessionDialog,
    RenameDialog, UnifiedDeleteDialog, WelcomeDialog,
};
use super::status_poller::StatusPoller;

/// View mode for the home screen
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    #[default]
    Agent,
    Terminal,
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
    pub(super) welcome_dialog: Option<WelcomeDialog>,
    pub(super) changelog_dialog: Option<ChangelogDialog>,
    pub(super) info_dialog: Option<InfoDialog>,

    // Search
    pub(super) search_active: bool,
    pub(super) search_query: String,
    pub(super) filtered_items: Option<Vec<usize>>,

    // Tool availability
    pub(super) available_tools: AvailableTools,

    // Performance: background status polling
    pub(super) status_poller: StatusPoller,
    pub(super) pending_status_refresh: bool,

    // Performance: background deletion
    pub(super) deletion_poller: DeletionPoller,

    // Performance: preview caching
    pub(super) preview_cache: PreviewCache,
    pub(super) terminal_preview_cache: PreviewCache,
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
            welcome_dialog: None,
            changelog_dialog: None,
            info_dialog: None,
            search_active: false,
            search_query: String::new(),
            filtered_items: None,
            available_tools,
            status_poller: StatusPoller::new(),
            pending_status_refresh: false,
            deletion_poller: DeletionPoller::new(),
            preview_cache: PreviewCache::default(),
            terminal_preview_cache: PreviewCache::default(),
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
                        inst.status = update.status;
                        inst.last_error = update.last_error.clone();
                    }
                    if update.claude_session_id.is_some() {
                        inst.claude_session_id = update.claude_session_id.clone();
                    }
                }
                if let Some(inst) = self.instance_map.get_mut(&update.id) {
                    if inst.status != Status::Deleting {
                        inst.status = update.status;
                        inst.last_error = update.last_error;
                    }
                    if update.claude_session_id.is_some() {
                        inst.claude_session_id = update.claude_session_id;
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

    pub fn has_dialog(&self) -> bool {
        self.show_help
            || self.new_dialog.is_some()
            || self.confirm_dialog.is_some()
            || self.unified_delete_dialog.is_some()
            || self.group_delete_options_dialog.is_some()
            || self.rename_dialog.is_some()
            || self.welcome_dialog.is_some()
            || self.changelog_dialog.is_some()
            || self.info_dialog.is_some()
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

    pub fn start_terminal_for_instance(&mut self, id: &str) -> anyhow::Result<()> {
        if let Some(inst) = self.instances.iter_mut().find(|i| i.id == id) {
            inst.start_terminal()?;
        }
        if let Some(inst) = self.instance_map.get_mut(id) {
            inst.start_terminal()?;
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
}
