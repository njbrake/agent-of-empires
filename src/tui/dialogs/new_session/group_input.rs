use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tui_input::Input;

use super::NewSessionDialog;
use crate::tui::components::GroupGhostCompletion;

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
        self.group_ghost = GroupGhostCompletion::compute(&self.group, &self.existing_groups);
    }

    pub(super) fn accept_group_ghost(&mut self) {
        if let Some(ghost) = self.group_ghost.take() {
            if let Some(new_value) = ghost.accept(&self.group) {
                self.group = Input::new(new_value);
                self.recompute_group_ghost();
            }
        }
    }

    pub(super) fn clear_group_ghost(&mut self) {
        self.group_ghost = None;
    }

    pub(super) fn group_ghost_text(&self) -> Option<&str> {
        self.group_ghost.as_ref().map(|g| g.ghost_text())
    }
}
