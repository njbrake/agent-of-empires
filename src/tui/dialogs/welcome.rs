//! Welcome dialog for first-time users

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::*;

use super::DialogResult;
use crate::tui::styles::Theme;

pub struct WelcomeDialog;

impl WelcomeDialog {
    pub fn new() -> Self {
        Self
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult<()> {
        match key.code {
            KeyCode::Enter | KeyCode::Esc | KeyCode::Char(' ') => DialogResult::Submit(()),
            _ => DialogResult::Continue,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_area = super::centered_rect(area, 64, 14);

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .title(" Welcome to Agent of Empires ")
            .title_style(Style::default().fg(theme.accent).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([Constraint::Min(1), Constraint::Length(2)])
            .split(inner);

        let content = vec![
            Line::from("When you attach or start an AOE session, "),
            Line::from("you're working directly in tmux."),
            Line::from("Essential tmux commands:"),
            Line::from(""),
            Line::from(vec![
                Span::styled(
                    "  Ctrl+b then d   ",
                    Style::default().fg(theme.title).bold(),
                ),
                Span::styled(
                    "Detach (exit without stopping)",
                    Style::default().fg(theme.text),
                ),
            ]),
            Line::from(vec![
                Span::styled(
                    "  Ctrl+b then [   ",
                    Style::default().fg(theme.title).bold(),
                ),
                Span::styled(
                    "Scroll mode (arrows to scroll, q to exit)",
                    Style::default().fg(theme.text),
                ),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "Note: Press Ctrl+b, release, THEN press the next key.",
                Style::default().fg(theme.hint).italic(),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Press ? anytime for full keyboard shortcuts.",
                Style::default().fg(theme.dimmed),
            )),
        ];

        let paragraph = Paragraph::new(content).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, chunks[0]);

        let button = vec![
            Line::from(vec![Span::styled(
                "[Get Started]",
                Style::default().fg(theme.accent).bold(),
            )]),
            Line::from(vec![Span::styled(
                "(press Enter to continue)",
                Style::default().fg(theme.accent).bold(),
            )]),
        ];
        frame.render_widget(
            Paragraph::new(button).alignment(Alignment::Center),
            chunks[1],
        );
    }
}

impl Default for WelcomeDialog {
    fn default() -> Self {
        Self::new()
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
    fn test_enter_submits() {
        let mut dialog = WelcomeDialog::new();
        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, DialogResult::Submit(())));
    }

    #[test]
    fn test_esc_submits() {
        let mut dialog = WelcomeDialog::new();
        let result = dialog.handle_key(key(KeyCode::Esc));
        assert!(matches!(result, DialogResult::Submit(())));
    }

    #[test]
    fn test_space_submits() {
        let mut dialog = WelcomeDialog::new();
        let result = dialog.handle_key(key(KeyCode::Char(' ')));
        assert!(matches!(result, DialogResult::Submit(())));
    }

    #[test]
    fn test_other_keys_continue() {
        let mut dialog = WelcomeDialog::new();
        let result = dialog.handle_key(key(KeyCode::Char('x')));
        assert!(matches!(result, DialogResult::Continue));
    }
}
