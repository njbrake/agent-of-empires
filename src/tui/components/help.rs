//! Help overlay component

use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::tui::styles::Theme;

pub struct HelpOverlay;

impl HelpOverlay {
    pub fn render(frame: &mut Frame, area: Rect, theme: &Theme) {
        // Center the help dialog
        let dialog_width = 50;
        let dialog_height = 22;
        let x = area.x + (area.width.saturating_sub(dialog_width)) / 2;
        let y = area.y + (area.height.saturating_sub(dialog_height)) / 2;

        let dialog_area = Rect {
            x,
            y,
            width: dialog_width.min(area.width),
            height: dialog_height.min(area.height),
        };

        // Clear the background
        let clear = Block::default().style(Style::default().bg(theme.background));
        frame.render_widget(clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(" Keyboard Shortcuts ")
            .title_style(Style::default().fg(theme.title).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let shortcuts = vec![
            (
                "Navigation",
                vec![
                    ("j/↓", "Move down"),
                    ("k/↑", "Move up"),
                    ("h/←", "Collapse group"),
                    ("l/→", "Expand group"),
                    ("g", "Go to top"),
                    ("G", "Go to bottom"),
                ],
            ),
            (
                "Actions",
                vec![
                    ("Enter", "Attach to session"),
                    ("n", "New session"),
                    ("d", "Delete session/group"),
                    ("r", "Rename session"),
                    ("f", "Fork session (Claude)"),
                ],
            ),
            (
                "Other",
                vec![("/", "Search"), ("?", "Toggle help"), ("q", "Quit")],
            ),
        ];

        let mut lines: Vec<Line> = Vec::new();

        for (section, keys) in shortcuts {
            lines.push(Line::from(Span::styled(
                section,
                Style::default().fg(theme.accent).bold(),
            )));
            for (key, desc) in keys {
                lines.push(Line::from(vec![
                    Span::styled(format!("  {:10}", key), Style::default().fg(theme.waiting)),
                    Span::styled(desc, Style::default().fg(theme.text)),
                ]));
            }
            lines.push(Line::from(""));
        }

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
    }
}
