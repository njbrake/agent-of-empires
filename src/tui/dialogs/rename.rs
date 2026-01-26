//! Rename session dialog

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::*;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

use super::DialogResult;
use crate::tui::components::render_text_field;
use crate::tui::styles::Theme;

/// Data returned when the rename dialog is submitted
#[derive(Debug, Clone)]
pub struct RenameData {
    /// New title (empty string means keep current)
    pub title: String,
    /// New group path (None means keep current, Some("") means remove from group)
    pub group: Option<String>,
}

pub struct RenameDialog {
    current_title: String,
    current_group: String,
    new_title: Input,
    new_group: Input,
    focused_field: usize, // 0 = title, 1 = group
}

impl RenameDialog {
    pub fn new(current_title: &str, current_group: &str) -> Self {
        Self {
            current_title: current_title.to_string(),
            current_group: current_group.to_string(),
            new_title: Input::default(),
            new_group: Input::new(current_group.to_string()),
            focused_field: 0,
        }
    }

    fn focused_input(&mut self) -> &mut Input {
        match self.focused_field {
            0 => &mut self.new_title,
            _ => &mut self.new_group,
        }
    }

    fn next_field(&mut self) {
        self.focused_field = (self.focused_field + 1) % 2;
    }

    fn prev_field(&mut self) {
        self.focused_field = if self.focused_field == 0 { 1 } else { 0 };
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult<RenameData> {
        match key.code {
            KeyCode::Esc => DialogResult::Cancel,
            KeyCode::Enter => {
                let title_value = self.new_title.value().trim().to_string();
                let group_value = self.new_group.value().trim();

                // If title is empty and group hasn't changed, cancel (nothing to change)
                if title_value.is_empty() && group_value == self.current_group {
                    return DialogResult::Cancel;
                }

                // Determine the group value:
                // - Same as current means keep current group (None)
                // - Empty (and was non-empty) means remove from group (Some(""))
                // - Any other changed value means set new group
                let group = if group_value == self.current_group {
                    None
                } else if group_value.is_empty() {
                    Some(String::new())
                } else {
                    Some(group_value.to_string())
                };

                DialogResult::Submit(RenameData {
                    title: title_value,
                    group,
                })
            }
            KeyCode::Tab => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.prev_field();
                } else {
                    self.next_field();
                }
                DialogResult::Continue
            }
            KeyCode::Down => {
                self.next_field();
                DialogResult::Continue
            }
            KeyCode::Up => {
                self.prev_field();
                DialogResult::Continue
            }
            _ => {
                self.focused_input()
                    .handle_event(&crossterm::event::Event::Key(key));
                DialogResult::Continue
            }
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_width = 50;
        let dialog_height = 12;
        let x = area.x + (area.width.saturating_sub(dialog_width)) / 2;
        let y = area.y + (area.height.saturating_sub(dialog_height)) / 2;

        let dialog_area = Rect {
            x,
            y,
            width: dialog_width.min(area.width),
            height: dialog_height.min(area.height),
        };

        let clear = Clear;
        frame.render_widget(clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .title(" Edit Session ")
            .title_style(Style::default().fg(theme.title).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(1), // Current title
                Constraint::Length(1), // Current group
                Constraint::Length(1), // Spacer
                Constraint::Length(1), // New title label + field
                Constraint::Length(1), // New group label + field
                Constraint::Length(1), // Spacer
                Constraint::Min(1),    // Hint
            ])
            .split(inner);

        // Current title
        let current_title_line = Line::from(vec![
            Span::styled("Current title: ", Style::default().fg(theme.dimmed)),
            Span::styled(&self.current_title, Style::default().fg(theme.text)),
        ]);
        frame.render_widget(Paragraph::new(current_title_line), chunks[0]);

        // Current group
        let group_display = if self.current_group.is_empty() {
            "(none)".to_string()
        } else {
            self.current_group.clone()
        };
        let current_group_line = Line::from(vec![
            Span::styled("Current group: ", Style::default().fg(theme.dimmed)),
            Span::styled(group_display, Style::default().fg(theme.text)),
        ]);
        frame.render_widget(Paragraph::new(current_group_line), chunks[1]);

        // New title field
        render_text_field(
            frame,
            chunks[3],
            "New title:",
            &self.new_title,
            self.focused_field == 0,
            None,
            theme,
        );

        // New group field
        render_text_field(
            frame,
            chunks[4],
            "New group:",
            &self.new_group,
            self.focused_field == 1,
            None,
            theme,
        );

        // Hint
        let hint = Line::from(vec![
            Span::styled("Tab", Style::default().fg(theme.hint)),
            Span::raw(" switch  "),
            Span::styled("Enter", Style::default().fg(theme.hint)),
            Span::raw(" save  "),
            Span::styled("Esc", Style::default().fg(theme.hint)),
            Span::raw(" cancel  "),
            Span::raw("clear group to ungroup"),
        ]);
        frame.render_widget(Paragraph::new(hint), chunks[6]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }

    fn shift_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::SHIFT)
    }

    #[test]
    fn test_new_dialog() {
        let dialog = RenameDialog::new("Original Title", "work/frontend");
        assert_eq!(dialog.current_title, "Original Title");
        assert_eq!(dialog.current_group, "work/frontend");
        assert_eq!(dialog.new_title.value(), "");
        assert_eq!(dialog.new_group.value(), "work/frontend"); // Pre-populated with current group
        assert_eq!(dialog.focused_field, 0);
    }

    #[test]
    fn test_new_dialog_empty_group() {
        let dialog = RenameDialog::new("Title", "");
        assert_eq!(dialog.current_group, "");
    }

    #[test]
    fn test_esc_cancels() {
        let mut dialog = RenameDialog::new("Test", "group");
        let result = dialog.handle_key(key(KeyCode::Esc));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_enter_with_unchanged_fields_cancels() {
        let mut dialog = RenameDialog::new("Test", "group");
        // Title is empty, group is pre-populated but unchanged - should cancel
        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_enter_with_title_only_submits() {
        let mut dialog = RenameDialog::new("Old Title", "group");
        dialog.handle_key(key(KeyCode::Char('N')));
        dialog.handle_key(key(KeyCode::Char('e')));
        dialog.handle_key(key(KeyCode::Char('w')));

        let result = dialog.handle_key(key(KeyCode::Enter));
        match result {
            DialogResult::Submit(data) => {
                assert_eq!(data.title, "New");
                assert_eq!(data.group, None); // Group unchanged
            }
            _ => panic!("Expected Submit result"),
        }
    }

    #[test]
    fn test_enter_with_group_only_submits() {
        let mut dialog = RenameDialog::new("Title", "old-group");
        // Switch to group field and clear it
        dialog.handle_key(key(KeyCode::Tab));
        for _ in 0.."old-group".len() {
            dialog.handle_key(key(KeyCode::Backspace));
        }
        // Type new group
        for c in "new-group".chars() {
            dialog.handle_key(key(KeyCode::Char(c)));
        }

        let result = dialog.handle_key(key(KeyCode::Enter));
        match result {
            DialogResult::Submit(data) => {
                assert_eq!(data.title, ""); // Title unchanged
                assert_eq!(data.group, Some("new-group".to_string()));
            }
            _ => panic!("Expected Submit result"),
        }
    }

    #[test]
    fn test_enter_with_both_fields_submits() {
        let mut dialog = RenameDialog::new("Old Title", "old-group");
        // Type title
        for c in "New Title".chars() {
            dialog.handle_key(key(KeyCode::Char(c)));
        }
        // Switch to group field and clear it
        dialog.handle_key(key(KeyCode::Tab));
        for _ in 0.."old-group".len() {
            dialog.handle_key(key(KeyCode::Backspace));
        }
        // Type new group
        for c in "new-group".chars() {
            dialog.handle_key(key(KeyCode::Char(c)));
        }

        let result = dialog.handle_key(key(KeyCode::Enter));
        match result {
            DialogResult::Submit(data) => {
                assert_eq!(data.title, "New Title");
                assert_eq!(data.group, Some("new-group".to_string()));
            }
            _ => panic!("Expected Submit result"),
        }
    }

    #[test]
    fn test_clearing_group_removes_from_group() {
        let mut dialog = RenameDialog::new("Title", "some-group");
        // Switch to group field and clear it
        dialog.handle_key(key(KeyCode::Tab));
        // Clear the pre-populated value
        for _ in 0.."some-group".len() {
            dialog.handle_key(key(KeyCode::Backspace));
        }

        let result = dialog.handle_key(key(KeyCode::Enter));
        match result {
            DialogResult::Submit(data) => {
                assert_eq!(data.title, "");
                assert_eq!(data.group, Some(String::new())); // Empty string means ungroup
            }
            _ => panic!("Expected Submit result"),
        }
    }

    #[test]
    fn test_tab_switches_fields() {
        let mut dialog = RenameDialog::new("Test", "group");
        assert_eq!(dialog.focused_field, 0);

        dialog.handle_key(key(KeyCode::Tab));
        assert_eq!(dialog.focused_field, 1);

        dialog.handle_key(key(KeyCode::Tab));
        assert_eq!(dialog.focused_field, 0);
    }

    #[test]
    fn test_shift_tab_switches_fields_backwards() {
        let mut dialog = RenameDialog::new("Test", "group");
        assert_eq!(dialog.focused_field, 0);

        dialog.handle_key(shift_key(KeyCode::Tab));
        assert_eq!(dialog.focused_field, 1);

        dialog.handle_key(shift_key(KeyCode::Tab));
        assert_eq!(dialog.focused_field, 0);
    }

    #[test]
    fn test_down_switches_to_next_field() {
        let mut dialog = RenameDialog::new("Test", "group");
        assert_eq!(dialog.focused_field, 0);

        dialog.handle_key(key(KeyCode::Down));
        assert_eq!(dialog.focused_field, 1);
    }

    #[test]
    fn test_up_switches_to_previous_field() {
        let mut dialog = RenameDialog::new("Test", "group");
        dialog.focused_field = 1;

        dialog.handle_key(key(KeyCode::Up));
        assert_eq!(dialog.focused_field, 0);
    }

    #[test]
    fn test_char_input_goes_to_focused_field() {
        let mut dialog = RenameDialog::new("Test", "group");

        // Type in title field
        dialog.handle_key(key(KeyCode::Char('a')));
        assert_eq!(dialog.new_title.value(), "a");
        assert_eq!(dialog.new_group.value(), "group"); // Pre-populated

        // Switch to group and type (appends to pre-populated value)
        dialog.handle_key(key(KeyCode::Tab));
        dialog.handle_key(key(KeyCode::Char('b')));
        assert_eq!(dialog.new_title.value(), "a");
        assert_eq!(dialog.new_group.value(), "groupb");
    }

    #[test]
    fn test_backspace_removes_char_from_focused_field() {
        let mut dialog = RenameDialog::new("Test", "group");
        dialog.handle_key(key(KeyCode::Char('a')));
        dialog.handle_key(key(KeyCode::Char('b')));
        dialog.handle_key(key(KeyCode::Char('c')));

        dialog.handle_key(key(KeyCode::Backspace));
        assert_eq!(dialog.new_title.value(), "ab");
    }

    #[test]
    fn test_current_values_preserved() {
        let mut dialog = RenameDialog::new("Original", "original-group");
        dialog.handle_key(key(KeyCode::Char('N')));
        dialog.handle_key(key(KeyCode::Char('e')));
        dialog.handle_key(key(KeyCode::Char('w')));

        assert_eq!(dialog.current_title, "Original");
        assert_eq!(dialog.current_group, "original-group");
        assert_eq!(dialog.new_title.value(), "New");
    }

    #[test]
    fn test_full_workflow_type_both_and_submit() {
        let mut dialog = RenameDialog::new("Old Name", "old/group");

        // Type new title
        for c in "Renamed Project".chars() {
            dialog.handle_key(key(KeyCode::Char(c)));
        }

        // Switch to group and clear it, then type new group
        dialog.handle_key(key(KeyCode::Tab));
        for _ in 0.."old/group".len() {
            dialog.handle_key(key(KeyCode::Backspace));
        }
        for c in "new/group".chars() {
            dialog.handle_key(key(KeyCode::Char(c)));
        }

        let result = dialog.handle_key(key(KeyCode::Enter));
        match result {
            DialogResult::Submit(data) => {
                assert_eq!(data.title, "Renamed Project");
                assert_eq!(data.group, Some("new/group".to_string()));
            }
            _ => panic!("Expected Submit"),
        }
    }

    #[test]
    fn test_full_workflow_type_and_cancel() {
        let mut dialog = RenameDialog::new("Old Name", "group");

        dialog.handle_key(key(KeyCode::Char('N')));
        dialog.handle_key(key(KeyCode::Char('e')));
        dialog.handle_key(key(KeyCode::Char('w')));

        let result = dialog.handle_key(key(KeyCode::Esc));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_whitespace_is_trimmed() {
        let mut dialog = RenameDialog::new("Test", "group");
        for c in "  New Title  ".chars() {
            dialog.handle_key(key(KeyCode::Char(c)));
        }
        dialog.handle_key(key(KeyCode::Tab));
        // Clear pre-populated value first
        for _ in 0.."group".len() {
            dialog.handle_key(key(KeyCode::Backspace));
        }
        for c in "  new-group  ".chars() {
            dialog.handle_key(key(KeyCode::Char(c)));
        }

        let result = dialog.handle_key(key(KeyCode::Enter));
        match result {
            DialogResult::Submit(data) => {
                assert_eq!(data.title, "New Title");
                assert_eq!(data.group, Some("new-group".to_string()));
            }
            _ => panic!("Expected Submit"),
        }
    }

    #[test]
    fn test_left_right_arrow_moves_cursor_in_input() {
        let mut dialog = RenameDialog::new("Test", "group");
        dialog.handle_key(key(KeyCode::Char('a')));
        dialog.handle_key(key(KeyCode::Char('b')));
        dialog.handle_key(key(KeyCode::Char('c')));

        // Move cursor left and insert
        dialog.handle_key(key(KeyCode::Left));
        dialog.handle_key(key(KeyCode::Char('X')));

        assert_eq!(dialog.new_title.value(), "abXc");
    }
}
