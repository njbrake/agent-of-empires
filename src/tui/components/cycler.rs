//! Shared left/right cycler fields for the New and Restart session dialogs.
//!
//! Both modals let the user cycle a profile and an AI tool before launch.
//! Centralizing the span construction keeps the two dialogs visually
//! identical; previously the restart dialog carried its own divergent
//! `AI:` label and `< value >` tool styling.

use ratatui::prelude::*;

use crate::tui::styles::Theme;

/// Spans for a `Label: < value >` cycler, the profile-picker style.
///
/// When `count` is 1 or 0 the angle-bracket affordances are dropped and only
/// the value is shown.
pub fn profile_cycler_spans(
    label: &str,
    value: &str,
    count: usize,
    focused: bool,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let label_style = if focused {
        Style::default().fg(theme.accent).underlined()
    } else {
        Style::default().fg(theme.text)
    };
    let value_style = if focused {
        Style::default().fg(theme.accent).bold()
    } else {
        Style::default().fg(theme.accent)
    };
    let mut spans = vec![Span::styled(label.to_string(), label_style), Span::raw(" ")];
    if count > 1 {
        spans.push(Span::styled("< ", Style::default().fg(theme.dimmed)));
        spans.push(Span::styled(value.to_string(), value_style));
        spans.push(Span::styled(" >", Style::default().fg(theme.dimmed)));
    } else {
        spans.push(Span::styled(value.to_string(), value_style));
    }
    spans
}

/// Spans for a `Label: ← ● value  [n/m] →` cycler, the AI-tool picker style.
///
/// `index` is 0-based. When `total` is 1 or 0 the bullet, count badge, and
/// arrow affordances are dropped and only the value is shown, matching the
/// read-only single-tool rendering in the New Session dialog.
pub fn tool_cycler_spans(
    label: &str,
    value: &str,
    index: usize,
    total: usize,
    focused: bool,
    theme: &Theme,
) -> Vec<Span<'static>> {
    let label_style = if focused {
        Style::default().fg(theme.accent).underlined()
    } else {
        Style::default().fg(theme.text)
    };

    if total <= 1 {
        // Even when the cycler has nothing to cycle, the focused field
        // still shows its underline so users can see which row Tab landed
        // on.
        return vec![
            Span::styled(label.to_string(), label_style),
            Span::raw(" "),
            Span::styled(value.to_string(), Style::default().fg(theme.accent)),
        ];
    }

    let dimmed = Style::default().fg(theme.dimmed);
    let accent = Style::default().fg(theme.accent).bold();

    let mut spans = vec![Span::styled(label.to_string(), label_style), Span::raw(" ")];
    if focused {
        spans.push(Span::styled("← ", dimmed));
    }
    spans.push(Span::styled("● ", accent));
    spans.push(Span::styled(value.to_string(), accent));
    spans.push(Span::styled(format!("  [{}/{}]", index + 1, total), dimmed));
    if focused {
        spans.push(Span::styled("  →", dimmed));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contents(spans: &[Span<'static>]) -> Vec<String> {
        spans.iter().map(|s| s.content.to_string()).collect()
    }

    #[test]
    fn profile_cycler_shows_brackets_when_multiple() {
        let theme = Theme::default();
        let spans = profile_cycler_spans("Profile:", "work", 3, false, &theme);
        assert_eq!(contents(&spans), ["Profile:", " ", "< ", "work", " >"]);
    }

    #[test]
    fn profile_cycler_drops_brackets_when_single() {
        let theme = Theme::default();
        let spans = profile_cycler_spans("Profile:", "default", 1, false, &theme);
        assert_eq!(contents(&spans), ["Profile:", " ", "default"]);
    }

    #[test]
    fn profile_cycler_underlines_label_when_focused() {
        let theme = Theme::default();
        let spans = profile_cycler_spans("Profile:", "work", 3, true, &theme);
        assert!(spans[0].style.add_modifier.contains(Modifier::UNDERLINED));
    }

    #[test]
    fn tool_cycler_focused_shows_arrows_and_badge() {
        let theme = Theme::default();
        let spans = tool_cycler_spans("Tool:", "claude", 0, 3, true, &theme);
        assert_eq!(
            contents(&spans),
            ["Tool:", " ", "← ", "● ", "claude", "  [1/3]", "  →"]
        );
    }

    #[test]
    fn tool_cycler_unfocused_drops_arrows_keeps_badge() {
        let theme = Theme::default();
        let spans = tool_cycler_spans("Tool:", "codex", 1, 3, false, &theme);
        assert_eq!(contents(&spans), ["Tool:", " ", "● ", "codex", "  [2/3]"]);
    }

    #[test]
    fn tool_cycler_single_tool_is_plain() {
        let theme = Theme::default();
        let spans = tool_cycler_spans("Tool:", "claude", 0, 1, false, &theme);
        assert_eq!(contents(&spans), ["Tool:", " ", "claude"]);
    }

    #[test]
    fn tool_cycler_single_tool_underlines_label_when_focused() {
        // Single-tool early return must still respect focus so users
        // can see which row Tab landed on.
        let theme = Theme::default();
        let spans = tool_cycler_spans("Tool:", "claude", 0, 1, true, &theme);
        assert!(spans[0].style.add_modifier.contains(Modifier::UNDERLINED));
    }
}
