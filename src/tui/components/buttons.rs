//! Centered "[Yes]    [No]" button row used by destructive-confirm dialogs.
//!
//! Used by `confirm` and `delete_options`. If a third caller needs a
//! different button label set, generalize then.

use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::tui::styles::Theme;

/// Render a centered `[Yes]    [No]` row. Yes uses `theme.error`, No uses
/// `theme.running`; the unfocused button uses `theme.dimmed`.
pub fn render_yes_no(frame: &mut Frame, area: Rect, theme: &Theme, yes_focused: bool) {
    let yes_style = if yes_focused {
        Style::default().fg(theme.error).bold()
    } else {
        Style::default().fg(theme.dimmed)
    };
    let no_style = if yes_focused {
        Style::default().fg(theme.dimmed)
    } else {
        Style::default().fg(theme.running).bold()
    };
    let line = Line::from(vec![
        Span::styled("[Yes]", yes_style),
        Span::raw("    "),
        Span::styled("[No]", no_style),
    ]);
    frame.render_widget(Paragraph::new(line).alignment(Alignment::Center), area);
}
