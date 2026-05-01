//! Input handling for HomeView

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::Position;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

use super::{HomeView, TerminalMode, ViewMode};
use crate::session::config::{load_config, save_config, GroupByMode, SortOrder};
use crate::session::{list_profiles, repo_config, resolve_config_or_warn, Item, Status};
use crate::tui::app::Action;
#[cfg(feature = "serve")]
use crate::tui::dialogs::ServeAction;
use crate::tui::dialogs::{
    ConfirmDialog, DeleteDialogConfig, DialogResult, GroupDeleteOptionsDialog, HookTrustAction,
    HooksInstallDialog, InfoDialog, NewSessionData, NewSessionDialog, NoAgentsAction,
    ProfilePickerAction, RenameDialog, RenameMode, SendMessageDialog, UnifiedDeleteDialog,
};
use crate::tui::diff::{DiffAction, DiffView};
use crate::tui::settings::{SettingsAction, SettingsView};

impl HomeView {
    pub fn is_diff_open(&self) -> bool {
        self.diff_view.is_some()
    }

    pub fn has_selected_session(&self) -> bool {
        self.selected_session.is_some()
    }

    pub fn hit_preview(&self, col: u16, row: u16) -> bool {
        self.preview_area.contains(Position::from((col, row)))
    }

    pub fn hit_diff(&self, col: u16, row: u16) -> bool {
        self.diff_area.contains(Position::from((col, row)))
    }

    pub fn handle_key(
        &mut self,
        key: KeyEvent,
        update_info: Option<&crate::update::UpdateInfo>,
    ) -> Option<Action> {
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
                        let config = resolve_config_or_warn(
                            self.active_profile.as_deref().unwrap_or("default"),
                        );
                        let theme_name = if config.theme.name.is_empty() {
                            "empire".to_string()
                        } else {
                            config.theme.name
                        };
                        return Some(Action::SetTheme(theme_name));
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
                    let config =
                        resolve_config_or_warn(self.active_profile.as_deref().unwrap_or("default"));
                    let theme_name = if config.theme.name.is_empty() {
                        "empire".to_string()
                    } else {
                        config.theme.name
                    };
                    return Some(Action::SetTheme(theme_name));
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

        // Handle serve view (full-screen takeover)
        #[cfg(feature = "serve")]
        if let Some(ref mut serve) = self.serve_view {
            match serve.handle_key(key) {
                ServeAction::Continue => return None,
                ServeAction::Close => {
                    self.serve_view = None;
                    return None;
                }
            }
        }

        // Handle no-agents dialog (highest priority, blocks all interaction)
        if let Some(dialog) = &mut self.no_agents_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel | DialogResult::Submit(NoAgentsAction::Quit) => {
                    return Some(Action::Quit);
                }
                DialogResult::Submit(NoAgentsAction::Recheck) => {
                    let tools = crate::tmux::AvailableTools::detect();
                    if tools.any_available() {
                        self.set_available_tools(tools);
                        self.no_agents_dialog = None;
                    }
                    // If still no agents, keep dialog open (user can try again)
                }
            }
            return None;
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
                                    repo_config::merge_hooks_with_config(&data.profile, hooks);
                                return self.create_session_with_hooks(data, merged);
                            }
                            HookTrustAction::Skip => {
                                let fallback =
                                    repo_config::resolve_global_profile_hooks(&data.profile);
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
                        let config = crate::session::config::load_config().ok().flatten();
                        let hooks_enabled = config
                            .as_ref()
                            .map(|c| c.session.agent_status_hooks)
                            .unwrap_or(true);
                        let acknowledged = config
                            .as_ref()
                            .map(|c| c.app_state.has_acknowledged_agent_hooks)
                            .unwrap_or(false);

                        if hooks_enabled && !acknowledged {
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
                    self.pending_force_remove_session = None;
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
                    } else if action == "force_remove_session" {
                        if let Some(session_id) = self.pending_force_remove_session.take() {
                            if let Err(e) = self.force_remove_session(&session_id) {
                                tracing::error!("Failed to force remove session: {}", e);
                            }
                        }
                    } else if action == "quit_during_creation" {
                        return Some(Action::Quit);
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
            let mode = dialog.mode();
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    self.rename_dialog = None;
                    self.group_rename_context = None;
                }
                DialogResult::Submit(data) => {
                    self.rename_dialog = None;
                    match mode {
                        RenameMode::Session => {
                            if let Err(e) = self.rename_selected(
                                &data.title,
                                data.group.as_deref(),
                                data.profile.as_deref(),
                            ) {
                                tracing::error!("Failed to rename session: {}", e);
                            }
                        }
                        RenameMode::Group => {
                            if let Err(e) = self.rename_selected_group(
                                data.group.as_deref(),
                                data.profile.as_deref(),
                            ) {
                                tracing::error!("Failed to rename group: {}", e);
                            }
                        }
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

        // Send message dialog
        if let Some(dialog) = &mut self.send_message_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    self.send_message_dialog = None;
                    self.pending_send_session = None;
                }
                DialogResult::Submit(message) => {
                    self.send_message_dialog = None;
                    if let Some(session_id) = self.pending_send_session.take() {
                        if let Some(inst) = self.get_instance(&session_id) {
                            match crate::tmux::Session::new(&inst.id, &inst.title) {
                                Ok(tmux_session) => {
                                    let delay = crate::agents::send_keys_enter_delay(&inst.tool);
                                    if let Err(e) =
                                        tmux_session.send_keys_with_delay(&message, delay)
                                    {
                                        self.info_dialog = Some(InfoDialog::new(
                                            "Send Failed",
                                            &format!("Failed to send message: {}", e),
                                        ));
                                    } else {
                                        self.stamp_last_accessed(&session_id);
                                    }
                                }
                                Err(e) => {
                                    self.info_dialog = Some(InfoDialog::new(
                                        "Send Failed",
                                        &format!("Failed to resolve session: {}", e),
                                    ));
                                }
                            }
                        }
                    }
                }
            }
            return None;
        }

        if let Some(dialog) = &mut self.update_confirm_dialog {
            use crate::tui::dialogs::DialogResult;
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    self.update_confirm_dialog = None;
                }
                DialogResult::Submit(()) => {
                    let method = dialog.method.clone();
                    let version = dialog.latest_version.clone();
                    self.update_confirm_dialog = None;
                    return Some(Action::SpawnUpdate(method, version));
                }
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

        // In strict_hotkeys mode, normalize shifted/ctrl keys to their standard
        // equivalents so the match block below doesn't need duplication.
        //
        // Mapping (strict mode only):
        //   Shift+letter actions -> lowercase: N->n, X->x, D->d, R->r, S->s, M->m, T->t, C->c, Q->q, O->o
        //   Ctrl+letter relocated bindings -> uppercase: Ctrl+T->T, Ctrl+D->D, Ctrl+R->R, Ctrl+P->P, Ctrl+N->N
        //   Ctrl+G -> g (group toggle was lowercase)
        //   Bare lowercase action letters -> blocked (return None)
        let key = if self.strict_hotkeys {
            self.normalize_strict_key(key)
        } else {
            Some(key)
        };
        let key = key?;

        // Normal mode keybindings
        match key.code {
            KeyCode::Esc if !self.search_matches.is_empty() => {
                self.search_matches.clear();
                self.search_match_index = 0;
                self.search_query = Input::default();
            }
            KeyCode::Char('q') => return Some(Action::Quit),
            KeyCode::Char('?') => {
                self.show_help = true;
            }
            KeyCode::Char('P') => {
                self.show_profile_picker();
            }
            #[cfg(feature = "serve")]
            KeyCode::Char('R') => {
                self.serve_view = Some(crate::tui::dialogs::ServeView::new());
            }
            #[cfg(not(feature = "serve"))]
            KeyCode::Char('R') => {
                self.info_dialog = Some(InfoDialog::new(
                    "Serve unavailable",
                    "This `aoe` binary was built without the `serve` feature, \
                     so the web dashboard, local network serving, and \
                     Cloudflare Tunnel integration are not included.\n\n\
                     To serve to your phone (LAN / Tailscale / tunnel):\n\
                       \u{2022} Install a release build from GitHub Releases, or\n\
                       \u{2022} Build from source with:\n\
                         cargo build --release --features serve\n\n\
                     Once you have a `serve`-enabled binary, press R again to \
                     open the serve dialog.",
                ));
            }
            KeyCode::Char('t') => {
                self.view_mode = match self.view_mode {
                    ViewMode::Agent => ViewMode::Terminal,
                    ViewMode::Terminal => ViewMode::Agent,
                };
            }
            KeyCode::Char('T') => {
                // Quick-attach to paired terminal from any view
                if let Some(id) = &self.selected_session {
                    if let Some(inst) = self.get_instance(id) {
                        if matches!(inst.status, Status::Deleting | Status::Creating) {
                            return None;
                        }
                    }
                    let terminal_mode = if let Some(inst) = self.get_instance(id) {
                        if inst.is_sandboxed() {
                            self.get_terminal_mode(id)
                        } else {
                            TerminalMode::Host
                        }
                    } else {
                        TerminalMode::Host
                    };
                    return Some(Action::AttachTerminal(id.clone(), terminal_mode));
                }
            }
            KeyCode::Char('c') if self.view_mode == ViewMode::Terminal => {
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
                } else if self.creating_stub_id.is_some() {
                    self.info_dialog = Some(InfoDialog::new(
                        "Please Wait",
                        "A session is already being created. Wait for it to finish or press Ctrl+C to cancel.",
                    ));
                } else if !self.available_tools.any_available() {
                    self.show_no_agents();
                } else {
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
                } else if self.creating_stub_id.is_some() {
                    self.info_dialog = Some(InfoDialog::new(
                        "Please Wait",
                        "A session is already being created. Wait for it to finish or press Ctrl+C to cancel.",
                    ));
                } else {
                    // Pre-filled new session from selection
                    let prefill_path = self
                        .selected_session
                        .as_ref()
                        .and_then(|id| self.get_instance(id))
                        .map(|inst| {
                            inst.worktree_info
                                .as_ref()
                                .map(|wt| wt.main_repo_path.clone())
                                .unwrap_or_else(|| inst.project_path.clone())
                        });
                    let prefill_group = self
                        .selected_session
                        .as_ref()
                        .and_then(|id| self.get_instance(id))
                        .and_then(|inst| {
                            if inst.group_path.is_empty() {
                                None
                            } else {
                                Some(inst.group_path.clone())
                            }
                        })
                        .or_else(|| self.selected_group.clone());

                    if prefill_path.is_some() || prefill_group.is_some() {
                        let existing_groups: Vec<String> =
                            self.all_groups().iter().map(|g| g.path.clone()).collect();
                        let current_profile = self
                            .profile_for_cursor(self.cursor)
                            .or_else(|| self.active_profile.clone())
                            .unwrap_or_else(|| "default".to_string());
                        let profiles =
                            list_profiles().unwrap_or_else(|_| vec![current_profile.clone()]);
                        let mut dialog = NewSessionDialog::new(
                            self.available_tools.clone(),
                            existing_groups,
                            &current_profile,
                            profiles,
                        );
                        if let Some(path) = prefill_path {
                            dialog.set_path(path);
                        }
                        if let Some(group) = prefill_group {
                            dialog.set_group(group);
                        }
                        self.new_dialog = Some(dialog);
                    }
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
            KeyCode::Char('u') => {
                if let Some(info) = update_info {
                    if info.available && self.update_confirm_dialog.is_none() {
                        let method = match crate::update::install::detect_install_method() {
                            Ok(m) => m,
                            Err(e) => {
                                tracing::warn!("update detection failed: {e}");
                                return None;
                            }
                        };
                        use crate::update::install::InstallMethod;
                        if !matches!(
                            &method,
                            InstallMethod::Homebrew | InstallMethod::Tarball { .. }
                        ) {
                            let msg = match &method {
                                InstallMethod::Nix => {
                                    "Nix install: run `nix run github:njbrake/agent-of-empires` to update".to_string()
                                }
                                InstallMethod::Cargo => {
                                    "Cargo install: run `cargo install --git https://github.com/njbrake/agent-of-empires aoe`".to_string()
                                }
                                InstallMethod::Unknown { .. } => {
                                    "Unknown install method: run `aoe update` in a terminal for instructions".to_string()
                                }
                                _ => unreachable!(),
                            };
                            return Some(Action::SetTransientStatus(msg));
                        }
                        let needs_sudo = matches!(
                            &method,
                            InstallMethod::Tarball { binary_path }
                                if !crate::update::install::parent_is_writable(binary_path)
                        );
                        self.update_confirm_dialog =
                            Some(crate::tui::dialogs::UpdateConfirmDialog::new(
                                info.current_version.clone(),
                                info.latest_version.clone(),
                                method,
                                needs_sudo,
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
                        if matches!(
                            inst.status,
                            Status::Stopped | Status::Deleting | Status::Creating
                        ) {
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
                        if inst.status == Status::Creating {
                            return None;
                        }
                        if inst.status == Status::Deleting {
                            let message = format!(
                                "'{}' is stuck deleting. Force remove it from the session list? \
                                 (worktrees, branches, and containers will not be cleaned up)",
                                inst.title
                            );
                            self.pending_force_remove_session = Some(session_id.clone());
                            self.confirm_dialog = Some(ConfirmDialog::new(
                                "Force Remove",
                                &message,
                                "force_remove_session",
                            ));
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
                            project_path: Some(inst.project_path.clone()),
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
                    if self.group_by == GroupByMode::Project {
                        self.info_dialog = Some(InfoDialog::new(
                            "Cannot Modify Project Groups",
                            "Project groups are automatic. Press 'g' to switch to manual grouping to manage groups.",
                        ));
                        return None;
                    }
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
            KeyCode::Char('r') => {
                if let Some(id) = &self.selected_session {
                    if let Some(inst) = self.get_instance(id) {
                        if matches!(inst.status, Status::Deleting | Status::Creating) {
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
                } else if let Some(group_path) = &self.selected_group {
                    if self.group_by == GroupByMode::Project {
                        self.info_dialog = Some(InfoDialog::new(
                            "Cannot Modify Project Groups",
                            "Project groups are automatic. Press 'g' to switch to manual grouping to manage groups.",
                        ));
                        return None;
                    }
                    let group_path = group_path.clone();
                    let current_profile = self
                        .selected_group_profile
                        .clone()
                        .or_else(|| self.active_profile.clone())
                        .unwrap_or_else(|| "default".to_string());
                    let profiles =
                        list_profiles().unwrap_or_else(|_| vec![current_profile.clone()]);
                    let existing_groups: Vec<String> =
                        self.all_groups().iter().map(|g| g.path.clone()).collect();
                    self.group_rename_context = Some(super::GroupRenameContext {
                        old_path: group_path.clone(),
                        old_profile: current_profile.clone(),
                    });
                    self.rename_dialog = Some(RenameDialog::new_for_group(
                        &group_path,
                        &current_profile,
                        profiles,
                        existing_groups,
                    ));
                }
            }
            KeyCode::Char('m') => {
                if let Some(id) = self.selected_session.clone() {
                    if let Some(inst) = self.get_instance(&id) {
                        if inst.status == Status::Creating {
                            return None;
                        }
                        let title = inst.title.clone();
                        let inst_id = inst.id.clone();
                        let tmux_session = crate::tmux::Session::new(&inst_id, &title).ok();
                        let is_running = tmux_session.as_ref().is_some_and(|s| s.exists());
                        if is_running {
                            self.pending_send_session = Some(id);
                            self.send_message_dialog = Some(SendMessageDialog::new(&title));
                        }
                    }
                }
            }
            KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.apply_sort_order(self.sort_order.cycle_reverse());
            }
            KeyCode::Char('o') => {
                self.apply_sort_order(self.sort_order.cycle());
            }
            // iPad-friendly ±10 aliases for PageUp/PageDown. iPads have no
            // PageUp/PageDown keys, and Cmd combos are typically stripped by
            // SSH/Mosh before reaching the TTY. Shift+Up/Down arrives intact
            // on every terminal we test, and `{` / `}` (Shift+`[` / Shift+`]`)
            // pass through as plain chars so Cmd+Shift+`[` / `]` works whether
            // or not the terminal forwards Cmd. Both bind to the same step
            // size as PageUp/PageDown to keep the mental model simple.
            KeyCode::Up if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.move_cursor(-10);
            }
            KeyCode::Down if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.move_cursor(10);
            }
            KeyCode::Char('{') => {
                self.move_cursor(-10);
            }
            KeyCode::Char('}') => {
                self.move_cursor(10);
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
            KeyCode::Home => {
                self.cursor = 0;
                self.update_selected();
            }
            KeyCode::Char('g') => {
                self.apply_group_by(self.group_by.cycle());
            }
            KeyCode::End | KeyCode::Char('G') if !self.flat_items.is_empty() => {
                self.cursor = self.flat_items.len() - 1;
                self.update_selected();
            }
            KeyCode::Enter => {
                if let Some(id) = &self.selected_session {
                    if let Some(inst) = self.get_instance(id) {
                        if matches!(inst.status, Status::Deleting | Status::Creating) {
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
            KeyCode::Char('w') => {
                self.jump_to_next_waiting();
            }
            _ => {}
        }

        None
    }

    fn jump_to_next_waiting(&mut self) {
        let len = self.flat_items.len();
        if len == 0 {
            return;
        }

        // Pass 1: forward-walk from cursor+1, wrapping, for the next Waiting
        // session OR a freshly-stopped Idle session (within
        // `idle_decay_window`). Both states are "needs your attention" and
        // cycle together so repeated `w` taps move through the actionable
        // backlog regardless of which hook fired.
        let window = self.idle_decay_window;
        let start = (self.cursor + 1) % len;
        for i in 0..len - 1 {
            let idx = (start + i) % len;
            let id = match self.flat_items.get(idx) {
                Some(Item::Session { id, .. }) => id.clone(),
                _ => continue,
            };
            if let Some(inst) = self.get_instance(&id) {
                let is_actionable = inst.status == Status::Waiting
                    || matches!(inst.idle_age(), Some(age) if age < window);
                if is_actionable {
                    self.cursor = idx;
                    self.update_selected();
                    return;
                }
            }
        }

        // Pass 2: fall back to the most-recently-accessed Idle session, skipping
        // the cursor. Sessions never attached (last_accessed_at == None) rank
        // last but remain eligible.
        let mut best: Option<(usize, Option<chrono::DateTime<chrono::Utc>>)> = None;
        for idx in 0..len {
            if idx == self.cursor {
                continue;
            }
            let id = match self.flat_items.get(idx) {
                Some(Item::Session { id, .. }) => id.clone(),
                _ => continue,
            };
            let Some(inst) = self.get_instance(&id) else {
                continue;
            };
            if inst.status != Status::Idle {
                continue;
            }
            let ts = inst.last_accessed_at;
            let beats = match best {
                None => true,
                Some((_, b)) => match (ts, b) {
                    (Some(a), Some(b)) => a > b,
                    (Some(_), None) => true,
                    (None, _) => false,
                },
            };
            if beats {
                best = Some((idx, ts));
            }
        }

        if let Some((idx, _)) = best {
            self.cursor = idx;
            self.update_selected();
            return;
        }

        self.info_dialog = Some(InfoDialog::new(
            "No Available Sessions",
            "No sessions are currently waiting or idle.",
        ));
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
            let prev_session = self.selected_session.clone();
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
            if self.selected_session != prev_session {
                self.preview_scroll_offset = 0;
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

    fn apply_group_by(&mut self, new_mode: GroupByMode) {
        self.group_by = new_mode;
        self.flat_items = self.build_flat_items();
        self.cursor = self.cursor.min(self.flat_items.len().saturating_sub(1));
        self.update_selected();
        match load_config().map(|c| c.unwrap_or_default()) {
            Ok(mut config) => {
                config.app_state.group_by = Some(self.group_by);
                if let Err(e) = save_config(&config) {
                    tracing::warn!("Failed to save group_by mode: {}", e);
                }
            }
            Err(e) => {
                tracing::warn!("Failed to load config for group_by save: {}", e);
            }
        }
    }

    fn toggle_group_collapsed(&mut self, path: &str) {
        if self.group_by == GroupByMode::Project {
            let collapsed = self
                .project_group_collapsed
                .get(path)
                .copied()
                .unwrap_or(false);
            self.project_group_collapsed
                .insert(path.to_string(), !collapsed);
            self.flat_items = self.build_flat_items();
            return;
        }
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

    /// Scroll the preview pane up by one mouse-wheel step. Returns `true` if
    /// the UI should redraw. When the diff view is open, scroll the diff
    /// content instead.
    pub fn handle_scroll_up(&mut self) -> bool {
        const STEP: u16 = 3;
        if let Some(ref mut diff) = self.diff_view {
            diff.scroll_up(STEP);
            return true;
        }
        if self.selected_session.is_none() || self.has_dialog() {
            return false;
        }

        let active_cache = match self.view_mode {
            ViewMode::Agent => &self.preview_cache,
            ViewMode::Terminal => {
                let terminal_mode = self
                    .selected_session
                    .as_ref()
                    .and_then(|id| self.get_instance(id))
                    .map(|inst| {
                        if inst.is_sandboxed() {
                            self.get_terminal_mode(&inst.id)
                        } else {
                            TerminalMode::Host
                        }
                    })
                    .unwrap_or(TerminalMode::Host);
                match terminal_mode {
                    TerminalMode::Container => &self.container_terminal_preview_cache,
                    TerminalMode::Host => &self.terminal_preview_cache,
                }
            }
        };

        let visible_height = active_cache.dimensions.1.saturating_sub(1) as usize;
        let real_max = active_cache.captured_lines.saturating_sub(visible_height) as u16;

        let new_offset = self.preview_scroll_offset.saturating_add(STEP);
        let clamped = new_offset.min(real_max);
        if clamped == self.preview_scroll_offset {
            return false;
        }
        self.preview_scroll_offset = clamped;
        true
    }

    /// Scroll the preview pane down by one mouse-wheel step. Returns `true`
    /// if the UI should redraw. When the diff view is open, scroll the diff
    /// content instead.
    pub fn handle_scroll_down(&mut self) -> bool {
        const STEP: u16 = 3;
        if let Some(ref mut diff) = self.diff_view {
            diff.scroll_down(STEP);
            return true;
        }
        if self.selected_session.is_none() || self.has_dialog() {
            return false;
        }
        if self.preview_scroll_offset == 0 {
            return false;
        }
        self.preview_scroll_offset = self.preview_scroll_offset.saturating_sub(STEP);
        true
    }

    /// Route a bracketed paste event to the active text input dialog.
    pub fn handle_paste(&mut self, text: &str) {
        if let Some(ref mut settings) = self.settings_view {
            settings.handle_paste(text);
            return;
        }
        if let Some(ref mut dialog) = self.rename_dialog {
            dialog.handle_paste(text);
            return;
        }
        if let Some(ref mut dialog) = self.send_message_dialog {
            dialog.handle_paste(text);
            return;
        }
        if let Some(ref mut dialog) = self.new_dialog {
            dialog.handle_paste(text);
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

        scored.sort_by_key(|a| std::cmp::Reverse(a.1));
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

        scored.sort_by_key(|a| std::cmp::Reverse(a.1));
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
                let merged = repo_config::merge_hooks_with_config(&data.profile, repo_hooks);
                self.create_session_with_hooks(data, merged)
            }
            Ok(repo_config::HookTrustStatus::NoHooks) => {
                let fallback = repo_config::resolve_global_profile_hooks(&data.profile);
                self.create_session_with_hooks(data, fallback)
            }
            Err(e) => {
                tracing::warn!("Failed to check repo hooks: {}", e);
                let fallback = repo_config::resolve_global_profile_hooks(&data.profile);
                self.create_session_with_hooks(data, fallback)
            }
        }
    }

    /// Create a session with optional hooks. Delegates to the background
    /// `CreationPoller` when hooks are present, when the session is sandboxed,
    /// or when a worktree branch is requested (to avoid freezing the TUI on
    /// slow git hooks like `post-checkout`).
    fn create_session_with_hooks(
        &mut self,
        data: NewSessionData,
        hooks: Option<crate::session::HooksConfig>,
    ) -> Option<Action> {
        let has_hooks = hooks
            .as_ref()
            .is_some_and(|h| !h.on_create.is_empty() || !h.on_launch.is_empty());
        let has_worktree = data.worktree_branch.as_ref().is_some_and(|b| !b.is_empty());

        if data.sandbox || has_hooks || has_worktree {
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

    /// In strict_hotkeys mode, normalize key events so the main match block
    /// doesn't need per-key duplication. Returns `None` to swallow bare
    /// lowercase action letters that would otherwise fire destructive actions.
    fn normalize_strict_key(&self, key: KeyEvent) -> Option<KeyEvent> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let bare = key.modifiers == KeyModifiers::NONE;
        let shift_only = key.modifiers == KeyModifiers::SHIFT;
        let has_search = !self.search_matches.is_empty();

        // n/N are dual-purpose: search next/prev AND new session/new-from-selection.
        // When search matches exist, let them through unchanged for vi-style navigation.
        if has_search {
            match key.code {
                KeyCode::Char('n') if bare => return Some(key),
                KeyCode::Char('N') if bare || shift_only => return Some(key),
                _ => {}
            }
        }

        match key.code {
            // Ctrl+letter relocations: map to the uppercase letter they replace
            // Ctrl+T -> T (attach terminal), Ctrl+D -> D (diff view),
            // Ctrl+R -> R (serve), Ctrl+P -> P (profiles), Ctrl+N -> N (new from selection)
            KeyCode::Char(c @ ('t' | 'd' | 'r' | 'p' | 'n')) if ctrl => Some(KeyEvent::new(
                KeyCode::Char(c.to_ascii_uppercase()),
                KeyModifiers::NONE,
            )),
            // Ctrl+G -> g (toggle group by)
            KeyCode::Char('g') if ctrl => {
                Some(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE))
            }
            // Ctrl+O stays as-is (cycle sort backward, already handled by its own arm)
            KeyCode::Char('o') if ctrl => Some(key),
            // Shifted action letters: map to lowercase equivalents
            // N->n (new), X->x (stop), S->s (settings), M->m (message),
            // T->t (toggle view), C->c (container toggle), Q->q (quit), O->o (sort)
            KeyCode::Char(c @ ('N' | 'X' | 'S' | 'M' | 'T' | 'C' | 'Q' | 'O'))
                if bare || shift_only =>
            {
                Some(KeyEvent::new(
                    KeyCode::Char(c.to_ascii_lowercase()),
                    KeyModifiers::NONE,
                ))
            }
            // D -> d (delete) and R -> r (rename) in strict mode
            // (the original uppercase D=diff and R=serve are now behind Ctrl)
            KeyCode::Char(c @ ('D' | 'R')) if bare || shift_only => Some(KeyEvent::new(
                KeyCode::Char(c.to_ascii_lowercase()),
                KeyModifiers::NONE,
            )),
            // Block bare lowercase action letters that would fire without a modifier
            KeyCode::Char('q' | 'n' | 't' | 'c' | 's' | 'd' | 'x' | 'r' | 'm' | 'o' | 'g')
                if bare =>
            {
                None
            }
            // Everything else passes through unchanged (navigation, ?, /, Enter, etc.)
            _ => Some(key),
        }
    }
}
