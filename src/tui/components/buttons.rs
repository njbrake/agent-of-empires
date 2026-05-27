//! Centered "[Yes]    [No]" button row used by destructive-confirm dialogs.
//!
//! Used by `confirm` and `delete_options`. If a third caller needs a
//! different button label set, generalize then.

use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::tui::styles::Theme;

/// Width of the rendered "[Yes]    [No]" row: 5 (Yes) + 4 spaces + 4
/// (No) = 13 cells. Kept as a constant so the click hit-test math
/// stays in lockstep with the renderer.
const YES_NO_ROW_WIDTH: u16 = 13;

/// Render a centered `[Yes]    [No]` row. Yes uses `theme.error`, No uses
/// `theme.running`; the unfocused button uses `theme.dimmed`. Returns
/// `(yes_rect, no_rect)` covering the visible glyphs, so callers that
/// want mouse-clickable buttons can hit-test the same cells the user
/// sees. Both rects collapse to zero-width if the row doesn't fit in
/// `area` (a degenerate render the caller can ignore).
pub fn render_yes_no(
    frame: &mut Frame,
    area: Rect,
    theme: &Theme,
    yes_focused: bool,
) -> (Rect, Rect) {
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

    if area.width < YES_NO_ROW_WIDTH || area.height == 0 {
        return (Rect::default(), Rect::default());
    }
    // Ratatui centers with `(width - line_len) / 2` for the left
    // offset; mirror that here so the rects line up with the actual
    // glyphs, not just the row.
    let left_pad = (area.width - YES_NO_ROW_WIDTH) / 2;
    let yes_x = area.x + left_pad;
    let no_x = yes_x + 9; // "[Yes]" + 4 spaces
    (
        Rect::new(yes_x, area.y, 5, 1),
        Rect::new(no_x, area.y, 4, 1),
    )
}
