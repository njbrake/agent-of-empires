//! Send message dialog with multi-line text area

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::*;
use ratatui_textarea::TextArea;

use super::DialogResult;
use crate::tui::responsive;
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
            // Shift+Enter inserts a newline.
            // Most terminals send Shift+Enter as ESC + CR (\x1b\r), which crossterm
            // decodes as Alt+Enter, so we accept both ALT and SHIFT modifiers.
            KeyCode::Enter
                if key.modifiers.contains(KeyModifiers::SHIFT)
                    || key.modifiers.contains(KeyModifiers::ALT) =>
            {
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

    pub fn handle_paste(&mut self, text: &str) {
        self.text_area.insert_str(text);
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        // 2 for borders + 1 per content line, min 3 (single line), max 12,
        // capped to viewport so the popover never paints under the iOS soft
        // keyboard if Event::Resize lands mid-render.
        let content_lines = self.text_area.lines().len() as u16;
        let height = (content_lines + 2).clamp(3, 12).min(area.height.max(3));
        let dialog_width = responsive::dialog_width(area.width);
        let dialog_area = super::centered_rect(area, dialog_width, height);

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.accent))
            .title(format!(" > {} ", self.session_title))
            .title_style(Style::default().fg(theme.accent).bold())
            .title_bottom(
                Line::from(vec![
                    Span::styled(" Enter", Style::default().fg(theme.accent)),
                    Span::styled(" send ", Style::default().fg(theme.dimmed)),
                    Span::styled("Esc", Style::default().fg(theme.accent)),
                    Span::styled(" cancel ", Style::default().fg(theme.dimmed)),
                ])
                .right_aligned(),
            );

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let mut text_area_clone = self.text_area.clone();
        text_area_clone.set_style(Style::default().fg(theme.text));
        text_area_clone.set_cursor_style(Style::default().fg(theme.background).bg(theme.accent));

        frame.render_widget(&text_area_clone, inner);
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

    fn alt_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::ALT)
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
    fn test_alt_enter_adds_newline() {
        let mut dialog = SendMessageDialog::new("Test Session");
        dialog.handle_key(key(KeyCode::Char('a')));
        let result = dialog.handle_key(alt_key(KeyCode::Enter));
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

    #[test]
    fn test_paste_single_line() {
        let mut dialog = SendMessageDialog::new("Test Session");
        dialog.handle_paste("hello world");
        assert_eq!(dialog.get_text(), "hello world");
    }

    #[test]
    fn test_paste_multiline() {
        let mut dialog = SendMessageDialog::new("Test Session");
        dialog.handle_paste("line1\nline2\nline3");
        assert_eq!(dialog.get_text(), "line1\nline2\nline3");
    }

    #[test]
    fn test_paste_then_submit() {
        let mut dialog = SendMessageDialog::new("Test Session");
        dialog.handle_paste("pasted text");
        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, DialogResult::Submit(ref s) if s == "pasted text"));
    }

    #[test]
    fn test_paste_appends_to_existing() {
        let mut dialog = SendMessageDialog::new("Test Session");
        dialog.handle_key(key(KeyCode::Char('h')));
        dialog.handle_key(key(KeyCode::Char('i')));
        dialog.handle_key(key(KeyCode::Char(' ')));
        dialog.handle_paste("world");
        assert_eq!(dialog.get_text(), "hi world");
    }
}
