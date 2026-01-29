//! Input handling for the diff view

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

use super::DiffView;

/// Result of handling a key event in the diff view
pub enum DiffAction {
    /// Continue showing the diff view
    Continue,
    /// Close the diff view
    Close,
    /// Launch external editor for a file
    EditFile(PathBuf),
}

impl DiffView {
    /// Handle a key event
    pub fn handle_key(&mut self, key: KeyEvent) -> DiffAction {
        // Clear transient messages on any key
        self.success_message = None;

        // Handle help overlay
        if self.show_help {
            match key.code {
                KeyCode::Esc | KeyCode::Char('?') => {
                    self.show_help = false;
                }
                _ => {}
            }
            return DiffAction::Continue;
        }

        // Handle branch selection dialog
        if self.branch_select.is_some() {
            return self.handle_branch_select_key(key);
        }

        // Normal diff view mode
        self.handle_normal_key(key)
    }

    fn handle_normal_key(&mut self, key: KeyEvent) -> DiffAction {
        match (key.code, key.modifiers) {
            // Close view
            (KeyCode::Esc, _) | (KeyCode::Char('q'), _) => DiffAction::Close,

            // File navigation (j/k always navigate between files)
            (KeyCode::Up, _) | (KeyCode::Char('k'), _) => {
                self.prev_file();
                DiffAction::Continue
            }
            (KeyCode::Down, _) | (KeyCode::Char('j'), _) => {
                self.next_file();
                DiffAction::Continue
            }

            // Diff scrolling
            (KeyCode::PageUp, _) => {
                self.page_up();
                DiffAction::Continue
            }
            (KeyCode::PageDown, _) => {
                self.page_down();
                DiffAction::Continue
            }
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                self.half_page_up();
                DiffAction::Continue
            }
            (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                self.half_page_down();
                DiffAction::Continue
            }
            (KeyCode::Home, _) | (KeyCode::Char('g'), _) => {
                self.scroll_offset = 0;
                DiffAction::Continue
            }
            (KeyCode::End, _) | (KeyCode::Char('G'), _) => {
                self.scroll_offset = self.total_lines.saturating_sub(self.visible_lines);
                DiffAction::Continue
            }

            // Open external editor
            (KeyCode::Char('e'), _) | (KeyCode::Enter, _) => {
                if let Some(file) = self.selected_file() {
                    let full_path = self.repo_path.join(&file.path);
                    return DiffAction::EditFile(full_path);
                }
                DiffAction::Continue
            }

            // Branch selection
            (KeyCode::Char('b'), _) => {
                self.open_branch_select();
                DiffAction::Continue
            }

            // Refresh
            (KeyCode::Char('r'), _) => {
                if let Err(e) = self.refresh_files() {
                    self.error_message = Some(format!("Failed to refresh: {}", e));
                }
                DiffAction::Continue
            }

            // Help
            (KeyCode::Char('?'), _) => {
                self.show_help = true;
                DiffAction::Continue
            }

            _ => DiffAction::Continue,
        }
    }

    fn handle_branch_select_key(&mut self, key: KeyEvent) -> DiffAction {
        let Some(state) = &mut self.branch_select else {
            return DiffAction::Continue;
        };

        match key.code {
            KeyCode::Esc => {
                self.branch_select = None;
            }
            KeyCode::Enter => {
                let branch = state.branches.get(state.selected).cloned();
                if let Some(branch) = branch {
                    self.select_branch(branch);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if state.selected > 0 {
                    state.selected -= 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if state.selected < state.branches.len().saturating_sub(1) {
                    state.selected += 1;
                }
            }
            _ => {}
        }
        DiffAction::Continue
    }

    /// Handle a mouse event
    pub fn handle_mouse(&mut self, mouse: MouseEvent) -> DiffAction {
        // Don't handle mouse in help overlay or branch select dialog
        if self.show_help || self.branch_select.is_some() {
            return DiffAction::Continue;
        }

        // Mouse scroll always scrolls the diff content
        match mouse.kind {
            MouseEventKind::ScrollUp => {
                self.scroll_up(3);
                DiffAction::Continue
            }
            MouseEventKind::ScrollDown => {
                self.scroll_down(3);
                DiffAction::Continue
            }
            _ => DiffAction::Continue,
        }
    }
}
