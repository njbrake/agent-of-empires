//! Rendering for NewSessionDialog

use ratatui::prelude::*;
use ratatui::widgets::*;

use super::{NewSessionDialog, FIELD_HELP, HELP_DIALOG_WIDTH, SPINNER_FRAMES};
use crate::tui::components::{render_text_field, render_text_field_with_ghost};
use crate::tui::styles::Theme;

impl NewSessionDialog {
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        // If loading, render the loading overlay instead
        if self.loading {
            self.render_loading(frame, area, theme);
            return;
        }

        // If in sandbox config mode, render that overlay instead
        if self.sandbox_config_mode {
            self.render_sandbox_config(frame, area, theme);
            return;
        }

        // If in tool config mode, render that overlay instead
        if self.tool_config_mode {
            self.render_tool_config(frame, area, theme);
            return;
        }

        let has_profile_selection = self.has_profile_selection();
        let has_tool_selection = self.available_tools.len() > 1;
        let has_sandbox = self.docker_available;
        let has_worktree = !self.worktree_branch.value().is_empty();
        let dialog_width = 80;

        // Build constraints dynamically based on visible fields only
        let mut constraints = Vec::new();
        if has_profile_selection {
            constraints.push(Constraint::Length(2)); // Profile
        }
        constraints.extend([
            Constraint::Length(2), // Title
            Constraint::Length(2), // Path
            Constraint::Length(2), // Tool (always shown, interactive or not)
            Constraint::Length(2), // YOLO mode checkbox (always visible)
            Constraint::Length(2), // Worktree Branch
        ]);
        if has_worktree {
            constraints.push(Constraint::Length(2)); // New Branch checkbox
        }
        if has_sandbox {
            constraints.push(Constraint::Length(2)); // Sandbox checkbox (summary only)
        }
        constraints.push(Constraint::Length(2)); // Group (always, at the bottom)

        // For errors, calculate how many lines we need based on the text length.
        // Inner width = dialog_width - 2 (border) - 2 (margin) = 76
        let error_lines: u16 = if let Some(error) = &self.error_message {
            let inner_width = (dialog_width - 4) as usize;
            let error_text = format!("✗ Error: {}", error);
            let needed = (error_text.len() as u16).div_ceil(inner_width as u16);
            needed.clamp(2, 6)
        } else {
            1
        };
        constraints.push(Constraint::Min(error_lines)); // Hints/errors

        // Compute dialog height from actual constraints
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

        // Field index calculations (must match handle_key)
        let base = if has_profile_selection { 1 } else { 0 };
        let title_field = base;
        let yolo_mode_field = base + 2 + if has_tool_selection { 1 } else { 0 };
        let worktree_field = yolo_mode_field + 1;
        let new_branch_field = worktree_field + 1;
        let mut next_field_idx = if has_worktree {
            new_branch_field + 1
        } else {
            worktree_field + 1
        };
        let sandbox_field = if has_sandbox {
            let f = next_field_idx;
            next_field_idx += 1;
            f
        } else {
            usize::MAX
        };
        let group_field = next_field_idx;

        // Profile picker (only when multiple profiles)
        if has_profile_selection {
            self.render_profile_field(frame, chunks[ci], theme);
            ci += 1;
        }

        // Title
        render_text_field(
            frame,
            chunks[ci],
            "Title:",
            &self.title,
            self.focused_field == title_field,
            Some("(random civ)"),
            theme,
        );
        ci += 1;

        // Path
        let path_placeholder = if self.focused_field == self.path_field() {
            Some("(Ctrl+P to browse directories)")
        } else {
            None
        };
        self.render_path_field(frame, chunks[ci], path_placeholder, theme);
        ci += 1;

        // Tool (always shown, interactive or read-only)
        let tool_field = base + 2;
        let is_tool_focused = has_tool_selection && self.focused_field == tool_field;
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

            // Show Ctrl+P hint and summary of tool config
            let has_config =
                !self.extra_args.value().is_empty() || !self.command_override.value().is_empty();
            if has_config {
                tool_spans.push(Span::styled(
                    "  (configured)",
                    Style::default().fg(theme.dimmed),
                ));
            }
            if is_tool_focused {
                tool_spans.push(Span::styled(
                    if has_config {
                        "  Ctrl+P: edit"
                    } else {
                        "  (Ctrl+P to configure)"
                    },
                    Style::default().fg(theme.dimmed),
                ));
            }

            frame.render_widget(Paragraph::new(Line::from(tool_spans)), chunks[ci]);
        } else {
            let tool_style = Style::default().fg(theme.text);
            let mut tool_spans = vec![
                Span::styled("Tool:", tool_style),
                Span::raw(" "),
                Span::styled(self.available_tools[0], Style::default().fg(theme.accent)),
            ];

            let has_config =
                !self.extra_args.value().is_empty() || !self.command_override.value().is_empty();
            if has_config {
                tool_spans.push(Span::styled(
                    "  (configured)",
                    Style::default().fg(theme.dimmed),
                ));
            }

            frame.render_widget(Paragraph::new(Line::from(tool_spans)), chunks[ci]);
        }
        ci += 1;

        // YOLO Mode checkbox
        {
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
                Span::styled("YOLO Mode:", yolo_label_style),
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
        }

        // Worktree Branch
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

        // Sandbox checkbox with summary (only when Docker available)
        if has_sandbox {
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

            let mut spans = vec![
                Span::styled("Sandbox:", sandbox_label_style),
                Span::raw(" "),
                Span::styled(checkbox, checkbox_style),
                Span::styled(
                    " Run in Docker",
                    if self.sandbox_enabled {
                        Style::default().fg(theme.accent)
                    } else {
                        Style::default().fg(theme.dimmed)
                    },
                ),
            ];

            if self.sandbox_enabled {
                spans.push(Span::styled(
                    "  (Ctrl+P to configure)",
                    Style::default().fg(theme.dimmed),
                ));
            }

            frame.render_widget(Paragraph::new(Line::from(spans)), chunks[ci]);
            ci += 1;
        }

        // Group (always visible, at the bottom before hints)
        let group_placeholder =
            if !self.existing_groups.is_empty() && self.focused_field == group_field {
                Some("(Ctrl+P to browse groups)")
            } else {
                None
            };
        render_text_field_with_ghost(
            frame,
            chunks[ci],
            "Group:",
            &self.group,
            self.focused_field == group_field,
            group_placeholder,
            self.group_ghost_text(),
            theme,
        );
        ci += 1;

        // Hints/errors (last chunk)
        let hint_chunk = ci;
        if self.confirm_create_dir.is_some() {
            let selected = self.confirm_create_dir.unwrap_or(false);
            let yes_style = if selected {
                Style::default().fg(theme.accent).bold()
            } else {
                Style::default().fg(theme.dimmed)
            };
            let no_style = if !selected {
                Style::default().fg(theme.accent).bold()
            } else {
                Style::default().fg(theme.dimmed)
            };
            let line = Line::from(vec![
                Span::styled(
                    "⚠ Path does not exist. Create? ",
                    Style::default().fg(theme.error),
                ),
                Span::styled("[y]es", yes_style),
                Span::raw(" "),
                Span::styled("[N]o", no_style),
            ]);
            frame.render_widget(Paragraph::new(line), chunks[hint_chunk]);
        } else if let Some(error) = &self.error_message {
            let error_text = format!("✗ Error: {}", error);
            let error_paragraph = Paragraph::new(error_text)
                .style(Style::default().fg(theme.error))
                .wrap(Wrap { trim: true });
            frame.render_widget(error_paragraph, chunks[hint_chunk]);
        } else {
            let mut hint_spans = Vec::new();
            hint_spans.push(Span::styled("Tab", Style::default().fg(theme.hint)));
            hint_spans.push(Span::raw(" next  "));
            if has_tool_selection {
                hint_spans.push(Span::styled("←/→", Style::default().fg(theme.hint)));
                hint_spans.push(Span::raw(" tool  "));
            }
            if self.focused_field == self.path_field() {
                if self.ghost_text().is_some() {
                    hint_spans.push(Span::styled("→", Style::default().fg(theme.hint)));
                    hint_spans.push(Span::raw(" accept  "));
                }
                hint_spans.push(Span::styled("C-←/M-b", Style::default().fg(theme.hint)));
                hint_spans.push(Span::raw(" prev seg  "));
                hint_spans.push(Span::styled("Home/C-a", Style::default().fg(theme.hint)));
                hint_spans.push(Span::raw(" start  "));
                hint_spans.push(Span::styled("C-p", Style::default().fg(theme.hint)));
                hint_spans.push(Span::raw(" browse  "));
            }
            if self.focused_field == group_field && !self.existing_groups.is_empty() {
                if self.group_ghost_text().is_some() {
                    hint_spans.push(Span::styled("→", Style::default().fg(theme.hint)));
                    hint_spans.push(Span::raw(" accept  "));
                }
                hint_spans.push(Span::styled("C-p", Style::default().fg(theme.hint)));
                hint_spans.push(Span::raw(" groups  "));
            }
            if self.focused_field == tool_field {
                hint_spans.push(Span::styled("C-p", Style::default().fg(theme.hint)));
                hint_spans.push(Span::raw(" configure  "));
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

    fn render_profile_field(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let is_focused = self.focused_field == 0;
        let label_style = if is_focused {
            Style::default().fg(theme.accent).underlined()
        } else {
            Style::default().fg(theme.text)
        };

        let selected = self.selected_profile();
        let profile_style = if is_focused {
            Style::default().fg(theme.accent).bold()
        } else {
            Style::default().fg(theme.accent)
        };

        let mut spans = vec![Span::styled("Profile:", label_style), Span::raw(" ")];

        if self.available_profiles.len() > 1 {
            spans.push(Span::styled("< ", Style::default().fg(theme.dimmed)));
            spans.push(Span::styled(selected, profile_style));
            spans.push(Span::styled(" >", Style::default().fg(theme.dimmed)));
        } else {
            spans.push(Span::styled(selected, profile_style));
        }

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_path_field(
        &self,
        frame: &mut Frame,
        area: Rect,
        placeholder: Option<&str>,
        theme: &Theme,
    ) {
        let is_focused = self.focused_field == self.path_field();
        let flashing_invalid = self.is_path_invalid_flash_active();

        let label_color = if flashing_invalid {
            theme.error
        } else if is_focused {
            theme.accent
        } else {
            theme.text
        };
        let value_color = if flashing_invalid {
            theme.error
        } else if is_focused {
            theme.accent
        } else {
            theme.text
        };

        let label_style = if is_focused {
            Style::default().fg(label_color).underlined()
        } else {
            Style::default().fg(label_color)
        };
        let value_style = Style::default().fg(value_color);

        let value = self.path.value();
        let mut spans = vec![Span::styled("Path:", label_style), Span::raw(" ")];

        if value.is_empty() && !is_focused {
            if let Some(placeholder_text) = placeholder {
                spans.push(Span::styled(placeholder_text, value_style));
            }
        } else if is_focused {
            let cursor_pos = self.path.visual_cursor();
            let cursor_style = if flashing_invalid {
                Style::default().fg(theme.background).bg(theme.error)
            } else {
                Style::default().fg(theme.background).bg(theme.accent)
            };

            let before: String = value.chars().take(cursor_pos).collect();
            let cursor_char: String = value
                .chars()
                .nth(cursor_pos)
                .map(|c| c.to_string())
                .unwrap_or_else(|| " ".to_string());
            let after: String = value.chars().skip(cursor_pos + 1).collect();

            if !before.is_empty() {
                spans.push(Span::styled(before, value_style));
            }
            spans.push(Span::styled(cursor_char, cursor_style));
            if !after.is_empty() {
                spans.push(Span::styled(after, value_style));
            }
            if let Some(ghost) = self.ghost_text() {
                spans.push(Span::styled(ghost, Style::default().fg(theme.dimmed)));
            }
        } else {
            spans.push(Span::styled(value, value_style));
        }

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_sandbox_config(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_width: u16 = 72;

        // Sandbox config fields: image, env, inherited
        let env_list_height: u16 = if self.env_list_expanded {
            (2 + self.extra_env.len() as u16).clamp(4, 8)
        } else {
            2
        };
        let inherited_height: u16 = 2 + self.inherited_settings.len().max(1) as u16;

        let constraints = vec![
            Constraint::Length(2),                // Image
            Constraint::Length(env_list_height),  // Environment
            Constraint::Length(inherited_height), // Inherited settings
            Constraint::Min(1),                   // Hints
        ];

        let fields_height: u16 = constraints
            .iter()
            .map(|c| match c {
                Constraint::Length(n) => *n,
                Constraint::Min(n) => *n,
                _ => 0,
            })
            .sum();
        let dialog_height = fields_height + 4;

        let dialog_area = crate::tui::dialogs::centered_rect(area, dialog_width, dialog_height);

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .title(" Sandbox Configuration ")
            .title_style(Style::default().fg(theme.title).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints(constraints)
            .split(inner);

        let mut ci = 0;

        // Image field
        render_text_field(
            frame,
            chunks[ci],
            "Image:",
            &self.sandbox_image,
            self.sandbox_focused_field == 0,
            None,
            theme,
        );
        ci += 1;

        // Environment
        self.render_env_field(frame, chunks[ci], self.sandbox_focused_field == 1, theme);
        ci += 1;

        // Inherited settings (always visible, not focusable)
        self.render_inherited_field(frame, chunks[ci], theme);
        ci += 1;

        // Hints
        let hint_spans = vec![
            Span::styled("Tab", Style::default().fg(theme.hint)),
            Span::raw(" next  "),
            Span::styled("Enter", Style::default().fg(theme.hint)),
            Span::raw(" edit  "),
            Span::styled("Esc", Style::default().fg(theme.hint)),
            Span::raw(" back"),
        ];
        frame.render_widget(Paragraph::new(Line::from(hint_spans)), chunks[ci]);

        if self.show_help {
            self.render_help_overlay(frame, area, theme);
        }
    }

    fn render_tool_config(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_width: u16 = 72;

        let constraints = vec![
            Constraint::Length(2), // Command Override
            Constraint::Length(2), // Extra Args
            Constraint::Min(1),    // Hints
        ];

        let fields_height: u16 = constraints
            .iter()
            .map(|c| match c {
                Constraint::Length(n) => *n,
                Constraint::Min(n) => *n,
                _ => 0,
            })
            .sum();
        let dialog_height = fields_height + 4;

        let selected_tool = self
            .available_tools
            .get(self.tool_index)
            .copied()
            .unwrap_or("claude");
        let title = format!(" Tool Configuration: {} ", selected_tool);

        let dialog_area = crate::tui::dialogs::centered_rect(area, dialog_width, dialog_height);

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .title(title)
            .title_style(Style::default().fg(theme.title).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints(constraints)
            .split(inner);

        // Command Override
        let cmd_placeholder = if self.tool_config_focused_field == 0 {
            Some("(replaces default binary)")
        } else if self.command_override.value().is_empty() {
            Some("(default)")
        } else {
            None
        };
        render_text_field(
            frame,
            chunks[0],
            "Command:",
            &self.command_override,
            self.tool_config_focused_field == 0,
            cmd_placeholder,
            theme,
        );

        // Extra Args
        let args_placeholder = if self.tool_config_focused_field == 1 {
            Some("(e.g. --port 8080)")
        } else if self.extra_args.value().is_empty() {
            Some("(none)")
        } else {
            None
        };
        render_text_field(
            frame,
            chunks[1],
            "Extra Args:",
            &self.extra_args,
            self.tool_config_focused_field == 1,
            args_placeholder,
            theme,
        );

        // Hints
        let hint_spans = vec![
            Span::styled("Tab", Style::default().fg(theme.hint)),
            Span::raw(" next  "),
            Span::styled("Enter", Style::default().fg(theme.hint)),
            Span::raw(" done  "),
            Span::styled("Esc", Style::default().fg(theme.hint)),
            Span::raw(" back"),
        ];
        frame.render_widget(Paragraph::new(Line::from(hint_spans)), chunks[2]);

        if self.show_help {
            self.render_help_overlay(frame, area, theme);
        }
    }

    fn render_env_field(&self, frame: &mut Frame, area: Rect, is_focused: bool, theme: &Theme) {
        let label_style = if is_focused {
            Style::default().fg(theme.accent).underlined()
        } else {
            Style::default().fg(theme.text)
        };

        if !self.env_list_expanded {
            // Collapsed view
            let count = self.extra_env.len();
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
                Span::styled("Environment:", label_style),
                Span::raw(" "),
                Span::styled(summary, summary_style),
            ]);
            frame.render_widget(Paragraph::new(line), area);
        } else {
            // Expanded view with list
            let mut lines: Vec<Line> = Vec::new();

            // Header with controls hint
            let header = Line::from(vec![
                Span::styled("Environment:", label_style),
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
                    for (i, entry) in self.extra_env.iter().enumerate() {
                        let prefix = if i == self.env_selected_index {
                            "  > "
                        } else {
                            "    "
                        };
                        lines.push(Line::from(Span::styled(
                            format!("{}{}", prefix, entry),
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
                    for (i, entry) in self.extra_env.iter().enumerate() {
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
                                format!("{}{}", prefix, entry),
                                Style::default().fg(theme.text),
                            )));
                        }
                    }
                }
            } else {
                // Normal list display
                if self.extra_env.is_empty() {
                    lines.push(Line::from(Span::styled(
                        "    (press 'a' to add KEY or KEY=VALUE)",
                        Style::default().fg(theme.dimmed),
                    )));
                } else {
                    for (i, entry) in self.extra_env.iter().enumerate() {
                        let is_selected = i == self.env_selected_index;
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
            }

            frame.render_widget(Paragraph::new(lines), area);
        }
    }

    fn render_inherited_field(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let label_style = Style::default().fg(theme.dimmed);
        let mut lines: Vec<Line> = Vec::new();

        lines.push(Line::from(Span::styled("Inherited Settings:", label_style)));

        if self.inherited_settings.is_empty() {
            lines.push(Line::from(Span::styled(
                "    (all defaults)",
                Style::default().fg(theme.dimmed),
            )));
        } else {
            for (label, value) in &self.inherited_settings {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("    {}: ", label),
                        Style::default().fg(theme.dimmed),
                    ),
                    Span::styled(value.as_str(), Style::default().fg(theme.accent)),
                ]));
            }
        }

        frame.render_widget(Paragraph::new(lines), area);
    }

    fn render_help_overlay(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let has_tool_selection = self.available_tools.len() > 1;
        let has_sandbox = self.docker_available;
        let show_sandbox_options_help = has_sandbox && self.sandbox_enabled;

        let dialog_width: u16 = HELP_DIALOG_WIDTH;
        let has_profile_selection = self.has_profile_selection();
        // Base fields: Title, Path, YOLO, Worktree, New Branch, Group + close hint
        let base_height: u16 = 20;
        let dialog_height: u16 = base_height
            + if has_profile_selection { 3 } else { 0 }
            + if has_tool_selection { 3 } else { 0 }
            + if has_sandbox { 3 } else { 0 }
            + if show_sandbox_options_help { 12 } else { 0 };

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
            if idx == 0 && !has_profile_selection {
                continue; // Profile
            }
            // idx 1 (Title), idx 2 (Path) always shown
            if idx == 3 && !has_tool_selection {
                continue; // Tool
            }
            // idx 4 (YOLO), 5 (Worktree), 6 (New Branch) always shown
            if idx == 7 && !has_sandbox {
                continue; // Sandbox
            }
            if (8..=9).contains(&idx) && !show_sandbox_options_help {
                continue; // Image, Env
            }
            // idx 10 (Group) always shown

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

            let status_text = if let Some(ref cmd) = self.current_hook {
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

            let used = 1 + visible_lines.len();
            let available = dialog_height.saturating_sub(4) as usize;
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
