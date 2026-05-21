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
    builtin_commands, CommandPaletteDialog, ConfirmDialog, DeleteDialogConfig, DialogResult,
    GroupDeleteOptionsDialog, HookTrustAction, HooksInstallDialog, InfoDialog, NewSessionData,
    NewSessionDialog, NoAgentsAction, PaletteAction, PaletteCommand, PaletteGroup,
    ProfilePickerAction, ProjectsDialog, RenameDialog, RenameMode, SendMessageDialog,
    UnifiedDeleteDialog,
};
use crate::tui::diff::{DiffAction, DiffView};
use crate::tui::settings::{SettingsAction, SettingsView};

fn resolve_hook_install_agent(
    tool_name: &str,
    session_config: &crate::session::config::SessionConfig,
) -> Option<&'static crate::agents::AgentDef> {
    crate::agents::get_agent(tool_name)
        .or_else(|| {
            session_config
                .agent_detect_as
                .get(tool_name)
                .and_then(|detect_as| crate::agents::get_agent(detect_as))
        })
        .filter(|agent| agent.hook_config.is_some())
}

pub(super) fn parse_hotkey(s: &str) -> Option<(KeyCode, KeyModifiers)> {
    let (modifier, key) = s.split_once('+')?;
    if !modifier.eq_ignore_ascii_case("alt") {
        return None;
    }
    let mut chars = key.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    Some((KeyCode::Char(ch.to_ascii_lowercase()), KeyModifiers::ALT))
}

/// Validate tool hotkey strings, returning a list of human-readable warning
/// lines for any that fail to parse. Tool hotkeys must match the
/// `Alt+<single-char>` shape; everything else is rejected so the user gets
/// a clear error rather than a silently dead binding.
pub(super) fn validate_tool_hotkeys(
    tools: &std::collections::HashMap<String, crate::session::config::ToolSessionConfig>,
) -> Vec<String> {
    let mut warnings = Vec::new();
    for (name, config) in tools {
        if let Some(ref hotkey) = config.hotkey {
            if parse_hotkey(hotkey).is_none() {
                let msg = format!(
                    "Tool '{}': invalid hotkey '{}' (expected format: Alt+<letter>)",
                    name, hotkey
                );
                tracing::warn!("{}", msg);
                warnings.push(msg);
            }
        }
    }
    warnings
}

/// Build a sorted lookup list of `(tool_name, KeyCode, KeyModifiers)` for
/// every tool whose `hotkey` parses. Sorted by tool name so the
/// alphabetically-first tool wins on duplicate hotkeys. Built once at
/// startup and on settings reload, then iterated on every keystroke;
/// keep it cheap.
pub(super) fn build_tool_hotkey_cache(
    tools: &std::collections::HashMap<String, crate::session::config::ToolSessionConfig>,
) -> Vec<(String, KeyCode, KeyModifiers)> {
    let mut sorted: Vec<_> = tools.iter().collect();
    sorted.sort_by_key(|(name, _)| name.to_owned());
    sorted
        .into_iter()
        .filter_map(|(name, config)| {
            let hotkey_str = config.hotkey.as_deref()?;
            let (code, modifiers) = parse_hotkey(hotkey_str)?;
            Some((name.clone(), code, modifiers))
        })
        .collect()
}

impl HomeView {
    pub fn is_diff_open(&self) -> bool {
        self.diff_view.is_some()
    }

    pub fn hit_preview(&self, col: u16, row: u16) -> bool {
        self.preview_area.contains(Position::from((col, row)))
    }

    pub fn hit_diff(&self, col: u16, row: u16) -> bool {
        self.diff_area.contains(Position::from((col, row)))
    }

    fn open_tool_picker(&mut self) {
        self.tool_picker_dialog = Some(crate::tui::dialogs::ToolPickerDialog::new(
            &self.tool_configs,
        ));
    }

    /// Check if the key event matches any configured tool session hotkey.
    /// On duplicate hotkeys, the alphabetically-first tool name wins
    /// (the cache is built sorted by tool name).
    fn match_tool_hotkey(&self, key: &KeyEvent) -> Option<String> {
        for (name, code, modifiers) in &self.tool_hotkey_cache {
            if key.code == *code && key.modifiers == *modifiers {
                return Some(name.clone());
            }
        }
        None
    }

    pub fn hit_list(&self, col: u16, row: u16) -> bool {
        self.list_area.contains(Position::from((col, row)))
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
                            "default".to_string()
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
                        "default".to_string()
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
            let action = diff_view.handle_key(key);
            if let Some((session_id, new_override)) = diff_view.take_pending_override() {
                self.mutate_instance(&session_id, |inst| {
                    inst.base_branch_override = new_override;
                });
            }
            match action {
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

        // Command palette captures input ahead of the help overlay so its own
        // Esc/Enter/text keys reach it without going through the action match.
        if let Some(palette) = &mut self.command_palette {
            match palette.handle_key(key) {
                DialogResult::Continue => return None,
                DialogResult::Cancel => {
                    self.command_palette = None;
                    return None;
                }
                DialogResult::Submit(action) => {
                    self.command_palette = None;
                    return self.dispatch_palette_action(action, update_info);
                }
            }
        }

        // Handle tool picker dialog
        if let Some(picker) = &mut self.tool_picker_dialog {
            match picker.handle_key(key) {
                DialogResult::Continue => return None,
                DialogResult::Cancel => {
                    self.tool_picker_dialog = None;
                    return None;
                }
                DialogResult::Submit(tool_name) => {
                    self.tool_picker_dialog = None;
                    self.view_mode = ViewMode::Tool(tool_name);
                    self.preview_scroll_offset = 0;
                    self.tool_preview_cache = super::PreviewCache::default();
                    return None;
                }
            }
        }

        if let Some(dialog) = &mut self.snooze_duration_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    self.snooze_duration_dialog = None;
                    self.pending_snooze_session = None;
                }
                DialogResult::Submit(minutes) => {
                    self.snooze_duration_dialog = None;
                    let sid = self.pending_snooze_session.take();
                    if let Some(id) = sid {
                        if let Err(e) = self.snooze_session_for(&id, minutes) {
                            tracing::error!("snooze_session_for failed: {}", e);
                        }
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
                        if let Err(e) = crate::session::config::save_config(&config) {
                            tracing::warn!(target: "tui.input", "Failed to save config: {e}");
                        }
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
                                    tracing::error!(target: "tui.input", "Failed to trust repo: {}", e);
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

                    let resolved_config = resolve_config_or_warn(&data.profile);
                    if let Some(hook_agent) =
                        resolve_hook_install_agent(&tool_name, &resolved_config.session)
                    {
                        let config = crate::session::config::load_config().ok().flatten();
                        let hooks_enabled = resolved_config.session.agent_status_hooks;
                        let acknowledged = config
                            .as_ref()
                            .map(|c| c.app_state.has_acknowledged_agent_hooks)
                            .unwrap_or(false);

                        if hooks_enabled && !acknowledged {
                            self.hooks_install_dialog = Some(HooksInstallDialog::new_for_profile(
                                hook_agent.name,
                                Some(&data.profile),
                            ));
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
                            tracing::error!(target: "tui.input", "Failed to delete group: {}", e);
                        }
                    } else if action == "stop_session" {
                        if let Some(session_id) = self.pending_stop_session.take() {
                            return Some(Action::StopSession(session_id));
                        }
                    } else if action == "force_remove_session" {
                        if let Some(session_id) = self.pending_force_remove_session.take() {
                            if let Err(e) = self.force_remove_session(&session_id) {
                                tracing::error!(target: "tui.input", "Failed to force remove session: {}", e);
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
                        tracing::error!(target: "tui.input", "Failed to delete session: {}", e);
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
                            tracing::error!(target: "tui.input", "Failed to delete group with sessions: {}", e);
                        }
                    } else if let Err(e) = self.delete_selected_group() {
                        tracing::error!(target: "tui.input", "Failed to delete group: {}", e);
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
                                tracing::error!(target: "tui.input", "Failed to rename session: {}", e);
                            }
                        }
                        RenameMode::Group => {
                            if let Err(e) = self.rename_selected_group(
                                data.group.as_deref(),
                                data.profile.as_deref(),
                            ) {
                                tracing::error!(target: "tui.input", "Failed to rename group: {}", e);
                            }
                        }
                    }
                }
            }
            return None;
        }

        if let Some(dialog) = &mut self.projects_dialog {
            match dialog.handle_key(key) {
                DialogResult::Continue => {}
                DialogResult::Cancel | DialogResult::Submit(()) => {
                    self.projects_dialog = None;
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
                            tracing::error!(target: "tui.input", "Failed to switch profile: {}", e);
                        }
                    }
                    ProfilePickerAction::Created(name) => {
                        self.profile_picker_dialog = None;
                        match crate::session::create_profile(&name) {
                            Ok(()) => {
                                if let Err(e) = self.switch_profile(Some(name)) {
                                    tracing::error!(target: "tui.input", "Failed to switch to new profile: {}", e);
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
                        // Defer the actual work to execute_action so the app
                        // loop can render a status indicator first. The send
                        // path may need to start a Docker container or wait
                        // for an agent splash to settle (up to several seconds
                        // total); doing it inline here would freeze the TUI
                        // with no feedback.
                        return Some(Action::SendMessage(session_id, message));
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

        // Search mode. Intentionally takes priority over the Ctrl+K palette
        // binding below: while the search input is focused, every key (including
        // Ctrl+K) feeds the search box. Users can press Esc to exit search and
        // then open the palette. Don't move this block past the Ctrl+K check
        // unless you want palette activation to clobber search input.
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

        // Ctrl+K opens the command palette regardless of strict-hotkey mode.
        // Activated here (before strict normalization) so the binding stays
        // discoverable on every keymap.
        if matches!(key.code, KeyCode::Char('k') | KeyCode::Char('K'))
            && key.modifiers.contains(KeyModifiers::CONTROL)
        {
            self.open_command_palette();
            return None;
        }

        // In strict_hotkeys mode, normalize shifted/ctrl keys to their standard
        // equivalents so the match block below doesn't need duplication.
        //
        // Mapping (strict mode only):
        //   Shift+letter actions -> pass through unchanged: each has its own
        //     `Char('UPPER') if self.strict_hotkeys` arm in the main match.
        //   Ctrl+letter relocated bindings -> uppercase: Ctrl+T->T, Ctrl+D->D, Ctrl+R->R, Ctrl+P->P, Ctrl+N->N
        //   Ctrl+G -> g (group toggle was lowercase)
        //   Bare lowercase action letters -> blocked (return None)
        let key = if self.strict_hotkeys {
            self.normalize_strict_key(key)
        } else {
            Some(key)
        };
        let key = key?;

        self.dispatch_action_key(key, update_info)
    }

    /// Run the main action dispatch (the giant match block) on a key.
    /// Extracted from `handle_key` so the command palette can synthesize
    /// keys and run them through the same code path without re-entering
    /// dialog routing or strict-mode normalization.
    fn dispatch_action_key(
        &mut self,
        key: KeyEvent,
        update_info: Option<&crate::update::UpdateInfo>,
    ) -> Option<Action> {
        // Dynamic tool session hotkeys (checked before static match block)
        if let Some(tool_name) = self.match_tool_hotkey(&key) {
            if matches!(&self.view_mode, ViewMode::Tool(current) if current == &tool_name) {
                self.view_mode = ViewMode::Agent;
            } else {
                self.view_mode = ViewMode::Tool(tool_name);
                self.preview_scroll_offset = 0;
                self.tool_preview_cache = super::PreviewCache::default();
            }
            return None;
        }

        // Normal mode keybindings
        match key.code {
            KeyCode::Esc if !self.search_matches.is_empty() => {
                self.search_matches.clear();
                self.search_match_index = 0;
                self.search_query = Input::default();
            }
            KeyCode::Esc if matches!(self.view_mode, ViewMode::Tool(_)) => {
                self.view_mode = ViewMode::Agent;
            }
            KeyCode::Char('q') => return Some(Action::Quit),
            // `w` / `W`: toggle snooze on the cursor's session. Snooze is
            // "temporary archive": the row sinks to tier 99 for `config.
            // session.snooze_duration_minutes` (default 30), renders
            // italic+dim with a `z ` prefix and remaining-time in the age
            // column, then rejoins the active Attention sort when the
            // timer elapses (lazy; `is_snoozed()` just compares against
            // now). Pressing w/W on a snoozed row wakes it immediately.
            // Mnemonic: Wait. Separate namespace from archive (`z`/`Z`)
            // and favorite (`f`/`F`). Session-only for v1.
            KeyCode::Char('w') if !self.strict_hotkeys => {
                if let Err(e) = self.toggle_snooze_at_cursor() {
                    tracing::error!("toggle_snooze_at_cursor failed: {}", e);
                }
            }
            KeyCode::Char('W') if self.strict_hotkeys => {
                if let Err(e) = self.toggle_snooze_at_cursor() {
                    tracing::error!("toggle_snooze_at_cursor failed: {}", e);
                }
            }
            // `h` / `H`: alias for `w` / `W` (snooze). Mnemonic: Hide,
            // borrowed from email-app conventions where H snoozes the
            // focused message off the list for a while. Plain `h` was
            // previously a vim-style left/collapse alias, but ← already
            // covers that, and users with email-app muscle memory keep
            // reaching for H expecting snooze. `w`/`W` stays functional
            // for backward compat; `h`/`H` is the advertised binding.
            KeyCode::Char('h') if !self.strict_hotkeys => {
                if let Err(e) = self.toggle_snooze_at_cursor() {
                    tracing::error!("toggle_snooze_at_cursor failed: {}", e);
                }
            }
            KeyCode::Char('H') if self.strict_hotkeys => {
                if let Err(e) = self.toggle_snooze_at_cursor() {
                    tracing::error!("toggle_snooze_at_cursor failed: {}", e);
                }
            }
            // `f` / `F`: toggle favorite on the cursor's session. Within
            // the Attention sort, favorited rows pin above non-favorited
            // peers in the same status tier; a favorited Running stays in
            // the Running bucket but bubbles above plain Running rows.
            // Render layer (`render.rs`) adds bold + underline and a
            // leading `* ` glyph. Favorite survives an unsnooze (positive
            // care-more signal) but archive clears it (mutex in
            // `Instance::archive()`).
            KeyCode::Char('f') if !self.strict_hotkeys => match self.toggle_favorite_at_cursor() {
                Ok(Some(msg)) => return Some(Action::SetTransientStatus(msg)),
                Ok(None) => {}
                Err(e) => tracing::error!("toggle_favorite_at_cursor failed: {}", e),
            },
            KeyCode::Char('F') if self.strict_hotkeys => match self.toggle_favorite_at_cursor() {
                Ok(Some(msg)) => return Some(Action::SetTransientStatus(msg)),
                Ok(None) => {}
                Err(e) => tracing::error!("toggle_favorite_at_cursor failed: {}", e),
            },
            KeyCode::Char('?') => {
                self.show_help = true;
            }
            KeyCode::Char('P') => {
                self.show_profile_picker();
            }
            KeyCode::Char('p') if !self.strict_hotkeys => {
                let profile = self.active_profile.as_deref().unwrap_or("default");
                self.projects_dialog = Some(ProjectsDialog::new(profile));
            }
            KeyCode::Char('p')
                if self.strict_hotkeys && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.show_profile_picker();
            }
            #[cfg(feature = "serve")]
            KeyCode::Char('R') if !self.strict_hotkeys => {
                self.serve_view = Some(crate::tui::dialogs::ServeView::new());
            }
            #[cfg(feature = "serve")]
            KeyCode::Char('r')
                if self.strict_hotkeys && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.serve_view = Some(crate::tui::dialogs::ServeView::new());
            }
            #[cfg(not(feature = "serve"))]
            KeyCode::Char('R') if !self.strict_hotkeys => {
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
            #[cfg(not(feature = "serve"))]
            KeyCode::Char('r')
                if self.strict_hotkeys && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
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
            KeyCode::Char('t') if !self.strict_hotkeys => {
                self.view_mode = match self.view_mode {
                    ViewMode::Agent => ViewMode::Terminal,
                    ViewMode::Terminal | ViewMode::Tool(_) => ViewMode::Agent,
                };
            }
            KeyCode::Char('T') if self.strict_hotkeys => {
                self.view_mode = match self.view_mode {
                    ViewMode::Agent => ViewMode::Terminal,
                    ViewMode::Terminal | ViewMode::Tool(_) => ViewMode::Agent,
                };
            }
            KeyCode::Char('T') if !self.strict_hotkeys => {
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
            KeyCode::Char('t')
                if self.strict_hotkeys && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
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
            KeyCode::Char('c') if !self.strict_hotkeys && self.view_mode == ViewMode::Terminal => {
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
            KeyCode::Char('C') if self.strict_hotkeys && self.view_mode == ViewMode::Terminal => {
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
            KeyCode::Char(';') => {
                if matches!(self.view_mode, ViewMode::Tool(_)) {
                    self.view_mode = ViewMode::Agent;
                } else if !self.tool_configs.is_empty() {
                    self.open_tool_picker();
                }
            }
            KeyCode::Char('/') => {
                self.search_active = true;
                self.search_query = Input::default();
            }
            KeyCode::Char('n') if !self.search_matches.is_empty() => {
                self.search_match_index = (self.search_match_index + 1) % self.search_matches.len();
                self.cursor = self.search_matches[self.search_match_index];
                self.update_selected();
            }
            KeyCode::Char('n') if !self.strict_hotkeys => {
                if self.creating_stub_id.is_some() {
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
            KeyCode::Char('N') if self.strict_hotkeys && self.search_matches.is_empty() => {
                if self.creating_stub_id.is_some() {
                    self.info_dialog = Some(InfoDialog::new(
                        "Please Wait",
                        "A session is already being created. Wait for it to finish or press Ctrl+C to cancel.",
                    ));
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
            KeyCode::Char('N') if !self.search_matches.is_empty() => {
                self.search_match_index = if self.search_match_index == 0 {
                    self.search_matches.len() - 1
                } else {
                    self.search_match_index - 1
                };
                self.cursor = self.search_matches[self.search_match_index];
                self.update_selected();
            }
            KeyCode::Char('N') if !self.strict_hotkeys => {
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
            KeyCode::Char('n')
                if self.strict_hotkeys && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                // Strict mode: Ctrl+N = prefill-new (legacy Shift+N relocation)
                if self.creating_stub_id.is_some() {
                    self.info_dialog = Some(InfoDialog::new(
                        "Please Wait",
                        "A session is already being created. Wait for it to finish or press Ctrl+C to cancel.",
                    ));
                } else {
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
            KeyCode::Char('s') if !self.strict_hotkeys => {
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
            KeyCode::Char('S') if self.strict_hotkeys => {
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
                        tracing::error!(target: "tui.input", "Failed to open settings: {}", e);
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
                                tracing::warn!(target: "tui.input", "update detection failed: {e}");
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
            KeyCode::Char('D') if !self.strict_hotkeys => {
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
                let session_id_owned = inst.id.clone();
                let profile = inst.source_profile.clone();
                let base_override = inst.base_branch_override.clone();
                match DiffView::new_for_session(
                    repo_path,
                    Some(session_id_owned),
                    profile,
                    base_override,
                ) {
                    Ok(view) => self.diff_view = Some(view),
                    Err(e) => {
                        tracing::error!(target: "tui.input", "Failed to open diff view: {}", e);
                        self.info_dialog = Some(InfoDialog::new(
                            "Error",
                            &format!("Failed to open diff view: {}", e),
                        ));
                    }
                }
            }
            KeyCode::Char('d')
                if self.strict_hotkeys && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                // Strict mode: Ctrl+D = diff (legacy Shift+D relocation)
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
            KeyCode::Char('x') if !self.strict_hotkeys => {
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
            KeyCode::Char('X') if self.strict_hotkeys => {
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
            KeyCode::Char('d') if !self.strict_hotkeys => {
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
            KeyCode::Char('D') if self.strict_hotkeys => {
                // Strict mode: Shift+D = delete (was lowercase 'd' action)
                if self.view_mode == ViewMode::Terminal {
                    self.info_dialog = Some(InfoDialog::new(
                        "Cannot Delete Terminal",
                        "Terminals cannot be deleted directly. Switch to Agent View (press Shift+T) and delete the agent session instead.",
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
                            "Project groups are automatic. Press Shift+G to switch to manual grouping to manage groups.",
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
            KeyCode::Char('r') if !self.strict_hotkeys => {
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
            KeyCode::Char('R') if self.strict_hotkeys => {
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
                            "Project groups are automatic. Press Shift+G to switch to manual grouping to manage groups.",
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
            KeyCode::Char('m') if !self.strict_hotkeys => {
                self.open_send_message_dialog();
            }
            KeyCode::Char('M') if self.strict_hotkeys => {
                self.open_send_message_dialog();
            }
            KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.apply_sort_order(self.sort_order.cycle_reverse());
            }
            // Plain lowercase 'o' cycles sort only OUTSIDE strict mode. In strict
            // mode, bare 'o' falls through to the typing-guard catch-all (compose
            // dialog), per the no-destructive-lowercase contract.
            KeyCode::Char('o') if !self.strict_hotkeys => {
                self.apply_sort_order(self.sort_order.cycle());
            }
            // Shift+O in strict mode arrives here as Char('O') (normalize_strict_key
            // no longer lowercases 'O') so it's the one key that cycles sort in
            // strict mode. Also matches Shift+O in non-strict mode.
            KeyCode::Char('O') => {
                self.apply_sort_order(self.sort_order.cycle());
            }
            // ±10 navigation: Shift+Up/Down, PageUp/PageDown, OR { / }.
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
            KeyCode::Char('g')
                if self.strict_hotkeys && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                self.apply_group_by(self.group_by.cycle());
            }
            KeyCode::Char('g') if !self.strict_hotkeys => {
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
                        if inst.is_cockpit_mode() {
                            #[cfg(feature = "serve")]
                            {
                                return Some(Action::OpenCockpit(id.clone()));
                            }
                            #[cfg(not(feature = "serve"))]
                            {
                                return Some(Action::SetTransientStatus(
                                    "Cockpit session: rebuild with --features serve to attach"
                                        .to_string(),
                                ));
                            }
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
                        ViewMode::Tool(ref tool_name) => {
                            Some(Action::AttachToolSession(id.clone(), tool_name.clone()))
                        }
                    };
                } else if let Some(Item::Group { path, .. }) = self.flat_items.get(self.cursor) {
                    let path = path.clone();
                    self.toggle_group_collapsed(&path);
                }
            }
            // `<` shrinks the list pane width; `>` grows it. Capital
            // H/L used to be aliases here but H is now the advertised
            // snooze key (mnemonic: Hide), so width controls live on
            // the angle-bracket characters only.
            KeyCode::Char('<') => {
                self.shrink_list();
            }
            KeyCode::Char('>') => {
                self.grow_list();
            }
            KeyCode::Left => {
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
            // Upstream PR #796 added `w` for jump-to-next-waiting after the
            // snooze feature (a19337b) had already taken `w`/`W`. In non-strict
            // mode the snooze arm at line 707 catches first, so this jump arm
            // was always dead. In strict mode it leaked through and preempted
            // the typing-guard below; bare `w` jumped the cursor instead of
            // opening compose like every other lowercase letter. Gate it.
            KeyCode::Char('w') if !self.strict_hotkeys => {
                self.jump_to_next_waiting();
            }
            // Strict-mode typing guard: any bare lowercase letter that isn't a
            // navigation key (j/k/h/l) is treated as inadvertent typing; open
            // the compose dialog pre-filled with that character instead of
            // firing an action or swallowing the keypress.
            KeyCode::Char(c)
                if self.strict_hotkeys
                    && key.modifiers == KeyModifiers::NONE
                    && c.is_ascii_lowercase() =>
            {
                self.capture_letter_to_compose(c);
            }
            _ => {}
        }

        None
    }

    /// Build and show the command palette. Combines the static `builtin_commands`
    /// with dynamic jump-to-session and jump-to-group entries built from the
    /// current `flat_items`.
    fn open_command_palette(&mut self) {
        let serve_enabled = cfg!(feature = "serve");
        let mut entries: Vec<PaletteCommand> = builtin_commands(serve_enabled, self.strict_hotkeys);

        // Quit command (separate so the lifetime mapping is clear and we
        // can keep it out of `builtin_commands` to avoid pulling KeyCode
        // imports into the palette module).
        let quit_hotkey = if self.strict_hotkeys { "Q" } else { "q" };
        entries.push(PaletteCommand {
            id: "quit",
            title: "Quit Agent of Empires".to_string(),
            group: PaletteGroup::Settings,
            keywords: vec!["exit", "close"],
            hotkey: quit_hotkey,
            payload: PaletteAction::Key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
        });

        // Dynamic session/group entries: one per flat_items row, so the user
        // can fuzzy-search and jump straight to it. We tag in-flight sessions
        // (Creating / Deleting) in the title so the user knows that picking
        // Stop/Delete from the palette will be a no-op for those rows.
        for (idx, item) in self.flat_items.iter().enumerate() {
            match item {
                Item::Session { id, .. } => {
                    let Some(inst) = self.get_instance(id) else {
                        continue;
                    };
                    let status_tag = match inst.status {
                        Status::Creating => " [creating]",
                        Status::Deleting => " [deleting]",
                        Status::Stopped => " [stopped]",
                        _ => "",
                    };
                    let title = if inst.group_path.is_empty() {
                        format!("Jump to session: {}{}", inst.title, status_tag)
                    } else {
                        format!(
                            "Jump to session: {} ({}){}",
                            inst.title, inst.group_path, status_tag
                        )
                    };
                    entries.push(PaletteCommand {
                        id: "jump-session",
                        title,
                        group: PaletteGroup::Sessions,
                        keywords: vec!["session", "jump", "select"],
                        hotkey: "",
                        payload: PaletteAction::JumpToCursor(idx),
                    });
                }
                Item::Group { name, path, .. } => {
                    let label = if name == path {
                        format!("Jump to group: {}", name)
                    } else {
                        format!("Jump to group: {} ({})", name, path)
                    };
                    entries.push(PaletteCommand {
                        id: "jump-group",
                        title: label,
                        group: PaletteGroup::Groups,
                        keywords: vec!["group", "jump"],
                        hotkey: "",
                        payload: PaletteAction::JumpToCursor(idx),
                    });
                }
            }
        }

        // Tool session entries, sorted by name for stable palette ordering
        // (matches the tool picker dialog's alphabetical order).
        let mut tools_sorted: Vec<_> = self.tool_configs.iter().collect();
        tools_sorted.sort_by_key(|(name, _)| name.to_owned());
        for (name, config) in tools_sorted {
            let hotkey_label = config
                .hotkey
                .as_deref()
                .map(|h| format!(" [{}]", h))
                .unwrap_or_default();
            entries.push(PaletteCommand {
                id: "tool-session",
                title: format!("Open tool: {}{}", name, hotkey_label),
                group: PaletteGroup::Actions,
                keywords: vec!["tool", "session"],
                hotkey: "",
                payload: PaletteAction::ToolSession(name.clone()),
            });
        }

        self.command_palette = Some(CommandPaletteDialog::new(entries));
    }

    /// Apply a palette pick. `Key` re-enters the action dispatch with the
    /// synthesized event (bypassing strict normalization, which the palette
    /// already accounts for); `JumpToCursor` moves the selection.
    fn dispatch_palette_action(
        &mut self,
        action: PaletteAction,
        update_info: Option<&crate::update::UpdateInfo>,
    ) -> Option<Action> {
        match action {
            PaletteAction::Key(synth) => {
                // Clear leftover search-cycle state before dispatching. Some
                // action keys (`n`, `N`) are dual-purpose: they cycle search
                // matches when matches are active, otherwise open new-session
                // dialogs. The palette's mental model is "run the named
                // action," so we drop search state here to make sure a pick
                // of "New session" never silently turns into a search-cycle.
                if !self.search_matches.is_empty() {
                    self.search_matches.clear();
                    self.search_match_index = 0;
                }
                self.dispatch_action_key(synth, update_info)
            }
            PaletteAction::JumpToCursor(idx) => {
                if !self.flat_items.is_empty() {
                    self.cursor = idx.min(self.flat_items.len() - 1);
                    self.update_selected();
                }
                None
            }
            PaletteAction::ToolSession(tool_name) => {
                self.view_mode = ViewMode::Tool(tool_name);
                self.preview_scroll_offset = 0;
                self.tool_preview_cache = super::PreviewCache::default();
                None
            }
        }
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
                tracing::warn!(target: "tui.input", "Failed to save sort order: {}", e);
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
                    tracing::warn!(target: "tui.input", "Failed to save group_by mode: {}", e);
                }
            }
            Err(e) => {
                tracing::warn!(target: "tui.input", "Failed to load config for group_by save: {}", e);
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
            tracing::error!(target: "tui.input", "Failed to save group state: {}", e);
        }
    }

    /// Route a mouse-wheel-up at (col, row) to the pane under the cursor:
    /// diff view (if open) → diff scroll; list pane → list cursor up;
    /// preview pane → preview scroll. Returns `true` if the UI should
    /// redraw. Position-aware so iOS-Mosh touch-scroll moves the LIST
    /// when the user is touching the list pane (regardless of whether
    /// a session is currently selected).
    pub fn handle_scroll_up(&mut self, col: u16, row: u16) -> bool {
        const STEP: u16 = 3;
        if let Some(ref mut diff) = self.diff_view {
            diff.scroll_up(STEP);
            return true;
        }
        if self.has_dialog() {
            return false;
        }
        if self.hit_list(col, row) {
            self.move_cursor(-1);
            return true;
        }
        if !self.hit_preview(col, row) {
            return false;
        }
        // Wheel over preview with no session selected: fall through to list nav.
        if self.selected_session.is_none() {
            self.move_cursor(-1);
            return true;
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
            ViewMode::Tool(_) => &self.tool_preview_cache,
        };

        let visible_height = active_cache.dimensions.1.saturating_sub(1) as usize;
        let real_max = active_cache.captured_lines.saturating_sub(visible_height) as u16;

        let new_offset = self.preview_scroll_offset.saturating_add(STEP);
        let clamped = new_offset.min(real_max);
        if clamped == self.preview_scroll_offset {
            // Preview already at top; fall through to list nav so the wheel isn't a no-op.
            self.move_cursor(-1);
            return true;
        }
        self.preview_scroll_offset = clamped;
        true
    }

    /// Route a mouse-wheel-down at (col, row); see handle_scroll_up.
    pub fn handle_scroll_down(&mut self, col: u16, row: u16) -> bool {
        const STEP: u16 = 3;
        if let Some(ref mut diff) = self.diff_view {
            diff.scroll_down(STEP);
            return true;
        }
        if self.has_dialog() {
            return false;
        }
        if self.hit_list(col, row) {
            self.move_cursor(1);
            return true;
        }
        if !self.hit_preview(col, row) {
            return false;
        }
        // Wheel over preview with no session selected: fall through to list nav.
        if self.selected_session.is_none() {
            self.move_cursor(1);
            return true;
        }
        if self.preview_scroll_offset == 0 {
            // Preview already at bottom; fall through to list nav so the wheel isn't a no-op.
            self.move_cursor(1);
            return true;
        }
        self.preview_scroll_offset = self.preview_scroll_offset.saturating_sub(STEP);
        true
    }

    /// Route a bracketed paste event to the active text input dialog.
    ///
    /// Active text-input dialogs (rename / send_message / new) win first so
    /// multi-line voice/dictation lands in the dialog the user is actively
    /// typing into. The settings view is checked last; its paste handler
    /// strips newlines (settings/input.rs handle_paste sanitizes), which
    /// would destroy multi-line dictation if we checked it first.
    pub fn handle_paste(&mut self, text: &str) {
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
            return;
        }
        if let Some(ref mut settings) = self.settings_view {
            settings.handle_paste(text);
            return;
        }

        // No dialog open: route the paste into a new compose dialog if the
        // selected session is runnable. If not, stash in pending_paste so the
        // next dialog open (typically the next `m` press) drains it. Never
        // throw voice text on the floor; losing dictation is worse than
        // silently catching it.
        if let Some((id, title)) = self.resolve_paste_target() {
            self.pending_send_session = Some(id);
            let mut dialog = SendMessageDialog::new(&title);
            dialog.handle_paste(text);
            self.send_message_dialog = Some(dialog);
            return;
        }

        // No running sessions at all (or all Creating). Stash for later;
        // the user will see the text on next 'm' / dialog open.
        match self.pending_paste.as_mut() {
            Some(buf) => buf.push_str(text),
            None => self.pending_paste = Some(text.to_string()),
        }
    }

    /// Open the send-message dialog for the currently-selected running session.
    /// If pending_paste has accumulated text from earlier untargeted pastes,
    /// drain it into the dialog so voice/dictation captured before a session
    /// was picked still gets used. No-op if no running session is targetable.
    fn open_send_message_dialog(&mut self) {
        let Some((id, title)) = self.resolve_paste_target() else {
            return;
        };
        self.pending_send_session = Some(id);
        let mut dialog = SendMessageDialog::new(&title);
        if let Some(buf) = self.pending_paste.take() {
            if !buf.is_empty() {
                dialog.handle_paste(&buf);
            }
        }
        self.send_message_dialog = Some(dialog);
    }

    /// Resolve a target session id + title for an untargeted paste/type-burst.
    /// Only returns Some when an explicit, runnable session is selected.
    ///
    /// Cases that return None (caller stashes to `pending_paste`):
    /// - Cursor on a group header (`selected_session` is None).
    /// - No selection at all (empty list, no sessions).
    /// - Selected session is non-running (Stopped, Error, Creating, or tmux
    ///   pane gone).
    ///
    /// Why no first-running fallback: silently dispatching paste/dictation
    /// to "whichever session sorts first" misroutes voice messages across
    /// groups. A user with cursor on the "backend" group expanding it to
    /// browse, dictating, and having the paste land in a "frontend" session
    /// is exactly the misrouting the archived-selection fix is preventing.
    /// Stashing to `pending_paste` is strictly better: the status-bar
    /// indicator surfaces the captured count, and the next `m` against a
    /// runnable selection drains it into the compose dialog.
    ///
    /// Defensive fall-through: when `selected_session` references an id
    /// that no longer maps to an instance (deleted underneath us between
    /// select and paste, shouldn't happen in steady state), we also stash
    /// rather than reroute.
    fn resolve_paste_target(&self) -> Option<(String, String)> {
        let pick = |inst: &crate::session::Instance| -> Option<(String, String)> {
            if inst.status == Status::Creating {
                return None;
            }
            let tmux_session = crate::tmux::Session::new(&inst.id, &inst.title).ok();
            if tmux_session.as_ref().is_some_and(|s| s.exists()) {
                Some((inst.id.clone(), inst.title.clone()))
            } else {
                None
            }
        };

        let id = self.selected_session.as_ref()?;
        let inst = self.get_instance(id)?;
        pick(inst)
    }

    /// Strict-mode typing guard: a bare lowercase letter was pressed outside
    /// navigation (j/k/h/l). Treat it as inadvertent typing; open the compose
    /// dialog for the selected session pre-filled with that character. Mirrors
    /// handle_paste's dialog-delegation + fallback logic.
    fn capture_letter_to_compose(&mut self, c: char) {
        let s = c.to_string();
        if let Some(ref mut dialog) = self.send_message_dialog {
            dialog.handle_paste(&s);
            return;
        }
        if let Some(ref mut dialog) = self.new_dialog {
            dialog.handle_paste(&s);
            return;
        }
        if let Some(ref mut dialog) = self.rename_dialog {
            dialog.handle_paste(&s);
            return;
        }

        if let Some((id, title)) = self.resolve_paste_target() {
            self.pending_send_session = Some(id);
            let mut dialog = SendMessageDialog::new(&title);
            dialog.handle_paste(&s);
            self.send_message_dialog = Some(dialog);
            return;
        }

        match self.pending_paste.as_mut() {
            Some(buf) => buf.push_str(&s),
            None => self.pending_paste = Some(s),
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
                tracing::warn!(target: "tui.input", "Failed to check repo hooks: {}", e);
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
        let has_worktree = data.worktree_enabled;

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
                tracing::error!(target: "tui.input", "Failed to create session: {}", e);
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
            // Shifted action letters pass through unchanged. Each letter has its
            // own `Char('UPPER') if self.strict_hotkeys` arm in the main match.
            // Lowercasing here would route the chord into a dead arm guarded
            // `if !self.strict_hotkeys`, so the action would silently no-op.
            // Affects D (delete), R (rename), N, X, S, M, T, C, Q, O.
            //
            // Side benefit: passing through unchanged also makes the chords work
            // on iOS Mosh, where Shift+letter is delivered as the bare uppercase
            // keycode without a Shift modifier.
            // Bare lowercase letters pass through; the main match falls through
            // to a catch-all that opens the compose dialog pre-filled with the
            // letter (strict-mode typing-guard). Navigation keys j/k/h/l are
            // handled by their own arms before the catch-all fires.
            _ => Some(key),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::config::{SessionConfig, ToolSessionConfig};

    #[test]
    fn hook_install_agent_uses_detect_as_for_custom_codex_wrapper() {
        let mut config = SessionConfig::default();
        config
            .agent_detect_as
            .insert("wrapped-codex".to_string(), "codex".to_string());

        let agent = resolve_hook_install_agent("wrapped-codex", &config).unwrap();

        assert_eq!(agent.name, "codex");
    }

    #[test]
    fn hook_install_agent_keeps_builtin_agent_resolution_first() {
        let mut config = SessionConfig::default();
        config
            .agent_detect_as
            .insert("opencode".to_string(), "codex".to_string());

        assert!(resolve_hook_install_agent("opencode", &config).is_none());
    }

    #[test]
    fn hook_install_agent_ignores_unknown_detect_as_target() {
        let mut config = SessionConfig::default();
        config
            .agent_detect_as
            .insert("wrapped-agent".to_string(), "missing-agent".to_string());

        assert!(resolve_hook_install_agent("wrapped-agent", &config).is_none());
    }

    #[test]
    fn parse_hotkey_accepts_alt_letter() {
        let (code, mods) = parse_hotkey("Alt+g").expect("valid");
        assert_eq!(code, KeyCode::Char('g'));
        assert_eq!(mods, KeyModifiers::ALT);
    }

    #[test]
    fn parse_hotkey_is_case_insensitive_on_modifier() {
        assert!(parse_hotkey("alt+g").is_some());
        assert!(parse_hotkey("ALT+g").is_some());
        assert!(parse_hotkey("aLt+g").is_some());
    }

    #[test]
    fn parse_hotkey_normalizes_letter_to_lowercase() {
        let (code, _) = parse_hotkey("Alt+G").expect("valid");
        assert_eq!(code, KeyCode::Char('g'));
    }

    #[test]
    fn parse_hotkey_accepts_digit() {
        let (code, mods) = parse_hotkey("Alt+1").expect("valid");
        assert_eq!(code, KeyCode::Char('1'));
        assert_eq!(mods, KeyModifiers::ALT);
    }

    #[test]
    fn parse_hotkey_rejects_non_alt_modifier() {
        assert!(parse_hotkey("Ctrl+g").is_none());
        assert!(parse_hotkey("Shift+g").is_none());
        assert!(parse_hotkey("Cmd+g").is_none());
    }

    #[test]
    fn parse_hotkey_rejects_multi_char_key() {
        assert!(parse_hotkey("Alt+gg").is_none());
        assert!(parse_hotkey("Alt+F1").is_none());
    }

    #[test]
    fn parse_hotkey_rejects_missing_modifier() {
        assert!(parse_hotkey("g").is_none());
        assert!(parse_hotkey("Alt").is_none());
        assert!(parse_hotkey("").is_none());
    }

    #[test]
    fn parse_hotkey_rejects_wrong_separator() {
        assert!(parse_hotkey("Alt-g").is_none());
        assert!(parse_hotkey("Alt g").is_none());
    }

    #[test]
    fn validate_tool_hotkeys_reports_each_invalid_entry() {
        let mut tools = std::collections::HashMap::new();
        tools.insert(
            "lazygit".to_string(),
            ToolSessionConfig {
                command: "lazygit".into(),
                hotkey: Some("Alt+g".into()),
            },
        );
        tools.insert(
            "yazi".to_string(),
            ToolSessionConfig {
                command: "yazi".into(),
                hotkey: Some("Ctrl+f".into()),
            },
        );
        tools.insert(
            "tig".to_string(),
            ToolSessionConfig {
                command: "tig".into(),
                hotkey: Some("Alt+too-long".into()),
            },
        );
        let warnings = validate_tool_hotkeys(&tools);
        assert_eq!(warnings.len(), 2);
        let joined = warnings.join("|");
        assert!(joined.contains("yazi"));
        assert!(joined.contains("tig"));
        assert!(!joined.contains("lazygit"));
    }

    #[test]
    fn validate_tool_hotkeys_empty_when_all_valid_or_unset() {
        let mut tools = std::collections::HashMap::new();
        tools.insert(
            "lazygit".to_string(),
            ToolSessionConfig {
                command: "lazygit".into(),
                hotkey: Some("Alt+g".into()),
            },
        );
        tools.insert(
            "rg".to_string(),
            ToolSessionConfig {
                command: "rg --files".into(),
                hotkey: None,
            },
        );
        assert!(validate_tool_hotkeys(&tools).is_empty());
    }

    #[test]
    fn build_tool_hotkey_cache_sorts_by_name_and_skips_invalid() {
        let mut tools = std::collections::HashMap::new();
        tools.insert(
            "zoxide".to_string(),
            ToolSessionConfig {
                command: "z".into(),
                hotkey: Some("Alt+z".into()),
            },
        );
        tools.insert(
            "lazygit".to_string(),
            ToolSessionConfig {
                command: "lazygit".into(),
                hotkey: Some("Alt+g".into()),
            },
        );
        tools.insert(
            "broken".to_string(),
            ToolSessionConfig {
                command: "x".into(),
                hotkey: Some("Ctrl+x".into()),
            },
        );
        tools.insert(
            "no-hotkey".to_string(),
            ToolSessionConfig {
                command: "y".into(),
                hotkey: None,
            },
        );

        let cache = build_tool_hotkey_cache(&tools);
        // Two valid entries, sorted by name.
        assert_eq!(cache.len(), 2);
        assert_eq!(cache[0].0, "lazygit");
        assert_eq!(cache[0].1, KeyCode::Char('g'));
        assert_eq!(cache[0].2, KeyModifiers::ALT);
        assert_eq!(cache[1].0, "zoxide");
        assert_eq!(cache[1].1, KeyCode::Char('z'));
    }

    #[test]
    fn build_tool_hotkey_cache_tie_break_favors_alphabetically_first_name() {
        let mut tools = std::collections::HashMap::new();
        // Both bind Alt+g; alphabetical winner is "alpha".
        tools.insert(
            "beta".to_string(),
            ToolSessionConfig {
                command: "b".into(),
                hotkey: Some("Alt+g".into()),
            },
        );
        tools.insert(
            "alpha".to_string(),
            ToolSessionConfig {
                command: "a".into(),
                hotkey: Some("Alt+g".into()),
            },
        );
        let cache = build_tool_hotkey_cache(&tools);
        assert_eq!(cache[0].0, "alpha");
        assert_eq!(cache[1].0, "beta");
    }
}
