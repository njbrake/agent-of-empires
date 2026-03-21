//! Send message dialog with multi-line text area

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::*;
use tui_textarea::TextArea;

use super::DialogResult;
use crate::tui::styles::Theme;

pub struct SendMessageDialog {
    session_title: String,
    text_area: TextArea<'static>,
}

impl SendMessageDialog {
    pub fn new(session_title: &str) -> Self {
        let mut text_area = TextArea::new(vec![String::new()]);
        text_area.set_cursor_line_style(Style::default());

        Self {
            session_title: session_title.to_string(),
            text_area,
        }
    }

    fn get_text(&self) -> String {
        self.text_area.lines().join("\n")
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult<String> {
        match key.code {
            KeyCode::Esc => DialogResult::Cancel,
            // Shift+Enter inserts a newline
            KeyCode::Enter if key.modifiers.contains(KeyModifiers::SHIFT) => {
                self.text_area.insert_newline();
                DialogResult::Continue
            }
            // Plain Enter sends
            KeyCode::Enter => {
                let value = self.get_text().trim().to_string();
                if value.is_empty() {
                    DialogResult::Cancel
                } else {
                    DialogResult::Submit(value)
                }
            }
            _ => {
                self.text_area.input(key);
                DialogResult::Continue
            }
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_area = super::centered_rect(area, 70, 12);

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(format!(" {} ", self.session_title))
            .title_style(Style::default().fg(theme.title).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(3),
                Constraint::Length(1),
            ])
            .split(inner);

        // Prompt line styled like a terminal
        let prompt = Line::from(vec![
            Span::styled(" > ", Style::default().fg(theme.accent).bold()),
            Span::styled(
                "Type a message for the agent",
                Style::default().fg(theme.dimmed),
            ),
        ]);
        frame.render_widget(Paragraph::new(prompt), chunks[0]);

        // Text area without extra border, just indented to align with prompt
        let textarea_block = Block::default().padding(ratatui::widgets::Padding::horizontal(1));

        let mut text_area_clone = self.text_area.clone();
        text_area_clone.set_block(textarea_block);
        text_area_clone.set_style(Style::default().fg(theme.text));
        text_area_clone.set_cursor_style(Style::default().fg(theme.background).bg(theme.accent));

        frame.render_widget(&text_area_clone, chunks[1]);

        // Hint bar
        let hint = Line::from(vec![
            Span::styled(" Enter", Style::default().fg(theme.accent)),
            Span::styled(" send  ", Style::default().fg(theme.dimmed)),
            Span::styled("Shift+Enter", Style::default().fg(theme.accent)),
            Span::styled(" newline  ", Style::default().fg(theme.dimmed)),
            Span::styled("Esc", Style::default().fg(theme.accent)),
            Span::styled(" cancel", Style::default().fg(theme.dimmed)),
        ]);
        frame.render_widget(Paragraph::new(hint), chunks[2]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn shift_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::SHIFT)
    }

    #[test]
    fn test_esc_cancels() {
        let mut dialog = SendMessageDialog::new("Test Session");
        let result = dialog.handle_key(key(KeyCode::Esc));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_enter_on_empty_cancels() {
        let mut dialog = SendMessageDialog::new("Test Session");
        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_enter_with_text_submits() {
        let mut dialog = SendMessageDialog::new("Test Session");
        dialog.handle_key(key(KeyCode::Char('h')));
        dialog.handle_key(key(KeyCode::Char('i')));
        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, DialogResult::Submit(ref s) if s == "hi"));
    }

    #[test]
    fn test_typing_continues() {
        let mut dialog = SendMessageDialog::new("Test Session");
        let result = dialog.handle_key(key(KeyCode::Char('a')));
        assert!(matches!(result, DialogResult::Continue));
    }

    #[test]
    fn test_shift_enter_adds_newline() {
        let mut dialog = SendMessageDialog::new("Test Session");
        dialog.handle_key(key(KeyCode::Char('a')));
        let result = dialog.handle_key(shift_key(KeyCode::Enter));
        assert!(matches!(result, DialogResult::Continue));
        dialog.handle_key(key(KeyCode::Char('b')));
        assert_eq!(dialog.get_text(), "a\nb");
    }

    #[test]
    fn test_multiline_submit() {
        let mut dialog = SendMessageDialog::new("Test Session");
        dialog.handle_key(key(KeyCode::Char('l')));
        dialog.handle_key(key(KeyCode::Char('1')));
        dialog.handle_key(shift_key(KeyCode::Enter));
        dialog.handle_key(key(KeyCode::Char('l')));
        dialog.handle_key(key(KeyCode::Char('2')));
        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, DialogResult::Submit(ref s) if s == "l1\nl2"));
    }
}
