//! Three-pane render of a cockpit session: transcript / status banner /
//! composer. Tool-card breakdowns are intentionally minimal in the MVP
//! (one-liner per tool call); rich diff / image / file previews are
//! deferred to the followup issues called out in the implementation
//! plan. Press `o` from the transcript pane to open the web cockpit
//! for full-fidelity inspection.

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Frame;

use super::input::Focus;
use super::reducer::{ActivityRow, CockpitTranscript, NoteKind, ToolCallRow};
use super::state::CockpitViewState;
use crate::cockpit::approvals::ApprovalDecision;
use crate::tui::styles::Theme;

pub fn render(frame: &mut Frame, area: Rect, theme: &Theme, state: &CockpitViewState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),    // transcript
            Constraint::Length(1), // status line
            Constraint::Length(composer_height(state)),
        ])
        .split(area);

    render_transcript(frame, chunks[0], theme, state);
    render_status(frame, chunks[1], theme, state);
    render_composer(frame, chunks[2], theme, state);
}

/// Top + bottom border rows wrapping the composer textarea.
const COMPOSER_BORDER_ROWS: u16 = 2;
/// Maximum content rows the composer is allowed to take before the
/// transcript starts losing space. Multi-line prompts beyond this
/// scroll inside the textarea instead of growing the pane.
const COMPOSER_MAX_CONTENT_ROWS: u16 = 6;

fn composer_height(state: &CockpitViewState) -> u16 {
    // Composer is `1 + COMPOSER_BORDER_ROWS = 3` rows tall by default,
    // growing one row per typed newline up to
    // `COMPOSER_MAX_CONTENT_ROWS + COMPOSER_BORDER_ROWS = 8` rows so
    // multi-line prompts don't squash the transcript.
    let lines = state.composer.lines().len().max(1) as u16;
    lines.clamp(1, COMPOSER_MAX_CONTENT_ROWS) + COMPOSER_BORDER_ROWS
}

fn render_transcript(frame: &mut Frame, area: Rect, theme: &Theme, state: &CockpitViewState) {
    let title = format!(
        " Cockpit · {}{} ",
        state.session_id,
        match state.transcript.current_mode.as_deref() {
            Some(m) => format!(" · mode: {m}"),
            None => String::new(),
        }
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(border_style(theme, state, Focus::Transcript));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines = transcript_lines(&state.transcript, state.selected_approval, state.focus);
    // Clamp scroll against the *wrapped* visual row count, not
    // `lines.len()`. Streaming `AgentMessage` rows grew text inside
    // a single logical line: Paragraph's wrap inflated the
    // rendered row count while `lines.len()` stayed constant, so
    // `state.scroll_offset = u16::MAX` (stick to bottom) clipped
    // short of the newest chunk. Tool calls didn't show the bug
    // because each call adds whole new Line entries.
    let total = visual_line_count(&lines, inner.width);
    let max = total.saturating_sub(inner.height);
    let scroll = (state.scroll_offset.min(max), 0);
    let para = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll(scroll);
    frame.render_widget(para, inner);
}

/// Estimate the number of terminal rows `lines` will occupy when
/// rendered into a paragraph of width `width`. Each `Line`'s display
/// width divided by the available columns, rounded up, summed. Used
/// to keep `scroll_offset = u16::MAX` pinned to the bottom as
/// streaming chunks grow inside a single logical line.
fn visual_line_count(lines: &[Line], width: u16) -> u16 {
    if width == 0 {
        return lines.len() as u16;
    }
    let w = width as usize;
    let mut total: usize = 0;
    for line in lines {
        let lw = line.width().max(1);
        total = total.saturating_add(lw.div_ceil(w));
    }
    total.min(u16::MAX as usize) as u16
}

fn render_status(frame: &mut Frame, area: Rect, theme: &Theme, state: &CockpitViewState) {
    let mut spans: Vec<Span> = Vec::new();
    if let Some(toast) = &state.toast {
        let color = match toast.kind {
            super::state::ToastKind::Info => theme.title,
            super::state::ToastKind::Error => theme.error,
        };
        spans.push(Span::styled(
            format!(" {} ", toast.text),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ));
    }
    if let Some(banner) = &state.transcript.status_text {
        spans.push(Span::styled(
            format!(" {banner} "),
            Style::default().fg(theme.title),
        ));
    }
    if state.transcript.context_primer_pending {
        spans.push(Span::styled(
            " context lost; next prompt re-primes ",
            Style::default().fg(theme.error),
        ));
    }
    if state.transcript.lagged {
        spans.push(Span::styled(
            " broadcast lagged; refetching ",
            Style::default().fg(theme.error),
        ));
    }
    if !state.transcript.pending_approvals.is_empty() {
        let n = state.transcript.pending_approvals.len();
        spans.push(Span::styled(
            format!(
                " {n} pending approval{}; Tab to focus ",
                if n == 1 { "" } else { "s" }
            ),
            Style::default().fg(theme.error),
        ));
    }
    if spans.is_empty() {
        // Footer help when nothing else is going on.
        spans.push(Span::styled(
            help_hint(state.focus),
            Style::default().fg(theme.hint),
        ));
    }
    let para = Paragraph::new(Line::from(spans));
    frame.render_widget(para, area);
}

fn render_composer(frame: &mut Frame, area: Rect, theme: &Theme, state: &CockpitViewState) {
    let title = match state.focus {
        Focus::Composer => " Composer (Enter=send, Shift+Enter=newline, Esc=back) ",
        _ => " Composer (Tab/i to focus) ",
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(border_style(theme, state, Focus::Composer));
    // ratatui-textarea borrows the Frame's buffer indirectly via
    // widget impl; render the block first, then the textarea inside.
    let inner = block.inner(area);
    frame.render_widget(block, area);
    frame.render_widget(&state.composer, inner);
    if matches!(state.focus, Focus::Composer) && inner.width > 0 && inner.height > 0 {
        let cursor = state.composer.screen_cursor();
        let max_x = inner.x.saturating_add(inner.width.saturating_sub(1));
        let max_y = inner.y.saturating_add(inner.height.saturating_sub(1));
        let cursor_x = inner.x.saturating_add(cursor.col as u16).min(max_x);
        let cursor_y = inner.y.saturating_add(cursor.row as u16).min(max_y);
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}

fn transcript_lines<'a>(
    transcript: &'a CockpitTranscript,
    selected_approval: Option<usize>,
    focus: Focus,
) -> Vec<Line<'a>> {
    let mut out: Vec<Line<'a>> = Vec::new();
    let mut approval_render_idx: usize = 0;
    for row in &transcript.rows {
        match row {
            ActivityRow::UserPrompt(text) => {
                out.push(Line::from(Span::styled(
                    format!("you  ▸ {text}"),
                    Style::default().add_modifier(Modifier::BOLD),
                )));
                out.push(Line::default());
            }
            ActivityRow::AgentMessage(text) => {
                for chunk_line in text.lines() {
                    out.push(Line::from(format!("aoe  {chunk_line}")));
                }
                if text.is_empty() {
                    out.push(Line::from("aoe  …"));
                }
                out.push(Line::default());
            }
            ActivityRow::ToolCall(tool) => {
                out.extend(render_tool_lines(tool));
                out.push(Line::default());
            }
            ActivityRow::Approval(row) => {
                let highlighted = focus == Focus::Approval
                    && selected_approval
                        .map(|i| i == approval_render_idx)
                        .unwrap_or(false);
                approval_render_idx += 1;
                let mut header = Vec::new();
                header.push(Span::raw(if highlighted { "▶ " } else { "  " }));
                header.push(Span::styled(
                    format!("approval · {} ", row.title),
                    Style::default().add_modifier(Modifier::BOLD),
                ));
                if row.destructive {
                    header.push(Span::styled(
                        "[destructive] ",
                        Style::default().add_modifier(Modifier::BOLD),
                    ));
                }
                header.push(Span::styled(
                    format!("nonce={}", row.nonce),
                    Style::default().add_modifier(Modifier::DIM),
                ));
                out.push(Line::from(header));
                let body = match row.decision {
                    Some(ApprovalDecision::Allow) => "  → allowed",
                    Some(ApprovalDecision::AllowAlways) => "  → allow-always",
                    Some(ApprovalDecision::Deny) => "  → denied",
                    Some(ApprovalDecision::Cancelled) => "  → cancelled",
                    None => "  press a / A / d to resolve, Esc to leave",
                };
                out.push(Line::from(body));
                out.push(Line::default());
            }
            ActivityRow::Plan(steps) => {
                out.push(Line::from(Span::styled(
                    "plan",
                    Style::default().add_modifier(Modifier::BOLD),
                )));
                for step in steps {
                    let marker = match step.status {
                        crate::cockpit::state::PlanStepStatus::Pending => "[ ]",
                        crate::cockpit::state::PlanStepStatus::InProgress => "[~]",
                        crate::cockpit::state::PlanStepStatus::Done => "[x]",
                        crate::cockpit::state::PlanStepStatus::Cancelled => "[-]",
                    };
                    out.push(Line::from(format!("  {marker} {}", step.title)));
                }
                out.push(Line::default());
            }
            ActivityRow::Note { kind, text } => {
                let modifier = match kind {
                    NoteKind::Info => Modifier::DIM,
                    NoteKind::Warning => Modifier::BOLD,
                    NoteKind::Error => Modifier::BOLD,
                };
                out.push(Line::from(Span::styled(
                    format!("· {text}"),
                    Style::default().add_modifier(modifier),
                )));
                out.push(Line::default());
            }
        }
    }
    if out.is_empty() {
        out.push(Line::from(Span::styled(
            "(no events yet, waiting for the agent…)",
            Style::default().add_modifier(Modifier::DIM),
        )));
    }
    out
}

/// Return the first `max_chars` characters of `s`, or `None` if `s`
/// is already short enough. Char-safe so an LLM response that places a
/// multi-byte codepoint at the truncation boundary doesn't panic the
/// TUI (byte-slicing `&s[..N]` would).
fn truncate_chars(s: &str, max_chars: usize) -> Option<String> {
    let mut iter = s.char_indices();
    if let Some((byte_idx, _)) = iter.nth(max_chars) {
        Some(s[..byte_idx].to_string())
    } else {
        None
    }
}

fn render_tool_lines(tool: &ToolCallRow) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let header = format!(
        "tool {} · {}",
        match tool.completed.as_ref() {
            None => "▶",
            Some(c) if c.ok => "✓",
            Some(_) => "✗",
        },
        tool.name
    );
    lines.push(Line::from(Span::styled(
        header,
        Style::default().add_modifier(Modifier::BOLD),
    )));
    if !tool.args.is_empty() {
        let truncated = match truncate_chars(&tool.args, 200) {
            Some(head) => format!("  $ {head}…"),
            None => format!("  $ {}", tool.args),
        };
        lines.push(Line::from(truncated));
    }
    if let Some(completion) = &tool.completed {
        let content = if completion.content.is_empty() {
            if completion.ok {
                "  (no output)".to_string()
            } else {
                "  (tool failed; press `o` for details)".to_string()
            }
        } else if let Some(head) = truncate_chars(&completion.content, 400) {
            format!("  {head}…\n  (output truncated; press `o` for full)")
        } else {
            completion
                .content
                .lines()
                .map(|l| format!("  {l}"))
                .collect::<Vec<_>>()
                .join("\n")
        };
        for line in content.lines() {
            lines.push(Line::from(line.to_string()));
        }
    }
    lines
}

fn border_style(theme: &Theme, state: &CockpitViewState, this_focus: Focus) -> Style {
    if state.focus == this_focus {
        Style::default().fg(theme.title)
    } else {
        Style::default().fg(theme.border)
    }
}

fn help_hint(focus: Focus) -> &'static str {
    match focus {
        Focus::Composer => " Enter=send · Shift+Enter=newline · Esc=back · Ctrl-C=cancel ",
        Focus::Transcript => " j/k=scroll · i=compose · Tab=approvals · o=browser · Esc=exit ",
        Focus::Approval => " a=allow · A=always · d=deny · Esc=back ",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visual_line_count_counts_wrapped_rows() {
        // 40 chars at width 10 wraps to 4 visual rows.
        let lines = vec![Line::from("a".repeat(40))];
        assert_eq!(visual_line_count(&lines, 10), 4);
    }

    #[test]
    fn visual_line_count_floors_empty_line_to_one() {
        // A logical empty line still occupies one row.
        let lines = vec![Line::default()];
        assert_eq!(visual_line_count(&lines, 10), 1);
    }

    #[test]
    fn visual_line_count_handles_zero_width() {
        // Degenerate area (e.g. during teardown); fall back to logical
        // line count so we don't divide by zero.
        let lines = vec![Line::from("x"), Line::from("y")];
        assert_eq!(visual_line_count(&lines, 0), 2);
    }

    #[test]
    fn visual_line_count_streaming_growth_advances_max_scroll() {
        // Regression for the agent-message auto-scroll bug: as a
        // single logical line grows, the visual row count must
        // grow so `scroll_offset = u16::MAX` keeps tracking the
        // bottom.
        let short = vec![Line::from("a".repeat(20))];
        let long = vec![Line::from("a".repeat(200))];
        assert!(visual_line_count(&long, 40) > visual_line_count(&short, 40));
    }

    #[test]
    fn truncate_chars_returns_none_when_already_short() {
        assert_eq!(truncate_chars("hi", 10), None);
    }

    #[test]
    fn truncate_chars_respects_utf8_codepoint_boundaries() {
        // Regression for the byte-slice panic: a 4-byte codepoint
        // straddling the requested byte boundary used to crash the
        // TUI with `byte index N is not a char boundary`.
        // 3 ASCII + 4-byte emoji (U+1F600) repeated; ask for 4 chars.
        let s = "abc😀def😀ghi😀";
        let head = truncate_chars(s, 4).expect("longer than 4 chars");
        assert_eq!(head, "abc😀");
        assert!(s.chars().count() > 4);
    }

    #[test]
    fn truncate_chars_handles_pure_multibyte_input() {
        // Pure non-ASCII (CJK ideographs are 3 bytes each in UTF-8).
        let s = "日本語のテスト";
        let head = truncate_chars(s, 3).expect("longer than 3 chars");
        assert_eq!(head, "日本語");
    }
}
