use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tui_input::Input;

use super::NewSessionDialog;
use crate::tui::components::longest_common_prefix;

pub(super) struct GroupGhostCompletion {
    input_snapshot: String,
    cursor_snapshot: usize,
    pub(super) ghost_text: String,
}

impl NewSessionDialog {
    pub(super) fn handle_group_shortcuts(&mut self, key: KeyEvent, group_field: usize) -> bool {
        if self.focused_field != group_field {
            return false;
        }

        // Right arrow at end of input with ghost: accept ghost text
        if key.code == KeyCode::Right && key.modifiers == KeyModifiers::NONE {
            let cursor = self.group.visual_cursor();
            let char_len = self.group.value().chars().count();
            if cursor >= char_len && self.group_ghost.is_some() {
                self.accept_group_ghost();
                return true;
            }
            return false;
        }

        // End key at end of input with ghost: accept ghost text
        if key.code == KeyCode::End && key.modifiers == KeyModifiers::NONE {
            let cursor = self.group.visual_cursor();
            let char_len = self.group.value().chars().count();
            if cursor >= char_len && self.group_ghost.is_some() {
                self.accept_group_ghost();
                return true;
            }
            return false;
        }

        false
    }

    pub(super) fn recompute_group_ghost(&mut self) {
        self.group_ghost = None;

        if self.existing_groups.is_empty() {
            return;
        }

        let value = self.group.value().to_string();
        if value.is_empty() {
            return;
        }

        let char_len = value.chars().count();
        let cursor_char = self.group.visual_cursor().min(char_len);

        // Only show ghost when cursor is at end of input
        if cursor_char < char_len {
            return;
        }

        let mut matches: Vec<String> = self
            .existing_groups
            .iter()
            .filter(|g| g.starts_with(&value))
            .cloned()
            .collect();

        if matches.is_empty() {
            return;
        }
        matches.sort();

        let ghost_text = if matches.len() == 1 {
            matches[0][value.len()..].to_string()
        } else {
            let common = longest_common_prefix(&matches);
            if common.len() > value.len() {
                common[value.len()..].to_string()
            } else {
                matches[0][value.len()..].to_string()
            }
        };

        if ghost_text.is_empty() {
            return;
        }

        self.group_ghost = Some(GroupGhostCompletion {
            input_snapshot: value,
            cursor_snapshot: cursor_char,
            ghost_text,
        });
    }

    pub(super) fn accept_group_ghost(&mut self) -> bool {
        let ghost = match self.group_ghost.take() {
            Some(g) => g,
            None => return false,
        };

        let value = self.group.value().to_string();
        let cursor_char = self.group.visual_cursor().min(value.chars().count());

        // Staleness check
        if ghost.input_snapshot != value || ghost.cursor_snapshot != cursor_char {
            return false;
        }

        let mut new_value = value;
        new_value.push_str(&ghost.ghost_text);
        self.group = Input::new(new_value);
        self.recompute_group_ghost();
        true
    }

    pub(super) fn clear_group_ghost(&mut self) {
        self.group_ghost = None;
    }

    pub(super) fn group_ghost_text(&self) -> Option<&str> {
        self.group_ghost.as_ref().map(|g| g.ghost_text.as_str())
    }
}
