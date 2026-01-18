//! Input handling for HomeView

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{HomeView, ViewMode};
use crate::session::{flatten_tree, Item, Status};
use crate::tui::app::Action;
use crate::tui::dialogs::{
    ConfirmDialog, DeleteDialogConfig, DialogResult, GroupDeleteOptionsDialog, InfoDialog,
    NewSessionDialog, RenameDialog, UnifiedDeleteDialog,
};

impl HomeView {
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
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
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    self.new_dialog = None;
                }
                DialogResult::Submit(data) => match self.create_session(data) {
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
                DialogResult::Continue => {}
                DialogResult::Cancel => {
                    self.confirm_dialog = None;
                }
                DialogResult::Submit(_) => {
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
                DialogResult::Submit(new_title) => {
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

    pub(super) fn move_cursor(&mut self, delta: i32) {
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

    pub(super) fn update_selected(&mut self) {
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

    pub(super) fn update_filter(&mut self) {
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
}
