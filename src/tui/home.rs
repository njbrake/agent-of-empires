//! Home view - main session list and navigation

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::*;
use std::collections::HashMap;
use std::time::Instant;

use super::app::Action;
use super::components::{HelpOverlay, Preview};
use super::deletion_poller::{DeletionPoller, DeletionRequest};
use super::dialogs::{
    ChangelogDialog, ConfirmDialog, DeleteDialogConfig, DeleteOptions, GroupDeleteOptions,
    GroupDeleteOptionsDialog, InfoDialog, NewSessionDialog, RenameDialog, UnifiedDeleteDialog,
    WelcomeDialog,
};
use super::status_poller::StatusPoller;
use super::styles::Theme;
use crate::session::{
    flatten_tree, list_profiles, Group, GroupTree, Instance, Item, Status, Storage,
};
use crate::tmux::AvailableTools;
use crate::update::UpdateInfo;

/// View mode for the home screen
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    #[default]
    Agent,
    Terminal,
}

/// Cached preview content to avoid subprocess calls on every frame
struct PreviewCache {
    session_id: Option<String>,
    content: String,
    last_refresh: Instant,
    dimensions: (u16, u16),
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

const INDENTS: [&str; 10] = [
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

fn get_indent(depth: usize) -> &'static str {
    INDENTS.get(depth).copied().unwrap_or(INDENTS[9])
}

const ICON_RUNNING: &str = "●";
const ICON_WAITING: &str = "◐";
const ICON_IDLE: &str = "○";
const ICON_ERROR: &str = "✕";
const ICON_STARTING: &str = "◌";
const ICON_DELETING: &str = "✗";
const ICON_COLLAPSED: &str = "▶";
const ICON_EXPANDED: &str = "▼";

pub struct HomeView {
    storage: Storage,
    instances: Vec<Instance>,
    instance_map: HashMap<String, Instance>,
    groups: Vec<Group>,
    group_tree: GroupTree,
    flat_items: Vec<Item>,

    // UI state
    cursor: usize,
    selected_session: Option<String>,
    selected_group: Option<String>,
    view_mode: ViewMode,

    // Dialogs
    show_help: bool,
    new_dialog: Option<NewSessionDialog>,
    confirm_dialog: Option<ConfirmDialog>,
    unified_delete_dialog: Option<UnifiedDeleteDialog>,
    group_delete_options_dialog: Option<GroupDeleteOptionsDialog>,
    rename_dialog: Option<RenameDialog>,
    welcome_dialog: Option<WelcomeDialog>,
    changelog_dialog: Option<ChangelogDialog>,
    info_dialog: Option<InfoDialog>,

    // Search
    search_active: bool,
    search_query: String,
    filtered_items: Option<Vec<usize>>,

    // Tool availability
    available_tools: AvailableTools,

    // Performance: background status polling
    status_poller: StatusPoller,
    pending_status_refresh: bool,

    // Performance: background deletion
    deletion_poller: DeletionPoller,

    // Performance: preview caching
    preview_cache: PreviewCache,
    terminal_preview_cache: PreviewCache,
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

        // Ensure cursor is valid
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

    fn get_next_profile(&self) -> Option<String> {
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

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        // Handle welcome/changelog dialogs first (highest priority)
        if let Some(dialog) = &mut self.welcome_dialog {
            match dialog.handle_key(key) {
                super::dialogs::DialogResult::Continue => {}
                super::dialogs::DialogResult::Cancel | super::dialogs::DialogResult::Submit(_) => {
                    self.welcome_dialog = None;
                }
            }
            return None;
        }

        if let Some(dialog) = &mut self.changelog_dialog {
            match dialog.handle_key(key) {
                super::dialogs::DialogResult::Continue => {}
                super::dialogs::DialogResult::Cancel | super::dialogs::DialogResult::Submit(_) => {
                    self.changelog_dialog = None;
                }
            }
            return None;
        }

        if let Some(dialog) = &mut self.info_dialog {
            match dialog.handle_key(key) {
                super::dialogs::DialogResult::Continue => {}
                super::dialogs::DialogResult::Cancel | super::dialogs::DialogResult::Submit(_) => {
                    self.info_dialog = None;
                }
            }
            return None;
        }

        // Handle other dialog input
        if self.show_help {
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
            ) {
                self.show_help = false;
            }
            return None;
        }

        let dialog_result = self
            .new_dialog
            .as_mut()
            .map(|dialog| dialog.handle_key(key));

        if let Some(result) = dialog_result {
            match result {
                super::dialogs::DialogResult::Continue => {}
                super::dialogs::DialogResult::Cancel => {
                    self.new_dialog = None;
                }
                super::dialogs::DialogResult::Submit(data) => match self.create_session(data) {
                    Ok(session_id) => {
                        self.new_dialog = None;
                        return Some(Action::AttachSession(session_id));
                    }
                    Err(e) => {
                        tracing::error!("Failed to create session: {}", e);
                        if let Some(dialog) = &mut self.new_dialog {
                            dialog.set_error(e.to_string());
                        }
                    }
                },
            }
            return None;
        }

        if let Some(dialog) = &mut self.confirm_dialog {
            match dialog.handle_key(key) {
                super::dialogs::DialogResult::Continue => {}
                super::dialogs::DialogResult::Cancel => {
                    self.confirm_dialog = None;
                }
                super::dialogs::DialogResult::Submit(_) => {
                    let action = dialog.action().to_string();
                    self.confirm_dialog = None;
                    if action == "delete_group" {
                        if let Err(e) = self.delete_selected_group() {
                            tracing::error!("Failed to delete group: {}", e);
                        }
                    }
                }
            }
            return None;
        }

        if let Some(dialog) = &mut self.unified_delete_dialog {
            match dialog.handle_key(key) {
                super::dialogs::DialogResult::Continue => {}
                super::dialogs::DialogResult::Cancel => {
                    self.unified_delete_dialog = None;
                }
                super::dialogs::DialogResult::Submit(options) => {
                    self.unified_delete_dialog = None;
                    if let Err(e) = self.delete_selected(&options) {
                        tracing::error!("Failed to delete session: {}", e);
                    }
                }
            }
            return None;
        }

        if let Some(dialog) = &mut self.group_delete_options_dialog {
            match dialog.handle_key(key) {
                super::dialogs::DialogResult::Continue => {}
                super::dialogs::DialogResult::Cancel => {
                    self.group_delete_options_dialog = None;
                }
                super::dialogs::DialogResult::Submit(options) => {
                    self.group_delete_options_dialog = None;
                    if options.delete_sessions {
                        if let Err(e) = self.delete_group_with_sessions(&options) {
                            tracing::error!("Failed to delete group with sessions: {}", e);
                        }
                    } else if let Err(e) = self.delete_selected_group() {
                        tracing::error!("Failed to delete group: {}", e);
                    }
                }
            }
            return None;
        }

        if let Some(dialog) = &mut self.rename_dialog {
            match dialog.handle_key(key) {
                super::dialogs::DialogResult::Continue => {}
                super::dialogs::DialogResult::Cancel => {
                    self.rename_dialog = None;
                }
                super::dialogs::DialogResult::Submit(new_title) => {
                    self.rename_dialog = None;
                    if let Err(e) = self.rename_selected(&new_title) {
                        tracing::error!("Failed to rename session: {}", e);
                    }
                }
            }
            return None;
        }

        // Search mode
        if self.search_active {
            match key.code {
                KeyCode::Esc => {
                    self.search_active = false;
                    self.search_query.clear();
                    self.filtered_items = None;
                }
                KeyCode::Enter => {
                    self.search_active = false;
                }
                KeyCode::Backspace => {
                    self.search_query.pop();
                    self.update_filter();
                }
                KeyCode::Char(c) => {
                    self.search_query.push(c);
                    self.update_filter();
                }
                _ => {}
            }
            return None;
        }

        // Normal mode keybindings
        match key.code {
            KeyCode::Char('q') => return Some(Action::Quit),
            KeyCode::Char('?') => {
                self.show_help = true;
            }
            KeyCode::Char('P') => {
                if let Some(next) = self.get_next_profile() {
                    return Some(Action::SwitchProfile(next));
                }
            }
            KeyCode::Char('t') => {
                self.view_mode = match self.view_mode {
                    ViewMode::Agent => ViewMode::Terminal,
                    ViewMode::Terminal => ViewMode::Agent,
                };
            }
            KeyCode::Char('/') => {
                self.search_active = true;
                self.search_query.clear();
            }
            KeyCode::Char('n') => {
                let existing_titles: Vec<String> =
                    self.instances.iter().map(|i| i.title.clone()).collect();
                self.new_dialog = Some(NewSessionDialog::new(
                    self.available_tools.clone(),
                    existing_titles,
                ));
            }
            KeyCode::Char('d') => {
                // Deletion only allowed in Agent View
                if self.view_mode == ViewMode::Terminal {
                    self.info_dialog = Some(InfoDialog::new(
                        "Cannot Delete Terminal",
                        "Terminals cannot be deleted directly. Switch to Agent View (press 't') and delete the agent session instead.",
                    ));
                    return None;
                }
                if let Some(session_id) = &self.selected_session {
                    if let Some(inst) = self.instance_map.get(session_id) {
                        if inst.status == Status::Deleting {
                            return None;
                        }

                        let config = DeleteDialogConfig {
                            worktree_branch: inst
                                .worktree_info
                                .as_ref()
                                .filter(|wt| wt.managed_by_aoe)
                                .map(|wt| wt.branch.clone()),
                            has_sandbox: inst.sandbox_info.as_ref().is_some_and(|s| s.enabled),
                        };

                        self.unified_delete_dialog =
                            Some(UnifiedDeleteDialog::new(inst.title.clone(), config));
                    } else {
                        self.unified_delete_dialog = Some(UnifiedDeleteDialog::new(
                            "Unknown Session".to_string(),
                            DeleteDialogConfig::default(),
                        ));
                    }
                } else if let Some(group_path) = &self.selected_group {
                    let prefix = format!("{}/", group_path);
                    let session_count = self
                        .instances
                        .iter()
                        .filter(|i| {
                            i.group_path == *group_path || i.group_path.starts_with(&prefix)
                        })
                        .count();

                    if session_count > 0 {
                        let has_managed_worktrees =
                            self.group_has_managed_worktrees(group_path, &prefix);
                        self.group_delete_options_dialog = Some(GroupDeleteOptionsDialog::new(
                            group_path.clone(),
                            session_count,
                            has_managed_worktrees,
                        ));
                    } else {
                        let message =
                            format!("Are you sure you want to delete group '{}'?", group_path);
                        self.confirm_dialog =
                            Some(ConfirmDialog::new("Delete Group", &message, "delete_group"));
                    }
                }
            }
            KeyCode::Char('r') if !key.modifiers.contains(KeyModifiers::SHIFT) => {
                if let Some(id) = &self.selected_session {
                    if let Some(inst) = self.instance_map.get(id) {
                        if inst.status == Status::Deleting {
                            return None;
                        }
                        self.rename_dialog = Some(RenameDialog::new(&inst.title));
                    }
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_cursor(-1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_cursor(1);
            }
            KeyCode::PageUp => {
                self.move_cursor(-10);
            }
            KeyCode::PageDown => {
                self.move_cursor(10);
            }
            KeyCode::Home | KeyCode::Char('g') if key.modifiers.contains(KeyModifiers::NONE) => {
                self.cursor = 0;
                self.update_selected();
            }
            KeyCode::End | KeyCode::Char('G') => {
                if !self.flat_items.is_empty() {
                    self.cursor = self.flat_items.len() - 1;
                    self.update_selected();
                }
            }
            KeyCode::Enter => {
                if let Some(id) = &self.selected_session {
                    if let Some(inst) = self.instance_map.get(id) {
                        if inst.status == Status::Deleting {
                            return None;
                        }
                    }
                    return match self.view_mode {
                        ViewMode::Agent => Some(Action::AttachSession(id.clone())),
                        ViewMode::Terminal => Some(Action::AttachTerminal(id.clone())),
                    };
                } else if let Some(Item::Group { path, .. }) = self.flat_items.get(self.cursor) {
                    self.group_tree.toggle_collapsed(path);
                    self.flat_items = flatten_tree(&self.group_tree, &self.instances);
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if let Some(Item::Group {
                    path, collapsed, ..
                }) = self.flat_items.get(self.cursor)
                {
                    if !collapsed {
                        self.group_tree.toggle_collapsed(path);
                        self.flat_items = flatten_tree(&self.group_tree, &self.instances);
                    }
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if let Some(Item::Group {
                    path, collapsed, ..
                }) = self.flat_items.get(self.cursor)
                {
                    if *collapsed {
                        self.group_tree.toggle_collapsed(path);
                        self.flat_items = flatten_tree(&self.group_tree, &self.instances);
                    }
                }
            }
            _ => {}
        }

        None
    }

    fn move_cursor(&mut self, delta: i32) {
        let items = if let Some(ref filtered) = self.filtered_items {
            filtered.len()
        } else {
            self.flat_items.len()
        };

        if items == 0 {
            return;
        }

        let new_cursor = if delta < 0 {
            self.cursor.saturating_sub((-delta) as usize)
        } else {
            (self.cursor + delta as usize).min(items - 1)
        };

        self.cursor = new_cursor;
        self.update_selected();
    }

    fn update_selected(&mut self) {
        let item_idx = if let Some(ref filtered) = self.filtered_items {
            filtered.get(self.cursor).copied()
        } else {
            Some(self.cursor)
        };

        if let Some(idx) = item_idx {
            if let Some(item) = self.flat_items.get(idx) {
                match item {
                    Item::Session { id, .. } => {
                        self.selected_session = Some(id.clone());
                        self.selected_group = None;
                    }
                    Item::Group { path, .. } => {
                        self.selected_session = None;
                        self.selected_group = Some(path.clone());
                    }
                }
            }
        }
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

    fn update_filter(&mut self) {
        if self.search_query.is_empty() {
            self.filtered_items = None;
            return;
        }

        let query = self.search_query.to_lowercase();
        let mut matches = Vec::new();

        for (idx, item) in self.flat_items.iter().enumerate() {
            match item {
                Item::Session { id, .. } => {
                    if let Some(inst) = self.instance_map.get(id) {
                        if inst.title_lower.contains(&query)
                            || inst.project_path_lower.contains(&query)
                        {
                            matches.push(idx);
                        }
                    }
                }
                Item::Group { name, path, .. } => {
                    if name.to_lowercase().contains(&query) || path.to_lowercase().contains(&query)
                    {
                        matches.push(idx);
                    }
                }
            }
        }

        self.filtered_items = Some(matches);
        self.cursor = 0;
        self.update_selected();
    }

    fn create_session(&mut self, data: super::dialogs::NewSessionData) -> anyhow::Result<String> {
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

    fn delete_selected(&mut self, options: &DeleteOptions) -> anyhow::Result<()> {
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

    fn delete_selected_group(&mut self) -> anyhow::Result<()> {
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

    fn delete_group_with_sessions(&mut self, options: &GroupDeleteOptions) -> anyhow::Result<()> {
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

    fn group_has_managed_worktrees(&self, group_path: &str, prefix: &str) -> bool {
        self.instances.iter().any(|i| {
            (i.group_path == group_path || i.group_path.starts_with(prefix))
                && i.worktree_info.as_ref().is_some_and(|wt| wt.managed_by_aoe)
        })
    }

    fn rename_selected(&mut self, new_title: &str) -> anyhow::Result<()> {
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

    pub fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
        update_info: Option<&UpdateInfo>,
    ) {
        // Layout: main area + status bar + optional update bar at bottom
        let constraints = if update_info.is_some() {
            vec![
                Constraint::Min(0),
                Constraint::Length(1),
                Constraint::Length(1),
            ]
        } else {
            vec![Constraint::Min(0), Constraint::Length(1)]
        };
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        // Layout: left panel (list) and right panel (preview)
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
            .split(main_chunks[0]);

        self.render_list(frame, chunks[0], theme);
        self.render_preview(frame, chunks[1], theme);
        self.render_status_bar(frame, main_chunks[1], theme);

        if let Some(info) = update_info {
            self.render_update_bar(frame, main_chunks[2], theme, info);
        }

        // Render dialogs on top
        if self.show_help {
            HelpOverlay::render(frame, area, theme);
        }

        if let Some(dialog) = &self.new_dialog {
            dialog.render(frame, area, theme);
        }

        if let Some(dialog) = &self.confirm_dialog {
            dialog.render(frame, area, theme);
        }

        if let Some(dialog) = &self.unified_delete_dialog {
            dialog.render(frame, area, theme);
        }

        if let Some(dialog) = &self.group_delete_options_dialog {
            dialog.render(frame, area, theme);
        }

        if let Some(dialog) = &self.rename_dialog {
            dialog.render(frame, area, theme);
        }

        if let Some(dialog) = &self.welcome_dialog {
            dialog.render(frame, area, theme);
        }

        if let Some(dialog) = &self.changelog_dialog {
            dialog.render(frame, area, theme);
        }

        if let Some(dialog) = &self.info_dialog {
            dialog.render(frame, area, theme);
        }
    }

    fn render_list(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let title = match self.view_mode {
            ViewMode::Agent => format!(" Agent of Empires [{}] ", self.storage.profile()),
            ViewMode::Terminal => format!(" Terminals [{}] ", self.storage.profile()),
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(title)
            .title_style(Style::default().fg(theme.title).bold());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.instances.is_empty() && self.groups.is_empty() {
            let empty_text = vec![
                Line::from(""),
                Line::from("No sessions yet").style(Style::default().fg(theme.dimmed)),
                Line::from(""),
                Line::from("Press 'n' to create one").style(Style::default().fg(theme.hint)),
                Line::from("or 'agent-of-empires add .'").style(Style::default().fg(theme.hint)),
            ];
            let para = Paragraph::new(empty_text).alignment(Alignment::Center);
            frame.render_widget(para, inner);
            return;
        }

        let indices: Vec<usize> = if let Some(ref filtered) = self.filtered_items {
            filtered.clone()
        } else {
            (0..self.flat_items.len()).collect()
        };

        let list_items: Vec<ListItem> = indices
            .iter()
            .enumerate()
            .filter_map(|(display_idx, &item_idx)| {
                self.flat_items.get(item_idx).map(|item| {
                    let is_selected = display_idx == self.cursor;
                    self.render_item(item, is_selected, theme)
                })
            })
            .collect();

        let list =
            List::new(list_items).highlight_style(Style::default().bg(theme.session_selection));

        frame.render_widget(list, inner);

        // Render search bar if active
        if self.search_active {
            let search_area = Rect {
                x: inner.x,
                y: inner.y + inner.height.saturating_sub(1),
                width: inner.width,
                height: 1,
            };
            let search_text = format!("/{}", self.search_query);
            let search_para = Paragraph::new(search_text).style(Style::default().fg(theme.search));
            frame.render_widget(search_para, search_area);
        }
    }

    fn render_item(&self, item: &Item, is_selected: bool, theme: &Theme) -> ListItem<'_> {
        let indent = get_indent(item.depth());

        use std::borrow::Cow;

        let (icon, text, style): (&str, Cow<str>, Style) = match item {
            Item::Group {
                name,
                collapsed,
                session_count,
                ..
            } => {
                let icon = if *collapsed {
                    ICON_COLLAPSED
                } else {
                    ICON_EXPANDED
                };
                let text = Cow::Owned(format!("{} ({})", name, session_count));
                let style = Style::default().fg(theme.group).bold();
                (icon, text, style)
            }
            Item::Session { id, .. } => {
                if let Some(inst) = self.instance_map.get(id) {
                    match self.view_mode {
                        ViewMode::Agent => {
                            let icon = match inst.status {
                                Status::Running => ICON_RUNNING,
                                Status::Waiting => ICON_WAITING,
                                Status::Idle => ICON_IDLE,
                                Status::Error => ICON_ERROR,
                                Status::Starting => ICON_STARTING,
                                Status::Deleting => ICON_DELETING,
                            };
                            let color = match inst.status {
                                Status::Running => theme.running,
                                Status::Waiting => theme.waiting,
                                Status::Idle => theme.idle,
                                Status::Error => theme.error,
                                Status::Starting => theme.dimmed,
                                Status::Deleting => theme.waiting,
                            };
                            let style = Style::default().fg(color);
                            (icon, Cow::Borrowed(&inst.title), style)
                        }
                        ViewMode::Terminal => {
                            let terminal_running = inst
                                .terminal_tmux_session()
                                .map(|s| s.exists())
                                .unwrap_or(false);
                            let (icon, color) = if terminal_running {
                                (ICON_RUNNING, theme.running)
                            } else {
                                (ICON_IDLE, theme.dimmed)
                            };
                            let style = Style::default().fg(color);
                            (icon, Cow::Borrowed(&inst.title), style)
                        }
                    }
                } else {
                    (
                        "?",
                        Cow::Borrowed(id.as_str()),
                        Style::default().fg(theme.dimmed),
                    )
                }
            }
        };

        let mut line_spans = Vec::with_capacity(5);
        line_spans.push(Span::raw(indent));
        line_spans.push(Span::styled(format!("{} ", icon), style));
        line_spans.push(Span::styled(
            text.into_owned(),
            if is_selected { style.bold() } else { style },
        ));

        if let Item::Session { id, .. } = item {
            if let Some(inst) = self.instance_map.get(id) {
                if let Some(wt_info) = &inst.worktree_info {
                    line_spans.push(Span::styled(
                        format!("  {}", wt_info.branch),
                        Style::default().fg(Color::Cyan),
                    ));
                }
                if inst.is_sandboxed() {
                    line_spans.push(Span::styled(
                        " [sandbox]",
                        Style::default().fg(Color::Magenta),
                    ));
                }
            }
        }

        let line = Line::from(line_spans);

        if is_selected {
            ListItem::new(line).style(Style::default().bg(theme.session_selection))
        } else {
            ListItem::new(line)
        }
    }

    /// Refresh preview cache if needed (session changed, dimensions changed, or timer expired)
    fn refresh_preview_cache_if_needed(&mut self, width: u16, height: u16) {
        const PREVIEW_REFRESH_MS: u128 = 250; // Refresh preview 4x/second max

        let needs_refresh = match &self.selected_session {
            Some(id) => {
                self.preview_cache.session_id.as_ref() != Some(id)
                    || self.preview_cache.dimensions != (width, height)
                    || self.preview_cache.last_refresh.elapsed().as_millis() > PREVIEW_REFRESH_MS
            }
            None => false,
        };

        if needs_refresh {
            if let Some(id) = &self.selected_session {
                if let Some(inst) = self.instance_map.get(id) {
                    self.preview_cache.content = inst
                        .capture_output_with_size(height as usize, width, height)
                        .unwrap_or_default();
                    self.preview_cache.session_id = Some(id.clone());
                    self.preview_cache.dimensions = (width, height);
                    self.preview_cache.last_refresh = Instant::now();
                }
            }
        }
    }

    /// Refresh terminal preview cache if needed
    fn refresh_terminal_preview_cache_if_needed(&mut self, width: u16, height: u16) {
        const PREVIEW_REFRESH_MS: u128 = 250;

        let needs_refresh = match &self.selected_session {
            Some(id) => {
                self.terminal_preview_cache.session_id.as_ref() != Some(id)
                    || self.terminal_preview_cache.dimensions != (width, height)
                    || self
                        .terminal_preview_cache
                        .last_refresh
                        .elapsed()
                        .as_millis()
                        > PREVIEW_REFRESH_MS
            }
            None => false,
        };

        if needs_refresh {
            if let Some(id) = &self.selected_session {
                if let Some(inst) = self.instance_map.get(id) {
                    self.terminal_preview_cache.content = inst
                        .terminal_tmux_session()
                        .and_then(|s| s.capture_pane(height as usize))
                        .unwrap_or_default();
                    self.terminal_preview_cache.session_id = Some(id.clone());
                    self.terminal_preview_cache.dimensions = (width, height);
                    self.terminal_preview_cache.last_refresh = Instant::now();
                }
            }
        }
    }

    fn render_preview(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let title = match self.view_mode {
            ViewMode::Agent => " Preview ",
            ViewMode::Terminal => " Terminal Preview ",
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(title)
            .title_style(Style::default().fg(theme.title));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        match self.view_mode {
            ViewMode::Agent => {
                // Refresh cache before borrowing from instance_map to avoid borrow conflicts
                self.refresh_preview_cache_if_needed(inner.width, inner.height);

                if let Some(id) = &self.selected_session {
                    if let Some(inst) = self.instance_map.get(id) {
                        Preview::render_with_cache(
                            frame,
                            inner,
                            inst,
                            &self.preview_cache.content,
                            theme,
                        );
                    }
                } else {
                    let hint = Paragraph::new("Select a session to preview")
                        .style(Style::default().fg(theme.dimmed))
                        .alignment(Alignment::Center);
                    frame.render_widget(hint, inner);
                }
            }
            ViewMode::Terminal => {
                self.refresh_terminal_preview_cache_if_needed(inner.width, inner.height);

                if let Some(id) = &self.selected_session {
                    if let Some(inst) = self.instance_map.get(id) {
                        let terminal_running = inst
                            .terminal_tmux_session()
                            .map(|s| s.exists())
                            .unwrap_or(false);
                        Preview::render_terminal_preview(
                            frame,
                            inner,
                            inst,
                            terminal_running,
                            &self.terminal_preview_cache.content,
                            theme,
                        );
                    }
                } else {
                    let hint = Paragraph::new("Select a session to preview terminal")
                        .style(Style::default().fg(theme.dimmed))
                        .alignment(Alignment::Center);
                    frame.render_widget(hint, inner);
                }
            }
        }
    }

    fn render_status_bar(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let key_style = Style::default().fg(theme.accent).bold();
        let desc_style = Style::default().fg(theme.dimmed);
        let sep_style = Style::default().fg(theme.border);
        let mode_style = Style::default().fg(theme.waiting).bold();

        let mode_indicator = match self.view_mode {
            ViewMode::Agent => "[Agent]",
            ViewMode::Terminal => "[Term]",
        };

        let spans = vec![
            Span::styled(format!(" {} ", mode_indicator), mode_style),
            Span::styled("│", sep_style),
            Span::styled(" j/k", key_style),
            Span::styled(" Nav ", desc_style),
            Span::styled("│", sep_style),
            Span::styled(" Enter", key_style),
            Span::styled(" Attach ", desc_style),
            Span::styled("│", sep_style),
            Span::styled(" t", key_style),
            Span::styled(" View ", desc_style),
            Span::styled("│", sep_style),
            Span::styled(" n", key_style),
            Span::styled(" New ", desc_style),
            Span::styled("│", sep_style),
            Span::styled(" d", key_style),
            Span::styled(" Del ", desc_style),
            Span::styled("│", sep_style),
            Span::styled(" /", key_style),
            Span::styled(" Search ", desc_style),
            Span::styled("│", sep_style),
            Span::styled(" ?", key_style),
            Span::styled(" Help ", desc_style),
            Span::styled("│", sep_style),
            Span::styled(" q", key_style),
            Span::styled(" Quit", desc_style),
        ];

        let status = Paragraph::new(Line::from(spans)).style(Style::default().bg(theme.selection));
        frame.render_widget(status, area);
    }

    fn render_update_bar(&self, frame: &mut Frame, area: Rect, theme: &Theme, info: &UpdateInfo) {
        let update_style = Style::default().fg(theme.waiting).bold();
        let text = format!(
            " update available {} -> {}",
            info.current_version, info.latest_version
        );
        let bar = Paragraph::new(Line::from(Span::styled(text, update_style)))
            .style(Style::default().bg(theme.selection));
        frame.render_widget(bar, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use serial_test::serial;
    use tempfile::TempDir;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    struct TestEnv {
        _temp: TempDir,
        view: HomeView,
    }

    fn create_test_env_empty() -> TestEnv {
        let temp = TempDir::new().unwrap();
        std::env::set_var("HOME", temp.path());
        let storage = Storage::new("test").unwrap();
        let tools = AvailableTools {
            claude: true,
            opencode: false,
        };
        let view = HomeView::new(storage, tools).unwrap();
        TestEnv { _temp: temp, view }
    }

    fn create_test_env_with_sessions(count: usize) -> TestEnv {
        let temp = TempDir::new().unwrap();
        std::env::set_var("HOME", temp.path());
        let storage = Storage::new("test").unwrap();
        let mut instances = Vec::new();
        for i in 0..count {
            instances.push(Instance::new(
                &format!("session{}", i),
                &format!("/tmp/{}", i),
            ));
        }
        storage.save(&instances).unwrap();

        let tools = AvailableTools {
            claude: true,
            opencode: false,
        };
        let view = HomeView::new(storage, tools).unwrap();
        TestEnv { _temp: temp, view }
    }

    fn create_test_env_with_groups() -> TestEnv {
        let temp = TempDir::new().unwrap();
        std::env::set_var("HOME", temp.path());
        let storage = Storage::new("test").unwrap();
        let mut instances = Vec::new();

        let inst1 = Instance::new("ungrouped", "/tmp/u");
        instances.push(inst1);

        let mut inst2 = Instance::new("work-project", "/tmp/work");
        inst2.group_path = "work".to_string();
        instances.push(inst2);

        let mut inst3 = Instance::new("personal-project", "/tmp/personal");
        inst3.group_path = "personal".to_string();
        instances.push(inst3);

        storage.save(&instances).unwrap();

        let tools = AvailableTools {
            claude: true,
            opencode: false,
        };
        let view = HomeView::new(storage, tools).unwrap();
        TestEnv { _temp: temp, view }
    }

    #[test]
    #[serial]
    fn test_initial_cursor_position() {
        let env = create_test_env_with_sessions(3);
        assert_eq!(env.view.cursor, 0);
    }

    #[test]
    #[serial]
    fn test_q_returns_quit_action() {
        let mut env = create_test_env_empty();
        let action = env.view.handle_key(key(KeyCode::Char('q')));
        assert_eq!(action, Some(Action::Quit));
    }

    #[test]
    #[serial]
    fn test_question_mark_opens_help() {
        let mut env = create_test_env_empty();
        assert!(!env.view.show_help);
        env.view.handle_key(key(KeyCode::Char('?')));
        assert!(env.view.show_help);
    }

    #[test]
    #[serial]
    fn test_help_closes_on_esc() {
        let mut env = create_test_env_empty();
        env.view.show_help = true;
        env.view.handle_key(key(KeyCode::Esc));
        assert!(!env.view.show_help);
    }

    #[test]
    #[serial]
    fn test_help_closes_on_question_mark() {
        let mut env = create_test_env_empty();
        env.view.show_help = true;
        env.view.handle_key(key(KeyCode::Char('?')));
        assert!(!env.view.show_help);
    }

    #[test]
    #[serial]
    fn test_help_closes_on_q() {
        let mut env = create_test_env_empty();
        env.view.show_help = true;
        env.view.handle_key(key(KeyCode::Char('q')));
        assert!(!env.view.show_help);
    }

    #[test]
    #[serial]
    fn test_has_dialog_returns_true_for_help() {
        let mut env = create_test_env_empty();
        assert!(!env.view.has_dialog());
        env.view.show_help = true;
        assert!(env.view.has_dialog());
    }

    #[test]
    #[serial]
    fn test_n_opens_new_dialog() {
        let mut env = create_test_env_empty();
        assert!(env.view.new_dialog.is_none());
        env.view.handle_key(key(KeyCode::Char('n')));
        assert!(env.view.new_dialog.is_some());
    }

    #[test]
    #[serial]
    fn test_has_dialog_returns_true_for_new_dialog() {
        let mut env = create_test_env_empty();
        env.view.new_dialog = Some(NewSessionDialog::new(
            AvailableTools {
                claude: true,
                opencode: false,
            },
            Vec::new(),
        ));
        assert!(env.view.has_dialog());
    }

    #[test]
    #[serial]
    fn test_cursor_down_j() {
        let mut env = create_test_env_with_sessions(5);
        assert_eq!(env.view.cursor, 0);
        env.view.handle_key(key(KeyCode::Char('j')));
        assert_eq!(env.view.cursor, 1);
    }

    #[test]
    #[serial]
    fn test_cursor_down_arrow() {
        let mut env = create_test_env_with_sessions(5);
        assert_eq!(env.view.cursor, 0);
        env.view.handle_key(key(KeyCode::Down));
        assert_eq!(env.view.cursor, 1);
    }

    #[test]
    #[serial]
    fn test_cursor_up_k() {
        let mut env = create_test_env_with_sessions(5);
        env.view.cursor = 3;
        env.view.handle_key(key(KeyCode::Char('k')));
        assert_eq!(env.view.cursor, 2);
    }

    #[test]
    #[serial]
    fn test_cursor_up_arrow() {
        let mut env = create_test_env_with_sessions(5);
        env.view.cursor = 3;
        env.view.handle_key(key(KeyCode::Up));
        assert_eq!(env.view.cursor, 2);
    }

    #[test]
    #[serial]
    fn test_cursor_bounds_at_top() {
        let mut env = create_test_env_with_sessions(5);
        env.view.cursor = 0;
        env.view.handle_key(key(KeyCode::Up));
        assert_eq!(env.view.cursor, 0);
    }

    #[test]
    #[serial]
    fn test_cursor_bounds_at_bottom() {
        let mut env = create_test_env_with_sessions(5);
        env.view.cursor = 4;
        env.view.handle_key(key(KeyCode::Down));
        assert_eq!(env.view.cursor, 4);
    }

    #[test]
    #[serial]
    fn test_page_down() {
        let mut env = create_test_env_with_sessions(20);
        env.view.cursor = 0;
        env.view.handle_key(key(KeyCode::PageDown));
        assert_eq!(env.view.cursor, 10);
    }

    #[test]
    #[serial]
    fn test_page_up() {
        let mut env = create_test_env_with_sessions(20);
        env.view.cursor = 15;
        env.view.handle_key(key(KeyCode::PageUp));
        assert_eq!(env.view.cursor, 5);
    }

    #[test]
    #[serial]
    fn test_page_down_clamps_to_end() {
        let mut env = create_test_env_with_sessions(5);
        env.view.cursor = 0;
        env.view.handle_key(key(KeyCode::PageDown));
        assert_eq!(env.view.cursor, 4);
    }

    #[test]
    #[serial]
    fn test_page_up_clamps_to_start() {
        let mut env = create_test_env_with_sessions(5);
        env.view.cursor = 3;
        env.view.handle_key(key(KeyCode::PageUp));
        assert_eq!(env.view.cursor, 0);
    }

    #[test]
    #[serial]
    fn test_home_key() {
        let mut env = create_test_env_with_sessions(10);
        env.view.cursor = 7;
        env.view.handle_key(key(KeyCode::Home));
        assert_eq!(env.view.cursor, 0);
    }

    #[test]
    #[serial]
    fn test_end_key() {
        let mut env = create_test_env_with_sessions(10);
        env.view.cursor = 3;
        env.view.handle_key(key(KeyCode::End));
        assert_eq!(env.view.cursor, 9);
    }

    #[test]
    #[serial]
    fn test_g_key_goes_to_start() {
        let mut env = create_test_env_with_sessions(10);
        env.view.cursor = 7;
        env.view.handle_key(key(KeyCode::Char('g')));
        assert_eq!(env.view.cursor, 0);
    }

    #[test]
    #[serial]
    fn test_uppercase_g_goes_to_end() {
        let mut env = create_test_env_with_sessions(10);
        env.view.cursor = 3;
        env.view.handle_key(key(KeyCode::Char('G')));
        assert_eq!(env.view.cursor, 9);
    }

    #[test]
    #[serial]
    fn test_cursor_movement_on_empty_list() {
        let mut env = create_test_env_empty();
        env.view.handle_key(key(KeyCode::Down));
        assert_eq!(env.view.cursor, 0);
        env.view.handle_key(key(KeyCode::Up));
        assert_eq!(env.view.cursor, 0);
    }

    #[test]
    #[serial]
    fn test_enter_on_session_returns_attach_action() {
        let mut env = create_test_env_with_sessions(3);
        env.view.cursor = 1;
        env.view.update_selected();
        let action = env.view.handle_key(key(KeyCode::Enter));
        assert!(matches!(action, Some(Action::AttachSession(_))));
    }

    #[test]
    #[serial]
    fn test_slash_enters_search_mode() {
        let mut env = create_test_env_with_sessions(3);
        assert!(!env.view.search_active);
        env.view.handle_key(key(KeyCode::Char('/')));
        assert!(env.view.search_active);
        assert!(env.view.search_query.is_empty());
    }

    #[test]
    #[serial]
    fn test_search_mode_captures_chars() {
        let mut env = create_test_env_with_sessions(3);
        env.view.handle_key(key(KeyCode::Char('/')));
        env.view.handle_key(key(KeyCode::Char('t')));
        env.view.handle_key(key(KeyCode::Char('e')));
        env.view.handle_key(key(KeyCode::Char('s')));
        env.view.handle_key(key(KeyCode::Char('t')));
        assert_eq!(env.view.search_query, "test");
    }

    #[test]
    #[serial]
    fn test_search_mode_backspace() {
        let mut env = create_test_env_with_sessions(3);
        env.view.handle_key(key(KeyCode::Char('/')));
        env.view.handle_key(key(KeyCode::Char('a')));
        env.view.handle_key(key(KeyCode::Char('b')));
        env.view.handle_key(key(KeyCode::Backspace));
        assert_eq!(env.view.search_query, "a");
    }

    #[test]
    #[serial]
    fn test_search_mode_esc_exits_and_clears() {
        let mut env = create_test_env_with_sessions(3);
        env.view.handle_key(key(KeyCode::Char('/')));
        env.view.handle_key(key(KeyCode::Char('x')));
        env.view.handle_key(key(KeyCode::Esc));
        assert!(!env.view.search_active);
        assert!(env.view.search_query.is_empty());
        assert!(env.view.filtered_items.is_none());
    }

    #[test]
    #[serial]
    fn test_search_mode_enter_exits_keeps_filter() {
        let mut env = create_test_env_with_sessions(3);
        env.view.handle_key(key(KeyCode::Char('/')));
        env.view.handle_key(key(KeyCode::Char('s')));
        env.view.handle_key(key(KeyCode::Enter));
        assert!(!env.view.search_active);
        assert_eq!(env.view.search_query, "s");
    }

    #[test]
    #[serial]
    fn test_d_on_session_opens_delete_dialog() {
        let mut env = create_test_env_with_sessions(3);
        env.view.update_selected();
        assert!(env.view.unified_delete_dialog.is_none());
        env.view.handle_key(key(KeyCode::Char('d')));
        assert!(env.view.unified_delete_dialog.is_some());
    }

    #[test]
    #[serial]
    fn test_d_on_group_with_sessions_opens_group_delete_options_dialog() {
        let mut env = create_test_env_with_groups();
        env.view.cursor = 1;
        env.view.update_selected();
        assert!(env.view.selected_group.is_some());
        assert!(env.view.group_delete_options_dialog.is_none());
        env.view.handle_key(key(KeyCode::Char('d')));
        assert!(env.view.group_delete_options_dialog.is_some());
    }

    #[test]
    #[serial]
    fn test_selected_session_updates_on_cursor_move() {
        let mut env = create_test_env_with_sessions(3);
        let first_id = env.view.selected_session.clone();
        env.view.handle_key(key(KeyCode::Down));
        assert_ne!(env.view.selected_session, first_id);
    }

    #[test]
    #[serial]
    fn test_selected_group_set_when_on_group() {
        let mut env = create_test_env_with_groups();
        for i in 0..env.view.flat_items.len() {
            env.view.cursor = i;
            env.view.update_selected();
            if matches!(env.view.flat_items.get(i), Some(Item::Group { .. })) {
                assert!(env.view.selected_group.is_some());
                assert!(env.view.selected_session.is_none());
                return;
            }
        }
        panic!("No group found in flat_items");
    }

    #[test]
    #[serial]
    fn test_filter_matches_session_title() {
        let mut env = create_test_env_with_sessions(5);
        env.view.search_query = "session2".to_string();
        env.view.update_filter();
        assert!(env.view.filtered_items.is_some());
        let filtered = env.view.filtered_items.as_ref().unwrap();
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    #[serial]
    fn test_filter_case_insensitive() {
        let mut env = create_test_env_with_sessions(5);
        env.view.search_query = "SESSION2".to_string();
        env.view.update_filter();
        assert!(env.view.filtered_items.is_some());
        let filtered = env.view.filtered_items.as_ref().unwrap();
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    #[serial]
    fn test_filter_matches_path() {
        let mut env = create_test_env_with_sessions(5);
        env.view.search_query = "/tmp/3".to_string();
        env.view.update_filter();
        assert!(env.view.filtered_items.is_some());
        let filtered = env.view.filtered_items.as_ref().unwrap();
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    #[serial]
    fn test_filter_matches_group_name() {
        let mut env = create_test_env_with_groups();
        env.view.search_query = "work".to_string();
        env.view.update_filter();
        assert!(env.view.filtered_items.is_some());
        let filtered = env.view.filtered_items.as_ref().unwrap();
        assert!(!filtered.is_empty());
    }

    #[test]
    #[serial]
    fn test_filter_empty_query_clears_filter() {
        let mut env = create_test_env_with_sessions(5);
        env.view.search_query = "session".to_string();
        env.view.update_filter();
        assert!(env.view.filtered_items.is_some());

        env.view.search_query.clear();
        env.view.update_filter();
        assert!(env.view.filtered_items.is_none());
    }

    #[test]
    #[serial]
    fn test_filter_resets_cursor() {
        let mut env = create_test_env_with_sessions(5);
        env.view.cursor = 3;
        env.view.search_query = "session".to_string();
        env.view.update_filter();
        assert_eq!(env.view.cursor, 0);
    }

    #[test]
    #[serial]
    fn test_filter_no_matches() {
        let mut env = create_test_env_with_sessions(5);
        env.view.search_query = "nonexistent".to_string();
        env.view.update_filter();
        assert!(env.view.filtered_items.is_some());
        let filtered = env.view.filtered_items.as_ref().unwrap();
        assert_eq!(filtered.len(), 0);
    }

    #[test]
    #[serial]
    fn test_cursor_moves_within_filtered_list() {
        let mut env = create_test_env_with_sessions(10);
        env.view.search_query = "session".to_string();
        env.view.update_filter();
        let filtered_count = env.view.filtered_items.as_ref().unwrap().len();

        env.view.cursor = 0;
        for _ in 0..(filtered_count + 5) {
            env.view.handle_key(key(KeyCode::Down));
        }
        assert_eq!(env.view.cursor, filtered_count - 1);
    }

    #[test]
    #[serial]
    fn test_r_opens_rename_dialog() {
        let mut env = create_test_env_with_sessions(3);
        env.view.update_selected();
        assert!(env.view.rename_dialog.is_none());
        env.view.handle_key(key(KeyCode::Char('r')));
        assert!(env.view.rename_dialog.is_some());
    }

    #[test]
    #[serial]
    fn test_rename_dialog_not_opened_on_group() {
        let mut env = create_test_env_with_groups();
        env.view.cursor = 1;
        env.view.update_selected();
        assert!(env.view.selected_group.is_some());
        assert!(env.view.rename_dialog.is_none());
        env.view.handle_key(key(KeyCode::Char('r')));
        assert!(env.view.rename_dialog.is_none());
    }

    #[test]
    #[serial]
    fn test_has_dialog_returns_true_for_rename_dialog() {
        let mut env = create_test_env_with_sessions(1);
        env.view.update_selected();
        assert!(!env.view.has_dialog());
        env.view.handle_key(key(KeyCode::Char('r')));
        assert!(env.view.has_dialog());
    }

    #[test]
    #[serial]
    fn test_select_session_by_id() {
        let mut env = create_test_env_with_sessions(3);
        let session_id = env.view.instances[1].id.clone();

        assert_eq!(env.view.cursor, 0);

        env.view.select_session_by_id(&session_id);

        assert_eq!(env.view.cursor, 1);
        assert_eq!(env.view.selected_session, Some(session_id));
    }

    #[test]
    #[serial]
    fn test_select_session_by_id_nonexistent() {
        let mut env = create_test_env_with_sessions(3);

        assert_eq!(env.view.cursor, 0);
        env.view.select_session_by_id("nonexistent-id");
        assert_eq!(env.view.cursor, 0);
    }

    #[test]
    #[serial]
    fn test_get_next_profile_single_profile_returns_none() {
        let env = create_test_env_empty();
        assert!(env.view.get_next_profile().is_none());
    }

    #[test]
    #[serial]
    fn test_get_next_profile_cycles_through_profiles() {
        let temp = TempDir::new().unwrap();
        std::env::set_var("HOME", temp.path());

        crate::session::create_profile("alpha").unwrap();
        crate::session::create_profile("beta").unwrap();
        crate::session::create_profile("gamma").unwrap();

        let storage = Storage::new("alpha").unwrap();
        let tools = AvailableTools {
            claude: true,
            opencode: false,
        };
        let view = HomeView::new(storage, tools).unwrap();

        // From alpha -> beta
        assert_eq!(view.get_next_profile(), Some("beta".to_string()));
    }

    #[test]
    #[serial]
    fn test_get_next_profile_wraps_around() {
        let temp = TempDir::new().unwrap();
        std::env::set_var("HOME", temp.path());

        crate::session::create_profile("alpha").unwrap();
        crate::session::create_profile("beta").unwrap();

        // Start on beta (last alphabetically)
        let storage = Storage::new("beta").unwrap();
        let tools = AvailableTools {
            claude: true,
            opencode: false,
        };
        let view = HomeView::new(storage, tools).unwrap();

        // From beta -> alpha (wraps)
        assert_eq!(view.get_next_profile(), Some("alpha".to_string()));
    }

    #[test]
    #[serial]
    fn test_uppercase_p_returns_switch_profile_action() {
        let temp = TempDir::new().unwrap();
        std::env::set_var("HOME", temp.path());

        crate::session::create_profile("first").unwrap();
        crate::session::create_profile("second").unwrap();

        let storage = Storage::new("first").unwrap();
        let tools = AvailableTools {
            claude: true,
            opencode: false,
        };
        let mut view = HomeView::new(storage, tools).unwrap();

        let action = view.handle_key(key(KeyCode::Char('P')));
        assert_eq!(action, Some(Action::SwitchProfile("second".to_string())));
    }

    #[test]
    #[serial]
    fn test_uppercase_p_does_nothing_with_single_profile() {
        let env = create_test_env_empty();
        let mut view = env.view;

        let action = view.handle_key(key(KeyCode::Char('P')));
        assert_eq!(action, None);
    }

    #[test]
    #[serial]
    fn test_t_toggles_view_mode() {
        let env = create_test_env_empty();
        let mut view = env.view;

        assert_eq!(view.view_mode, ViewMode::Agent);

        view.handle_key(key(KeyCode::Char('t')));
        assert_eq!(view.view_mode, ViewMode::Terminal);

        view.handle_key(key(KeyCode::Char('t')));
        assert_eq!(view.view_mode, ViewMode::Agent);
    }

    #[test]
    #[serial]
    fn test_enter_returns_attach_terminal_in_terminal_view() {
        let env = create_test_env_with_sessions(1);
        let mut view = env.view;

        // In Agent view, Enter returns AttachSession
        let action = view.handle_key(key(KeyCode::Enter));
        assert!(matches!(action, Some(Action::AttachSession(_))));

        // Switch to Terminal view
        view.handle_key(key(KeyCode::Char('t')));
        assert_eq!(view.view_mode, ViewMode::Terminal);

        // In Terminal view, Enter returns AttachTerminal
        let action = view.handle_key(key(KeyCode::Enter));
        assert!(matches!(action, Some(Action::AttachTerminal(_))));
    }

    #[test]
    #[serial]
    fn test_d_shows_info_dialog_in_terminal_view() {
        let env = create_test_env_with_sessions(1);
        let mut view = env.view;

        // Switch to Terminal view
        view.handle_key(key(KeyCode::Char('t')));
        assert_eq!(view.view_mode, ViewMode::Terminal);

        // Press 'd' - should show info dialog, not delete dialog
        assert!(view.info_dialog.is_none());
        view.handle_key(key(KeyCode::Char('d')));
        assert!(view.info_dialog.is_some());
        assert!(view.unified_delete_dialog.is_none());
    }

    #[test]
    #[serial]
    fn test_has_dialog_includes_info_dialog() {
        let env = create_test_env_empty();
        let mut view = env.view;

        assert!(!view.has_dialog());

        view.info_dialog = Some(InfoDialog::new("Test", "Test message"));
        assert!(view.has_dialog());
    }
}
