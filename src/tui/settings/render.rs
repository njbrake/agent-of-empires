//! Rendering for the settings view

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{
        Block, Borders, Clear, List, ListItem, Padding, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState,
    },
    Frame,
};
use tui_input::Input;

use super::{FieldValue, SettingsCategory, SettingsFocus, SettingsScope, SettingsView};
use crate::tui::styles::Theme;

/// Detect if we're running over SSH
fn is_ssh_session() -> bool {
    std::env::var("SSH_CONNECTION").is_ok()
        || std::env::var("SSH_CLIENT").is_ok()
        || std::env::var("SSH_TTY").is_ok()
}

impl SettingsView {
    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        // Clear the area
        frame.render_widget(Clear, area);

        // Main layout: title bar, content, footer
        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // Title/tabs
                Constraint::Min(10),   // Content
                Constraint::Length(3), // Footer/help
            ])
            .split(area);

        self.render_header(frame, layout[0], theme);
        self.render_content(frame, layout[1], theme);
        self.render_footer(frame, layout[2], theme);
    }

    fn render_header(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let block = Block::default()
            .borders(Borders::BOTTOM)
            .border_style(Style::default().fg(theme.border));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let modified = if self.has_changes { " *" } else { "" };

        let scope_style = |scope: SettingsScope| -> Style {
            if self.scope == scope {
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.dimmed)
            }
        };

        let global_style = scope_style(SettingsScope::Global);
        let profile_style = scope_style(SettingsScope::Profile);

        let mut spans = vec![
            Span::styled("  Settings", Style::default().fg(theme.text)),
            Span::styled(modified, Style::default().fg(theme.error)),
            Span::raw("    "),
            Span::styled("[ ", Style::default().fg(theme.border)),
            Span::styled("Global", global_style),
            Span::styled(" ]", Style::default().fg(theme.border)),
            Span::raw("  "),
            Span::styled("[ ", Style::default().fg(theme.border)),
            Span::styled(format!("Profile: {}", self.profile), profile_style),
            Span::styled(" ]", Style::default().fg(theme.border)),
        ];

        // Show Repo tab when a project path is available
        if self.project_path.is_some() {
            let repo_style = scope_style(SettingsScope::Repo);
            spans.push(Span::raw("  "));
            spans.push(Span::styled("[ ", Style::default().fg(theme.border)));
            spans.push(Span::styled("Repo", repo_style));
            spans.push(Span::styled(" ]", Style::default().fg(theme.border)));
        }

        frame.render_widget(Paragraph::new(Line::from(spans)), inner);
    }

    fn render_content(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        // Split into categories (left) and fields (right)
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(20), // Categories
                Constraint::Min(40),    // Fields
            ])
            .split(area);

        self.render_categories(frame, layout[0], theme);
        self.render_fields(frame, layout[1], theme);
    }

    fn render_categories(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let is_focused = self.focus == SettingsFocus::Categories;

        let border_style = if is_focused {
            Style::default().fg(theme.accent)
        } else {
            Style::default().fg(theme.border)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .padding(Padding::horizontal(1));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let items: Vec<ListItem> = self
            .categories
            .iter()
            .enumerate()
            .map(|(i, cat)| {
                let style = if i == self.selected_category {
                    if is_focused {
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme.text)
                    }
                } else {
                    Style::default().fg(theme.dimmed)
                };

                let prefix = if i == self.selected_category {
                    "> "
                } else {
                    "  "
                };

                ListItem::new(format!("{}{}", prefix, cat.label())).style(style)
            })
            .collect();

        let list = List::new(items);
        frame.render_widget(list, inner);
    }

    fn render_fields(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let is_focused = self.focus == SettingsFocus::Fields;

        let border_style = if is_focused {
            Style::default().fg(theme.accent)
        } else {
            Style::default().fg(theme.border)
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .padding(Padding::new(1, 1, 0, 0));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.fields.is_empty() {
            let msg = if self.scope == SettingsScope::Repo {
                "No repo-level settings for this category"
            } else {
                "No settings in this category"
            };
            let msg = Paragraph::new(msg).style(Style::default().fg(theme.dimmed));
            frame.render_widget(msg, inner);
            return;
        }

        // Show SSH warning for Sound category
        let current_category = self.categories[self.selected_category];
        let warning_offset = if current_category == SettingsCategory::Sound && is_ssh_session() {
            let warning = vec![
                Line::from(vec![
                    Span::styled("⚠ ", Style::default().fg(theme.waiting)),
                    Span::styled(
                        "Warning: Audio playback doesn't work over SSH",
                        Style::default().fg(theme.waiting),
                    ),
                ]),
                Line::from(vec![Span::styled(
                    "  Sounds require local terminal with audio output.",
                    Style::default().fg(theme.dimmed),
                )]),
                Line::from(""),
            ];
            let warning_widget = Paragraph::new(warning);
            let warning_area = Rect {
                x: inner.x,
                y: inner.y,
                width: inner.width,
                height: 3,
            };
            frame.render_widget(warning_widget, warning_area);
            3u16
        } else {
            0u16
        };

        // Reserve space for messages at the bottom
        let has_message = self.error_message.is_some() || self.success_message.is_some();
        let message_height: u16 = if has_message { 2 } else { 0 };
        let fields_viewport_height = inner
            .height
            .saturating_sub(message_height)
            .saturating_sub(warning_offset);
        self.fields_viewport_height = fields_viewport_height;

        // Calculate total content height
        let mut total_content_height = 0u16;
        for (i, field) in self.fields.iter().enumerate() {
            if i > 0 {
                total_content_height += 1; // spacing between fields
            }
            total_content_height += self.field_height(field, i);
        }

        let scroll_offset = self.fields_scroll_offset;

        // Render fields with scroll offset applied
        let mut y_pos = 0u16; // absolute position in content space
        for (i, field) in self.fields.iter().enumerate() {
            let field_h = self.field_height(field, i);
            let field_top = y_pos;
            let field_bottom = y_pos + field_h;

            // Skip fields entirely above the viewport
            if field_bottom <= scroll_offset {
                y_pos += field_h + 1;
                continue;
            }

            // Stop if we're past the viewport
            if field_top >= scroll_offset + fields_viewport_height {
                break;
            }

            let visible_y = field_top.saturating_sub(scroll_offset);
            let is_selected = i == self.selected_field && is_focused;
            let field_area = Rect {
                x: inner.x,
                y: inner.y + visible_y + warning_offset,
                width: inner.width,
                height: field_h.min(fields_viewport_height.saturating_sub(visible_y)),
            };

            self.render_field(frame, field_area, field, i, is_selected, theme);
            y_pos += field_h + 1; // +1 for spacing
        }

        // Render scrollbar if content overflows
        if total_content_height > fields_viewport_height {
            let scrollbar_area = Rect {
                x: area.x + area.width - 1,
                y: area.y + 1,
                width: 1,
                height: area.height.saturating_sub(2),
            };

            let mut scrollbar_state = ScrollbarState::new(
                total_content_height.saturating_sub(fields_viewport_height) as usize,
            )
            .position(scroll_offset as usize);

            frame.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .track_style(Style::default().fg(theme.border))
                    .thumb_style(Style::default().fg(theme.dimmed)),
                scrollbar_area,
                &mut scrollbar_state,
            );
        }

        // Render messages at the bottom if present
        if let Some(ref error) = self.error_message {
            let msg_area = Rect {
                x: inner.x,
                y: inner.y + inner.height.saturating_sub(2),
                width: inner.width,
                height: 1,
            };
            let msg = Paragraph::new(error.as_str()).style(Style::default().fg(theme.error));
            frame.render_widget(msg, msg_area);
        } else if let Some(ref success) = self.success_message {
            let msg_area = Rect {
                x: inner.x,
                y: inner.y + inner.height.saturating_sub(2),
                width: inner.width,
                height: 1,
            };
            let msg = Paragraph::new(success.as_str()).style(Style::default().fg(theme.running));
            frame.render_widget(msg, msg_area);
        }
    }

    pub(super) fn field_height(&self, field: &super::SettingField, index: usize) -> u16 {
        match &field.value {
            FieldValue::List(items) => {
                if self.list_edit_state.is_some() && index == self.selected_field {
                    // label + description + header + items + add prompt
                    1 + 1 + 1 + items.len() as u16 + 1
                } else {
                    1 + 1 + 1 // Label + description + summary
                }
            }
            _ => 1 + 1 + 1, // Label + description + value
        }
    }

    fn render_field(
        &self,
        frame: &mut Frame,
        area: Rect,
        field: &super::SettingField,
        index: usize,
        is_selected: bool,
        theme: &Theme,
    ) {
        let label_style = if is_selected {
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.text)
        };

        // Show override indicator for profile scope
        let override_indicator = if field.has_override && self.scope == SettingsScope::Profile {
            Span::styled(" (override)", Style::default().fg(theme.accent))
        } else {
            Span::raw("")
        };

        let label = Line::from(vec![
            Span::styled(field.label, label_style),
            override_indicator,
        ]);

        frame.render_widget(Paragraph::new(label), area);

        let description_area = Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: 1,
        };
        frame.render_widget(
            Paragraph::new(field.description).style(Style::default().fg(theme.dimmed)),
            description_area,
        );

        let value_area = Rect {
            y: area.y + 1,
            ..area
        };

        match &field.value {
            FieldValue::Bool(value) => {
                self.render_bool_field(frame, value_area, *value, is_selected, theme);
            }
            FieldValue::Text(value) => {
                self.render_text_field(frame, value_area, value, index, is_selected, theme);
            }
            FieldValue::OptionalText(value) => {
                let display = value.as_deref().unwrap_or("");
                self.render_text_field(frame, value_area, display, index, is_selected, theme);
            }
            FieldValue::Number(value) => {
                self.render_number_field(frame, value_area, *value, index, is_selected, theme);
            }
            FieldValue::Select { selected, options } => {
                self.render_select_field(frame, value_area, *selected, options, is_selected, theme);
            }
            FieldValue::List(items) => {
                self.render_list_field(frame, value_area, items, index, is_selected, theme);
            }
        }
    }

    fn render_bool_field(
        &self,
        frame: &mut Frame,
        area: Rect,
        value: bool,
        is_selected: bool,
        theme: &Theme,
    ) {
        let value_area = Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: 1,
        };

        let checkbox = if value { "[x]" } else { "[ ]" };
        let style = if is_selected {
            Style::default().fg(theme.accent)
        } else {
            Style::default().fg(theme.dimmed)
        };

        let text = format!(
            "{} {}",
            checkbox,
            if value { "Enabled" } else { "Disabled" }
        );
        frame.render_widget(Paragraph::new(text).style(style), value_area);
    }

    fn render_text_field(
        &self,
        frame: &mut Frame,
        area: Rect,
        value: &str,
        index: usize,
        is_selected: bool,
        theme: &Theme,
    ) {
        let value_area = Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width.min(50),
            height: 1,
        };

        let is_editing = self.editing_input.is_some() && index == self.selected_field;

        if is_editing {
            // Render with inverse-video cursor
            let input = self.editing_input.as_ref().unwrap();
            self.render_input_with_cursor(frame, value_area, input, theme);
        } else {
            let style = if is_selected {
                Style::default().fg(theme.accent)
            } else {
                Style::default().fg(theme.dimmed)
            };

            let display = if value.is_empty() {
                "(empty)".to_string()
            } else {
                value.to_string()
            };

            frame.render_widget(Paragraph::new(display).style(style), value_area);
        }
    }

    /// Build spans for text with an inverse-video cursor at the given position
    fn build_cursor_spans(value: &str, cursor_pos: usize, theme: &Theme) -> Vec<Span<'static>> {
        let value_style = Style::default().fg(theme.accent);
        let cursor_style = Style::default().fg(theme.background).bg(theme.accent);

        let before: String = value.chars().take(cursor_pos).collect();
        let cursor_char: String = value
            .chars()
            .nth(cursor_pos)
            .map(|c| c.to_string())
            .unwrap_or_else(|| " ".to_string());
        let after: String = value.chars().skip(cursor_pos + 1).collect();

        let mut spans = Vec::new();
        if !before.is_empty() {
            spans.push(Span::styled(before, value_style));
        }
        spans.push(Span::styled(cursor_char, cursor_style));
        if !after.is_empty() {
            spans.push(Span::styled(after, value_style));
        }
        spans
    }

    /// Render an Input with inverse-video cursor styling
    fn render_input_with_cursor(
        &self,
        frame: &mut Frame,
        area: Rect,
        input: &Input,
        theme: &Theme,
    ) {
        let spans = Self::build_cursor_spans(input.value(), input.visual_cursor(), theme);
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    /// Render a list item with prefix and inverse-video cursor
    fn render_list_item_with_cursor(
        &self,
        frame: &mut Frame,
        area: Rect,
        prefix: &str,
        input: &Input,
        theme: &Theme,
    ) {
        let value_style = Style::default().fg(theme.accent);
        let mut spans = vec![Span::styled(prefix.to_string(), value_style)];
        spans.extend(Self::build_cursor_spans(
            input.value(),
            input.visual_cursor(),
            theme,
        ));
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_number_field(
        &self,
        frame: &mut Frame,
        area: Rect,
        value: u64,
        index: usize,
        is_selected: bool,
        theme: &Theme,
    ) {
        let value_area = Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width.min(20),
            height: 1,
        };

        let is_editing = self.editing_input.is_some() && index == self.selected_field;

        if is_editing {
            // Render with inverse-video cursor
            let input = self.editing_input.as_ref().unwrap();
            self.render_input_with_cursor(frame, value_area, input, theme);
        } else {
            let style = if is_selected {
                Style::default().fg(theme.accent)
            } else {
                Style::default().fg(theme.dimmed)
            };

            frame.render_widget(Paragraph::new(value.to_string()).style(style), value_area);
        }
    }

    fn render_select_field(
        &self,
        frame: &mut Frame,
        area: Rect,
        selected: usize,
        options: &[String],
        is_selected: bool,
        theme: &Theme,
    ) {
        let value_area = Rect {
            x: area.x,
            y: area.y + 1,
            width: area.width,
            height: 1,
        };

        let style = if is_selected {
            Style::default().fg(theme.accent)
        } else {
            Style::default().fg(theme.dimmed)
        };

        let display = options.get(selected).map(|s| s.as_str()).unwrap_or("?");
        let arrows = if is_selected { " < >" } else { "" };
        frame.render_widget(
            Paragraph::new(format!("{}{}", display, arrows)).style(style),
            value_area,
        );
    }

    fn render_list_field(
        &self,
        frame: &mut Frame,
        area: Rect,
        items: &[String],
        index: usize,
        is_selected: bool,
        theme: &Theme,
    ) {
        let is_expanded = self.list_edit_state.is_some() && index == self.selected_field;

        if !is_expanded {
            // Collapsed view - show count
            let value_area = Rect {
                x: area.x,
                y: area.y + 1,
                width: area.width,
                height: 1,
            };

            let style = if is_selected {
                Style::default().fg(theme.accent)
            } else {
                Style::default().fg(theme.dimmed)
            };

            let text = if items.is_empty() {
                "(empty)".to_string()
            } else {
                format!("[{} items]", items.len())
            };

            frame.render_widget(Paragraph::new(text).style(style), value_area);
        } else {
            // Expanded view - show all items
            let list_state = self.list_edit_state.as_ref().unwrap();

            let header_area = Rect {
                x: area.x,
                y: area.y + 1,
                width: area.width,
                height: 1,
            };

            let header = Line::from(vec![
                Span::styled("Items: ", Style::default().fg(theme.dimmed)),
                Span::styled(
                    "(a)dd (d)elete (Enter)edit (Esc)close",
                    Style::default().fg(theme.dimmed),
                ),
            ]);
            frame.render_widget(Paragraph::new(header), header_area);

            // Render items
            for (i, item) in items.iter().enumerate() {
                let item_y = area.y + 2 + i as u16;
                if item_y >= area.y + area.height {
                    break;
                }

                let item_area = Rect {
                    x: area.x + 2,
                    y: item_y,
                    width: area.width.saturating_sub(2),
                    height: 1,
                };

                let style = if i == list_state.selected_index {
                    Style::default()
                        .fg(theme.accent)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme.dimmed)
                };

                let prefix = if i == list_state.selected_index {
                    "> "
                } else {
                    "  "
                };

                // If editing this item (not adding new), render with cursor
                if let Some(input) = list_state
                    .editing_item
                    .as_ref()
                    .filter(|_| i == list_state.selected_index && !list_state.adding_new)
                {
                    self.render_list_item_with_cursor(frame, item_area, prefix, input, theme);
                } else {
                    let display = format!("{}{}", prefix, item);
                    frame.render_widget(Paragraph::new(display).style(style), item_area);
                }
            }

            // Show add prompt if adding new
            if list_state.adding_new {
                let add_y = area.y + 2 + items.len() as u16;
                if add_y < area.y + area.height {
                    let add_area = Rect {
                        x: area.x + 2,
                        y: add_y,
                        width: area.width.saturating_sub(2),
                        height: 1,
                    };

                    if let Some(input) = &list_state.editing_item {
                        self.render_list_item_with_cursor(frame, add_area, "> ", input, theme);
                    }
                }
            }
        }
    }

    fn render_footer(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(theme.border));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        let help_text = if self.editing_input.is_some() {
            "Enter: confirm | Esc: cancel"
        } else if self.list_edit_state.is_some() {
            "a: add | d: delete | Enter: edit | Esc: close list"
        } else {
            "Tab: switch scope | Arrow keys: navigate | Enter: edit | Space: toggle | Ctrl+s: save | Esc: close"
        };

        let help = Paragraph::new(help_text)
            .style(Style::default().fg(theme.dimmed))
            .alignment(ratatui::layout::Alignment::Center);

        frame.render_widget(help, inner);
    }
}
