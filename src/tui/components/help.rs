//! Help overlay component

use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::session::config::SortOrder;
use crate::tui::styles::Theme;

const DIALOG_WIDTH: u16 = 50;
const DIALOG_HEIGHT: u16 = 40;
#[cfg(test)]
const BORDER_HEIGHT: u16 = 2;
#[cfg(test)]
const BORDER_WIDTH: u16 = 2;
#[cfg(test)]
const KEY_COLUMN_WIDTH: usize = 12; // 2 spaces indent + 10 chars for key

fn shortcuts(strict: bool) -> Vec<(&'static str, Vec<(&'static str, &'static str)>)> {
    if strict {
        vec![
            (
                "Navigation",
                vec![
                    ("j/↓", "Move down"),
                    ("k/↑", "Move up"),
                    ("h/←", "Collapse group"),
                    ("l/→", "Expand group"),
                    ("Home/End", "Go to top / bottom"),
                    ("PgUp/Dn", "Move 10 items up / down"),
                    ("w", "Next waiting/idle session"),
                ],
            ),
            (
                "Actions (strict mode)",
                vec![
                    ("Enter", "Attach to session"),
                    ("Ctrl+T", "Attach to terminal"),
                    ("N", "New session"),
                    ("Ctrl+N", "New from selection"),
                    ("X", "Stop session"),
                    ("D", "Delete session/group"),
                    ("R", "Rename session/group"),
                    ("M", "Send message to agent"),
                ],
            ),
            (
                "Views",
                vec![
                    ("T", "Toggle Agent/Terminal view"),
                    ("C", "Toggle container/host (sandbox)"),
                    ("Ctrl+D", "Diff view (git changes)"),
                    ("H/L", "Resize list panel"),
                    ("O", "Cycle sort forward"),
                    ("Ctrl+O", "Cycle sort backward"),
                    ("Ctrl+G", "Toggle group by project"),
                ],
            ),
            (
                "Other",
                vec![
                    ("/", "Search"),
                    ("n/N", "Next/prev match"),
                    ("S", "Settings"),
                    ("P", "Profiles"),
                    ("Ctrl+R", "Serve (LAN / Tunnel)"),
                    ("Shift+drag", "Select text in preview"),
                    ("?", "Toggle help"),
                    ("Q", "Quit"),
                ],
            ),
        ]
    } else {
        vec![
            (
                "Navigation",
                vec![
                    ("j/↓", "Move down"),
                    ("k/↑", "Move up"),
                    ("h/←", "Collapse group"),
                    ("l/→", "Expand group"),
                    ("Home/End", "Go to top / bottom"),
                    ("PgUp/Dn", "Move 10 items up / down"),
                    ("w", "Next waiting/idle session"),
                ],
            ),
            (
                "Actions",
                vec![
                    ("Enter", "Attach to session"),
                    ("T", "Attach to terminal"),
                    ("n", "New session"),
                    ("N", "New from selection"),
                    ("x", "Stop session"),
                    ("d", "Delete session/group"),
                    ("r", "Rename session/group"),
                    ("m", "Send message to agent"),
                ],
            ),
            (
                "Views",
                vec![
                    ("t", "Toggle Agent/Terminal view"),
                    ("c", "Toggle container/host (sandbox)"),
                    ("D", "Diff view (git changes)"),
                    ("H/L", "Resize list panel"),
                    ("o", "Cycle sort forward"),
                    ("Ctrl+o", "Cycle sort backward"),
                    ("g", "Toggle group by project"),
                ],
            ),
            (
                "Other",
                vec![
                    ("/", "Search"),
                    ("n/N", "Next/prev match"),
                    ("s", "Settings"),
                    ("P", "Profiles"),
                    ("R", "Serve (LAN / Tunnel)"),
                    ("Shift+drag", "Select text in preview"),
                    ("?", "Toggle help"),
                    ("q", "Quit"),
                ],
            ),
        ]
    }
}

#[cfg(test)]
fn content_line_count(strict: bool) -> usize {
    let sections = shortcuts(strict);
    let last_idx = sections.len().saturating_sub(1);
    let mut count = 0;
    for (idx, (section, keys)) in sections.iter().enumerate() {
        count += 1; // section header
        count += keys.len(); // shortcut lines

        // Add extra line for sort label after Views section
        if *section == "Views" {
            count += 1;
        }

        if idx != last_idx {
            count += 1; // blank separator between sections
        }
    }
    count
}

pub struct HelpOverlay;

impl HelpOverlay {
    pub fn render(
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
        sort_order: SortOrder,
        strict_hotkeys: bool,
    ) {
        let x = area.x + (area.width.saturating_sub(DIALOG_WIDTH)) / 2;
        let y = area.y + (area.height.saturating_sub(DIALOG_HEIGHT)) / 2;

        let dialog_area = Rect {
            x,
            y,
            width: DIALOG_WIDTH.min(area.width),
            height: DIALOG_HEIGHT.min(area.height),
        };

        frame.render_widget(Clear, dialog_area);

        let version = format!(" v{} ", env!("CARGO_PKG_VERSION"));
        let block = Block::default()
            .style(Style::default().bg(theme.background))
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.border))
            .title(Line::styled(
                " Keyboard Shortcuts ",
                Style::default().fg(theme.title).bold(),
            ))
            .title_bottom(Line::styled(version, Style::default().fg(theme.dimmed)).right_aligned());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let mut lines: Vec<Line> = Vec::new();
        let sort_label = format!("(current sort: {})", sort_order.label());

        let sections = shortcuts(strict_hotkeys);
        let last_idx = sections.len().saturating_sub(1);
        for (idx, (section, keys)) in sections.iter().enumerate() {
            lines.push(Line::from(Span::styled(
                *section,
                Style::default().fg(theme.accent).bold(),
            )));
            for (key, desc) in keys {
                lines.push(Line::from(vec![
                    Span::styled(format!("  {:10}", key), Style::default().fg(theme.waiting)),
                    Span::styled(*desc, Style::default().fg(theme.text)),
                ]));
            }

            // Add sort label after "Views" section
            if *section == "Views" {
                lines.push(Line::from(vec![
                    Span::styled(format!("  {:10}", ""), Style::default().fg(theme.waiting)),
                    Span::styled(sort_label.as_str(), Style::default().fg(theme.text)),
                ]));
            }

            if idx != last_idx {
                lines.push(Line::from(""));
            }
        }

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_contains_resize_shortcut() {
        for strict in [false, true] {
            let all = shortcuts(strict);
            let views_section = all.iter().find(|(name, _)| *name == "Views");
            assert!(views_section.is_some(), "Views section should exist");
            let (_, keys) = views_section.unwrap();
            assert!(
                keys.iter().any(|(k, _)| *k == "H/L"),
                "Views section should contain H/L resize shortcut"
            );
        }
    }

    #[test]
    fn help_content_fits_in_dialog() {
        let available_height = (DIALOG_HEIGHT - BORDER_HEIGHT) as usize;
        let available_width = (DIALOG_WIDTH - BORDER_WIDTH) as usize;
        for strict in [false, true] {
            let content_lines = content_line_count(strict);
            assert!(
                content_lines <= available_height,
                "Help content ({content_lines} lines, strict={strict}) exceeds dialog inner height ({available_height} lines)"
            );
            for (section, keys) in shortcuts(strict) {
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
}
