//! Rendering for HomeView

use chrono::{DateTime, Utc};
use ratatui::prelude::*;
use ratatui::widgets::*;
use std::time::{Duration, Instant};

use rattles::presets::prelude as spinners;

use super::{
    get_indent, HomeView, TerminalMode, ViewMode, ICON_COLLAPSED, ICON_DELETING, ICON_ERROR,
    ICON_EXPANDED, ICON_IDLE, ICON_STOPPED, ICON_UNKNOWN,
};
use crate::session::config::GroupByMode;
use crate::session::{Item, Status};
use crate::tui::components::{HelpOverlay, Preview};
use crate::tui::responsive;
use crate::tui::styles::Theme;
use crate::update::UpdateInfo;

/// Derive a frame offset from a session's creation timestamp so that
/// sessions started at different times show visually distinct spinner positions.
fn session_offset(created_at: &DateTime<Utc>) -> usize {
    created_at.timestamp_millis() as usize
}

/// Extra rows captured beyond the visible window so moderate scrolls don't
/// force a fresh capture on every wheel tick. Cache invalidation uses the same
/// reserve to decide when the captured window can no longer cover the
/// requested scroll.
const CAPTURE_BUFFER: u16 = 20;

/// Number of pane lines to capture for the preview, accounting for the user's
/// scrollback offset. A small buffer is added so moderate scrolls don't force a
/// fresh capture on every wheel tick.
fn capture_lines_for(height: u16, scroll_offset: u16) -> usize {
    height
        .saturating_add(scroll_offset)
        .saturating_add(CAPTURE_BUFFER) as usize
}

/// Decide whether the cached capture window still covers the requested scroll.
/// Returns true when the cache must be re-captured because the visible window
/// (plus BUFFER headroom) would run past the end of the captured content.
fn scroll_exceeds_cache(cache_captured_lines: usize, height: u16, scroll_offset: u16) -> bool {
    let needed = (height as usize)
        .saturating_add(scroll_offset as usize)
        .saturating_add(CAPTURE_BUFFER as usize);
    needed > cache_captured_lines
}

/// Clamp the user's preview scroll offset to what the freshly captured pane
/// can actually render. Prevents the offset from drifting into "phantom"
/// territory (M3 from the multi-AI review) when tmux history is shorter than
/// `MAX_PREVIEW_SCROLL`.
fn clamp_scroll_to_capture(scroll_offset: u16, captured_lines: usize, area_height: u16) -> u16 {
    let visible = area_height.saturating_sub(1) as usize;
    let real_max = captured_lines.saturating_sub(visible) as u16;
    scroll_offset.min(real_max)
}

fn spinner_running(created_at: &DateTime<Utc>) -> &'static str {
    spinners::dots()
        .set_interval(Duration::from_millis(220))
        .offset(session_offset(created_at))
        .current_frame()
}

fn spinner_waiting(created_at: &DateTime<Utc>) -> &'static str {
    spinners::orbit()
        .set_interval(Duration::from_millis(400))
        .offset(session_offset(created_at))
        .current_frame()
}

fn spinner_starting(created_at: &DateTime<Utc>) -> &'static str {
    spinners::breathe()
        .set_interval(Duration::from_millis(180))
        .offset(session_offset(created_at))
        .current_frame()
}

/// Format a timestamp as a compact relative age (e.g. `3m`, `2h`, `4d`, `2mo`).
/// Returns an empty string for `None` so callers can unconditionally substitute
/// the result without guarding for absence.
fn format_relative_age(ts: Option<DateTime<Utc>>) -> String {
    let Some(ts) = ts else {
        return String::new();
    };
    let now = Utc::now();
    let secs = (now - ts).num_seconds();
    if secs <= 0 {
        return "<1m".to_string();
    }
    if secs < 60 {
        return "<1m".to_string();
    }
    let mins = secs / 60;
    if mins < 60 {
        return format!("{}m", mins);
    }
    let hours = mins / 60;
    if hours < 24 {
        return format!("{}h", hours);
    }
    let days = hours / 24;
    if days < 30 {
        return format!("{}d", days);
    }
    let months = days / 30;
    format!("{}mo", months)
}

/// Minimum column width required to render the last-activity column.
/// When the session list is narrower than this, the column is hidden entirely.
/// Compared against `inner.width` (list pane minus 2-char border), so this is
/// effectively `home_list_width - 2`. Keeping it at 30 lets the column appear
/// for users who set `home_list_width` in the 35–45 range (the common narrow-
/// pane setting) and for mobile clients with tight pane widths; the 6-char
/// age slot plus ~24 chars for title/branch still fits comfortably.
const LAST_ACTIVITY_MIN_WIDTH: u16 = 30;

/// Width reserved for the right-aligned last-activity column:
/// 5 chars for the label (e.g. `"<1m"`, `"30mo"`) + 1 char left padding.
const LAST_ACTIVITY_SLOT: usize = 6;

impl HomeView {
    pub fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
        update_info: Option<&UpdateInfo>,
        update_status: Option<&str>,
    ) {
        // Settings view takes over the whole screen
        if let Some(ref mut settings) = self.settings_view {
            settings.render(frame, area, theme);
            // Render unsaved changes confirmation dialog over settings
            if self.settings_close_confirm {
                if let Some(dialog) = &self.confirm_dialog {
                    dialog.render(frame, area, theme);
                }
            }
            return;
        }

        // Diff view takes over the whole screen
        if self.diff_view.is_some() {
            self.preview_area = Rect::default();
            self.diff_area = self.active_diff_area(area);
        }
        if let Some(ref mut diff) = self.diff_view {
            // Compute diff for selected file if not cached
            let _ = diff.get_current_diff();

            diff.render(frame, area, theme);
            return;
        }

        // Serve view takes over the whole screen
        #[cfg(feature = "serve")]
        if let Some(ref serve) = self.serve_view {
            serve.render(frame, area, theme);
            return;
        }

        // Layout: main area + status bar + optional update bar at bottom
        let constraints = if update_info.is_some() {
            vec![
                Constraint::Min(0),
                Constraint::Length(1),
                Constraint::Length(1),
            ]
        } else {
            vec![Constraint::Min(0), Constraint::Length(1)]
        };
        let main_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(area);

        // Below STACKED_BREAKPOINT (80 cols), put the list above the preview
        // instead of side-by-side. At 80 cols a side-by-side preview is only
        // ~45 cols (with default list_width 35), too cramped for output;
        // stacking gives the preview the full width.
        let available_width = main_chunks[0].width;
        if available_width < responsive::STACKED_BREAKPOINT {
            let main_height = main_chunks[0].height;
            let list_height = responsive::stacked_list_height(main_height);
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(list_height),
                    Constraint::Min(responsive::STACKED_PREVIEW_MIN),
                ])
                .split(main_chunks[0]);

            self.render_list(frame, chunks[0], theme);
            self.render_preview(frame, chunks[1], theme);
        } else {
            // Side-by-side: cap list width so the preview pane keeps its
            // usability floor (PREVIEW_MIN_WIDTH).
            let effective_list_width = self
                .list_width
                .min(available_width.saturating_sub(responsive::PREVIEW_MIN_WIDTH))
                .max(10);
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Length(effective_list_width),
                    Constraint::Min(responsive::PREVIEW_MIN_WIDTH),
                ])
                .split(main_chunks[0]);

            self.render_list(frame, chunks[0], theme);
            self.render_preview(frame, chunks[1], theme);
        }
        self.render_status_bar(frame, main_chunks[1], theme);

        if let Some(info) = update_info {
            self.render_update_bar(frame, main_chunks[2], theme, info, update_status);
        }

        // Render dialogs on top
        if self.show_help {
            HelpOverlay::render(frame, area, theme, self.sort_order, self.strict_hotkeys);
        }

        // Each Option<Dialog> field on HomeView gets the same render dispatch:
        // if present, call render(frame, area, theme). Macro-collapsed to keep
        // the list of active dialog types in one place — adding a new dialog
        // means adding one line here, not stamping out another five-line
        // if-let block.
        macro_rules! render_dialogs {
            ($($field:ident),* $(,)?) => {
                $(
                    if let Some(dialog) = &self.$field {
                        dialog.render(frame, area, theme);
                    }
                )*
            };
        }

        render_dialogs!(
            new_dialog,
            confirm_dialog,
            unified_delete_dialog,
            group_delete_options_dialog,
            rename_dialog,
            hooks_install_dialog,
            hook_trust_dialog,
            welcome_dialog,
            no_agents_dialog,
            changelog_dialog,
            info_dialog,
            profile_picker_dialog,
            send_message_dialog,
            update_confirm_dialog,
        );
    }

    fn active_diff_area(&self, area: Rect) -> Rect {
        let Some(diff) = &self.diff_view else {
            return Rect::default();
        };

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(10),
                Constraint::Length(3),
            ])
            .split(area);
        let content_area = layout[1];
        let effective_file_list_width = diff
            .file_list_width
            .min(content_area.width.saturating_sub(40))
            .max(5);
        let panes = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(effective_file_list_width),
                Constraint::Min(40),
            ])
            .split(content_area);
        Block::default().borders(Borders::ALL).inner(panes[1])
    }

    fn render_list(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let group_suffix = if self.group_by == GroupByMode::Project {
            " (by project)"
        } else {
            ""
        };
        let title = match self.view_mode {
            ViewMode::Agent => format!(
                " Agent of Empires [{}]{} ",
                self.active_profile_display(),
                group_suffix
            ),
            ViewMode::Terminal => format!(
                " Terminals [{}]{} ",
                self.active_profile_display(),
                group_suffix
            ),
        };
        let (border_color, title_color) = match self.view_mode {
            ViewMode::Agent => (theme.border, theme.title),
            ViewMode::Terminal => (theme.terminal_border, theme.terminal_border),
        };
        let block = Block::default()
            .borders(Borders::TOP | Borders::LEFT | Borders::BOTTOM)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color))
            .title(title)
            .title_style(Style::default().fg(title_color).bold())
            .padding(Padding::horizontal(1));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.instances().is_empty() && !self.has_any_groups() {
            let empty_text = vec![
                Line::from(""),
                Line::from("No sessions yet").style(Style::default().fg(theme.dimmed)),
                Line::from(""),
                Line::from("Press 'n' to create one").style(Style::default().fg(theme.hint)),
                Line::from("or 'aoe add .'").style(Style::default().fg(theme.hint)),
            ];
            let para = Paragraph::new(empty_text).alignment(Alignment::Center);
            frame.render_widget(para, inner);
            return;
        }

        let visible_height = if self.search_active {
            (inner.height as usize).saturating_sub(1)
        } else {
            inner.height as usize
        };
        let scroll = crate::tui::components::scroll::calculate_scroll(
            self.flat_items.len(),
            self.cursor,
            visible_height,
        );

        let mut lines: Vec<Line> = Vec::new();

        if scroll.has_more_above {
            lines.push(Line::from(Span::styled(
                format!("  [{} more above]", scroll.scroll_offset),
                Style::default().fg(theme.dimmed),
            )));
        }

        for (i, item) in self
            .flat_items
            .iter()
            .skip(scroll.scroll_offset)
            .take(scroll.list_visible)
            .enumerate()
        {
            let abs_idx = i + scroll.scroll_offset;
            let is_selected = abs_idx == self.cursor;
            let is_match =
                !self.search_matches.is_empty() && self.search_matches.contains(&abs_idx);
            let mut line = self.render_item_line(item, is_selected, is_match, theme, inner.width);
            if is_selected {
                // Pad to full width so the selection background fills the entire row
                let pad = (inner.width as usize).saturating_sub(line.width());
                if pad > 0 {
                    line.spans.push(Span::raw(" ".repeat(pad)));
                }
                line = line.style(Style::default().bg(theme.session_selection));
            }
            lines.push(line);
        }

        if scroll.has_more_below {
            let remaining = self.flat_items.len() - scroll.scroll_offset - scroll.list_visible;
            lines.push(Line::from(Span::styled(
                format!("  [{} more below]", remaining),
                Style::default().fg(theme.dimmed),
            )));
        }

        frame.render_widget(Paragraph::new(lines), inner);

        // Render search bar if active
        if self.search_active {
            let search_area = Rect {
                x: inner.x,
                y: inner.y + inner.height.saturating_sub(1),
                width: inner.width,
                height: 1,
            };

            let value = self.search_query.value();
            let cursor_pos = self.search_query.visual_cursor();
            let cursor_style = Style::default().fg(theme.background).bg(theme.search);
            let text_style = Style::default().fg(theme.search);

            // Split value into: before cursor, char at cursor, after cursor
            let before: String = value.chars().take(cursor_pos).collect();
            let cursor_char: String = value
                .chars()
                .nth(cursor_pos)
                .map(|c| c.to_string())
                .unwrap_or_else(|| " ".to_string());
            let after: String = value.chars().skip(cursor_pos + 1).collect();

            let mut spans = vec![Span::styled("/", text_style)];
            if !before.is_empty() {
                spans.push(Span::styled(before, text_style));
            }
            spans.push(Span::styled(cursor_char, cursor_style));
            if !after.is_empty() {
                spans.push(Span::styled(after, text_style));
            }

            if !self.search_matches.is_empty() {
                let count_text = format!(
                    " [{}/{}]",
                    self.search_match_index + 1,
                    self.search_matches.len()
                );
                spans.push(Span::styled(count_text, Style::default().fg(theme.dimmed)));
            } else if !value.is_empty() {
                spans.push(Span::styled(" [0/0]", Style::default().fg(theme.dimmed)));
            }

            frame.render_widget(Paragraph::new(Line::from(spans)), search_area);
        }
    }

    fn render_item_line(
        &self,
        item: &Item,
        is_selected: bool,
        is_match: bool,
        theme: &Theme,
        list_width: u16,
    ) -> Line<'static> {
        let indent = get_indent(item.depth());

        use std::borrow::Cow;

        let (icon, text, style): (&str, Cow<str>, Style) = match item {
            Item::Group {
                name,
                collapsed,
                session_count,
                ..
            } => {
                let icon = if *collapsed {
                    ICON_COLLAPSED
                } else {
                    ICON_EXPANDED
                };
                let text = Cow::Owned(format!("{} ({})", name, session_count));
                let style = Style::default().fg(theme.group).bold();
                (icon, text, style)
            }
            Item::Session { id, .. } => {
                if let Some(inst) = self.get_instance(id) {
                    match self.view_mode {
                        ViewMode::Agent => {
                            let icon = match inst.status {
                                Status::Running => spinner_running(&inst.created_at),
                                Status::Waiting => spinner_waiting(&inst.created_at),
                                Status::Idle => ICON_IDLE,
                                Status::Unknown => ICON_UNKNOWN,
                                Status::Stopped => ICON_STOPPED,
                                Status::Error => ICON_ERROR,
                                Status::Starting => spinner_starting(&inst.created_at),
                                Status::Deleting => ICON_DELETING,
                                Status::Creating => spinner_starting(&inst.created_at),
                            };
                            let color = match inst.status {
                                Status::Running => theme.running,
                                Status::Waiting => theme.waiting,
                                Status::Idle => theme.idle,
                                Status::Unknown => theme.waiting,
                                Status::Stopped => theme.dimmed,
                                Status::Error => theme.error,
                                Status::Starting => theme.dimmed,
                                Status::Deleting => theme.waiting,
                                Status::Creating => theme.accent,
                            };
                            let style = Style::default().fg(color);
                            (icon, Cow::Owned(inst.title.clone()), style)
                        }
                        ViewMode::Terminal => {
                            // For sandboxed sessions, check the appropriate terminal based on mode
                            let terminal_mode = if inst.is_sandboxed() {
                                self.get_terminal_mode(id)
                            } else {
                                TerminalMode::Host
                            };
                            let terminal_running = match terminal_mode {
                                TerminalMode::Container => inst
                                    .container_terminal_tmux_session()
                                    .map(|s| s.exists())
                                    .unwrap_or(false),
                                TerminalMode::Host => inst
                                    .terminal_tmux_session()
                                    .map(|s| s.exists())
                                    .unwrap_or(false),
                            };
                            let (icon, color) = if terminal_running {
                                (spinner_running(&inst.created_at), theme.terminal_active)
                            } else {
                                (ICON_IDLE, theme.dimmed)
                            };
                            let style = Style::default().fg(color);
                            (icon, Cow::Owned(inst.title.clone()), style)
                        }
                    }
                } else {
                    (
                        "?",
                        Cow::Owned(id.clone()),
                        Style::default().fg(theme.dimmed),
                    )
                }
            }
        };

        let mut line_spans = Vec::with_capacity(5);
        line_spans.push(Span::raw(indent));
        let icon_style = if is_match {
            Style::default().fg(theme.search)
        } else {
            style
        };
        line_spans.push(Span::styled(format!("{} ", icon), icon_style));
        line_spans.push(Span::styled(
            text.into_owned(),
            if is_selected { style.bold() } else { style },
        ));

        if let Item::Session { id, .. } = item {
            if let Some(inst) = self.get_instance(id) {
                if let Some(ws_info) = &inst.workspace_info {
                    line_spans.push(Span::styled(
                        format!("  {} [{} repos]", ws_info.branch, ws_info.repos.len()),
                        Style::default().fg(theme.branch),
                    ));
                } else if let Some(wt_info) = &inst.worktree_info {
                    if wt_info.branch != inst.title {
                        line_spans.push(Span::styled(
                            format!("  {}", wt_info.branch),
                            Style::default().fg(theme.branch),
                        ));
                    }
                }

                // Last-activity column: right-aligned, fixed-width slot just
                // before the terminal-mode/status badge. Hidden when the list
                // pane is too narrow to justify spending the horizontal budget.
                if list_width >= LAST_ACTIVITY_MIN_WIDTH {
                    let age = format_relative_age(inst.last_accessed_at);
                    // Reserve LAST_ACTIVITY_SLOT cells; right-align inside the
                    // slot so columns line up across rows of varying title
                    // length. Empty `age` still reserves space — keeping the
                    // column aligned is why we don't conditionally skip per
                    // row.
                    let padded = format!("{:>width$}", age, width = LAST_ACTIVITY_SLOT);
                    line_spans.push(Span::styled(padded, Style::default().fg(theme.dimmed)));
                }

                if self.view_mode == ViewMode::Terminal && inst.is_sandboxed() {
                    let mode = self.get_terminal_mode(id);
                    let mode_text = match mode {
                        TerminalMode::Container => " [container]",
                        TerminalMode::Host => " [host]",
                    };
                    line_spans.push(Span::styled(mode_text, Style::default().fg(theme.sandbox)));
                }
            }
        }

        Line::from(line_spans)
    }

    /// Refresh preview cache if needed (session changed, dimensions changed, or timer expired)
    fn refresh_preview_cache_if_needed(&mut self, width: u16, height: u16) {
        const PREVIEW_REFRESH_MS: u128 = 250; // Refresh preview 4x/second max

        let scroll_offset = self.preview_scroll_offset;
        let session_changed = match &self.selected_session {
            Some(id) => self.preview_cache.session_id.as_ref() != Some(id),
            None => false,
        };
        let dims_changed = self.preview_cache.dimensions != (width, height);
        let timer_expired =
            self.preview_cache.last_refresh.elapsed().as_millis() > PREVIEW_REFRESH_MS;
        // Only re-capture for scroll when the cached window can no longer
        // cover the requested offset. Wheel ticks inside the BUFFER headroom
        // re-render from the existing content without forking tmux.
        let scroll_exceeds =
            scroll_exceeds_cache(self.preview_cache.captured_lines, height, scroll_offset);

        let needs_refresh = self.selected_session.is_some()
            && (session_changed || dims_changed || timer_expired || scroll_exceeds);

        if needs_refresh {
            if let Some(id) = &self.selected_session {
                if let Some(inst) = self.get_instance(id) {
                    let capture_lines = capture_lines_for(height, scroll_offset);
                    let content = inst
                        .capture_output_with_size(capture_lines, width, height)
                        .unwrap_or_default();
                    self.preview_cache.captured_lines = content.lines().count();
                    self.preview_cache.content = content;
                    self.preview_cache.session_id = Some(id.clone());
                    self.preview_cache.dimensions = (width, height);
                    self.preview_cache.last_refresh = Instant::now();
                    self.preview_scroll_offset = clamp_scroll_to_capture(
                        self.preview_scroll_offset,
                        self.preview_cache.captured_lines,
                        height,
                    );
                }
            }
        }
    }

    /// Refresh terminal preview cache if needed (for host terminals)
    fn refresh_terminal_preview_cache_if_needed(&mut self, width: u16, height: u16) {
        const PREVIEW_REFRESH_MS: u128 = 250;

        let scroll_offset = self.preview_scroll_offset;
        let needs_refresh = match &self.selected_session {
            Some(id) => {
                self.terminal_preview_cache.session_id.as_ref() != Some(id)
                    || self.terminal_preview_cache.dimensions != (width, height)
                    || scroll_exceeds_cache(
                        self.terminal_preview_cache.captured_lines,
                        height,
                        scroll_offset,
                    )
                    || self
                        .terminal_preview_cache
                        .last_refresh
                        .elapsed()
                        .as_millis()
                        > PREVIEW_REFRESH_MS
            }
            None => false,
        };

        if needs_refresh {
            if let Some(id) = &self.selected_session {
                if let Some(inst) = self.get_instance(id) {
                    let capture_lines = capture_lines_for(height, scroll_offset);
                    let content = inst
                        .terminal_tmux_session()
                        .and_then(|s| s.capture_pane(capture_lines))
                        .unwrap_or_default();
                    self.terminal_preview_cache.captured_lines = content.lines().count();
                    self.terminal_preview_cache.content = content;
                    self.terminal_preview_cache.session_id = Some(id.clone());
                    self.terminal_preview_cache.dimensions = (width, height);
                    self.terminal_preview_cache.last_refresh = Instant::now();
                    self.preview_scroll_offset = clamp_scroll_to_capture(
                        self.preview_scroll_offset,
                        self.terminal_preview_cache.captured_lines,
                        height,
                    );
                }
            }
        }
    }

    /// Refresh container terminal preview cache if needed
    fn refresh_container_terminal_preview_cache_if_needed(&mut self, width: u16, height: u16) {
        const PREVIEW_REFRESH_MS: u128 = 250;

        let scroll_offset = self.preview_scroll_offset;
        let needs_refresh = match &self.selected_session {
            Some(id) => {
                self.container_terminal_preview_cache.session_id.as_ref() != Some(id)
                    || self.container_terminal_preview_cache.dimensions != (width, height)
                    || scroll_exceeds_cache(
                        self.container_terminal_preview_cache.captured_lines,
                        height,
                        scroll_offset,
                    )
                    || self
                        .container_terminal_preview_cache
                        .last_refresh
                        .elapsed()
                        .as_millis()
                        > PREVIEW_REFRESH_MS
            }
            None => false,
        };

        if needs_refresh {
            if let Some(id) = &self.selected_session {
                if let Some(inst) = self.get_instance(id) {
                    let capture_lines = capture_lines_for(height, scroll_offset);
                    let content = inst
                        .container_terminal_tmux_session()
                        .and_then(|s| s.capture_pane(capture_lines))
                        .unwrap_or_default();
                    self.container_terminal_preview_cache.captured_lines = content.lines().count();
                    self.container_terminal_preview_cache.content = content;
                    self.container_terminal_preview_cache.session_id = Some(id.clone());
                    self.container_terminal_preview_cache.dimensions = (width, height);
                    self.container_terminal_preview_cache.last_refresh = Instant::now();
                    self.preview_scroll_offset = clamp_scroll_to_capture(
                        self.preview_scroll_offset,
                        self.container_terminal_preview_cache.captured_lines,
                        height,
                    );
                }
            }
        }
    }

    fn render_preview(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let compact = area.width < responsive::STACKED_BREAKPOINT;
        let (border_color, title_color) = match self.view_mode {
            ViewMode::Agent => (theme.border, theme.title),
            ViewMode::Terminal => (theme.terminal_border, theme.terminal_border),
        };

        let mut block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color))
            .padding(Padding::horizontal(1));

        // In compact mode, hoist session name + status icon into the
        // outer title so the (now omitted) info header isn't missed.
        let compact_title: Option<Line> = if compact {
            self.selected_session
                .as_ref()
                .and_then(|id| self.get_instance(id))
                .map(|inst| {
                    let (icon, icon_color) = match inst.status {
                        Status::Running => (spinner_running(&inst.created_at), theme.running),
                        Status::Waiting => (spinner_waiting(&inst.created_at), theme.waiting),
                        Status::Idle => (ICON_IDLE, theme.idle),
                        Status::Unknown => (ICON_UNKNOWN, theme.waiting),
                        Status::Stopped => (ICON_STOPPED, theme.dimmed),
                        Status::Error => (ICON_ERROR, theme.error),
                        Status::Starting => (spinner_starting(&inst.created_at), theme.dimmed),
                        Status::Deleting => (ICON_DELETING, theme.waiting),
                        Status::Creating => (spinner_starting(&inst.created_at), theme.accent),
                    };
                    Line::from(vec![
                        Span::raw(" "),
                        Span::styled(icon, Style::default().fg(icon_color)),
                        Span::raw(" "),
                        Span::styled(inst.title.clone(), Style::default().fg(title_color).bold()),
                        Span::raw(" "),
                    ])
                })
        } else {
            None
        };

        if let Some(line) = compact_title {
            block = block.title(line);
        } else {
            let title = match self.view_mode {
                ViewMode::Agent => " Preview ",
                ViewMode::Terminal => " Terminal Preview ",
            };
            block = block
                .title(title)
                .title_style(Style::default().fg(title_color));
        }

        let inner = block.inner(area);
        self.preview_area = inner;
        self.diff_area = Rect::default();
        frame.render_widget(block, area);

        match self.view_mode {
            ViewMode::Agent => {
                // Check if selected session is being created (show hook progress)
                let is_creating = self
                    .selected_session
                    .as_ref()
                    .and_then(|id| self.get_instance(id))
                    .is_some_and(|inst| inst.status == Status::Creating);

                if is_creating {
                    self.render_creating_preview(frame, inner, theme);
                } else {
                    // Refresh cache before borrowing from instance_map to avoid borrow conflicts
                    self.refresh_preview_cache_if_needed(inner.width, inner.height);

                    if let Some(id) = &self.selected_session {
                        if let Some(inst) = self.get_instance(id) {
                            Preview::render_with_cache(
                                frame,
                                inner,
                                inst,
                                &self.preview_cache.content,
                                self.preview_scroll_offset,
                                theme,
                                compact,
                            );
                        }
                    } else {
                        let hint = Paragraph::new("Select a session to preview")
                            .style(Style::default().fg(theme.dimmed))
                            .alignment(Alignment::Center);
                        frame.render_widget(hint, inner);
                    }
                }
            }
            ViewMode::Terminal => {
                // Clone id early to avoid borrow conflicts
                let selected_id = self.selected_session.clone();

                if let Some(id) = selected_id {
                    // Determine which terminal to preview based on mode
                    let terminal_mode = if let Some(inst) = self.get_instance(&id) {
                        if inst.is_sandboxed() {
                            self.get_terminal_mode(&id)
                        } else {
                            TerminalMode::Host
                        }
                    } else {
                        TerminalMode::Host
                    };

                    // Refresh the appropriate cache before borrowing instance
                    match terminal_mode {
                        TerminalMode::Container => {
                            self.refresh_container_terminal_preview_cache_if_needed(
                                inner.width,
                                inner.height,
                            );
                        }
                        TerminalMode::Host => {
                            self.refresh_terminal_preview_cache_if_needed(
                                inner.width,
                                inner.height,
                            );
                        }
                    }

                    // Now borrow instance for rendering
                    if let Some(inst) = self.get_instance(&id) {
                        let (terminal_running, preview_content) = match terminal_mode {
                            TerminalMode::Container => {
                                let running = inst
                                    .container_terminal_tmux_session()
                                    .map(|s| s.exists())
                                    .unwrap_or(false);
                                (running, &self.container_terminal_preview_cache.content)
                            }
                            TerminalMode::Host => {
                                let running = inst
                                    .terminal_tmux_session()
                                    .map(|s| s.exists())
                                    .unwrap_or(false);
                                (running, &self.terminal_preview_cache.content)
                            }
                        };

                        Preview::render_terminal_preview(
                            frame,
                            inner,
                            inst,
                            terminal_running,
                            preview_content,
                            self.preview_scroll_offset,
                            theme,
                            compact,
                        );
                    }
                } else {
                    let hint = Paragraph::new("Select a session to preview terminal")
                        .style(Style::default().fg(theme.dimmed))
                        .alignment(Alignment::Center);
                    frame.render_widget(hint, inner);
                }
            }
        }
    }

    fn render_creating_preview(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let selected_id = match &self.selected_session {
            Some(id) => id.clone(),
            None => return,
        };

        let inst = match self.get_instance(&selected_id) {
            Some(inst) => inst,
            None => return,
        };

        let spinner = spinners::orbit()
            .set_interval(Duration::from_millis(400))
            .current_frame();

        // Info section (3 lines) + separator + hook output
        let info_height: u16 = 4;
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(info_height), Constraint::Min(1)])
            .split(area);

        // Info lines
        let info_lines = vec![
            Line::from(vec![
                Span::styled("Title:   ", Style::default().fg(theme.dimmed)),
                Span::styled(&inst.title, Style::default().fg(theme.text).bold()),
            ]),
            Line::from(vec![
                Span::styled("Path:    ", Style::default().fg(theme.dimmed)),
                Span::styled(&inst.project_path, Style::default().fg(theme.text)),
            ]),
            Line::from(vec![
                Span::styled("Status:  ", Style::default().fg(theme.dimmed)),
                Span::styled(
                    format!("{} Creating...", spinner),
                    Style::default().fg(theme.accent),
                ),
            ]),
            Line::from(""),
        ];
        frame.render_widget(Paragraph::new(info_lines), chunks[0]);

        // Hook output section
        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(theme.border))
            .title(" Hook Output ")
            .title_style(Style::default().fg(theme.dimmed));

        let inner = block.inner(chunks[1]);
        frame.render_widget(block, chunks[1]);

        let progress = self.creating_hook_progress.get(&selected_id);
        let inner_height = inner.height as usize;

        if let Some(progress) = progress {
            let mut lines: Vec<Line> = Vec::new();

            // Current hook command
            if let Some(ref cmd) = progress.current_hook {
                lines.push(Line::from(vec![
                    Span::styled(
                        format!(" {} ", spinner),
                        Style::default().fg(theme.accent).bold(),
                    ),
                    Span::styled(cmd.as_str(), Style::default().fg(theme.text)),
                ]));
            } else {
                lines.push(Line::from(Span::styled(
                    format!(" {} Preparing...", spinner),
                    Style::default().fg(theme.dimmed),
                )));
            }

            // Show the last N lines of output that fit
            let max_output = inner_height.saturating_sub(3);
            let start = progress.hook_output.len().saturating_sub(max_output);
            for line in &progress.hook_output[start..] {
                lines.push(Line::from(Span::styled(
                    format!("  {}", line),
                    Style::default().fg(theme.dimmed),
                )));
            }

            // Pad and add cancel hint
            let used = lines.len();
            let available = inner_height.saturating_sub(1);
            for _ in used..available {
                lines.push(Line::from(""));
            }
            lines.push(Line::from(vec![
                Span::styled(" Press ", Style::default().fg(theme.dimmed)),
                Span::styled("Ctrl+C", Style::default().fg(theme.hint)),
                Span::styled(" to cancel", Style::default().fg(theme.dimmed)),
            ]));

            frame.render_widget(Paragraph::new(lines), inner);
        } else {
            let hint = Paragraph::new(format!(" {} Setting up session...", spinner))
                .style(Style::default().fg(theme.dimmed));
            frame.render_widget(hint, inner);
        }
    }

    fn render_status_bar(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let key_style = Style::default().fg(theme.accent).bold();
        let desc_style = Style::default().fg(theme.dimmed);
        let sep_style = Style::default().fg(theme.border);
        let strict = self.strict_hotkeys;

        // Priority-tagged shortcut groups. Lower priority = kept longer when
        // the footer can't fit everything (iPhone Mosh landscape is ~80 cols,
        // where the full label set used to truncate Help/Quit). Essentials
        // (Nav / Enter / Help / Quit / Serve indicator) survive first;
        // Diff / Search / Mode / Group drop first. Groups render in the
        // declared order; a │ separator is inserted between kept groups
        // at render time.
        let mk = |key: &str, desc: &str| -> Vec<Span<'static>> {
            vec![
                Span::styled(format!(" {}", key), key_style),
                Span::styled(format!(" {} ", desc), desc_style),
            ]
        };

        let mut groups: Vec<(u8, Vec<Span<'static>>)> = Vec::new();

        // Serve indicator: shown only when the `aoe serve` daemon is live.
        // The TUI does not own the daemon, so we probe the PID file each
        // render. Mode comes from a PID-keyed cache so we don't read the
        // serve.mode file from disk on every frame.
        #[cfg(feature = "serve")]
        {
            let mode_label = crate::cli::serve::cached_serve_mode_label();
            if crate::cli::serve::daemon_pid().is_some() {
                let label = match mode_label {
                    Some(m) => format!(" \u{25CF} Serving ({}) ", m),
                    None => " \u{25CF} Serving ".to_string(),
                };
                groups.push((
                    0,
                    vec![Span::styled(
                        label,
                        Style::default().fg(theme.running).bold(),
                    )],
                ));
            }
        }

        groups.push((0, mk("j/k", "Nav")));

        if let Some(enter_action_text) = match self.flat_items.get(self.cursor) {
            Some(Item::Group {
                collapsed: true, ..
            }) => Some("Expand"),
            Some(Item::Group {
                collapsed: false, ..
            }) => Some("Collapse"),
            Some(Item::Session { .. }) => Some("Attach"),
            None => None,
        } {
            groups.push((0, mk("Enter", enter_action_text)));
        }

        groups.push((2, mk(if strict { "T" } else { "t" }, "View")));
        groups.push((3, mk(if strict { "^G" } else { "g" }, "Group")));

        // c: container/host toggle hint for sandboxed sessions in Terminal view
        if self.view_mode == ViewMode::Terminal {
            if let Some(id) = &self.selected_session {
                if let Some(inst) = self.get_instance(id) {
                    if inst.is_sandboxed() {
                        groups.push((4, mk(if strict { "C" } else { "c" }, "Mode")));
                    }
                }
            }
        }

        groups.push((2, mk(if strict { "N" } else { "n" }, "New")));

        if self.selected_session.is_some() {
            groups.push((3, mk(if strict { "M" } else { "m" }, "Msg")));
        }
        if !self.flat_items.is_empty() {
            groups.push((3, mk(if strict { "D" } else { "d" }, "Del")));
        }

        groups.push((4, mk("/", "Search")));
        groups.push((4, mk(if strict { "^D" } else { "D" }, "Diff")));
        groups.push((0, mk("?", "Help")));
        groups.push((0, mk(if strict { "Q" } else { "q" }, "Quit")));

        // Greedy pack by priority. Width of a group = sum of span char counts;
        // separator between kept groups adds 1 col each.
        let widths: Vec<usize> = groups
            .iter()
            .map(|(_, g)| g.iter().map(|s| s.content.chars().count()).sum::<usize>())
            .collect();
        let avail = area.width as usize;

        let mut order: Vec<usize> = (0..groups.len()).collect();
        order.sort_by_key(|&i| groups[i].0);

        let mut keep = vec![false; groups.len()];
        let mut used = 0usize;
        let mut count = 0usize;
        for i in order {
            let sep = if count == 0 { 0 } else { 1 };
            if used + widths[i] + sep <= avail {
                keep[i] = true;
                used += widths[i] + sep;
                count += 1;
            }
        }

        let mut spans: Vec<Span> = Vec::new();
        let mut first = true;
        for (i, (_, group)) in groups.into_iter().enumerate() {
            if !keep[i] {
                continue;
            }
            if !first {
                spans.push(Span::styled("│", sep_style));
            }
            spans.extend(group);
            first = false;
        }

        let status = Paragraph::new(Line::from(spans)).style(Style::default().bg(theme.selection));
        frame.render_widget(status, area);
    }

    fn render_update_bar(
        &self,
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
        info: &UpdateInfo,
        status: Option<&str>,
    ) {
        let update_style = Style::default().fg(theme.waiting).bold();
        let text = if let Some(s) = status {
            format!(" {s}  [Ctrl+x] dismiss")
        } else {
            format!(
                " update available {} → {}  [u] update  [Ctrl+x] dismiss",
                info.current_version, info.latest_version
            )
        };
        let bar = Paragraph::new(Line::from(Span::styled(text, update_style)))
            .style(Style::default().bg(theme.selection));
        frame.render_widget(bar, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_relative_age_none_returns_empty() {
        assert_eq!(format_relative_age(None), "");
    }

    #[test]
    fn format_relative_age_future_timestamp_returns_less_than_1m() {
        let future = Utc::now() + chrono::Duration::hours(1);
        assert_eq!(format_relative_age(Some(future)), "<1m");
    }

    #[test]
    fn format_relative_age_recent_returns_less_than_1m() {
        let recent = Utc::now() - chrono::Duration::seconds(30);
        assert_eq!(format_relative_age(Some(recent)), "<1m");
    }

    #[test]
    fn format_relative_age_minutes() {
        let ts = Utc::now() - chrono::Duration::minutes(5);
        assert_eq!(format_relative_age(Some(ts)), "5m");
    }

    #[test]
    fn format_relative_age_hours() {
        let ts = Utc::now() - chrono::Duration::hours(3);
        assert_eq!(format_relative_age(Some(ts)), "3h");
    }

    #[test]
    fn format_relative_age_days() {
        let ts = Utc::now() - chrono::Duration::days(7);
        assert_eq!(format_relative_age(Some(ts)), "7d");
    }

    #[test]
    fn format_relative_age_months() {
        let ts = Utc::now() - chrono::Duration::days(60);
        assert_eq!(format_relative_age(Some(ts)), "2mo");
    }

    #[test]
    fn capture_lines_for_adds_buffer_to_height() {
        assert_eq!(capture_lines_for(30, 0), 50);
    }

    #[test]
    fn capture_lines_for_extends_by_scroll_offset() {
        assert_eq!(capture_lines_for(30, 200), 250);
    }

    #[test]
    fn capture_lines_for_saturates_instead_of_overflowing() {
        assert_eq!(capture_lines_for(u16::MAX, u16::MAX), u16::MAX as usize);
    }

    #[test]
    fn scroll_exceeds_cache_false_when_buffer_covers_small_scroll() {
        // Cache was captured at scroll=0 with height=30, so
        // capture_lines_for(30, 0) = 30 + 0 + BUFFER(20) = 50 lines.
        // A wheel tick to scroll_offset=3 needs 30 + 3 + 20 = 53, but the
        // existing BUFFER reserve is what we check: the predicate should
        // only trip when `height + scroll + BUFFER > captured_lines`.
        //
        // With captured_lines = 60 (capture returned extra pane history),
        // small scroll increments must NOT force a re-capture.
        let height = 30u16;
        let captured = 60usize;
        assert!(!scroll_exceeds_cache(captured, height, 0));
        assert!(!scroll_exceeds_cache(captured, height, 3));
        assert!(!scroll_exceeds_cache(captured, height, 9));
    }

    #[test]
    fn scroll_exceeds_cache_true_when_scroll_runs_past_captured_window() {
        // Once the requested visible window + BUFFER exceeds captured_lines,
        // the cache can no longer cover the scroll and must be re-captured.
        let height = 30u16;
        let captured = 60usize;
        // height(30) + scroll(20) + BUFFER(20) = 70 > 60 → recapture.
        assert!(scroll_exceeds_cache(captured, height, 20));
    }

    #[test]
    fn scroll_exceeds_cache_true_for_empty_cache() {
        // First render: nothing captured yet, so any request forces capture.
        assert!(scroll_exceeds_cache(0, 30, 0));
    }
}
