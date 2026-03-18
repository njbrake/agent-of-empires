//! Input handling for HomeView

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent};
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

use super::{HomeView, TerminalMode, ViewMode};
use crate::session::config::{load_config, save_config, SortOrder};
use crate::session::{list_profiles, repo_config, resolve_config, Item, Status};
use crate::tui::app::Action;
use crate::tui::dialogs::{
    ConfirmDialog, DeleteDialogConfig, DialogResult, GroupDeleteOptionsDialog, HookTrustAction,
    HooksInstallDialog, InfoDialog, NewSessionData, NewSessionDialog, ProfilePickerAction,
    RenameDialog, UnifiedDeleteDialog,
};
use crate::tui::diff::{DiffAction, DiffView};
use crate::tui::settings::{SettingsAction, SettingsView};

impl HomeView {
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        // Handle unsaved changes confirmation for settings (shown over settings view)
        if self.settings_close_confirm {
            if let Some(dialog) = &mut self.confirm_dialog {
                match dialog.handle_key(key) {
                    DialogResult::Continue => return None,
                    DialogResult::Cancel => {
                        // User chose not to discard, go back to settings
                        self.confirm_dialog = None;
                        self.settings_close_confirm = false;
                        return None;
                    }
                    DialogResult::Submit(_) => {
                        // User chose to discard changes
                        if let Some(ref mut settings) = self.settings_view {
                            settings.force_close();
                        }
                        self.settings_view = None;
                        self.confirm_dialog = None;
                        self.settings_close_confirm = false;
                        // Revert theme to saved config (undo any preview)
                        if let Ok(config) =
                            resolve_config(self.active_profile.as_deref().unwrap_or("default"))
                        {
                            let theme_name = if config.theme.name.is_empty() {
                                "phosphor".to_string()
                            } else {
                                config.theme.name
                            };
                            return Some(Action::SetTheme(theme_name));
                        }
                        return None;
                    }
                }
            }
        }

        // Handle settings view (full-screen takeover)
        if let Some(ref mut settings) = self.settings_view {
            match settings.handle_key(key) {
                SettingsAction::Continue => {
                    return None;
                }
                SettingsAction::Close => {
                    self.settings_view = None;
                    // Refresh config-dependent state in case settings changed
                    self.refresh_from_config();
                    // Reload theme from saved config
                    if let Ok(config) =
                        resolve_config(self.active_profile.as_deref().unwrap_or("default"))
                    {
                        let theme_name = if config.theme.name.is_empty() {
                            "phosphor".to_string()
                        } else {
                            config.theme.name
                        };
                        return Some(Action::SetTheme(theme_name));
                    }
                    return None;
                }
                SettingsAction::UnsavedChangesWarning => {
                    // Show confirmation dialog
                    self.confirm_dialog = Some(ConfirmDialog::new(
                        "Unsaved Changes",
                        "You have unsaved changes. Discard them?",
                        "discard_settings",
                    ));
                    self.settings_close_confirm = true;
                    return None;
                }
                SettingsAction::PreviewTheme(name) => {
                    return Some(Action::SetTheme(name));
                }
            }
        }

        // Handle diff view (full-screen takeover)
        if let Some(ref mut diff_view) = self.diff_view {
            match diff_view.handle_key(key) {
                DiffAction::Continue => return None,
                DiffAction::Close => {
                    self.diff_view = None;
                    return None;
                }
                DiffAction::EditFile(path) => {
                    // Launch external editor (vim or nano)
                    return Some(Action::EditFile(path));
                }
            }
        }

        // Handle welcome/changelog dialogs first (highest priority)
        if let Some(dialog) = &mut self.welcome_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel | DialogResult::Submit(_) => {
                    self.welcome_dialog = None;
                }
            }
            return None;
        }

        if let Some(dialog) = &mut self.changelog_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel | DialogResult::Submit(_) => {
                    self.changelog_dialog = None;
                }
            }
            return None;
        }

        if let Some(dialog) = &mut self.info_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel | DialogResult::Submit(_) => {
                    self.info_dialog = None;
                    if let Some(session_id) = self.pending_attach_after_warning.take() {
                        return Some(Action::AttachSession(session_id));
                    }
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

        if let Some(dialog) = &mut self.hooks_install_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    self.hooks_install_dialog = None;
                    self.pending_hooks_install_data = None;
                }
                DialogResult::Submit(_) => {
                    self.hooks_install_dialog = None;
                    // Persist the acknowledgment
                    if let Ok(mut config) =
                        crate::session::config::load_config().map(|c| c.unwrap_or_default())
                    {
                        config.app_state.has_acknowledged_agent_hooks = true;
                        let _ = crate::session::config::save_config(&config);
                    }
                    // Resume session creation
                    if let Some(data) = self.pending_hooks_install_data.take() {
                        return self.continue_session_creation(data);
                    }
                }
            }
            return None;
        }

        if let Some(dialog) = &mut self.hook_trust_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    self.hook_trust_dialog = None;
                    self.pending_hook_trust_data = None;
                }
                DialogResult::Submit(action) => {
                    self.hook_trust_dialog = None;
                    if let Some(data) = self.pending_hook_trust_data.take() {
                        match action {
                            HookTrustAction::Trust {
                                hooks,
                                hooks_hash,
                                project_path,
                            } => {
                                if let Err(e) = repo_config::trust_repo(
                                    std::path::Path::new(&project_path),
                                    &hooks_hash,
                                ) {
                                    tracing::error!("Failed to trust repo: {}", e);
                                }
                                let merged =
                                    self.merge_repo_hooks_onto_config_for(&data.profile, hooks);
                                return self.create_session_with_hooks(data, merged);
                            }
                            HookTrustAction::Skip => {
                                let fallback = self.resolve_global_profile_hooks_for(&data.profile);
                                return self.create_session_with_hooks(data, fallback);
                            }
                        }
                    }
                }
            }
            return None;
        }

        let dialog_result = self
            .new_dialog
            .as_mut()
            .map(|dialog| dialog.handle_key(key));

        if let Some(result) = dialog_result {
            match result {
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    // If creation is pending, mark it as cancelled
                    if self.is_creation_pending() {
                        self.cancel_creation();
                    } else {
                        self.new_dialog = None;
                    }
                }
                DialogResult::Submit(data) => {
                    // Check if the tool uses hooks and user hasn't acknowledged yet
                    let tool_name = if data.tool.is_empty() {
                        "claude".to_string()
                    } else {
                        data.tool.clone()
                    };
                    let has_hooks = crate::agents::get_agent(&tool_name)
                        .and_then(|a| a.hook_config.as_ref())
                        .is_some();

                    if has_hooks {
                        let acknowledged = crate::session::config::load_config()
                            .ok()
                            .flatten()
                            .map(|c| c.app_state.has_acknowledged_agent_hooks)
                            .unwrap_or(false);

                        if !acknowledged {
                            self.hooks_install_dialog = Some(HooksInstallDialog::new(&tool_name));
                            self.pending_hooks_install_data = Some(data);
                            return None;
                        }
                    }

                    return self.continue_session_creation(data);
                }
            }
            return None;
        }

        if let Some(dialog) = &mut self.confirm_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    self.confirm_dialog = None;
                    self.pending_stop_session = None;
                }
                DialogResult::Submit(_) => {
                    let action = dialog.action().to_string();
                    self.confirm_dialog = None;
                    if action == "delete_group" {
                        if let Err(e) = self.delete_selected_group() {
                            tracing::error!("Failed to delete group: {}", e);
                        }
                    } else if action == "stop_session" {
                        if let Some(session_id) = self.pending_stop_session.take() {
                            return Some(Action::StopSession(session_id));
                        }
                    }
                }
            }
            return None;
        }

        if let Some(dialog) = &mut self.unified_delete_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    self.unified_delete_dialog = None;
                }
                DialogResult::Submit(options) => {
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
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    self.group_delete_options_dialog = None;
                }
                DialogResult::Submit(options) => {
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
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    self.rename_dialog = None;
                }
                DialogResult::Submit(data) => {
                    self.rename_dialog = None;
                    if let Err(e) = self.rename_selected(
                        &data.title,
                        data.group.as_deref(),
                        data.profile.as_deref(),
                    ) {
                        tracing::error!("Failed to rename session: {}", e);
                    }
                }
            }
            return None;
        }

        if let Some(dialog) = &mut self.profile_picker_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    self.profile_picker_dialog = None;
                }
                DialogResult::Submit(action) => match action {
                    ProfilePickerAction::Switch(name) => {
                        self.profile_picker_dialog = None;
                        // The synthetic "all" entry (only present in filtered mode)
                        // switches back to all-profiles mode
                        let profile = if self.active_profile.is_some() && name == "all" {
                            None
                        } else {
                            Some(name)
                        };
                        if let Err(e) = self.switch_profile(profile) {
                            tracing::error!("Failed to switch profile: {}", e);
                        }
                    }
                    ProfilePickerAction::Created(name) => {
                        self.profile_picker_dialog = None;
                        match crate::session::create_profile(&name) {
                            Ok(()) => {
                                if let Err(e) = self.switch_profile(Some(name)) {
                                    tracing::error!("Failed to switch to new profile: {}", e);
                                }
                            }
                            Err(e) => {
                                self.info_dialog = Some(InfoDialog::new(
                                    "Error",
                                    &format!("Failed to create profile: {}", e),
                                ));
                            }
                        }
                    }
                    ProfilePickerAction::Deleted(name) => {
                        match crate::session::delete_profile(&name) {
                            Ok(()) => {
                                self.show_profile_picker();
                            }
                            Err(e) => {
                                self.profile_picker_dialog = None;
                                self.info_dialog = Some(InfoDialog::new(
                                    "Error",
                                    &format!("Failed to delete profile: {}", e),
                                ));
                            }
                        }
                    }
                },
            }
            return None;
        }

        // Search mode
        if self.search_active {
            match key.code {
                KeyCode::Esc => {
                    self.search_active = false;
                    self.search_query = Input::default();
                    self.search_matches.clear();
                    self.search_match_index = 0;
                }
                KeyCode::Enter => {
                    self.search_active = false;
                    self.search_query = Input::default();
                    self.search_matches.clear();
                    self.search_match_index = 0;
                }
                _ => {
                    self.search_query
                        .handle_event(&crossterm::event::Event::Key(key));
                    self.update_search();
                }
            }
            return None;
        }

        // Normal mode keybindings
        match key.code {
            KeyCode::Esc => {
                if !self.search_matches.is_empty() {
                    self.search_matches.clear();
                    self.search_match_index = 0;
                    self.search_query = Input::default();
                }
            }
            KeyCode::Char('q') => return Some(Action::Quit),
            KeyCode::Char('?') => {
                self.show_help = true;
            }
            KeyCode::Char('P') => {
                self.show_profile_picker();
            }
            KeyCode::Char('t') => {
                self.view_mode = match self.view_mode {
                    ViewMode::Agent => ViewMode::Terminal,
                    ViewMode::Terminal => ViewMode::Agent,
                };
            }
            KeyCode::Char('c') => {
                // Toggle container/host terminal mode (only in Terminal view for sandboxed sessions)
                if self.view_mode == ViewMode::Terminal {
                    if let Some(id) = &self.selected_session {
                        if let Some(inst) = self.get_instance(id) {
                            if inst.is_sandboxed() {
                                let id = id.clone();
                                self.toggle_terminal_mode(&id);
                            } else {
                                self.info_dialog = Some(InfoDialog::new(
                                    "Not Available",
                                    "Only sandboxed sessions support container terminals. This session runs directly on the host.",
                                ));
                            }
                        }
                    }
                }
            }
            KeyCode::Char('/') => {
                self.search_active = true;
                self.search_query = Input::default();
            }
            KeyCode::Char('n') => {
                if !self.search_matches.is_empty() {
                    self.search_match_index =
                        (self.search_match_index + 1) % self.search_matches.len();
                    self.cursor = self.search_matches[self.search_match_index];
                    self.update_selected();
                } else {
                    let existing_titles: Vec<String> =
                        self.instances().iter().map(|i| i.title.clone()).collect();
                    let existing_groups: Vec<String> =
                        self.all_groups().iter().map(|g| g.path.clone()).collect();
                    let current_profile = self
                        .active_profile
                        .clone()
                        .unwrap_or_else(|| "default".to_string());
                    let profiles =
                        list_profiles().unwrap_or_else(|_| vec![current_profile.clone()]);
                    self.new_dialog = Some(NewSessionDialog::new(
                        self.available_tools.clone(),
                        existing_titles,
                        existing_groups,
                        &current_profile,
                        profiles,
                    ));
                }
            }
            KeyCode::Char('N') => {
                if !self.search_matches.is_empty() {
                    self.search_match_index = if self.search_match_index == 0 {
                        self.search_matches.len() - 1
                    } else {
                        self.search_match_index - 1
                    };
                    self.cursor = self.search_matches[self.search_match_index];
                    self.update_selected();
                }
            }
            KeyCode::Char('s') => {
                // Open settings view with selected session's project path (if any)
                let project_path = self
                    .selected_session
                    .as_ref()
                    .and_then(|id| self.get_instance(id))
                    .map(|inst| inst.project_path.clone());
                match SettingsView::new(
                    self.active_profile.as_deref().unwrap_or("default"),
                    project_path,
                ) {
                    Ok(view) => self.settings_view = Some(view),
                    Err(e) => {
                        tracing::error!("Failed to open settings: {}", e);
                        self.info_dialog = Some(InfoDialog::new(
                            "Error",
                            &format!("Failed to open settings: {}", e),
                        ));
                    }
                }
            }
            KeyCode::Char('D') => {
                // Open diff view - requires a selected session
                let Some(session_id) = &self.selected_session else {
                    self.info_dialog = Some(InfoDialog::new(
                        "No Session Selected",
                        "Select a session to view its diff.",
                    ));
                    return None;
                };

                let Some(inst) = self.get_instance(session_id) else {
                    self.info_dialog =
                        Some(InfoDialog::new("Error", "Could not find session data."));
                    return None;
                };

                let repo_path = std::path::PathBuf::from(&inst.project_path);
                match DiffView::new(repo_path) {
                    Ok(view) => self.diff_view = Some(view),
                    Err(e) => {
                        tracing::error!("Failed to open diff view: {}", e);
                        self.info_dialog = Some(InfoDialog::new(
                            "Error",
                            &format!("Failed to open diff view: {}", e),
                        ));
                    }
                }
            }
            KeyCode::Char('x') => {
                if let Some(session_id) = &self.selected_session {
                    if let Some(inst) = self.get_instance(session_id) {
                        if inst.status == Status::Stopped || inst.status == Status::Deleting {
                            return None;
                        }
                        let message = format!("Are you sure you want to stop '{}'?", inst.title);
                        self.pending_stop_session = Some(session_id.clone());
                        self.confirm_dialog =
                            Some(ConfirmDialog::new("Stop Session", &message, "stop_session"));
                    }
                }
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
                    if let Some(inst) = self.get_instance(session_id) {
                        if inst.status == Status::Deleting {
                            return None;
                        }

                        let config = DeleteDialogConfig {
                            worktree_branch: inst
                                .worktree_info
                                .as_ref()
                                .filter(|wt| wt.managed_by_aoe)
                                .map(|wt| wt.branch.clone())
                                .or_else(|| inst.workspace_info.as_ref().map(|w| w.branch.clone())),
                            has_sandbox: inst.sandbox_info.as_ref().is_some_and(|s| s.enabled),
                        };

                        let profile = self.active_profile.as_deref().unwrap_or("default");
                        self.unified_delete_dialog = Some(UnifiedDeleteDialog::new(
                            inst.title.clone(),
                            config,
                            profile,
                        ));
                    } else {
                        let profile = self.active_profile.as_deref().unwrap_or("default");
                        self.unified_delete_dialog = Some(UnifiedDeleteDialog::new(
                            "Unknown Session".to_string(),
                            DeleteDialogConfig::default(),
                            profile,
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
                        let has_containers = self.group_has_containers(group_path, &prefix);
                        self.group_delete_options_dialog = Some(GroupDeleteOptionsDialog::new(
                            group_path.clone(),
                            session_count,
                            has_managed_worktrees,
                            has_containers,
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
                    if let Some(inst) = self.get_instance(id) {
                        if inst.status == Status::Deleting {
                            return None;
                        }
                        let current_profile = self
                            .active_profile
                            .clone()
                            .unwrap_or_else(|| "default".to_string());
                        let profiles =
                            list_profiles().unwrap_or_else(|_| vec![current_profile.clone()]);
                        let existing_groups: Vec<String> =
                            self.all_groups().iter().map(|g| g.path.clone()).collect();
                        self.rename_dialog = Some(RenameDialog::new(
                            &inst.title,
                            &inst.group_path,
                            &current_profile,
                            profiles,
                            existing_groups,
                        ));
                    }
                }
            }
            KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.apply_sort_order(self.sort_order.cycle_reverse());
            }
            KeyCode::Char('o') => {
                self.apply_sort_order(self.sort_order.cycle());
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
                    if let Some(inst) = self.get_instance(id) {
                        if inst.status == Status::Deleting {
                            return None;
                        }
                    }
                    return match self.view_mode {
                        ViewMode::Agent => Some(Action::AttachSession(id.clone())),
                        ViewMode::Terminal => {
                            let terminal_mode = if let Some(inst) = self.get_instance(id) {
                                if inst.is_sandboxed() {
                                    self.get_terminal_mode(id)
                                } else {
                                    TerminalMode::Host
                                }
                            } else {
                                TerminalMode::Host
                            };
                            Some(Action::AttachTerminal(id.clone(), terminal_mode))
                        }
                    };
                } else if let Some(Item::Group { path, .. }) = self.flat_items.get(self.cursor) {
                    let path = path.clone();
                    self.toggle_group_collapsed(&path);
                }
            }
            KeyCode::Char('H') => {
                self.shrink_list();
            }
            KeyCode::Char('L') => {
                self.grow_list();
            }
            KeyCode::Left | KeyCode::Char('h') => {
                if let Some(Item::Group {
                    path, collapsed, ..
                }) = self.flat_items.get(self.cursor)
                {
                    if !collapsed {
                        let path = path.clone();
                        self.toggle_group_collapsed(&path);
                    }
                }
            }
            KeyCode::Right | KeyCode::Char('l') => {
                if let Some(Item::Group {
                    path, collapsed, ..
                }) = self.flat_items.get(self.cursor)
                {
                    if *collapsed {
                        let path = path.clone();
                        self.toggle_group_collapsed(&path);
                    }
                }
            }
            _ => {}
        }

        None
    }

    pub(super) fn move_cursor(&mut self, delta: i32) {
        if self.flat_items.is_empty() {
            return;
        }

        let new_cursor = if delta < 0 {
            self.cursor.saturating_sub((-delta) as usize)
        } else {
            (self.cursor + delta as usize).min(self.flat_items.len() - 1)
        };

        self.cursor = new_cursor;
        self.update_selected();
    }

    pub(super) fn update_selected(&mut self) {
        if let Some(item) = self.flat_items.get(self.cursor) {
            match item {
                Item::Session { id, .. } => {
                    self.selected_session = Some(id.clone());
                    self.selected_group = None;
                    self.selected_group_profile = None;
                }
                Item::Group { path, .. } => {
                    self.selected_session = None;
                    self.selected_group = Some(path.clone());
                    self.selected_group_profile = self.profile_for_cursor(self.cursor);
                }
            }
        }
    }

    fn apply_sort_order(&mut self, new_order: SortOrder) {
        self.sort_order = new_order;
        self.flat_items = self.build_flat_items();
        if self.search_active && !self.search_query.value().is_empty() {
            self.update_search();
        } else {
            self.cursor = self.cursor.min(self.flat_items.len().saturating_sub(1));
            self.update_selected();
        }
        if let Ok(mut config) = load_config().map(|c| c.unwrap_or_default()) {
            config.app_state.sort_order = Some(self.sort_order);
            if let Err(e) = save_config(&config) {
                tracing::warn!("Failed to save sort order: {}", e);
            }
        }
    }

    fn toggle_group_collapsed(&mut self, path: &str) {
        // Route to the correct profile's GroupTree
        let profile = self.profile_for_cursor(self.cursor);
        if let Some(profile) = profile {
            if let Some(tree) = self.group_trees.get_mut(&profile) {
                tree.toggle_collapsed(path);
            }
        }
        self.flat_items = self.build_flat_items();
        if let Err(e) = self.save() {
            tracing::error!("Failed to save group state: {}", e);
        }
    }

    /// Re-score matches after a reload without moving the cursor.
    pub(super) fn refresh_search_matches(&mut self) {
        let query = self.search_query.value();
        if query.is_empty() {
            self.search_matches.clear();
            self.search_match_index = 0;
            return;
        }

        use nucleo_matcher::pattern::{Atom, AtomKind, CaseMatching, Normalization};
        use nucleo_matcher::{Config, Matcher, Utf32Str};

        let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
        let atom = Atom::new(
            query,
            CaseMatching::Ignore,
            Normalization::Smart,
            AtomKind::Fuzzy,
            false,
        );

        let mut scored: Vec<(usize, u16)> = Vec::new();
        let mut buf = Vec::new();

        for (idx, item) in self.flat_items.iter().enumerate() {
            let haystack = match item {
                Item::Session { id, .. } => {
                    if let Some(inst) = self.get_instance(id) {
                        format!("{} {}", inst.title, inst.project_path)
                    } else {
                        continue;
                    }
                }
                Item::Group { name, path, .. } => {
                    format!("{} {}", name, path)
                }
            };

            let haystack_utf32 = Utf32Str::new(&haystack, &mut buf);
            if let Some(score) = atom.score(haystack_utf32, &mut matcher) {
                scored.push((idx, score));
            }
        }

        scored.sort_by(|a, b| b.1.cmp(&a.1));
        self.search_matches = scored.into_iter().map(|(idx, _)| idx).collect();
        // Clamp match_index in case matches shrank
        if self.search_matches.is_empty() {
            self.search_match_index = 0;
        } else if self.search_match_index >= self.search_matches.len() {
            self.search_match_index = self.search_matches.len() - 1;
        }
    }

    pub(super) fn update_search(&mut self) {
        self.search_matches.clear();
        self.search_match_index = 0;

        let query = self.search_query.value();
        if query.is_empty() {
            return;
        }

        use nucleo_matcher::pattern::{Atom, AtomKind, CaseMatching, Normalization};
        use nucleo_matcher::{Config, Matcher, Utf32Str};

        let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
        let atom = Atom::new(
            query,
            CaseMatching::Ignore,
            Normalization::Smart,
            AtomKind::Fuzzy,
            false,
        );

        let mut scored: Vec<(usize, u16)> = Vec::new();
        let mut buf = Vec::new();

        for (idx, item) in self.flat_items.iter().enumerate() {
            let haystack = match item {
                Item::Session { id, .. } => {
                    if let Some(inst) = self.get_instance(id) {
                        format!("{} {}", inst.title, inst.project_path)
                    } else {
                        continue;
                    }
                }
                Item::Group { name, path, .. } => {
                    format!("{} {}", name, path)
                }
            };

            let haystack_utf32 = Utf32Str::new(&haystack, &mut buf);
            if let Some(score) = atom.score(haystack_utf32, &mut matcher) {
                scored.push((idx, score));
            }
        }

        scored.sort_by(|a, b| b.1.cmp(&a.1));
        self.search_matches = scored.into_iter().map(|(idx, _)| idx).collect();

        if let Some(&best) = self.search_matches.first() {
            self.cursor = best;
            self.update_selected();
        }
    }

    /// Continue session creation after agent hooks acknowledgment.
    /// Runs the repo hook trust check and then creates the session.
    fn continue_session_creation(&mut self, data: NewSessionData) -> Option<Action> {
        match repo_config::check_hook_trust(std::path::Path::new(&data.path)) {
            Ok(repo_config::HookTrustStatus::NeedsTrust { hooks, hooks_hash }) => {
                use crate::tui::dialogs::HookTrustDialog;
                self.hook_trust_dialog =
                    Some(HookTrustDialog::new(hooks, hooks_hash, data.path.clone()));
                self.pending_hook_trust_data = Some(data);
                None
            }
            Ok(repo_config::HookTrustStatus::Trusted(repo_hooks)) => {
                let merged = self.merge_repo_hooks_onto_config_for(&data.profile, repo_hooks);
                self.create_session_with_hooks(data, merged)
            }
            Ok(repo_config::HookTrustStatus::NoHooks) => {
                let fallback = self.resolve_global_profile_hooks_for(&data.profile);
                self.create_session_with_hooks(data, fallback)
            }
            Err(e) => {
                tracing::warn!("Failed to check repo hooks: {}", e);
                let fallback = self.resolve_global_profile_hooks_for(&data.profile);
                self.create_session_with_hooks(data, fallback)
            }
        }
    }

    /// Create a session with optional hooks. Delegates to the background
    /// `CreationPoller` when hooks are present (to avoid freezing the TUI on
    /// slow commands like `npm install`) or when the session is sandboxed.
    fn create_session_with_hooks(
        &mut self,
        data: NewSessionData,
        hooks: Option<crate::session::HooksConfig>,
    ) -> Option<Action> {
        let has_hooks = hooks
            .as_ref()
            .is_some_and(|h| !h.on_create.is_empty() || !h.on_launch.is_empty());

        if data.sandbox || has_hooks {
            self.request_creation(data, hooks);
            return None;
        }

        match self.create_session(data) {
            Ok(session_id) => {
                self.new_dialog = None;
                Some(Action::AttachSession(session_id))
            }
            Err(e) => {
                tracing::error!("Failed to create session: {}", e);
                if let Some(dialog) = &mut self.new_dialog {
                    dialog.set_error(e.to_string());
                }
                None
            }
        }
    }

    /// Handle a mouse event
    pub fn handle_mouse(&mut self, mouse: MouseEvent) -> Option<Action> {
        // Pass mouse events to diff view if active
        if let Some(ref mut diff_view) = self.diff_view {
            match diff_view.handle_mouse(mouse) {
                DiffAction::Continue => return None,
                DiffAction::Close => {
                    self.diff_view = None;
                    return None;
                }
                DiffAction::EditFile(path) => {
                    return Some(Action::EditFile(path));
                }
            }
        }

        // No mouse handling for other views currently
        None
    }

    /// Resolve hooks from global+profile config for the given profile.
    fn resolve_global_profile_hooks_for(
        &self,
        profile: &str,
    ) -> Option<crate::session::HooksConfig> {
        let config = resolve_config(profile).ok()?;
        if config.hooks.on_create.is_empty() && config.hooks.on_launch.is_empty() {
            None
        } else {
            Some(config.hooks)
        }
    }

    /// Merge trusted repo hooks onto the resolved config for the given profile.
    fn merge_repo_hooks_onto_config_for(
        &self,
        profile: &str,
        repo_hooks: crate::session::HooksConfig,
    ) -> Option<crate::session::HooksConfig> {
        let mut base = resolve_config(profile).map(|c| c.hooks).unwrap_or_default();

        if !repo_hooks.on_create.is_empty() {
            base.on_create = repo_hooks.on_create;
        }
        if !repo_hooks.on_launch.is_empty() {
            base.on_launch = repo_hooks.on_launch;
        }

        if base.on_create.is_empty() && base.on_launch.is_empty() {
            None
        } else {
            Some(base)
        }
    }
}
