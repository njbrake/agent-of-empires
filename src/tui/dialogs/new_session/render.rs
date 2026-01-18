//! Rendering for NewSessionDialog

use ratatui::prelude::*;
use ratatui::widgets::*;

use super::{NewSessionDialog, FIELD_HELP, HELP_DIALOG_WIDTH};
use crate::tui::styles::Theme;

impl NewSessionDialog {
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let has_tool_selection = self.available_tools.len() > 1;
        let has_sandbox = self.docker_available;
        let sandbox_options_visible = has_sandbox && self.sandbox_enabled;
        let dialog_width = 80;
        let dialog_height = if sandbox_options_visible {
            24
        } else if has_sandbox {
            20
        } else {
            18
        };
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
            .title(" New Session ")
            .title_style(Style::default().fg(theme.title).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let mut constraints = vec![
            Constraint::Length(2), // Title
            Constraint::Length(2), // Path
            Constraint::Length(2), // Group
            Constraint::Length(2), // Tool
            Constraint::Length(2), // Worktree Branch
            Constraint::Length(2), // New Branch checkbox
            Constraint::Length(2), // Sandbox checkbox
        ];
        if sandbox_options_visible {
            constraints.push(Constraint::Length(2)); // Image field
            constraints.push(Constraint::Length(2)); // YOLO mode checkbox
        }
        constraints.push(Constraint::Min(1)); // Hints/errors

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints(constraints)
            .split(inner);

        let text_fields: [(&str, &tui_input::Input); 3] = [
            ("Title:", &self.title),
            ("Path:", &self.path),
            ("Group:", &self.group),
        ];

        for (idx, (label, input)) in text_fields.iter().enumerate() {
            let is_focused = idx == self.focused_field;
            let label_style = if is_focused {
                Style::default().fg(theme.accent).underlined()
            } else {
                Style::default().fg(theme.text)
            };
            let value_style = if is_focused {
                Style::default().fg(theme.accent)
            } else {
                Style::default().fg(theme.text)
            };

            let value = input.value();
            let cursor_pos = input.visual_cursor();

            let display_value = if value.is_empty() && idx == 0 {
                "(random civ)".to_string()
            } else if is_focused {
                let (before, after) = value.split_at(cursor_pos.min(value.len()));
                format!("{}█{}", before, after)
            } else {
                value.to_string()
            };

            let line = Line::from(vec![
                Span::styled(*label, label_style),
                Span::styled(format!(" {}", display_value), value_style),
            ]);

            frame.render_widget(Paragraph::new(line), chunks[idx]);
        }

        let is_tool_focused = self.focused_field == 3;
        let tool_style = if is_tool_focused && has_tool_selection {
            Style::default().fg(theme.accent).underlined()
        } else {
            Style::default().fg(theme.text)
        };

        if has_tool_selection {
            let label_style = if is_tool_focused && has_tool_selection {
                Style::default().fg(theme.accent).underlined()
            } else {
                Style::default().fg(theme.text)
            };

            let mut tool_spans = vec![Span::styled("Tool:", label_style), Span::raw(" ")];

            for (idx, tool_name) in self.available_tools.iter().enumerate() {
                let is_selected = idx == self.tool_index;
                let style = if is_selected {
                    Style::default().fg(theme.accent).bold()
                } else {
                    Style::default().fg(theme.dimmed)
                };

                if idx > 0 {
                    tool_spans.push(Span::raw("  "));
                }
                tool_spans.push(Span::styled(if is_selected { "● " } else { "○ " }, style));
                tool_spans.push(Span::styled(*tool_name, style));
            }

            let tool_line = Line::from(tool_spans);
            frame.render_widget(Paragraph::new(tool_line), chunks[3]);
        } else {
            let tool_line = Line::from(vec![
                Span::styled("Tool:", tool_style),
                Span::raw(" "),
                Span::styled(self.available_tools[0], Style::default().fg(theme.accent)),
            ]);
            frame.render_widget(Paragraph::new(tool_line), chunks[3]);
        }

        let worktree_field = if has_tool_selection { 4 } else { 3 };
        let new_branch_field = worktree_field + 1;

        let is_wt_focused = self.focused_field == worktree_field;
        let wt_label_style = if is_wt_focused {
            Style::default().fg(theme.accent).underlined()
        } else {
            Style::default().fg(theme.text)
        };
        let wt_value_style = if is_wt_focused {
            Style::default().fg(theme.accent)
        } else {
            Style::default().fg(theme.text)
        };

        let wt_value = self.worktree_branch.value();
        let wt_cursor_pos = self.worktree_branch.visual_cursor();
        let wt_display = if wt_value.is_empty() && !is_wt_focused {
            "(leave empty to skip worktree)".to_string()
        } else if is_wt_focused {
            let (before, after) = wt_value.split_at(wt_cursor_pos.min(wt_value.len()));
            format!("{}█{}", before, after)
        } else {
            wt_value.to_string()
        };
        let wt_line = Line::from(vec![
            Span::styled("Worktree Branch:", wt_label_style),
            Span::styled(format!(" {}", wt_display), wt_value_style),
        ]);
        frame.render_widget(Paragraph::new(wt_line), chunks[4]);

        let has_worktree = !wt_value.is_empty();
        let next_chunk = if has_worktree {
            let is_nb_focused = self.focused_field == new_branch_field;
            let nb_label_style = if is_nb_focused {
                Style::default().fg(theme.accent).underlined()
            } else {
                Style::default().fg(theme.text)
            };
            let checkbox = if self.create_new_branch { "[x]" } else { "[ ]" };
            let checkbox_style = if self.create_new_branch {
                Style::default().fg(theme.accent).bold()
            } else {
                Style::default().fg(theme.dimmed)
            };
            let nb_text = if self.create_new_branch {
                "Create new branch"
            } else {
                "Attach to existing branch"
            };
            let nb_line = Line::from(vec![
                Span::styled("New Branch:", nb_label_style),
                Span::raw(" "),
                Span::styled(checkbox, checkbox_style),
                Span::styled(
                    format!(" {}", nb_text),
                    if self.create_new_branch {
                        Style::default().fg(theme.accent)
                    } else {
                        Style::default().fg(theme.dimmed)
                    },
                ),
            ]);
            frame.render_widget(Paragraph::new(nb_line), chunks[5]);
            6
        } else {
            5
        };

        let hint_chunk = if has_sandbox {
            let sandbox_field = if has_worktree {
                new_branch_field + 1
            } else {
                worktree_field + 1
            };
            let is_sandbox_focused = self.focused_field == sandbox_field;
            let sandbox_label_style = if is_sandbox_focused {
                Style::default().fg(theme.accent).underlined()
            } else {
                Style::default().fg(theme.text)
            };

            let checkbox = if self.sandbox_enabled { "[x]" } else { "[ ]" };
            let checkbox_style = if self.sandbox_enabled {
                Style::default().fg(theme.accent).bold()
            } else {
                Style::default().fg(theme.dimmed)
            };

            let sandbox_line = Line::from(vec![
                Span::styled("Sandbox:", sandbox_label_style),
                Span::raw(" "),
                Span::styled(checkbox, checkbox_style),
                Span::styled(
                    " Run in Docker container",
                    if self.sandbox_enabled {
                        Style::default().fg(theme.accent)
                    } else {
                        Style::default().fg(theme.dimmed)
                    },
                ),
            ]);
            frame.render_widget(Paragraph::new(sandbox_line), chunks[next_chunk]);

            if sandbox_options_visible {
                let sandbox_image_field = sandbox_field + 1;
                let is_image_focused = self.focused_field == sandbox_image_field;
                let image_label_style = if is_image_focused {
                    Style::default().fg(theme.accent).underlined()
                } else {
                    Style::default().fg(theme.text)
                };
                let image_value_style = if is_image_focused {
                    Style::default().fg(theme.accent)
                } else {
                    Style::default().fg(theme.text)
                };

                let image_value = self.sandbox_image.value();
                let image_cursor_pos = self.sandbox_image.visual_cursor();

                let image_display = if is_image_focused {
                    let (before, after) =
                        image_value.split_at(image_cursor_pos.min(image_value.len()));
                    format!("{}█{}", before, after)
                } else {
                    image_value.to_string()
                };

                let image_line = Line::from(vec![
                    Span::styled("  Image:", image_label_style),
                    Span::styled(format!(" {}", image_display), image_value_style),
                ]);
                frame.render_widget(Paragraph::new(image_line), chunks[next_chunk + 1]);

                let yolo_mode_field = sandbox_image_field + 1;
                let is_yolo_focused = self.focused_field == yolo_mode_field;
                let yolo_label_style = if is_yolo_focused {
                    Style::default().fg(theme.accent).underlined()
                } else {
                    Style::default().fg(theme.text)
                };

                let yolo_checkbox = if self.yolo_mode { "[x]" } else { "[ ]" };
                let yolo_checkbox_style = if self.yolo_mode {
                    Style::default().fg(theme.accent).bold()
                } else {
                    Style::default().fg(theme.dimmed)
                };

                let yolo_line = Line::from(vec![
                    Span::styled("  YOLO Mode:", yolo_label_style),
                    Span::raw(" "),
                    Span::styled(yolo_checkbox, yolo_checkbox_style),
                    Span::styled(
                        " Skip permission prompts",
                        if self.yolo_mode {
                            Style::default().fg(theme.accent)
                        } else {
                            Style::default().fg(theme.dimmed)
                        },
                    ),
                ]);
                frame.render_widget(Paragraph::new(yolo_line), chunks[next_chunk + 2]);

                next_chunk + 3
            } else {
                next_chunk + 1
            }
        } else {
            next_chunk
        };

        if let Some(error) = &self.error_message {
            let error_line = Line::from(vec![
                Span::styled("✗ Error: ", Style::default().fg(Color::Red).bold()),
                Span::styled(error, Style::default().fg(Color::Red)),
            ]);
            frame.render_widget(Paragraph::new(error_line), chunks[hint_chunk]);
        } else {
            let hint = if has_tool_selection {
                Line::from(vec![
                    Span::styled("Tab", Style::default().fg(theme.hint)),
                    Span::raw(" next  "),
                    Span::styled("←/→", Style::default().fg(theme.hint)),
                    Span::raw(" tool  "),
                    Span::styled("Enter", Style::default().fg(theme.hint)),
                    Span::raw(" create  "),
                    Span::styled("?", Style::default().fg(theme.hint)),
                    Span::raw(" help  "),
                    Span::styled("Esc", Style::default().fg(theme.hint)),
                    Span::raw(" cancel"),
                ])
            } else {
                Line::from(vec![
                    Span::styled("Tab", Style::default().fg(theme.hint)),
                    Span::raw(" next  "),
                    Span::styled("Enter", Style::default().fg(theme.hint)),
                    Span::raw(" create  "),
                    Span::styled("?", Style::default().fg(theme.hint)),
                    Span::raw(" help  "),
                    Span::styled("Esc", Style::default().fg(theme.hint)),
                    Span::raw(" cancel"),
                ])
            };
            frame.render_widget(Paragraph::new(hint), chunks[hint_chunk]);
        }

        if self.show_help {
            self.render_help_overlay(frame, area, theme);
        }
    }

    fn render_help_overlay(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let has_tool_selection = self.available_tools.len() > 1;
        let has_sandbox = self.docker_available;
        let show_sandbox_options_help = has_sandbox && self.sandbox_enabled;

        let dialog_width: u16 = HELP_DIALOG_WIDTH;
        let base_height: u16 = 17;
        let dialog_height: u16 = base_height
            + if has_tool_selection { 3 } else { 0 }
            + if has_sandbox { 3 } else { 0 }
            + if show_sandbox_options_help { 6 } else { 0 };

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
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .title(" New Session Help ")
            .title_style(Style::default().fg(theme.title).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let mut lines: Vec<Line> = Vec::new();

        for (idx, help) in FIELD_HELP.iter().enumerate() {
            if idx == 3 && !has_tool_selection {
                continue;
            }
            if idx == 6 && !has_sandbox {
                continue;
            }
            if idx == 7 && !show_sandbox_options_help {
                continue;
            }
            if idx == 8 && !show_sandbox_options_help {
                continue;
            }

            lines.push(Line::from(Span::styled(
                help.name,
                Style::default().fg(theme.accent).bold(),
            )));
            lines.push(Line::from(Span::styled(
                format!("  {}", help.description),
                Style::default().fg(theme.text),
            )));
            lines.push(Line::from(""));
        }

        lines.push(Line::from(vec![
            Span::styled("Press ", Style::default().fg(theme.dimmed)),
            Span::styled("?", Style::default().fg(theme.hint)),
            Span::styled(" or ", Style::default().fg(theme.dimmed)),
            Span::styled("Esc", Style::default().fg(theme.hint)),
            Span::styled(" to close", Style::default().fg(theme.dimmed)),
        ]));

        frame.render_widget(Paragraph::new(lines), inner);
    }
}
