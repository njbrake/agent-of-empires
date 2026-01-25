//! Input handling for the settings view

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{FieldValue, ListEditState, SettingsFocus, SettingsScope, SettingsView};

/// Result of handling a key event in the settings view
pub enum SettingsAction {
    /// Continue showing the settings view
    Continue,
    /// Close the settings view (with optional unsaved changes warning)
    Close,
    /// Close was cancelled due to unsaved changes
    UnsavedChangesWarning,
}

impl SettingsView {
    pub fn handle_key(&mut self, key: KeyEvent) -> SettingsAction {
        // Clear transient messages on any key
        self.success_message = None;

        // Handle text editing mode
        if self.editing_text.is_some() {
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

            // Close
            (KeyCode::Esc, _) | (KeyCode::Char('q'), _) => {
                if self.has_changes {
                    SettingsAction::UnsavedChangesWarning
                } else {
                    SettingsAction::Close
                }
            }

            // Switch scope tabs
            (KeyCode::Tab, _) | (KeyCode::BackTab, _) => {
                self.scope = match self.scope {
                    SettingsScope::Global => SettingsScope::Profile,
                    SettingsScope::Profile => SettingsScope::Global,
                };
                self.rebuild_fields();
                SettingsAction::Continue
            }

            // Switch focus between categories and fields
            (KeyCode::Left, _) | (KeyCode::Char('h'), _) => {
                self.focus = SettingsFocus::Categories;
                SettingsAction::Continue
            }
            (KeyCode::Right, _) | (KeyCode::Char('l'), _) => {
                self.focus = SettingsFocus::Fields;
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
                            // Toggle boolean on Enter too
                            let new_value = !value;
                            self.fields[self.selected_field].value = FieldValue::Bool(new_value);
                            self.apply_field_to_config(self.selected_field);
                        }
                        FieldValue::Text(value) => {
                            self.editing_text = Some(value.clone());
                        }
                        FieldValue::OptionalText(value) => {
                            self.editing_text = Some(value.clone().unwrap_or_default());
                        }
                        FieldValue::Number(value) => {
                            self.editing_text = Some(value.to_string());
                        }
                        FieldValue::Select { selected, options } => {
                            // Cycle through options
                            let new_selected = (*selected + 1) % options.len();
                            self.fields[self.selected_field].value = FieldValue::Select {
                                selected: new_selected,
                                options: options.clone(),
                            };
                            self.apply_field_to_config(self.selected_field);
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

            // Reset field to default (clear profile override)
            (KeyCode::Char('r'), _) => {
                if self.scope == SettingsScope::Profile
                    && self.focus == SettingsFocus::Fields
                    && !self.fields.is_empty()
                {
                    self.clear_profile_override(self.selected_field);
                    self.rebuild_fields();
                }
                SettingsAction::Continue
            }

            _ => SettingsAction::Continue,
        }
    }

    fn handle_text_edit_key(&mut self, key: KeyEvent) -> SettingsAction {
        match key.code {
            KeyCode::Esc => {
                self.editing_text = None;
                self.error_message = None;
            }
            KeyCode::Enter => {
                if let Some(text) = self.editing_text.take() {
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
                                self.editing_text = Some(text);
                                return SettingsAction::Continue;
                            }
                        }
                        _ => {}
                    }

                    // Validate
                    if let Err(e) = field.validate() {
                        self.error_message = Some(e);
                        // Revert to editing
                        self.editing_text = match &field.value {
                            FieldValue::Text(v) => Some(v.clone()),
                            FieldValue::OptionalText(v) => Some(v.clone().unwrap_or_default()),
                            FieldValue::Number(v) => Some(v.to_string()),
                            _ => None,
                        };
                        return SettingsAction::Continue;
                    }

                    self.apply_field_to_config(self.selected_field);
                    self.error_message = None;
                }
            }
            KeyCode::Backspace => {
                if let Some(ref mut text) = self.editing_text {
                    text.pop();
                }
            }
            KeyCode::Char(c) => {
                if let Some(ref mut text) = self.editing_text {
                    text.push(c);
                }
            }
            _ => {}
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
                state.editing_item = Some(String::new());
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

                // Update state and apply config after releasing borrows
                if let Some(ref mut s) = self.list_edit_state {
                    s.selected_index = new_selected_idx;
                }
                self.apply_field_to_config(self.selected_field);
            }
            KeyCode::Enter => {
                // Edit selected item
                if let FieldValue::List(items) = &self.fields[self.selected_field].value {
                    if !items.is_empty() && state.selected_index < items.len() {
                        state.editing_item = Some(items[state.selected_index].clone());
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
            }
            KeyCode::Enter => {
                // Take the text and flags out to avoid borrow conflict
                let text = state.editing_item.take();
                let adding_new = state.adding_new;
                let selected_idx = state.selected_index;
                state.adding_new = false;

                if let Some(text) = text {
                    if !text.is_empty() {
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
                    }
                }
            }
            KeyCode::Backspace => {
                if let Some(ref mut text) = state.editing_item {
                    text.pop();
                }
            }
            KeyCode::Char(c) => {
                if let Some(ref mut text) = state.editing_item {
                    text.push(c);
                }
            }
            _ => {}
        }
        SettingsAction::Continue
    }

    fn clear_profile_override(&mut self, field_index: usize) {
        use super::SettingsCategory;

        if field_index >= self.fields.len() {
            return;
        }

        let field = &self.fields[field_index];
        let key = field.key;
        let category = field.category;

        match category {
            SettingsCategory::Updates => {
                if let Some(ref mut updates) = self.profile_config.updates {
                    match key {
                        "check_enabled" => updates.check_enabled = None,
                        "check_interval_hours" => updates.check_interval_hours = None,
                        "notify_in_cli" => updates.notify_in_cli = None,
                        _ => {}
                    }
                }
            }
            SettingsCategory::Worktree => {
                if let Some(ref mut worktree) = self.profile_config.worktree {
                    match key {
                        "path_template" => worktree.path_template = None,
                        "bare_repo_path_template" => worktree.bare_repo_path_template = None,
                        "auto_cleanup" => worktree.auto_cleanup = None,
                        _ => {}
                    }
                }
            }
            SettingsCategory::Sandbox => {
                if let Some(ref mut sandbox) = self.profile_config.sandbox {
                    match key {
                        "default_image" => sandbox.default_image = None,
                        "environment" => sandbox.environment = None,
                        "auto_cleanup" => sandbox.auto_cleanup = None,
                        "cpu_limit" => sandbox.cpu_limit = None,
                        "memory_limit" => sandbox.memory_limit = None,
                        _ => {}
                    }
                }
            }
            SettingsCategory::Tmux => {
                if let Some(ref mut tmux) = self.profile_config.tmux {
                    if key == "status_bar" {
                        tmux.status_bar = None;
                    }
                }
            }
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
        self.has_changes = false;
        self.rebuild_fields();
        Ok(())
    }
}
