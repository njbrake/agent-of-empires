//! Help overlay component

use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::tui::styles::Theme;

const DIALOG_WIDTH: u16 = 50;
const DIALOG_HEIGHT: u16 = 27;
#[cfg(test)]
const BORDER_HEIGHT: u16 = 2;
#[cfg(test)]
const BORDER_WIDTH: u16 = 2;
#[cfg(test)]
const KEY_COLUMN_WIDTH: usize = 12; // 2 spaces indent + 10 chars for key

fn shortcuts() -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    vec![
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
        ("Views", vec![("t", "Toggle Agent/Terminal view")]),
        (
            "Other",
            vec![
                ("/", "Search"),
                ("P", "Next profile"),
                ("?", "Toggle help"),
                ("q", "Quit"),
            ],
        ),
    ]
}

#[cfg(test)]
fn content_line_count() -> usize {
    let mut count = 0;
    for (_, keys) in shortcuts() {
        count += 1; // section header
        count += keys.len(); // shortcut lines
        count += 1; // empty line after section
    }
    count
}

pub struct HelpOverlay;

impl HelpOverlay {
    pub fn render(frame: &mut Frame, area: Rect, theme: &Theme) {
        let x = area.x + (area.width.saturating_sub(DIALOG_WIDTH)) / 2;
        let y = area.y + (area.height.saturating_sub(DIALOG_HEIGHT)) / 2;

        let dialog_area = Rect {
            x,
            y,
            width: DIALOG_WIDTH.min(area.width),
            height: DIALOG_HEIGHT.min(area.height),
        };

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .style(Style::default().bg(theme.background))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(" Keyboard Shortcuts ")
            .title_style(Style::default().fg(theme.title).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let mut lines: Vec<Line> = Vec::new();

        for (section, keys) in shortcuts() {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_content_fits_in_dialog() {
        let available_height = (DIALOG_HEIGHT - BORDER_HEIGHT) as usize;
        let content_lines = content_line_count();
        assert!(
            content_lines <= available_height,
            "Help content ({content_lines} lines) exceeds dialog inner height ({available_height} lines)"
        );

        let available_width = (DIALOG_WIDTH - BORDER_WIDTH) as usize;
        for (section, keys) in shortcuts() {
            assert!(
                section.len() <= available_width,
                "Section header '{section}' exceeds dialog width ({available_width} chars)"
            );
            for (key, desc) in keys {
                let line_width = KEY_COLUMN_WIDTH + desc.len();
                assert!(
                    line_width <= available_width,
                    "Shortcut '{key}' description '{desc}' exceeds dialog width ({line_width} > {available_width})"
                );
            }
        }
    }
}
