//! Rename session dialog

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::*;

use super::DialogResult;
use crate::tui::styles::Theme;

pub struct RenameDialog {
    current_title: String,
    new_title: String,
}

impl RenameDialog {
    pub fn new(current_title: &str) -> Self {
        Self {
            current_title: current_title.to_string(),
            new_title: String::new(),
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult<String> {
        match key.code {
            KeyCode::Esc => DialogResult::Cancel,
            KeyCode::Enter => {
                if self.new_title.is_empty() {
                    DialogResult::Cancel
                } else {
                    DialogResult::Submit(self.new_title.clone())
                }
            }
            KeyCode::Backspace => {
                self.new_title.pop();
                DialogResult::Continue
            }
            KeyCode::Char(c) => {
                self.new_title.push(c);
                DialogResult::Continue
            }
            _ => DialogResult::Continue,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_width = 50;
        let dialog_height = 8;
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
            .title(" Rename Session ")
            .title_style(Style::default().fg(theme.title).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(2),
                Constraint::Length(2),
                Constraint::Min(1),
            ])
            .split(inner);

        let current_line = Line::from(vec![
            Span::styled("Current: ", Style::default().fg(theme.dimmed)),
            Span::styled(&self.current_title, Style::default().fg(theme.text)),
        ]);
        frame.render_widget(Paragraph::new(current_line), chunks[0]);

        let new_line = Line::from(vec![
            Span::styled("New: ", Style::default().fg(theme.dimmed)),
            Span::styled(&self.new_title, Style::default().fg(theme.accent)),
            Span::styled("█", Style::default().fg(theme.accent)),
        ]);
        frame.render_widget(Paragraph::new(new_line), chunks[1]);

        let hint = Line::from(vec![
            Span::styled("Enter", Style::default().fg(theme.hint)),
            Span::raw(" rename  "),
            Span::styled("Esc", Style::default().fg(theme.hint)),
            Span::raw(" cancel"),
        ]);
        frame.render_widget(Paragraph::new(hint), chunks[2]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }

    #[test]
    fn test_new_dialog() {
        let dialog = RenameDialog::new("Original Title");
        assert_eq!(dialog.current_title, "Original Title");
        assert_eq!(dialog.new_title, "");
    }

    #[test]
    fn test_esc_cancels() {
        let mut dialog = RenameDialog::new("Test");
        let result = dialog.handle_key(key(KeyCode::Esc));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_enter_with_empty_title_cancels() {
        let mut dialog = RenameDialog::new("Test");
        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_enter_with_text_submits() {
        let mut dialog = RenameDialog::new("Old Title");
        dialog.handle_key(key(KeyCode::Char('N')));
        dialog.handle_key(key(KeyCode::Char('e')));
        dialog.handle_key(key(KeyCode::Char('w')));

        let result = dialog.handle_key(key(KeyCode::Enter));
        match result {
            DialogResult::Submit(title) => assert_eq!(title, "New"),
            _ => panic!("Expected Submit result"),
        }
    }

    #[test]
    fn test_char_input() {
        let mut dialog = RenameDialog::new("Test");
        let result = dialog.handle_key(key(KeyCode::Char('a')));
        assert!(matches!(result, DialogResult::Continue));
        assert_eq!(dialog.new_title, "a");
    }

    #[test]
    fn test_multiple_char_input() {
        let mut dialog = RenameDialog::new("Test");
        dialog.handle_key(key(KeyCode::Char('H')));
        dialog.handle_key(key(KeyCode::Char('e')));
        dialog.handle_key(key(KeyCode::Char('l')));
        dialog.handle_key(key(KeyCode::Char('l')));
        dialog.handle_key(key(KeyCode::Char('o')));

        assert_eq!(dialog.new_title, "Hello");
    }

    #[test]
    fn test_backspace_removes_char() {
        let mut dialog = RenameDialog::new("Test");
        dialog.handle_key(key(KeyCode::Char('a')));
        dialog.handle_key(key(KeyCode::Char('b')));
        dialog.handle_key(key(KeyCode::Char('c')));

        let result = dialog.handle_key(key(KeyCode::Backspace));
        assert!(matches!(result, DialogResult::Continue));
        assert_eq!(dialog.new_title, "ab");
    }

    #[test]
    fn test_backspace_on_empty_title() {
        let mut dialog = RenameDialog::new("Test");
        let result = dialog.handle_key(key(KeyCode::Backspace));
        assert!(matches!(result, DialogResult::Continue));
        assert_eq!(dialog.new_title, "");
    }

    #[test]
    fn test_unknown_key_continues() {
        let mut dialog = RenameDialog::new("Test");
        let result = dialog.handle_key(key(KeyCode::Tab));
        assert!(matches!(result, DialogResult::Continue));
        assert_eq!(dialog.new_title, "");
    }

    #[test]
    fn test_arrow_keys_continue() {
        let mut dialog = RenameDialog::new("Test");
        assert!(matches!(
            dialog.handle_key(key(KeyCode::Up)),
            DialogResult::Continue
        ));
        assert!(matches!(
            dialog.handle_key(key(KeyCode::Down)),
            DialogResult::Continue
        ));
        assert!(matches!(
            dialog.handle_key(key(KeyCode::Left)),
            DialogResult::Continue
        ));
        assert!(matches!(
            dialog.handle_key(key(KeyCode::Right)),
            DialogResult::Continue
        ));
    }

    #[test]
    fn test_special_characters_in_title() {
        let mut dialog = RenameDialog::new("Test");
        dialog.handle_key(key(KeyCode::Char('T')));
        dialog.handle_key(key(KeyCode::Char('e')));
        dialog.handle_key(key(KeyCode::Char('s')));
        dialog.handle_key(key(KeyCode::Char('t')));
        dialog.handle_key(key(KeyCode::Char(' ')));
        dialog.handle_key(key(KeyCode::Char('1')));
        dialog.handle_key(key(KeyCode::Char('2')));
        dialog.handle_key(key(KeyCode::Char('3')));

        assert_eq!(dialog.new_title, "Test 123");
    }

    #[test]
    fn test_current_title_preserved() {
        let mut dialog = RenameDialog::new("Original");
        dialog.handle_key(key(KeyCode::Char('N')));
        dialog.handle_key(key(KeyCode::Char('e')));
        dialog.handle_key(key(KeyCode::Char('w')));

        // Original title should be preserved
        assert_eq!(dialog.current_title, "Original");
        assert_eq!(dialog.new_title, "New");
    }

    #[test]
    fn test_full_workflow_type_and_submit() {
        let mut dialog = RenameDialog::new("Old Name");

        // Type new name
        for c in "Renamed Project".chars() {
            dialog.handle_key(key(KeyCode::Char(c)));
        }

        // Submit
        let result = dialog.handle_key(key(KeyCode::Enter));
        match result {
            DialogResult::Submit(title) => {
                assert_eq!(title, "Renamed Project");
            }
            _ => panic!("Expected Submit"),
        }
    }

    #[test]
    fn test_full_workflow_type_and_cancel() {
        let mut dialog = RenameDialog::new("Old Name");

        // Type new name
        dialog.handle_key(key(KeyCode::Char('N')));
        dialog.handle_key(key(KeyCode::Char('e')));
        dialog.handle_key(key(KeyCode::Char('w')));

        // Cancel with Esc
        let result = dialog.handle_key(key(KeyCode::Esc));
        assert!(matches!(result, DialogResult::Cancel));
    }
}
