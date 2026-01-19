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
