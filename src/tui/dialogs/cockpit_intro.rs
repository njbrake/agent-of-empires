//! One-time intro dialog announcing the cockpit feature on first run.
//!
//! Fires once after upgrade to a version that ships cockpit. Tracks
//! "user has seen this" via `AppStateConfig::has_seen_cockpit_intro`.

#![cfg(feature = "cockpit")]

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::*;

use super::DialogResult;
use crate::tui::styles::Theme;

#[derive(Default)]
pub struct CockpitIntroDialog;

impl CockpitIntroDialog {
    pub fn new() -> Self {
        Self
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult<()> {
        match key.code {
            KeyCode::Enter | KeyCode::Esc | KeyCode::Char(' ') | KeyCode::Char('q') => {
                DialogResult::Submit(())
            }
            _ => DialogResult::Continue,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_area = super::centered_rect(area, 70, 18);
        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.accent))
            .title(" New: Cockpit (Native Agent Rendering) ")
            .title_style(Style::default().fg(theme.accent).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([Constraint::Min(1), Constraint::Length(2)])
            .split(inner);

        let lines = vec![
            Line::from("aoe can now render structured agent state natively"),
            Line::from("(plan, tool calls, diffs, approvals) instead of"),
            Line::from("piping the agent through a tmux pane."),
            Line::from(""),
            Line::from(vec![
                Span::styled("Try it: ", Style::default().fg(theme.title).bold()),
                Span::styled(
                    "aoe add . --cmd claude --cockpit",
                    Style::default().fg(theme.text),
                ),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Verify: ", Style::default().fg(theme.title).bold()),
                Span::styled("aoe cockpit doctor", Style::default().fg(theme.text)),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("Docs:   ", Style::default().fg(theme.title).bold()),
                Span::styled(
                    "agent-of-empires.com/docs/cockpit",
                    Style::default().fg(theme.text),
                ),
            ]),
            Line::from(""),
            Line::from(vec![Span::styled(
                "Cockpit needs Node.js >= 20. Existing tmux sessions are unaffected.",
                Style::default().fg(theme.dimmed),
            )]),
        ];

        let para = Paragraph::new(lines)
            .style(Style::default().fg(theme.text))
            .wrap(Wrap { trim: false });
        frame.render_widget(para, chunks[0]);

        let footer = Paragraph::new(Line::from(vec![
            Span::styled("Enter / Esc / q ", Style::default().fg(theme.title).bold()),
            Span::styled("to dismiss", Style::default().fg(theme.dimmed)),
        ]))
        .alignment(Alignment::Center);
        frame.render_widget(footer, chunks[1]);
    }
}
