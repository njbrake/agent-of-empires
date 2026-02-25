use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

use super::{NewSessionDialog, PATH_FIELD};

const PATH_INVALID_FLASH_DURATION: Duration = Duration::from_millis(300);

enum PathAutocompleteOutcome {
    /// No directory matches were found; caller may fall back to normal Tab behavior.
    NoCandidates,
    /// Matches exist but input text did not change.
    NoChange,
    /// Input text was updated from completion.
    Changed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PathCompletionCycle {
    pub(super) parent_prefix: String,
    pub(super) suffix: String,
    pub(super) candidates: Vec<String>,
    /// None means we're showing only the common prefix, not a concrete candidate yet.
    pub(super) displayed_index: Option<usize>,
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

        if key.code == KeyCode::Tab && key.modifiers == KeyModifiers::NONE {
            match self.autocomplete_path_segment() {
                PathAutocompleteOutcome::NoCandidates => {
                    self.clear_path_completion_cycle();
                    if self.is_path_invalid() {
                        self.trigger_path_invalid_flash();
                        return true;
                    }
                    return false;
                }
                PathAutocompleteOutcome::NoChange => return true,
                PathAutocompleteOutcome::Changed => {
                    self.error_message = None;
                    self.path_invalid_flash_until = None;
                    return true;
                }
            }
        }

        if matches!(key.code, KeyCode::Home)
            || (key.code == KeyCode::Char('a') && key.modifiers.contains(KeyModifiers::CONTROL))
        {
            self.move_path_cursor_to(0);
            self.error_message = None;
            self.path_invalid_flash_until = None;
            self.clear_path_completion_cycle();
            return true;
        }

        if (key.code == KeyCode::Left && key.modifiers.contains(KeyModifiers::CONTROL))
            || (key.code == KeyCode::Char('b') && key.modifiers.contains(KeyModifiers::ALT))
        {
            self.move_path_cursor_to_previous_segment();
            self.error_message = None;
            self.path_invalid_flash_until = None;
            self.clear_path_completion_cycle();
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

    fn autocomplete_path_segment(&mut self) -> PathAutocompleteOutcome {
        let value = self.path.value().to_string();
        let cursor_char = self.path.visual_cursor().min(value.chars().count());
        let cursor_byte = char_to_byte_idx(&value, cursor_char);

        let segment_start = value[..cursor_byte].rfind('/').map_or(0, |idx| idx + 1);
        let segment_end = value[cursor_byte..]
            .find('/')
            .map_or(value.len(), |offset| cursor_byte + offset);

        let parent_prefix = &value[..segment_start];
        let current_segment = &value[segment_start..cursor_byte];
        let suffix = &value[segment_end..];
        let context_parent = parent_prefix.to_string();
        let context_suffix = suffix.to_string();

        if let Some(cycle) = self.path_completion_cycle.as_mut() {
            if cycle.parent_prefix == context_parent
                && cycle.suffix == context_suffix
                && cycle.candidates.len() > 1
            {
                let next_index = cycle
                    .displayed_index
                    .map(|idx| (idx + 1) % cycle.candidates.len())
                    .unwrap_or(0);
                cycle.displayed_index = Some(next_index);
                let replacement = cycle.candidates[next_index].clone();

                let mut completed = String::with_capacity(value.len() + replacement.len());
                completed.push_str(parent_prefix);
                completed.push_str(&replacement);
                completed.push_str(suffix);

                if completed == value {
                    return PathAutocompleteOutcome::NoChange;
                }

                let cursor_after_completion =
                    parent_prefix.chars().count() + replacement.chars().count();
                self.set_path_value_with_cursor(completed, cursor_after_completion);
                return PathAutocompleteOutcome::Changed;
            }
        }

        let Some(base_dir) = path_completion_base(parent_prefix) else {
            self.clear_path_completion_cycle();
            return PathAutocompleteOutcome::NoCandidates;
        };

        let include_hidden = current_segment.starts_with('.');
        let mut matches = Vec::new();
        let Ok(entries) = std::fs::read_dir(base_dir) else {
            self.clear_path_completion_cycle();
            return PathAutocompleteOutcome::NoCandidates;
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
            self.clear_path_completion_cycle();
            return PathAutocompleteOutcome::NoCandidates;
        }
        matches.sort();

        let replacement = if matches.len() == 1 {
            self.clear_path_completion_cycle();
            let mut segment = matches[0].clone();
            if suffix.is_empty() {
                segment.push('/');
            }
            segment
        } else {
            let common_prefix = longest_common_prefix(&matches);
            self.path_completion_cycle = Some(PathCompletionCycle {
                parent_prefix: context_parent,
                suffix: context_suffix,
                candidates: matches.clone(),
                displayed_index: None,
            });
            if common_prefix.chars().count() > current_segment.chars().count() {
                common_prefix
            } else if let Some(new_cycle) = self.path_completion_cycle.as_mut() {
                new_cycle.displayed_index = Some(0);
                new_cycle.candidates[0].clone()
            } else {
                matches[0].clone()
            }
        };

        let mut completed = String::with_capacity(value.len() + replacement.len());
        completed.push_str(parent_prefix);
        completed.push_str(&replacement);
        completed.push_str(suffix);

        if completed == value {
            return PathAutocompleteOutcome::NoChange;
        }

        let cursor_after_completion = parent_prefix.chars().count() + replacement.chars().count();
        self.set_path_value_with_cursor(completed, cursor_after_completion);
        PathAutocompleteOutcome::Changed
    }

    pub(super) fn clear_path_completion_cycle(&mut self) {
        self.path_completion_cycle = None;
    }

    fn is_path_invalid(&self) -> bool {
        let path = self.path.value().trim();
        path.is_empty() || !Path::new(path).is_dir()
    }

    fn trigger_path_invalid_flash(&mut self) {
        self.path_invalid_flash_until = Some(Instant::now() + PATH_INVALID_FLASH_DURATION);
    }

    pub(super) fn is_path_invalid_flash_active(&self) -> bool {
        self.path_invalid_flash_until.is_some()
    }
}
