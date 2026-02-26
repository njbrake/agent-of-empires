use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::PathBuf;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

use super::{NewSessionDialog, PATH_FIELD};

pub(super) struct PathGhostCompletion {
    input_snapshot: String,
    cursor_snapshot: usize,
    pub(super) ghost_text: String,
    #[allow(dead_code)]
    candidates: Vec<String>,
}

fn char_to_byte_idx(value: &str, char_idx: usize) -> usize {
    value
        .char_indices()
        .nth(char_idx)
        .map(|(idx, _)| idx)
        .unwrap_or(value.len())
}

fn longest_common_prefix(values: &[String]) -> String {
    if values.is_empty() {
        return String::new();
    }

    let mut prefix = values[0].clone();
    for value in &values[1..] {
        while !value.starts_with(&prefix) {
            if prefix.pop().is_none() {
                break;
            }
        }
        if prefix.is_empty() {
            break;
        }
    }
    prefix
}

fn path_completion_base(parent_prefix: &str) -> Option<PathBuf> {
    if parent_prefix.is_empty() {
        return Some(PathBuf::from("."));
    }

    let trimmed = parent_prefix.trim_end_matches('/');
    if trimmed.is_empty() {
        return Some(PathBuf::from("/"));
    }

    if trimmed == "~" {
        return dirs::home_dir();
    }

    if let Some(stripped) = trimmed.strip_prefix("~/") {
        return dirs::home_dir().map(|home| home.join(stripped));
    }

    Some(PathBuf::from(trimmed))
}

impl NewSessionDialog {
    pub(super) fn handle_path_shortcuts(&mut self, key: KeyEvent) -> bool {
        if self.focused_field != PATH_FIELD {
            return false;
        }

        // Right arrow at end of input with ghost: accept ghost text
        if key.code == KeyCode::Right && key.modifiers == KeyModifiers::NONE {
            let cursor = self.path.visual_cursor();
            let char_len = self.path.value().chars().count();
            if cursor >= char_len && self.path_ghost.is_some() {
                self.accept_path_ghost();
                return true;
            }
            return false;
        }

        // End key at end of input with ghost: accept ghost text
        if key.code == KeyCode::End && key.modifiers == KeyModifiers::NONE {
            let cursor = self.path.visual_cursor();
            let char_len = self.path.value().chars().count();
            if cursor >= char_len && self.path_ghost.is_some() {
                self.accept_path_ghost();
                return true;
            }
            return false;
        }

        if matches!(key.code, KeyCode::Home)
            || (key.code == KeyCode::Char('a') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            self.move_path_cursor_to(0);
            self.error_message = None;
            self.path_invalid_flash_until = None;
            self.recompute_path_ghost();
            return true;
        }

        if (key.code == KeyCode::Left && key.modifiers.contains(KeyModifiers::CONTROL))
            || (key.code == KeyCode::Char('b') && key.modifiers.contains(KeyModifiers::ALT))
        {
            self.move_path_cursor_to_previous_segment();
            self.error_message = None;
            self.path_invalid_flash_until = None;
            self.recompute_path_ghost();
            return true;
        }

        false
    }

    fn move_path_cursor_to(&mut self, target_char_idx: usize) {
        let char_len = self.path.value().chars().count();
        let target = target_char_idx.min(char_len);
        let current = self.path.visual_cursor().min(char_len);

        if target < current {
            for _ in 0..(current - target) {
                self.path
                    .handle_event(&crossterm::event::Event::Key(KeyEvent::new(
                        KeyCode::Left,
                        KeyModifiers::NONE,
                    )));
            }
        } else if target > current {
            for _ in 0..(target - current) {
                self.path
                    .handle_event(&crossterm::event::Event::Key(KeyEvent::new(
                        KeyCode::Right,
                        KeyModifiers::NONE,
                    )));
            }
        }
    }

    fn move_path_cursor_to_previous_segment(&mut self) {
        let chars: Vec<char> = self.path.value().chars().collect();
        let mut cursor = self.path.visual_cursor().min(chars.len());
        if cursor == 0 {
            return;
        }

        while cursor > 0 && chars[cursor - 1] == '/' {
            cursor -= 1;
        }
        while cursor > 0 && chars[cursor - 1] != '/' {
            cursor -= 1;
        }

        self.move_path_cursor_to(cursor);
    }

    fn set_path_value_with_cursor(&mut self, value: String, cursor_char_idx: usize) {
        self.path = Input::new(value);
        let total_chars = self.path.value().chars().count();
        let target = cursor_char_idx.min(total_chars);
        let left_steps = total_chars.saturating_sub(target);

        for _ in 0..left_steps {
            self.path
                .handle_event(&crossterm::event::Event::Key(KeyEvent::new(
                    KeyCode::Left,
                    KeyModifiers::NONE,
                )));
        }
    }

    pub(super) fn recompute_path_ghost(&mut self) {
        self.path_ghost = None;

        let value = self.path.value().to_string();
        let char_len = value.chars().count();
        let cursor_char = self.path.visual_cursor().min(char_len);

        // Only show ghost when cursor is at end of input
        if cursor_char < char_len {
            return;
        }

        let cursor_byte = char_to_byte_idx(&value, cursor_char);

        let segment_start = value[..cursor_byte].rfind('/').map_or(0, |idx| idx + 1);
        let parent_prefix = &value[..segment_start];
        let current_segment = &value[segment_start..cursor_byte];

        let Some(base_dir) = path_completion_base(parent_prefix) else {
            return;
        };

        let include_hidden = current_segment.starts_with('.');
        let mut matches = Vec::new();
        let Ok(entries) = std::fs::read_dir(&base_dir) else {
            return;
        };

        for entry in entries.flatten() {
            if !entry.path().is_dir() {
                continue;
            }
            let file_name = entry.file_name();
            let Some(name) = file_name.to_str() else {
                continue;
            };
            if !include_hidden && name.starts_with('.') {
                continue;
            }
            if name.starts_with(current_segment) {
                matches.push(name.to_string());
            }
        }

        if matches.is_empty() {
            return;
        }
        matches.sort();

        let ghost_text = if matches.len() == 1 {
            // Single match: ghost = remaining chars + /
            let remainder = &matches[0][current_segment.len()..];
            format!("{}/", remainder)
        } else {
            let common_prefix = longest_common_prefix(&matches);
            if common_prefix.len() > current_segment.len() {
                // Multiple matches with common prefix extension
                common_prefix[current_segment.len()..].to_string()
            } else {
                // Common prefix equals what's typed; show first candidate's remainder
                let remainder = &matches[0][current_segment.len()..];
                format!("{}/", remainder)
            }
        };

        if ghost_text.is_empty() {
            return;
        }

        self.path_ghost = Some(PathGhostCompletion {
            input_snapshot: value,
            cursor_snapshot: cursor_char,
            ghost_text,
            candidates: matches,
        });
    }

    pub(super) fn accept_path_ghost(&mut self) -> bool {
        let ghost = match self.path_ghost.take() {
            Some(g) => g,
            None => return false,
        };

        let value = self.path.value().to_string();
        let cursor_char = self.path.visual_cursor().min(value.chars().count());

        // Staleness check
        if ghost.input_snapshot != value || ghost.cursor_snapshot != cursor_char {
            return false;
        }

        let mut new_value = value;
        new_value.push_str(&ghost.ghost_text);
        let new_cursor = new_value.chars().count();
        self.set_path_value_with_cursor(new_value, new_cursor);
        self.error_message = None;
        self.path_invalid_flash_until = None;
        self.recompute_path_ghost();
        true
    }

    pub(super) fn clear_path_ghost(&mut self) {
        self.path_ghost = None;
    }

    pub(super) fn ghost_text(&self) -> Option<&str> {
        self.path_ghost.as_ref().map(|g| g.ghost_text.as_str())
    }

    pub(super) fn is_path_invalid_flash_active(&self) -> bool {
        self.path_invalid_flash_until.is_some()
    }
}
