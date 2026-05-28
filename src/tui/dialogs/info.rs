//! Info dialog for displaying informational messages

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::*;

use super::DialogResult;
use crate::tui::styles::Theme;

pub struct InfoDialog {
    title: String,
    message: String,
    width: u16,
    height: u16,
    /// Rect of the rendered `[OK]` button, captured during `render`.
    /// Lets `handle_click` accept either a click on the button OR a
    /// click anywhere on the dialog area (the latter is a quick
    /// "dismiss" gesture matching the keyboard's bare-Space behavior).
    ok_button_area: Rect,
    dialog_area: Rect,
}

impl InfoDialog {
    pub fn new(title: &str, message: &str) -> Self {
        Self {
            title: title.to_string(),
            message: message.to_string(),
            width: 50,
            height: 9,
            ok_button_area: Rect::default(),
            dialog_area: Rect::default(),
        }
    }

    /// A left-click anywhere inside the info dialog dismisses it,
    /// matching the keyboard's "any of Esc/Enter/Space closes" model.
    /// `None` when the click landed outside the dialog area, so the
    /// caller can decide whether to swallow it anyway.
    pub fn handle_click(&self, col: u16, row: u16) -> Option<DialogResult<()>> {
        if self
            .dialog_area
            .contains(ratatui::layout::Position::from((col, row)))
        {
            Some(DialogResult::Cancel)
        } else {
            None
        }
    }

    /// Customize the dialog's footprint. Useful for long, multi-paragraph
    /// messages (e.g. the startup config-warning) that would clip at the
    /// default 50x9.
    pub fn with_size(mut self, width: u16, height: u16) -> Self {
        self.width = width;
        self.height = height;
        self
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Char(' ') => DialogResult::Cancel,
            _ => DialogResult::Continue,
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_area = super::centered_rect(area, self.width, self.height);
        self.dialog_area = dialog_area;

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.border))
            .title(format!(" {} ", self.title))
            .title_style(Style::default().fg(theme.title).bold());

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

        // OK button. Centered inside the bottom chunk; the rect
        // tracks where the glyph actually lands so a click on the
        // button is targeted (not just "click anywhere to dismiss").
        let button = Line::from(vec![Span::styled(
            "[OK]",
            Style::default().fg(theme.accent).bold(),
        )]);
        let button_area = chunks[1];
        if button_area.width >= 4 {
            let left_pad = (button_area.width - 4) / 2;
            self.ok_button_area = Rect::new(button_area.x + left_pad, button_area.y, 4, 1);
        } else {
            self.ok_button_area = Rect::default();
        }

        frame.render_widget(
            Paragraph::new(button).alignment(Alignment::Center),
            button_area,
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
    fn test_esc_closes() {
        let mut dialog = InfoDialog::new("Test", "Message");
        let result = dialog.handle_key(key(KeyCode::Esc));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_enter_closes() {
        let mut dialog = InfoDialog::new("Test", "Message");
        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_space_closes() {
        let mut dialog = InfoDialog::new("Test", "Message");
        let result = dialog.handle_key(key(KeyCode::Char(' ')));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_other_keys_continue() {
        let mut dialog = InfoDialog::new("Test", "Message");
        let result = dialog.handle_key(key(KeyCode::Char('x')));
        assert!(matches!(result, DialogResult::Continue));
    }
}
