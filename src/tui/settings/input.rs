//! Input handling for the settings view

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

use crate::tui::dialogs::{CustomInstructionDialog, DialogResult};

use super::{FieldKey, FieldValue, ListEditState, SettingsFocus, SettingsScope, SettingsView};

/// Result of handling a key event in the settings view
pub enum SettingsAction {
    /// Continue showing the settings view
    Continue,
    /// Close the settings view (with optional unsaved changes warning)
    Close,
    /// Close was cancelled due to unsaved changes
    UnsavedChangesWarning,
    /// Live-preview a theme change (theme name)
    PreviewTheme(String),
}

impl SettingsView {
    pub fn handle_key(&mut self, key: KeyEvent) -> SettingsAction {
        // Clear transient messages on any key
        self.success_message = None;

        // Handle custom instruction dialog
        if let Some(ref mut dialog) = self.custom_instruction_dialog {
            match dialog.handle_key(key) {
                DialogResult::Submit(value) => {
                    let field = &mut self.fields[self.selected_field];
                    if let FieldValue::OptionalText(ref mut v) = field.value {
                        *v = value;
                    }
                    self.apply_field_to_config(self.selected_field);
                    self.custom_instruction_dialog = None;
                    return SettingsAction::Continue;
                }
                DialogResult::Cancel => {
                    self.custom_instruction_dialog = None;
                    return SettingsAction::Continue;
                }
                DialogResult::Continue => {
                    return SettingsAction::Continue;
                }
            }
        }

        // Handle help overlay
        if self.show_help {
            if matches!(
                key.code,
                KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q')
            ) {
                self.show_help = false;
            }
            return SettingsAction::Continue;
        }

        // Handle text editing mode
        if self.editing_input.is_some() {
            return self.handle_text_edit_key(key);
        }

        // Handle list editing mode
        if self.list_edit_state.is_some() {
            return self.handle_list_edit_key(key);
        }

        // Normal mode
        match (key.code, key.modifiers) {
            // Save
            (KeyCode::Char('s'), KeyModifiers::CONTROL) => {
                if let Err(e) = self.save() {
                    self.error_message = Some(format!("Failed to save: {}", e));
                }
                SettingsAction::Continue
            }

            // Close from anywhere
            (KeyCode::Char('q'), _) => {
                if self.has_changes {
                    SettingsAction::UnsavedChangesWarning
                } else {
                    SettingsAction::Close
                }
            }

            // Escape goes up one level
            (KeyCode::Esc, _) => match self.focus {
                SettingsFocus::Fields => {
                    self.focus = SettingsFocus::Categories;
                    SettingsAction::Continue
                }
                SettingsFocus::Categories => {
                    if self.has_changes {
                        SettingsAction::UnsavedChangesWarning
                    } else {
                        SettingsAction::Close
                    }
                }
            },

            // Switch scope: [ and ] cycle between Global / Profile / Repo
            (KeyCode::Char(']'), _) => {
                if self.has_changes {
                    return SettingsAction::UnsavedChangesWarning;
                }
                self.scope = match self.scope {
                    SettingsScope::Global => SettingsScope::Profile,
                    SettingsScope::Profile => {
                        if self.project_path.is_some() {
                            SettingsScope::Repo
                        } else {
                            SettingsScope::Global
                        }
                    }
                    SettingsScope::Repo => SettingsScope::Global,
                };
                self.rebuild_fields();
                SettingsAction::Continue
            }
            (KeyCode::Char('['), _) => {
                if self.has_changes {
                    return SettingsAction::UnsavedChangesWarning;
                }
                self.scope = match self.scope {
                    SettingsScope::Global => {
                        if self.project_path.is_some() {
                            SettingsScope::Repo
                        } else {
                            SettingsScope::Profile
                        }
                    }
                    SettingsScope::Profile => SettingsScope::Global,
                    SettingsScope::Repo => SettingsScope::Profile,
                };
                self.rebuild_fields();
                SettingsAction::Continue
            }

            // Cycle through profiles when in Profile scope: { and }
            (KeyCode::Char('}'), _) | (KeyCode::Char('{'), _) => {
                if self.scope == SettingsScope::Profile && !self.available_profiles.is_empty() {
                    if self.has_changes {
                        return SettingsAction::UnsavedChangesWarning;
                    }
                    let current_idx = self
                        .available_profiles
                        .iter()
                        .position(|p| p == &self.profile)
                        .unwrap_or(0);
                    let next_idx = if key.code == KeyCode::Char('}') {
                        (current_idx + 1) % self.available_profiles.len()
                    } else if current_idx == 0 {
                        self.available_profiles.len() - 1
                    } else {
                        current_idx - 1
                    };
                    let new_profile = self.available_profiles[next_idx].clone();
                    if let Err(e) = self.switch_profile(&new_profile) {
                        self.error_message = Some(format!("Failed to load profile: {}", e));
                    }
                }
                SettingsAction::Continue
            }

            // Switch focus between categories and fields
            (KeyCode::Tab, _) | (KeyCode::Right, _) | (KeyCode::Char('l'), _) => {
                self.focus = SettingsFocus::Fields;
                SettingsAction::Continue
            }
            (KeyCode::BackTab, _) | (KeyCode::Left, _) | (KeyCode::Char('h'), _) => {
                self.focus = SettingsFocus::Categories;
                SettingsAction::Continue
            }

            // Navigate up/down
            (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                match self.focus {
                    SettingsFocus::Categories => {
                        if self.selected_category > 0 {
                            self.selected_category -= 1;
                            self.rebuild_fields();
                        }
                    }
                    SettingsFocus::Fields => {
                        if self.selected_field > 0 {
                            self.selected_field -= 1;
                            self.ensure_field_visible(self.fields_viewport_height);
                        }
                    }
                }
                SettingsAction::Continue
            }
            (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                match self.focus {
                    SettingsFocus::Categories => {
                        if self.selected_category < self.categories.len().saturating_sub(1) {
                            self.selected_category += 1;
                            self.rebuild_fields();
                        }
                    }
                    SettingsFocus::Fields => {
                        if self.selected_field < self.fields.len().saturating_sub(1) {
                            self.selected_field += 1;
                            self.ensure_field_visible(self.fields_viewport_height);
                        }
                    }
                }
                SettingsAction::Continue
            }

            // Toggle boolean / edit field
            (KeyCode::Char(' '), _) => {
                if self.focus == SettingsFocus::Fields && !self.fields.is_empty() {
                    let field = &mut self.fields[self.selected_field];
                    if let FieldValue::Bool(ref mut value) = field.value {
                        *value = !*value;
                        self.apply_field_to_config(self.selected_field);
                    }
                }
                SettingsAction::Continue
            }

            // Enter - edit field or expand list
            (KeyCode::Enter, _) => {
                if self.focus == SettingsFocus::Fields && !self.fields.is_empty() {
                    let field = &self.fields[self.selected_field];
                    match &field.value {
                        FieldValue::Bool(value) => {
                            let new_value = !value;
                            self.fields[self.selected_field].value = FieldValue::Bool(new_value);
                            self.apply_field_to_config(self.selected_field);
                        }
                        FieldValue::Text(value) => {
                            self.editing_input = Some(Input::new(value.clone()));
                        }
                        FieldValue::OptionalText(value) => {
                            if field.key == FieldKey::CustomInstruction {
                                self.custom_instruction_dialog =
                                    Some(CustomInstructionDialog::new(value.clone()));
                            } else {
                                self.editing_input =
                                    Some(Input::new(value.clone().unwrap_or_default()));
                            }
                        }
                        FieldValue::Number(value) => {
                            self.editing_input = Some(Input::new(value.to_string()));
                        }
                        FieldValue::Select { selected, options } => {
                            let new_selected = (*selected + 1) % options.len();
                            let new_options = options.clone();
                            self.fields[self.selected_field].value = FieldValue::Select {
                                selected: new_selected,
                                options: new_options,
                            };
                            self.apply_field_to_config(self.selected_field);

                            if self.fields[self.selected_field].key == FieldKey::ThemeName {
                                if let FieldValue::Select { selected, options } =
                                    &self.fields[self.selected_field].value
                                {
                                    if let Some(name) = options.get(*selected) {
                                        return SettingsAction::PreviewTheme(name.clone());
                                    }
                                }
                            }
                        }
                        FieldValue::List(_) => {
                            // Expand list for editing
                            self.list_edit_state = Some(ListEditState::default());
                        }
                    }
                } else if self.focus == SettingsFocus::Categories {
                    // Move to fields when pressing Enter on a category
                    self.focus = SettingsFocus::Fields;
                }
                SettingsAction::Continue
            }

            // Toggle help overlay
            (KeyCode::Char('?'), _) => {
                self.show_help = true;
                SettingsAction::Continue
            }

            // Reset field to default (clear profile/repo override)
            (KeyCode::Char('r'), _) => {
                if (self.scope == SettingsScope::Profile || self.scope == SettingsScope::Repo)
                    && self.focus == SettingsFocus::Fields
                    && !self.fields.is_empty()
                {
                    let was_theme = self.fields[self.selected_field].key == FieldKey::ThemeName;
                    self.clear_profile_override(self.selected_field);
                    self.rebuild_fields();

                    if was_theme {
                        if let Some(field) =
                            self.fields.iter().find(|f| f.key == FieldKey::ThemeName)
                        {
                            if let FieldValue::Select { selected, options } = &field.value {
                                if let Some(name) = options.get(*selected) {
                                    return SettingsAction::PreviewTheme(name.clone());
                                }
                            }
                        }
                    }
                }
                SettingsAction::Continue
            }

            _ => SettingsAction::Continue,
        }
    }

    fn handle_text_edit_key(&mut self, key: KeyEvent) -> SettingsAction {
        match key.code {
            KeyCode::Esc => {
                self.editing_input = None;
                self.error_message = None;
            }
            KeyCode::Enter => {
                if let Some(input) = self.editing_input.take() {
                    let text = input.value().to_string();
                    let field = &mut self.fields[self.selected_field];

                    // Apply the new value
                    match &mut field.value {
                        FieldValue::Text(ref mut v) => {
                            *v = text;
                        }
                        FieldValue::OptionalText(ref mut v) => {
                            *v = if text.is_empty() { None } else { Some(text) };
                        }
                        FieldValue::Number(ref mut v) => {
                            if let Ok(n) = text.parse() {
                                *v = n;
                            } else {
                                self.error_message = Some("Invalid number".to_string());
                                self.editing_input = Some(Input::new(text));
                                return SettingsAction::Continue;
                            }
                        }
                        _ => {}
                    }

                    // Validate
                    if let Err(e) = field.validate() {
                        self.error_message = Some(e);
                        // Revert to editing
                        self.editing_input = match &field.value {
                            FieldValue::Text(v) => Some(Input::new(v.clone())),
                            FieldValue::OptionalText(v) => {
                                Some(Input::new(v.clone().unwrap_or_default()))
                            }
                            FieldValue::Number(v) => Some(Input::new(v.to_string())),
                            _ => None,
                        };
                        return SettingsAction::Continue;
                    }

                    self.apply_field_to_config(self.selected_field);
                    self.error_message = None;
                }
            }
            _ => {
                // Delegate all other key events to tui_input
                if let Some(ref mut input) = self.editing_input {
                    input.handle_event(&crossterm::event::Event::Key(key));
                }
            }
        }
        SettingsAction::Continue
    }

    fn handle_list_edit_key(&mut self, key: KeyEvent) -> SettingsAction {
        let state = match self.list_edit_state.as_mut() {
            Some(s) => s,
            None => return SettingsAction::Continue,
        };

        // If we're editing an item or adding new
        if state.editing_item.is_some() {
            return self.handle_list_item_edit_key(key);
        }

        match key.code {
            KeyCode::Esc => {
                self.list_edit_state = None;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if state.selected_index > 0 {
                    state.selected_index -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let FieldValue::List(items) = &self.fields[self.selected_field].value {
                    if state.selected_index < items.len().saturating_sub(1) {
                        state.selected_index += 1;
                    }
                }
            }
            KeyCode::Char('a') => {
                // Add new item
                state.adding_new = true;
                state.editing_item = Some(Input::default());
            }
            KeyCode::Char('d') => {
                // Delete selected item - capture index before borrowing fields
                let selected_idx = state.selected_index;
                let mut new_selected_idx = selected_idx;

                if let FieldValue::List(ref mut items) = self.fields[self.selected_field].value {
                    if !items.is_empty() && selected_idx < items.len() {
                        items.remove(selected_idx);
                        if selected_idx >= items.len() && !items.is_empty() {
                            new_selected_idx = items.len() - 1;
                        }
                    }
                }

                if let Some(ref mut s) = self.list_edit_state {
                    s.selected_index = new_selected_idx;
                }
                self.apply_field_to_config(self.selected_field);
            }
            KeyCode::Enter => {
                // Edit selected item
                if let FieldValue::List(items) = &self.fields[self.selected_field].value {
                    if !items.is_empty() && state.selected_index < items.len() {
                        state.editing_item = Some(Input::new(items[state.selected_index].clone()));
                    }
                }
            }
            _ => {}
        }
        SettingsAction::Continue
    }

    fn handle_list_item_edit_key(&mut self, key: KeyEvent) -> SettingsAction {
        let state = match self.list_edit_state.as_mut() {
            Some(s) => s,
            None => return SettingsAction::Continue,
        };

        match key.code {
            KeyCode::Esc => {
                state.editing_item = None;
                state.adding_new = false;
                self.error_message = None;
            }
            KeyCode::Enter => {
                // Take the input and flags out to avoid borrow conflict
                let input = state.editing_item.take();
                let adding_new = state.adding_new;
                let selected_idx = state.selected_index;
                state.adding_new = false;

                if let Some(input) = input {
                    let text = input.value().to_string();
                    if !text.is_empty() {
                        let field_key = self.fields[self.selected_field].key;

                        // Validate key=value format for agent override fields
                        if matches!(
                            field_key,
                            FieldKey::AgentExtraArgs | FieldKey::AgentCommandOverride
                        ) {
                            if let Err(msg) = validate_agent_key_value(&text) {
                                self.error_message = Some(msg);
                                // Re-open the editor so the user can fix the entry
                                if let Some(ref mut s) = self.list_edit_state {
                                    s.editing_item = Some(tui_input::Input::new(text));
                                    s.adding_new = adding_new;
                                }
                                return SettingsAction::Continue;
                            }
                        }

                        // Validate env var references before accepting
                        if field_key == FieldKey::Environment {
                            self.error_message = crate::session::validate_env_entry(&text);
                        }

                        if let FieldValue::List(ref mut items) =
                            self.fields[self.selected_field].value
                        {
                            if adding_new {
                                items.push(text);
                                if let Some(ref mut s) = self.list_edit_state {
                                    s.selected_index = items.len() - 1;
                                }
                            } else if selected_idx < items.len() {
                                items[selected_idx] = text;
                            }
                        }
                        self.apply_field_to_config(self.selected_field);
                        // Clear stale errors, but preserve env validation warnings set above
                        if field_key != FieldKey::Environment {
                            self.error_message = None;
                        }
                    }
                }
            }
            _ => {
                // Delegate all other key events to tui_input
                if let Some(ref mut input) = state.editing_item {
                    input.handle_event(&crossterm::event::Event::Key(key));
                }
            }
        }
        SettingsAction::Continue
    }

    fn clear_profile_override(&mut self, field_index: usize) {
        if field_index >= self.fields.len() {
            return;
        }

        let key = self.fields[field_index].key;

        // Pick the right ProfileConfig to clear from based on scope
        let config = if self.scope == SettingsScope::Repo {
            &mut self.repo_as_profile
        } else {
            &mut self.profile_config
        };

        match key {
            // Theme
            FieldKey::ThemeName => {
                if let Some(ref mut t) = config.theme {
                    t.name = None;
                }
            }
            // Updates
            FieldKey::CheckEnabled => {
                if let Some(ref mut u) = config.updates {
                    u.check_enabled = None;
                }
            }
            FieldKey::CheckIntervalHours => {
                if let Some(ref mut u) = config.updates {
                    u.check_interval_hours = None;
                }
            }
            FieldKey::NotifyInCli => {
                if let Some(ref mut u) = config.updates {
                    u.notify_in_cli = None;
                }
            }
            // Worktree
            FieldKey::PathTemplate => {
                if let Some(ref mut w) = config.worktree {
                    w.path_template = None;
                }
            }
            FieldKey::BareRepoPathTemplate => {
                if let Some(ref mut w) = config.worktree {
                    w.bare_repo_path_template = None;
                }
            }
            FieldKey::WorktreeAutoCleanup => {
                if let Some(ref mut w) = config.worktree {
                    w.auto_cleanup = None;
                }
            }
            FieldKey::DeleteBranchOnCleanup => {
                if let Some(ref mut w) = config.worktree {
                    w.delete_branch_on_cleanup = None;
                }
            }
            FieldKey::WorkspacePathTemplate => {
                if let Some(ref mut w) = config.worktree {
                    w.workspace_path_template = None;
                }
            }
            // Sandbox
            FieldKey::DefaultImage => {
                if let Some(ref mut s) = config.sandbox {
                    s.default_image = None;
                }
            }
            FieldKey::Environment => {
                if let Some(ref mut s) = config.sandbox {
                    s.environment = None;
                }
            }
            FieldKey::SandboxAutoCleanup => {
                if let Some(ref mut s) = config.sandbox {
                    s.auto_cleanup = None;
                }
            }
            // Tmux
            FieldKey::StatusBar => {
                if let Some(ref mut t) = config.tmux {
                    t.status_bar = None;
                }
            }
            FieldKey::Mouse => {
                if let Some(ref mut t) = config.tmux {
                    t.mouse = None;
                }
            }
            // Session
            FieldKey::DefaultTool => {
                if let Some(ref mut s) = config.session {
                    s.default_tool = None;
                }
            }
            FieldKey::SandboxEnabledByDefault => {
                if let Some(ref mut s) = config.sandbox {
                    s.enabled_by_default = None;
                }
            }
            FieldKey::YoloModeDefault => {
                if let Some(ref mut s) = config.session {
                    s.yolo_mode_default = None;
                }
            }
            FieldKey::AgentExtraArgs => {
                if let Some(ref mut s) = config.session {
                    s.agent_extra_args = None;
                }
            }
            FieldKey::AgentCommandOverride => {
                if let Some(ref mut s) = config.session {
                    s.agent_command_override = None;
                }
            }
            FieldKey::AgentStatusHooks => {
                if let Some(ref mut s) = config.session {
                    s.agent_status_hooks = None;
                }
            }
            FieldKey::DefaultTerminalMode => {
                if let Some(ref mut s) = config.sandbox {
                    s.default_terminal_mode = None;
                }
            }
            FieldKey::ExtraVolumes => {
                if let Some(ref mut s) = config.sandbox {
                    s.extra_volumes = None;
                }
            }
            FieldKey::PortMappings => {
                if let Some(ref mut s) = config.sandbox {
                    s.port_mappings = None;
                }
            }
            FieldKey::VolumeIgnores => {
                if let Some(ref mut s) = config.sandbox {
                    s.volume_ignores = None;
                }
            }
            FieldKey::MountSsh => {
                if let Some(ref mut s) = config.sandbox {
                    s.mount_ssh = None;
                }
            }
            FieldKey::CpuLimit => {
                if let Some(ref mut s) = config.sandbox {
                    s.cpu_limit = None;
                }
            }
            FieldKey::MemoryLimit => {
                if let Some(ref mut s) = config.sandbox {
                    s.memory_limit = None;
                }
            }
            FieldKey::CustomInstruction => {
                if let Some(ref mut s) = config.sandbox {
                    s.custom_instruction = None;
                }
            }
            FieldKey::ContainerRuntime => {
                if let Some(ref mut s) = config.sandbox {
                    s.container_runtime = None;
                }
            }
            // Sound
            FieldKey::SoundEnabled => {
                if let Some(ref mut s) = config.sound {
                    s.enabled = None;
                }
            }
            FieldKey::SoundMode => {
                if let Some(ref mut s) = config.sound {
                    s.mode = None;
                }
            }
            FieldKey::SoundOnStart => {
                if let Some(ref mut s) = config.sound {
                    s.on_start = None;
                }
            }
            FieldKey::SoundOnRunning => {
                if let Some(ref mut s) = config.sound {
                    s.on_running = None;
                }
            }
            FieldKey::SoundOnWaiting => {
                if let Some(ref mut s) = config.sound {
                    s.on_waiting = None;
                }
            }
            FieldKey::SoundOnIdle => {
                if let Some(ref mut s) = config.sound {
                    s.on_idle = None;
                }
            }
            FieldKey::SoundOnError => {
                if let Some(ref mut s) = config.sound {
                    s.on_error = None;
                }
            }
            // Hooks
            FieldKey::HookOnCreate => {
                if let Some(ref mut h) = config.hooks {
                    h.on_create = None;
                }
            }
            FieldKey::HookOnLaunch => {
                if let Some(ref mut h) = config.hooks {
                    h.on_launch = None;
                }
            }
        }

        // Sync repo_config when in Repo scope
        if self.scope == SettingsScope::Repo {
            self.repo_config = Some(crate::session::profile_to_repo_config(
                &self.repo_as_profile,
            ));
        }

        self.has_changes = true;
    }

    /// Force close without saving
    pub fn force_close(&mut self) {
        self.has_changes = false;
    }

    /// Discard changes and reload
    pub fn discard_changes(&mut self) -> anyhow::Result<()> {
        self.global_config = crate::session::Config::load()?;
        self.profile_config = crate::session::load_profile_config(&self.profile)?;
        self.repo_config = self.project_path.as_ref().and_then(|p| {
            crate::session::load_repo_config(std::path::Path::new(p))
                .ok()
                .flatten()
        });
        self.resolved_base =
            crate::session::merge_configs(self.global_config.clone(), &self.profile_config);
        self.repo_as_profile = self
            .repo_config
            .as_ref()
            .map(crate::session::repo_config_to_profile)
            .unwrap_or_default();
        self.has_changes = false;
        self.rebuild_fields();
        Ok(())
    }
}

/// Validate that an entry for AgentExtraArgs or AgentCommandOverride is in `agent_name=value` format.
fn validate_agent_key_value(text: &str) -> Result<(), String> {
    let Some((key, value)) = text.split_once('=') else {
        let names = crate::agents::agent_names().join(", ");
        return Err(format!(
            "Must be in agent_name=value format (e.g. claude=my-command). Known agents: {}",
            names
        ));
    };

    if key.is_empty() {
        return Err("Agent name cannot be empty".to_string());
    }

    if value.is_empty() {
        return Err("Value cannot be empty".to_string());
    }

    if crate::agents::get_agent(key).is_none() {
        let names = crate::agents::agent_names().join(", ");
        return Err(format!(
            "'{}' is not a known agent. Known agents: {}",
            key, names
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_agent_key_value_valid() {
        assert!(validate_agent_key_value("claude=my-wrapper").is_ok());
        assert!(validate_agent_key_value("opencode=--port 8080").is_ok());
    }

    #[test]
    fn test_validate_agent_key_value_missing_equals() {
        let err = validate_agent_key_value("just-a-command").unwrap_err();
        assert!(err.contains("agent_name=value"));
    }

    #[test]
    fn test_validate_agent_key_value_empty_key() {
        let err = validate_agent_key_value("=some-value").unwrap_err();
        assert!(err.contains("cannot be empty"));
    }

    #[test]
    fn test_validate_agent_key_value_empty_value() {
        let err = validate_agent_key_value("claude=").unwrap_err();
        assert!(err.contains("cannot be empty"));
    }

    #[test]
    fn test_validate_agent_key_value_unknown_agent() {
        let err = validate_agent_key_value("nonexistent=cmd").unwrap_err();
        assert!(err.contains("not a known agent"));
    }
}
