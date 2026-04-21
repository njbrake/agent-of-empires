//! Centered "[label]    [label]" button rows for dialog confirmations.
//!
//! Used by `confirm`, `delete_options`, and the Save/Cancel rows in editor
//! dialogs. Each button declares its label, the colour to use when focused,
//! and whether it is currently focused; unfocused buttons fall back to
//! `theme.dimmed`.

use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::tui::styles::Theme;

#[derive(Clone, Copy)]
pub struct DialogButton<'a> {
    pub label: &'a str,
    pub focused_color: Color,
    pub focused: bool,
}

/// Render `buttons` left-to-right with 4-space gaps, centered in `area`.
pub fn render_buttons(frame: &mut Frame, area: Rect, theme: &Theme, buttons: &[DialogButton]) {
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(buttons.len() * 2);
    for (i, b) in buttons.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw("    "));
        }
        let style = if b.focused {
            Style::default().fg(b.focused_color).bold()
        } else {
            Style::default().fg(theme.dimmed)
        };
        spans.push(Span::styled(format!("[{}]", b.label), style));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).alignment(Alignment::Center),
        area,
    );
}

/// Convenience: the canonical Yes / No row used by destructive confirms.
/// Yes uses `theme.error`, No uses `theme.running`.
pub fn render_yes_no(frame: &mut Frame, area: Rect, theme: &Theme, yes_focused: bool) {
    render_buttons(
        frame,
        area,
        theme,
        &[
            DialogButton {
                label: "Yes",
                focused_color: theme.error,
                focused: yes_focused,
            },
            DialogButton {
                label: "No",
                focused_color: theme.running,
                focused: !yes_focused,
            },
        ],
    );
}
