//! Confirmation dialog

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::*;

use super::DialogResult;
use crate::tui::styles::Theme;

pub struct ConfirmDialog {
    title: String,
    message: String,
    action: String,
    selected: bool, // true = Yes, false = No
}

impl ConfirmDialog {
    pub fn new(title: &str, message: &str, action: &str) -> Self {
        Self {
            title: title.to_string(),
            message: message.to_string(),
            action: action.to_string(),
            selected: false,
        }
    }

    pub fn action(&self) -> &str {
        &self.action
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => DialogResult::Cancel,
            KeyCode::Enter => {
                if self.selected {
                    DialogResult::Submit(())
                } else {
                    DialogResult::Cancel
                }
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => DialogResult::Submit(()),
            KeyCode::Left | KeyCode::Char('h') => {
                self.selected = true;
                DialogResult::Continue
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.selected = false;
                DialogResult::Continue
            }
            KeyCode::Tab => {
                self.selected = !self.selected;
                DialogResult::Continue
            }
            _ => DialogResult::Continue,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_area = super::centered_rect(area, 50, 8);

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.error))
            .title(format!(" {} ", self.title))
            .title_style(Style::default().fg(theme.error).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([Constraint::Min(1), Constraint::Length(2)])
            .split(inner);

        // Message
        let message = Paragraph::new(&*self.message)
            .style(Style::default().fg(theme.text))
            .wrap(Wrap { trim: true });
        frame.render_widget(message, chunks[0]);

        // Buttons
        let yes_style = if self.selected {
            Style::default().fg(theme.error).bold()
        } else {
            Style::default().fg(theme.dimmed)
        };
        let no_style = if !self.selected {
            Style::default().fg(theme.running).bold()
        } else {
            Style::default().fg(theme.dimmed)
        };

        let buttons = Line::from(vec![
            Span::raw("  "),
            Span::styled("[Yes]", yes_style),
            Span::raw("    "),
            Span::styled("[No]", no_style),
        ]);

        frame.render_widget(
            Paragraph::new(buttons).alignment(Alignment::Center),
            chunks[1],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn test_default_selection_is_no() {
        let dialog = ConfirmDialog::new("Test", "Are you sure?", "test_action");
        assert!(!dialog.selected);
    }

    #[test]
    fn test_action_accessor() {
        let dialog = ConfirmDialog::new("Title", "Message", "delete");
        assert_eq!(dialog.action(), "delete");
    }

    #[test]
    fn test_esc_cancels() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        let result = dialog.handle_key(key(KeyCode::Esc));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_n_cancels() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        let result = dialog.handle_key(key(KeyCode::Char('n')));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_uppercase_n_cancels() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        let result = dialog.handle_key(key(KeyCode::Char('N')));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_y_confirms() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        let result = dialog.handle_key(key(KeyCode::Char('y')));
        assert!(matches!(result, DialogResult::Submit(())));
    }

    #[test]
    fn test_uppercase_y_confirms() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        let result = dialog.handle_key(key(KeyCode::Char('Y')));
        assert!(matches!(result, DialogResult::Submit(())));
    }

    #[test]
    fn test_enter_with_no_selected_cancels() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_enter_with_yes_selected_submits() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        dialog.selected = true;
        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, DialogResult::Submit(())));
    }

    #[test]
    fn test_tab_toggles_selection() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        assert!(!dialog.selected);

        dialog.handle_key(key(KeyCode::Tab));
        assert!(dialog.selected);

        dialog.handle_key(key(KeyCode::Tab));
        assert!(!dialog.selected);
    }

    #[test]
    fn test_left_selects_yes() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        dialog.handle_key(key(KeyCode::Left));
        assert!(dialog.selected);
    }

    #[test]
    fn test_right_selects_no() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        dialog.selected = true;
        dialog.handle_key(key(KeyCode::Right));
        assert!(!dialog.selected);
    }

    #[test]
    fn test_h_selects_yes() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        dialog.handle_key(key(KeyCode::Char('h')));
        assert!(dialog.selected);
    }

    #[test]
    fn test_l_selects_no() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        dialog.selected = true;
        dialog.handle_key(key(KeyCode::Char('l')));
        assert!(!dialog.selected);
    }

    #[test]
    fn test_unknown_key_continues() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        let result = dialog.handle_key(key(KeyCode::Char('x')));
        assert!(matches!(result, DialogResult::Continue));
    }
}
