//! Rendering for the settings view

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Padding, Paragraph},
    Frame,
};
use tui_input::Input;

use super::{FieldValue, SettingsFocus, SettingsScope, SettingsView};
use crate::tui::styles::Theme;

impl SettingsView {
    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
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

        // Tabs: [ Global ] [ Profile: name ]
        let global_style = if self.scope == SettingsScope::Global {
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.dimmed)
        };

        let profile_style = if self.scope == SettingsScope::Profile {
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.dimmed)
        };

        let modified = if self.has_changes { " *" } else { "" };

        let tabs = Line::from(vec![
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
        ]);

        frame.render_widget(Paragraph::new(tabs), inner);
    }

    fn render_content(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
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

    fn render_fields(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
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
            let msg = Paragraph::new("No settings in this category")
                .style(Style::default().fg(theme.dimmed));
            frame.render_widget(msg, inner);
            return;
        }

        // Calculate how much space each field needs
        let mut y_offset = 0u16;
        for (i, field) in self.fields.iter().enumerate() {
            if y_offset >= inner.height {
                break;
            }

            let is_selected = i == self.selected_field && is_focused;
            let field_area = Rect {
                x: inner.x,
                y: inner.y + y_offset,
                width: inner.width,
                height: self.field_height(field, i),
            };

            self.render_field(frame, field_area, field, i, is_selected, theme);
            y_offset += field_area.height + 1; // +1 for spacing
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

    fn field_height(&self, field: &super::SettingField, index: usize) -> u16 {
        match &field.value {
            FieldValue::List(items) => {
                // If this field's list is expanded, show all items
                if self.list_edit_state.is_some() && index == self.selected_field {
                    3 + items.len() as u16 + 1 // label + items + add prompt
                } else {
                    2 // Just label and summary
                }
            }
            _ => 2, // Label + value
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

        match &field.value {
            FieldValue::Bool(value) => {
                self.render_bool_field(frame, area, *value, is_selected, theme);
            }
            FieldValue::Text(value) => {
                self.render_text_field(frame, area, value, index, is_selected, theme);
            }
            FieldValue::OptionalText(value) => {
                let display = value.as_deref().unwrap_or("");
                self.render_text_field(frame, area, display, index, is_selected, theme);
            }
            FieldValue::Number(value) => {
                self.render_number_field(frame, area, *value, index, is_selected, theme);
            }
            FieldValue::Select { selected, options } => {
                self.render_select_field(frame, area, *selected, options, is_selected, theme);
            }
            FieldValue::List(items) => {
                self.render_list_field(frame, area, items, index, is_selected, theme);
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
        spans.extend(Self::build_cursor_spans(input.value(), input.visual_cursor(), theme));
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
            // Collapsed view - just show count
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
                "(empty list)".to_string()
            } else {
                format!("[{} items] Press Enter to expand", items.len())
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

                // If editing this item, render with cursor
                if list_state.editing_item.is_some() && i == list_state.selected_index {
                    let input = list_state.editing_item.as_ref().unwrap();
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
