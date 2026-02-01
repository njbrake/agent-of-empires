//! Rendering for the diff view

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, List, ListItem, Padding, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState,
    },
    Frame,
};
use similar::ChangeTag;

use super::DiffView;
use crate::git::diff::FileStatus;
use crate::tui::styles::Theme;

/// Truncate a string from the left, adding an ellipsis prefix if it doesn't fit.
fn truncate_left(s: &str, max_width: usize) -> String {
    if s.len() <= max_width {
        return s.to_string();
    }
    if max_width <= 1 {
        return ".".to_string();
    }
    // "..." + tail of the string
    let tail_len = max_width.saturating_sub(1);
    let start = s.len() - tail_len;
    format!("\u{2026}{}", &s[start..])
}

impl DiffView {
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        // Clear the area
        frame.render_widget(Clear, area);

        // If branch select dialog is open, render it
        if self.branch_select.is_some() {
            self.render_with_branch_dialog(frame, area, theme);
            return;
        }

        // Main layout: header, content, footer
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Header
                Constraint::Min(10),   // Content
                Constraint::Length(3), // Footer
            ])
            .split(area);

        self.render_header(frame, layout[0], theme);
        self.render_content(frame, layout[1], theme);
        self.render_footer(frame, layout[2], theme);

        // Render help overlay if active
        if self.show_help {
            self.render_help(frame, area, theme);
        }

        // Render warning dialog on top of everything
        if let Some(ref dialog) = self.warning_dialog {
            dialog.render(frame, area, theme);
        }
    }

    fn render_header(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let block = Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(theme.border));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let file_count = self.files.len();
        let additions: usize = self.files.iter().map(|f| f.additions).sum();
        let deletions: usize = self.files.iter().map(|f| f.deletions).sum();

        // Get repo name from path
        let repo_name = self
            .repo_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("repo");

        let header = Line::from(vec![
            Span::styled(
                format!("  {} ", repo_name),
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            ),
            Span::styled("vs ", Style::default().fg(theme.dimmed)),
            Span::styled(&self.base_branch, Style::default().fg(theme.accent)),
            Span::styled("  |  ", Style::default().fg(theme.border)),
            Span::styled(
                format!("{} changed", file_count),
                Style::default().fg(theme.dimmed),
            ),
            Span::styled("  ", Style::default()),
            Span::styled(format!("+{}", additions), Style::default().fg(Color::Green)),
            Span::styled(" ", Style::default()),
            Span::styled(format!("-{}", deletions), Style::default().fg(Color::Red)),
        ]);

        frame.render_widget(Paragraph::new(header), inner);
    }

    fn render_content(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        // Split into file list (left) and diff content (right)
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(self.file_list_width),
                Constraint::Min(40),
            ])
            .split(area);

        self.render_file_list(frame, layout[0], theme);
        self.render_diff_content(frame, layout[1], theme);
    }

    fn render_file_list(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let block = Block::default()
            .title(" Files ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .padding(Padding::horizontal(1));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.files.is_empty() {
            let msg = Paragraph::new("No changes").style(Style::default().fg(theme.dimmed));
            frame.render_widget(msg, inner);
            return;
        }

        // Available width for the file path text (subtract borders, padding, prefix, status)
        let max_path_width = inner.width.saturating_sub(4) as usize; // "  M " = 4 chars

        let items: Vec<ListItem> = self
            .files
            .iter()
            .enumerate()
            .map(|(i, file)| {
                let is_selected = i == self.selected_file;

                let status_color = match file.status {
                    FileStatus::Added => Color::Green,
                    FileStatus::Modified => Color::Yellow,
                    FileStatus::Deleted => Color::Red,
                    FileStatus::Renamed => Color::Cyan,
                    FileStatus::Copied => Color::Cyan,
                    FileStatus::Untracked => Color::Gray,
                };

                let style = if is_selected {
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.dimmed)
                };

                let prefix = if is_selected { "> " } else { "  " };

                let display_path = if is_selected {
                    // Selected: show full path, truncate from left with ellipsis
                    let full = file.path.to_string_lossy();
                    truncate_left(&full, max_path_width)
                } else {
                    // Not selected: show filename only
                    file.path
                        .file_name()
                        .and_then(|s| s.to_str())
                        .unwrap_or("?")
                        .to_string()
                };

                let line = Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(
                        format!("{} ", file.status.indicator()),
                        Style::default().fg(status_color),
                    ),
                    Span::styled(display_path, style),
                ]);

                ListItem::new(line)
            })
            .collect();

        let list = List::new(items);
        frame.render_widget(list, inner);
    }

    fn render_diff_content(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let title = self
            .selected_file()
            .map(|f| format!(" {} ", f.path.display()))
            .unwrap_or_else(|| " Diff ".to_string());

        let block = Block::default()
            .title(title)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if let Some(file) = self.files.get(self.selected_file) {
            if let Some(diff) = self.diff_cache.get(&file.path) {
                if diff.is_binary {
                    let msg =
                        Paragraph::new("Binary file").style(Style::default().fg(theme.dimmed));
                    frame.render_widget(msg, inner);
                    return;
                }

                // Compute max line number for dynamic width
                let max_line_num = diff
                    .hunks
                    .iter()
                    .flat_map(|h| &h.lines)
                    .flat_map(|l| l.old_line_num.into_iter().chain(l.new_line_num))
                    .max()
                    .unwrap_or(0);
                let num_width = max_line_num.max(1).ilog10() as usize + 1;
                let blank: String = " ".repeat(num_width);

                // Build all diff lines
                let mut lines: Vec<Line> = Vec::new();

                for hunk in &diff.hunks {
                    let header = format!(
                        "@@ -{},{} +{},{} @@",
                        hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
                    );
                    lines.push(Line::from(Span::styled(
                        header,
                        Style::default().fg(Color::Cyan),
                    )));

                    for line in &hunk.lines {
                        let (prefix, style) = match line.tag {
                            ChangeTag::Delete => ("-", Style::default().fg(Color::Red)),
                            ChangeTag::Insert => ("+", Style::default().fg(Color::Green)),
                            ChangeTag::Equal => (" ", Style::default().fg(theme.dimmed)),
                        };

                        let old_num = line
                            .old_line_num
                            .map(|n| format!("{:>w$}", n, w = num_width))
                            .unwrap_or_else(|| blank.clone());
                        let new_num = line
                            .new_line_num
                            .map(|n| format!("{:>w$}", n, w = num_width))
                            .unwrap_or_else(|| blank.clone());

                        let content = line.content.trim_end_matches('\n');

                        lines.push(Line::from(vec![
                            Span::styled(
                                format!("{} {} ", old_num, new_num),
                                Style::default().fg(theme.dimmed),
                            ),
                            Span::styled(prefix, style),
                            Span::styled(content, style),
                        ]));
                    }

                    lines.push(Line::from(""));
                }

                // Update dimensions from actual content
                let total_lines = lines.len();
                let visible_lines = inner.height as usize;
                self.total_lines = total_lines as u16;
                self.visible_lines = visible_lines as u16;

                // Clamp scroll offset to valid range
                let max_scroll = total_lines.saturating_sub(visible_lines);
                if (self.scroll_offset as usize) > max_scroll {
                    self.scroll_offset = max_scroll as u16;
                }

                // Apply scrolling
                let scroll = self.scroll_offset as usize;
                let visible: Vec<Line> =
                    lines.into_iter().skip(scroll).take(visible_lines).collect();

                let paragraph = Paragraph::new(visible);
                frame.render_widget(paragraph, inner);

                // Render scrollbar
                if total_lines > visible_lines {
                    let scrollbar_area = Rect {
                        x: area.x + area.width - 1,
                        y: area.y + 1,
                        width: 1,
                        height: area.height.saturating_sub(2),
                    };
                    let mut scrollbar_state = ScrollbarState::new(max_scroll + 1).position(scroll);
                    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
                        .begin_symbol(Some("↑"))
                        .end_symbol(Some("↓"));
                    frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
                }
            } else {
                let msg =
                    Paragraph::new("Loading diff...").style(Style::default().fg(theme.dimmed));
                frame.render_widget(msg, inner);
            }
        } else {
            let msg = Paragraph::new("No file selected").style(Style::default().fg(theme.dimmed));
            frame.render_widget(msg, inner);
        }
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(theme.border));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Show error or success message, or help text
        let content = if let Some(ref error) = self.error_message {
            Line::from(Span::styled(error, Style::default().fg(theme.error)))
        } else if let Some(ref success) = self.success_message {
            Line::from(Span::styled(success, Style::default().fg(Color::Green)))
        } else {
            Line::from(vec![
                Span::styled("j/k", Style::default().fg(theme.accent)),
                Span::styled(": files  ", Style::default().fg(theme.dimmed)),
                Span::styled("h/l", Style::default().fg(theme.accent)),
                Span::styled(": resize  ", Style::default().fg(theme.dimmed)),
                Span::styled("scroll", Style::default().fg(theme.accent)),
                Span::styled(": diff  ", Style::default().fg(theme.dimmed)),
                Span::styled("e/Enter", Style::default().fg(theme.accent)),
                Span::styled(": edit  ", Style::default().fg(theme.dimmed)),
                Span::styled("b", Style::default().fg(theme.accent)),
                Span::styled(": branch  ", Style::default().fg(theme.dimmed)),
                Span::styled("?", Style::default().fg(theme.accent)),
                Span::styled(": help  ", Style::default().fg(theme.dimmed)),
                Span::styled("q/Esc", Style::default().fg(theme.accent)),
                Span::styled(": close", Style::default().fg(theme.dimmed)),
            ])
        };

        let paragraph = Paragraph::new(content).alignment(ratatui::layout::Alignment::Center);
        frame.render_widget(paragraph, inner);
    }

    fn render_with_branch_dialog(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        // Render the normal diff view first
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Length(3),
            ])
            .split(area);

        self.render_header(frame, layout[0], theme);
        self.render_content(frame, layout[1], theme);
        self.render_footer(frame, layout[2], theme);

        // Render branch selection dialog overlay
        let Some(state) = &self.branch_select else {
            return;
        };

        // Center the dialog
        let dialog_width = 40u16;
        let dialog_height = (state.branches.len() as u16 + 4).min(20);
        let dialog_x = (area.width.saturating_sub(dialog_width)) / 2;
        let dialog_y = (area.height.saturating_sub(dialog_height)) / 2;

        let dialog_area = Rect {
            x: area.x + dialog_x,
            y: area.y + dialog_y,
            width: dialog_width,
            height: dialog_height,
        };

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .title(" Select Branch ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .style(Style::default().bg(theme.background));

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let items: Vec<ListItem> = state
            .branches
            .iter()
            .enumerate()
            .map(|(i, branch)| {
                let is_selected = i == state.selected;
                let is_current = branch == &self.base_branch;

                let style = if is_selected {
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.text)
                };

                let prefix = if is_selected { "> " } else { "  " };
                let suffix = if is_current { " (current)" } else { "" };

                ListItem::new(Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(branch, style),
                    Span::styled(suffix, Style::default().fg(theme.dimmed)),
                ]))
            })
            .collect();

        let list = List::new(items);
        frame.render_widget(list, inner);
    }

    fn render_help(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_width = 55u16;
        let dialog_height = 19u16;

        let x = area.x + (area.width.saturating_sub(dialog_width)) / 2;
        let y = area.y + (area.height.saturating_sub(dialog_height)) / 2;

        let dialog_area = Rect {
            x,
            y,
            width: dialog_width.min(area.width),
            height: dialog_height.min(area.height),
        };

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .style(Style::default().bg(theme.background))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(" Diff View Help ")
            .title_style(
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            );

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let shortcuts = vec![
            (
                "Navigation",
                vec![
                    ("j/k, ↑/↓", "Navigate between files"),
                    ("PgUp/Dn", "Page up / down in diff"),
                    ("Ctrl+u/d", "Half-page up / down"),
                    ("g/G", "Go to top / bottom of diff"),
                    ("h/l, ←/→", "Shrink / grow file list"),
                ],
            ),
            (
                "Actions",
                vec![
                    ("e/Enter", "Edit file in external editor"),
                    ("b", "Select base branch"),
                    ("r", "Refresh diff"),
                ],
            ),
            (
                "Other",
                vec![("?", "Toggle this help"), ("q/Esc", "Close diff view")],
            ),
        ];

        let mut lines: Vec<Line> = Vec::new();

        for (section, keys) in shortcuts {
            lines.push(Line::from(Span::styled(
                section,
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            )));
            for (key, desc) in keys {
                lines.push(Line::from(vec![
                    Span::styled(format!("  {:14}", key), Style::default().fg(Color::Yellow)),
                    Span::styled(desc, Style::default().fg(theme.text)),
                ]));
            }
            lines.push(Line::from(""));
        }

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
    }
}
