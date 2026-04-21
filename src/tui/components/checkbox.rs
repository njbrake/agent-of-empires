//! Shared checkbox-line builder for dialog rendering.
//!
//! `delete_options` and `group_delete_options` both render rows of `[x]` /
//! `[ ]` checkboxes with a focus highlight. The shape (indent, checkbox glyph,
//! label, optional `(detail)` suffix) is identical; only the highlight style
//! differs slightly between dialogs (some use `bold` on the box, some use
//! `underlined` on the label). The caller passes a [`CheckboxStyle`] to
//! choose, keeping per-dialog visual tweaks while sharing layout + glyph
//! logic.

use ratatui::prelude::*;

use crate::tui::styles::Theme;

/// How a focused/checked checkbox should be styled. Each dialog picks its
/// own combination so neither has to change.
#[derive(Clone, Copy)]
pub struct CheckboxStyle {
    /// Color for both the box glyph and label when the row is focused.
    pub focused_color: Color,
    /// Color for the box glyph when checked but not focused.
    pub checked_color: Color,
    /// `bold()` the box glyph (used by `delete_options`).
    pub focused_glyph_bold: bool,
    /// `underlined()` the label (used by both, slightly different scope).
    pub focused_label_underlined: bool,
    /// `underlined()` the entire row when focused (used by group dialog).
    pub focused_row_underlined: bool,
}

impl CheckboxStyle {
    /// Style used by the per-session delete dialog: bold box, underlined label.
    pub fn delete_session(theme: &Theme) -> Self {
        Self {
            focused_color: theme.accent,
            checked_color: theme.error,
            focused_glyph_bold: true,
            focused_label_underlined: true,
            focused_row_underlined: false,
        }
    }

    /// Style used by the group-delete dialog: underline-the-row highlight.
    pub fn delete_group(theme: &Theme) -> Self {
        Self {
            focused_color: theme.error,
            checked_color: theme.error,
            focused_glyph_bold: false,
            focused_label_underlined: false,
            focused_row_underlined: true,
        }
    }
}

/// Build a single checkbox row.
///
/// `indent` is the number of leading spaces (use 0 / 4 / 8 to match the
/// existing dialogs).
pub fn checkbox_line(
    theme: &Theme,
    label: &str,
    detail: Option<&str>,
    indent: usize,
    checked: bool,
    focused: bool,
    style: CheckboxStyle,
) -> Line<'static> {
    let glyph = if checked { "[x]" } else { "[ ]" };

    let mut box_style = if focused {
        Style::default().fg(style.focused_color)
    } else if checked {
        Style::default().fg(style.checked_color)
    } else {
        Style::default().fg(theme.dimmed)
    };
    if focused && style.focused_glyph_bold {
        box_style = box_style.bold();
    }
    if focused && style.focused_row_underlined {
        box_style = box_style.underlined();
    }

    let mut label_style = if focused {
        Style::default().fg(style.focused_color)
    } else if checked {
        Style::default().fg(style.checked_color)
    } else {
        Style::default().fg(theme.text)
    };
    if focused && style.focused_label_underlined {
        label_style = label_style.underlined();
    }
    if focused && style.focused_row_underlined {
        label_style = label_style.underlined();
    }

    let mut spans = Vec::with_capacity(5);
    if indent > 0 {
        spans.push(Span::raw(" ".repeat(indent)));
    }
    spans.push(Span::styled(glyph.to_string(), box_style));
    spans.push(Span::raw(" "));
    spans.push(Span::styled(label.to_string(), label_style));
    if let Some(d) = detail {
        spans.push(Span::raw(" "));
        spans.push(Span::styled(
            format!("({})", d),
            Style::default().fg(theme.dimmed),
        ));
    }
    Line::from(spans)
}
