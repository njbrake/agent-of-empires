//! Shared text input rendering component

use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use tui_input::Input;

use crate::tui::styles::Theme;

/// Renders a text input field with a label and cursor.
///
/// When focused, displays an inverse-video cursor over the current character position.
/// When not focused, displays the value (or placeholder if empty).
pub fn render_text_field(
    frame: &mut Frame,
    area: Rect,
    label: &str,
    input: &Input,
    is_focused: bool,
    placeholder: Option<&str>,
    theme: &Theme,
) {
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

    let mut spans = vec![Span::styled(label, label_style), Span::raw(" ")];

    if value.is_empty() && !is_focused {
        if let Some(placeholder_text) = placeholder {
            spans.push(Span::styled(placeholder_text, value_style));
        }
    } else if is_focused {
        let cursor_pos = input.visual_cursor();
        let cursor_style = Style::default().fg(theme.background).bg(theme.accent);

        // Split value into: before cursor, char at cursor, after cursor
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
    } else {
        spans.push(Span::styled(value, value_style));
    }

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}
