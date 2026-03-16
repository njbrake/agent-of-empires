//! Shared text input rendering component

use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use tui_input::Input;

use crate::tui::styles::Theme;

/// Finds the longest common prefix among a set of strings.
pub fn longest_common_prefix(values: &[String]) -> String {
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

/// Ghost text completion state for group name autocomplete.
///
/// Computes and stores a ghost suggestion based on the current input value
/// and a list of existing group names. The ghost text is shown as dimmed text
/// after the cursor and can be accepted with Right/End.
pub struct GroupGhostCompletion {
    input_snapshot: String,
    cursor_snapshot: usize,
    ghost_text: String,
}

impl GroupGhostCompletion {
    /// Compute a ghost completion for the given input against existing groups.
    /// Returns `None` if there is no matching suggestion.
    pub fn compute(input: &Input, existing_groups: &[String]) -> Option<Self> {
        if existing_groups.is_empty() {
            return None;
        }

        let value = input.value().to_string();
        if value.is_empty() {
            return None;
        }

        let char_len = value.chars().count();
        let cursor_char = input.visual_cursor().min(char_len);

        // Only show ghost when cursor is at end of input
        if cursor_char < char_len {
            return None;
        }

        let mut matches: Vec<String> = existing_groups
            .iter()
            .filter(|g| g.starts_with(&value))
            .cloned()
            .collect();

        if matches.is_empty() {
            return None;
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
            return None;
        }

        Some(Self {
            input_snapshot: value,
            cursor_snapshot: cursor_char,
            ghost_text,
        })
    }

    /// Try to accept the ghost text into the input. Returns the new input value
    /// if the ghost was still valid (not stale), or `None` if stale.
    pub fn accept(self, input: &Input) -> Option<String> {
        let value = input.value().to_string();
        let cursor_char = input.visual_cursor().min(value.chars().count());

        // Staleness check
        if self.input_snapshot != value || self.cursor_snapshot != cursor_char {
            return None;
        }

        let mut new_value = value;
        new_value.push_str(&self.ghost_text);
        Some(new_value)
    }

    pub fn ghost_text(&self) -> &str {
        &self.ghost_text
    }
}

/// Renders a text input field with a label and cursor.
///
/// When focused, displays an inverse-video cursor over the current character position.
/// When not focused, displays the value (or placeholder if empty).
pub fn render_text_field(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    input: &Input,
    is_focused: bool,
    placeholder: Option<&str>,
    theme: &Theme,
) {
    render_text_field_with_ghost(
        frame,
        area,
        label,
        input,
        is_focused,
        placeholder,
        None,
        theme,
    );
}

/// Like `render_text_field` but with optional ghost (autocomplete) text.
/// If `ghost_text` is provided, it is rendered after the cursor in dimmed style.
#[allow(clippy::too_many_arguments)]
pub fn render_text_field_with_ghost(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    input: &Input,
    is_focused: bool,
    placeholder: Option<&str>,
    ghost_text: Option<&str>,
    theme: &Theme,
) {
    let label_style = if is_focused {
        Style::default().fg(theme.accent).underlined()
    } else {
        Style::default().fg(theme.text)
    };
    let value_style = if is_focused {
        Style::default().fg(theme.accent)
    } else {
        Style::default().fg(theme.text)
    };

    let value = input.value();

    let mut spans = vec![Span::styled(label, label_style), Span::raw(" ")];

    if value.is_empty() && !is_focused {
        if let Some(placeholder_text) = placeholder {
            spans.push(Span::styled(placeholder_text, value_style));
        }
    } else if is_focused {
        let cursor_pos = input.visual_cursor();
        let cursor_style = Style::default().fg(theme.background).bg(theme.accent);

        // Split value into: before cursor, char at cursor, after cursor
        let before: String = value.chars().take(cursor_pos).collect();
        let cursor_char: String = value
            .chars()
            .nth(cursor_pos)
            .map(|c| c.to_string())
            .unwrap_or_else(|| " ".to_string());
        let after: String = value.chars().skip(cursor_pos + 1).collect();

        if !before.is_empty() {
            spans.push(Span::styled(before, value_style));
        }
        spans.push(Span::styled(cursor_char, cursor_style));
        if !after.is_empty() {
            spans.push(Span::styled(after, value_style));
        }
        if let Some(ghost) = ghost_text {
            spans.push(Span::styled(ghost, Style::default().fg(theme.dimmed)));
        }
    } else {
        spans.push(Span::styled(value, value_style));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn groups(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    // --- longest_common_prefix tests ---

    #[test]
    fn lcp_empty_input() {
        assert_eq!(longest_common_prefix(&[]), "");
    }

    #[test]
    fn lcp_single_value() {
        assert_eq!(longest_common_prefix(&groups(&["hello"])), "hello");
    }

    #[test]
    fn lcp_identical_values() {
        assert_eq!(longest_common_prefix(&groups(&["abc", "abc"])), "abc");
    }

    #[test]
    fn lcp_common_prefix() {
        assert_eq!(
            longest_common_prefix(&groups(&["work/api", "work/backend", "work/frontend"])),
            "work/"
        );
    }

    #[test]
    fn lcp_no_common_prefix() {
        assert_eq!(longest_common_prefix(&groups(&["alpha", "beta"])), "");
    }

    #[test]
    fn lcp_unicode() {
        assert_eq!(
            longest_common_prefix(&groups(&["cafe\u{0301}1", "cafe\u{0301}2"])),
            "cafe\u{0301}"
        );
    }

    #[test]
    fn lcp_one_is_prefix_of_another() {
        assert_eq!(
            longest_common_prefix(&groups(&["work", "work/frontend"])),
            "work"
        );
    }

    // --- GroupGhostCompletion tests ---

    #[test]
    fn ghost_no_groups() {
        let input = Input::new("w".to_string());
        assert!(GroupGhostCompletion::compute(&input, &[]).is_none());
    }

    #[test]
    fn ghost_empty_input() {
        let input = Input::default();
        let groups = groups(&["work"]);
        assert!(GroupGhostCompletion::compute(&input, &groups).is_none());
    }

    #[test]
    fn ghost_no_match() {
        let input = Input::new("z".to_string());
        let groups = groups(&["work", "personal"]);
        assert!(GroupGhostCompletion::compute(&input, &groups).is_none());
    }

    #[test]
    fn ghost_single_match() {
        let input = Input::new("per".to_string());
        let groups = groups(&["work", "personal"]);
        let ghost = GroupGhostCompletion::compute(&input, &groups).unwrap();
        assert_eq!(ghost.ghost_text(), "sonal");
    }

    #[test]
    fn ghost_multiple_matches_with_common_prefix() {
        let input = Input::new("w".to_string());
        let groups = groups(&["work/api", "work/backend"]);
        let ghost = GroupGhostCompletion::compute(&input, &groups).unwrap();
        assert_eq!(ghost.ghost_text(), "ork/");
    }

    #[test]
    fn ghost_multiple_matches_no_extra_common_prefix() {
        let input = Input::new("work/".to_string());
        let groups = groups(&["work/api", "work/backend"]);
        let ghost = GroupGhostCompletion::compute(&input, &groups).unwrap();
        // Common prefix is "work/" which equals input, so falls back to first sorted match
        assert_eq!(ghost.ghost_text(), "api");
    }

    #[test]
    fn ghost_exact_match_returns_none() {
        let input = Input::new("work".to_string());
        let groups = groups(&["work"]);
        // Ghost text would be empty since input == match
        assert!(GroupGhostCompletion::compute(&input, &groups).is_none());
    }

    #[test]
    fn ghost_case_sensitive() {
        let input = Input::new("W".to_string());
        let groups = groups(&["work"]);
        assert!(GroupGhostCompletion::compute(&input, &groups).is_none());
    }

    #[test]
    fn ghost_accept_valid() {
        let input = Input::new("per".to_string());
        let groups = groups(&["personal"]);
        let ghost = GroupGhostCompletion::compute(&input, &groups).unwrap();
        let result = ghost.accept(&input).unwrap();
        assert_eq!(result, "personal");
    }

    #[test]
    fn ghost_accept_stale_value() {
        let input = Input::new("per".to_string());
        let groups = groups(&["personal"]);
        let ghost = GroupGhostCompletion::compute(&input, &groups).unwrap();
        // Input changed after computing ghost
        let changed_input = Input::new("pers".to_string());
        assert!(ghost.accept(&changed_input).is_none());
    }
}
