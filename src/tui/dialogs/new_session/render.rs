//! Rendering for NewSessionDialog

use ratatui::prelude::*;
use ratatui::widgets::*;

use super::{NewSessionDialog, FIELD_HELP, HELP_DIALOG_WIDTH, SPINNER_FRAMES};
use crate::tui::components::render_text_field;
use crate::tui::styles::Theme;

impl NewSessionDialog {
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        // If loading, render the loading overlay instead
        if self.loading {
            self.render_loading(frame, area, theme);
            return;
        }

        let has_tool_selection = self.available_tools.len() > 1;
        let has_sandbox = self.docker_available;
        let has_worktree = !self.worktree_branch.value().is_empty();
        let sandbox_options_visible = has_sandbox && self.sandbox_enabled;
        let dialog_width = 80;
        // Calculate env list heights based on expanded state and number of items
        let env_list_height: u16 = if sandbox_options_visible {
            if self.env_list_expanded {
                (2 + self.extra_env_keys.len() as u16).clamp(4, 8)
            } else {
                2
            }
        } else {
            0
        };
        let env_values_list_height: u16 = if sandbox_options_visible {
            if self.env_values_list_expanded {
                (2 + self.extra_env_values.len() as u16).clamp(4, 8)
            } else {
                2
            }
        } else {
            0
        };

        // Build constraints dynamically based on visible fields only
        let mut constraints = vec![
            Constraint::Length(2), // Title
            Constraint::Length(2), // Path
            Constraint::Length(2), // Group
            Constraint::Length(2), // Tool (always shown, interactive or not)
            Constraint::Length(2), // Worktree Branch
        ];
        if has_worktree {
            constraints.push(Constraint::Length(2)); // New Branch checkbox
        }
        if has_sandbox {
            constraints.push(Constraint::Length(2)); // Sandbox checkbox
        }
        if sandbox_options_visible {
            constraints.push(Constraint::Length(2)); // Image field
            constraints.push(Constraint::Length(2)); // YOLO mode checkbox
            constraints.push(Constraint::Length(env_list_height)); // Env vars field
            constraints.push(Constraint::Length(env_values_list_height)); // Env values field
        }
        constraints.push(Constraint::Min(1)); // Hints/errors

        // Compute dialog height from actual constraints
        // border (2) + margin (2) + sum of field heights + hint line (2)
        let fields_height: u16 = constraints
            .iter()
            .map(|c| match c {
                Constraint::Length(n) => *n,
                Constraint::Min(n) => *n,
                _ => 0,
            })
            .sum();
        let dialog_height = fields_height + 4; // +2 border, +2 margin

        let dialog_area = crate::tui::dialogs::centered_rect(area, dialog_width, dialog_height);

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .title(" New Session ")
            .title_style(Style::default().fg(theme.title).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints(constraints)
            .split(inner);

        // Render fields sequentially, tracking chunk index to match dynamic constraints
        let mut ci = 0; // chunk index

        // Title, Path, Group (always visible)
        let group_placeholder = if !self.existing_groups.is_empty() && self.focused_field == 2 {
            Some("(Ctrl+P to browse groups)")
        } else {
            None
        };

        let path_placeholder = if self.focused_field == 1 {
            Some("(Ctrl+P to browse directories)")
        } else {
            None
        };

        let text_fields: [(&str, &tui_input::Input, Option<&str>); 3] = [
            ("Title:", &self.title, Some("(random civ)")),
            ("Path:", &self.path, path_placeholder),
            ("Group:", &self.group, group_placeholder),
        ];

        for (idx, (label, input, placeholder)) in text_fields.iter().enumerate() {
            render_text_field(
                frame,
                chunks[ci],
                label,
                input,
                idx == self.focused_field,
                *placeholder,
                theme,
            );
            ci += 1;
        }

        // Tool (always shown, interactive or read-only)
        let worktree_field = if has_tool_selection { 4 } else { 3 };
        let is_tool_focused = self.focused_field == 3;

        if has_tool_selection {
            let label_style = if is_tool_focused {
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

            frame.render_widget(Paragraph::new(Line::from(tool_spans)), chunks[ci]);
        } else {
            let tool_style = Style::default().fg(theme.text);
            let tool_line = Line::from(vec![
                Span::styled("Tool:", tool_style),
                Span::raw(" "),
                Span::styled(self.available_tools[0], Style::default().fg(theme.accent)),
            ]);
            frame.render_widget(Paragraph::new(tool_line), chunks[ci]);
        }
        ci += 1;

        // Worktree Branch (always visible)
        let worktree_placeholder = if self.focused_field == worktree_field {
            Some("(leave empty to skip | Ctrl+P to browse branches)")
        } else {
            Some("(leave empty to skip worktree)")
        };
        render_text_field(
            frame,
            chunks[ci],
            "Worktree Branch:",
            &self.worktree_branch,
            self.focused_field == worktree_field,
            worktree_placeholder,
            theme,
        );
        ci += 1;

        // New Branch checkbox (only when worktree is set)
        let new_branch_field = worktree_field + 1;
        if has_worktree {
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
            frame.render_widget(Paragraph::new(nb_line), chunks[ci]);
            ci += 1;
        }

        // Sandbox checkbox (only when Docker available)
        if has_sandbox {
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
            frame.render_widget(Paragraph::new(sandbox_line), chunks[ci]);
            ci += 1;

            if sandbox_options_visible {
                // Image field
                let sandbox_image_field = sandbox_field + 1;
                render_text_field(
                    frame,
                    chunks[ci],
                    "  Image:",
                    &self.sandbox_image,
                    self.focused_field == sandbox_image_field,
                    None,
                    theme,
                );
                ci += 1;

                // YOLO Mode checkbox
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
                frame.render_widget(Paragraph::new(yolo_line), chunks[ci]);
                ci += 1;

                // Environment variables field
                let env_field = yolo_mode_field + 1;
                self.render_env_field(frame, chunks[ci], env_field, theme);
                ci += 1;

                // Environment values field (KEY=VALUE)
                let env_values_field = env_field + 1;
                self.render_env_values_field(frame, chunks[ci], env_values_field, theme);
                ci += 1;
            }
        }

        // Hints/errors (last chunk)
        let hint_chunk = ci;
        if let Some(error) = &self.error_message {
            let error_text = format!("✗ Error: {}", error);
            let error_paragraph = Paragraph::new(error_text)
                .style(Style::default().fg(Color::Red))
                .wrap(Wrap { trim: true });
            frame.render_widget(error_paragraph, chunks[hint_chunk]);
        } else {
            let mut hint_spans = vec![
                Span::styled("Tab", Style::default().fg(theme.hint)),
                Span::raw(" next  "),
            ];
            if has_tool_selection {
                hint_spans.push(Span::styled("←/→", Style::default().fg(theme.hint)));
                hint_spans.push(Span::raw(" tool  "));
            }
            if self.focused_field == 1 {
                hint_spans.push(Span::styled("C-p", Style::default().fg(theme.hint)));
                hint_spans.push(Span::raw(" browse  "));
            }
            if self.focused_field == 2 && !self.existing_groups.is_empty() {
                hint_spans.push(Span::styled("C-p", Style::default().fg(theme.hint)));
                hint_spans.push(Span::raw(" groups  "));
            }
            if self.focused_field == worktree_field {
                hint_spans.push(Span::styled("C-p", Style::default().fg(theme.hint)));
                hint_spans.push(Span::raw(" branches  "));
            }
            hint_spans.push(Span::styled("Enter", Style::default().fg(theme.hint)));
            hint_spans.push(Span::raw(" create  "));
            hint_spans.push(Span::styled("?", Style::default().fg(theme.hint)));
            hint_spans.push(Span::raw(" help  "));
            hint_spans.push(Span::styled("Esc", Style::default().fg(theme.hint)));
            hint_spans.push(Span::raw(" cancel"));
            frame.render_widget(Paragraph::new(Line::from(hint_spans)), chunks[hint_chunk]);
        }

        if self.show_help {
            self.render_help_overlay(frame, area, theme);
        }

        if self.group_picker.is_active() {
            self.group_picker.render(frame, area, theme);
        }

        if self.branch_picker.is_active() {
            self.branch_picker.render(frame, area, theme);
        }

        if self.dir_picker.is_active() {
            self.dir_picker.render(frame, area, theme);
        }
    }

    fn render_env_field(&self, frame: &mut Frame, area: Rect, env_field: usize, theme: &Theme) {
        let is_focused = self.focused_field == env_field;
        let label_style = if is_focused {
            Style::default().fg(theme.accent).underlined()
        } else {
            Style::default().fg(theme.text)
        };

        if !self.env_list_expanded {
            // Collapsed view
            let count = self.extra_env_keys.len();
            let summary = if count == 0 {
                "(empty - press Enter to add)".to_string()
            } else {
                format!("[{} items]", count)
            };
            let summary_style = if count > 0 {
                Style::default().fg(theme.accent)
            } else {
                Style::default().fg(theme.dimmed)
            };

            let line = Line::from(vec![
                Span::styled("  Env Vars:", label_style),
                Span::raw(" "),
                Span::styled(summary, summary_style),
            ]);
            frame.render_widget(Paragraph::new(line), area);
        } else {
            // Expanded view with list
            let mut lines: Vec<Line> = Vec::new();

            // Header with controls hint
            let header = Line::from(vec![
                Span::styled("  Env Vars:", label_style),
                Span::styled(
                    " (a)dd (d)el (Enter)edit (Esc)close",
                    Style::default().fg(theme.dimmed),
                ),
            ]);
            lines.push(header);

            // Check if we're in editing/adding mode
            if let Some(ref input) = self.env_editing_input {
                if self.env_adding_new {
                    // Show existing items
                    for (i, key) in self.extra_env_keys.iter().enumerate() {
                        let prefix = if i == self.env_selected_index {
                            "  > "
                        } else {
                            "    "
                        };
                        lines.push(Line::from(Span::styled(
                            format!("{}{}", prefix, key),
                            Style::default().fg(theme.text),
                        )));
                    }
                    // Show input for new item
                    let input_line = Line::from(vec![
                        Span::styled("  + ", Style::default().fg(theme.accent)),
                        Span::styled(input.value(), Style::default().fg(theme.accent).bold()),
                        Span::styled("_", Style::default().fg(theme.accent)),
                    ]);
                    lines.push(input_line);
                } else {
                    // Editing existing item
                    for (i, key) in self.extra_env_keys.iter().enumerate() {
                        if i == self.env_selected_index {
                            // Show editable input
                            let input_line = Line::from(vec![
                                Span::styled("  > ", Style::default().fg(theme.accent)),
                                Span::styled(
                                    input.value(),
                                    Style::default().fg(theme.accent).bold(),
                                ),
                                Span::styled("_", Style::default().fg(theme.accent)),
                            ]);
                            lines.push(input_line);
                        } else {
                            let prefix = "    ";
                            lines.push(Line::from(Span::styled(
                                format!("{}{}", prefix, key),
                                Style::default().fg(theme.text),
                            )));
                        }
                    }
                }
            } else {
                // Normal list display
                if self.extra_env_keys.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "    (press 'a' to add)",
                        Style::default().fg(theme.dimmed),
                    )));
                } else {
                    for (i, key) in self.extra_env_keys.iter().enumerate() {
                        let is_selected = i == self.env_selected_index;
                        let prefix = if is_selected { "  > " } else { "    " };
                        let style = if is_selected {
                            Style::default().fg(theme.accent).bold()
                        } else {
                            Style::default().fg(theme.text)
                        };
                        lines.push(Line::from(Span::styled(
                            format!("{}{}", prefix, key),
                            style,
                        )));
                    }
                }
            }

            frame.render_widget(Paragraph::new(lines), area);
        }
    }

    fn render_env_values_field(
        &self,
        frame: &mut Frame,
        area: Rect,
        field_idx: usize,
        theme: &Theme,
    ) {
        let is_focused = self.focused_field == field_idx;
        let label_style = if is_focused {
            Style::default().fg(theme.accent).underlined()
        } else {
            Style::default().fg(theme.text)
        };

        if !self.env_values_list_expanded {
            let count = self.extra_env_values.len();
            let summary = if count == 0 {
                "(empty - press Enter to add)".to_string()
            } else {
                format!("[{} items]", count)
            };
            let summary_style = if count > 0 {
                Style::default().fg(theme.accent)
            } else {
                Style::default().fg(theme.dimmed)
            };

            let line = Line::from(vec![
                Span::styled("  Env Values:", label_style),
                Span::raw(" "),
                Span::styled(summary, summary_style),
            ]);
            frame.render_widget(Paragraph::new(line), area);
        } else {
            let mut lines: Vec<Line> = Vec::new();

            let header = Line::from(vec![
                Span::styled("  Env Values:", label_style),
                Span::styled(
                    " (a)dd (d)el (Enter)edit (Esc)close",
                    Style::default().fg(theme.dimmed),
                ),
            ]);
            lines.push(header);

            if let Some(ref input) = self.env_values_editing_input {
                if self.env_values_adding_new {
                    for (i, entry) in self.extra_env_values.iter().enumerate() {
                        let prefix = if i == self.env_values_selected_index {
                            "  > "
                        } else {
                            "    "
                        };
                        lines.push(Line::from(Span::styled(
                            format!("{}{}", prefix, entry),
                            Style::default().fg(theme.text),
                        )));
                    }
                    let input_line = Line::from(vec![
                        Span::styled("  + ", Style::default().fg(theme.accent)),
                        Span::styled(input.value(), Style::default().fg(theme.accent).bold()),
                        Span::styled("_", Style::default().fg(theme.accent)),
                    ]);
                    lines.push(input_line);
                } else {
                    for (i, entry) in self.extra_env_values.iter().enumerate() {
                        if i == self.env_values_selected_index {
                            let input_line = Line::from(vec![
                                Span::styled("  > ", Style::default().fg(theme.accent)),
                                Span::styled(
                                    input.value(),
                                    Style::default().fg(theme.accent).bold(),
                                ),
                                Span::styled("_", Style::default().fg(theme.accent)),
                            ]);
                            lines.push(input_line);
                        } else {
                            lines.push(Line::from(Span::styled(
                                format!("    {}", entry),
                                Style::default().fg(theme.text),
                            )));
                        }
                    }
                }
            } else if self.extra_env_values.is_empty() {
                lines.push(Line::from(Span::styled(
                    "    (press 'a' to add KEY=VALUE)",
                    Style::default().fg(theme.dimmed),
                )));
            } else {
                for (i, entry) in self.extra_env_values.iter().enumerate() {
                    let is_selected = i == self.env_values_selected_index;
                    let prefix = if is_selected { "  > " } else { "    " };
                    let style = if is_selected {
                        Style::default().fg(theme.accent).bold()
                    } else {
                        Style::default().fg(theme.text)
                    };
                    lines.push(Line::from(Span::styled(
                        format!("{}{}", prefix, entry),
                        style,
                    )));
                }
            }

            frame.render_widget(Paragraph::new(lines), area);
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
            + if show_sandbox_options_help { 12 } else { 0 }; // Image, YOLO, Env, Env Values

        let dialog_area = crate::tui::dialogs::centered_rect(area, dialog_width, dialog_height);

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
            if idx == 9 && !show_sandbox_options_help {
                continue;
            }
            if idx == 10 && !show_sandbox_options_help {
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

    fn render_loading(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let needs_extra_line = self.sandbox_enabled && self.needs_image_pull;
        let show_hook_output = self.has_hooks;
        let max_output_lines: usize = 6;

        let dialog_width: u16 = if show_hook_output {
            70
        } else if needs_extra_line {
            55
        } else {
            50
        };
        let dialog_height: u16 = if show_hook_output {
            // spinner line + command line + output lines + cancel hint + padding
            (6 + max_output_lines as u16).min(area.height)
        } else if needs_extra_line {
            9
        } else {
            7
        };

        let dialog_area = crate::tui::dialogs::centered_rect(area, dialog_width, dialog_height);

        frame.render_widget(Clear, dialog_area);

        let title = if show_hook_output {
            " Running Hooks "
        } else {
            " Creating Session "
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .title(title)
            .title_style(Style::default().fg(theme.title).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let spinner = SPINNER_FRAMES[self.spinner_frame];

        if show_hook_output {
            let mut lines = vec![];

            // Status line with spinner
            let status_text = if let Some(ref cmd) = self.current_hook {
                // Truncate long commands to fit the dialog
                let max_cmd_len = (dialog_width as usize).saturating_sub(12);
                if cmd.len() > max_cmd_len {
                    let truncated: String =
                        cmd.chars().take(max_cmd_len.saturating_sub(3)).collect();
                    format!("{}...", truncated)
                } else {
                    cmd.clone()
                }
            } else {
                "Preparing...".to_string()
            };

            lines.push(Line::from(vec![
                Span::styled(
                    format!(" {} ", spinner),
                    Style::default().fg(theme.accent).bold(),
                ),
                Span::styled(status_text, Style::default().fg(theme.text)),
            ]));

            // Show last N output lines
            let output_start = self.hook_output.len().saturating_sub(max_output_lines);
            let visible_lines = &self.hook_output[output_start..];
            let inner_width = (dialog_width as usize).saturating_sub(6);

            for line in visible_lines {
                let truncated = if line.len() > inner_width {
                    let t: String = line.chars().take(inner_width.saturating_sub(3)).collect();
                    format!("{}...", t)
                } else {
                    line.clone()
                };
                lines.push(Line::from(Span::styled(
                    format!("  {}", truncated),
                    Style::default().fg(theme.dimmed),
                )));
            }

            // Pad remaining lines so cancel hint stays at bottom
            let used = 1 + visible_lines.len(); // status + output
            let available = dialog_height.saturating_sub(4) as usize; // borders + cancel line
            for _ in used..available {
                lines.push(Line::from(""));
            }

            lines.push(Line::from(vec![
                Span::styled(" Press ", Style::default().fg(theme.dimmed)),
                Span::styled("Esc", Style::default().fg(theme.hint)),
                Span::styled(" to cancel", Style::default().fg(theme.dimmed)),
            ]));

            frame.render_widget(Paragraph::new(lines), inner);
        } else {
            let loading_text = if self.sandbox_enabled {
                if self.needs_image_pull {
                    "Pulling sandbox image..."
                } else {
                    "Setting up sandbox container..."
                }
            } else {
                "Creating session..."
            };

            let mut lines = vec![
                Line::from(""),
                Line::from(vec![
                    Span::styled(
                        format!("  {} ", spinner),
                        Style::default().fg(theme.accent).bold(),
                    ),
                    Span::styled(loading_text, Style::default().fg(theme.text)),
                ]),
            ];

            if needs_extra_line {
                lines.push(Line::from(Span::styled(
                    "    (first time may take a few minutes)",
                    Style::default().fg(theme.dimmed),
                )));
            }

            lines.push(Line::from(""));
            lines.push(Line::from(vec![
                Span::styled("  Press ", Style::default().fg(theme.dimmed)),
                Span::styled("Esc", Style::default().fg(theme.hint)),
                Span::styled(" to cancel", Style::default().fg(theme.dimmed)),
            ]));

            frame.render_widget(Paragraph::new(lines), inner);
        }
    }
}
