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
use crate::tui::styles::Theme;
use crate::update::UpdateInfo;

/// Derive a frame offset from a session's creation timestamp so that
/// sessions started at different times show visually distinct spinner positions.
fn session_offset(created_at: &DateTime<Utc>) -> usize {
    created_at.timestamp_millis() as usize
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
const LAST_ACTIVITY_MIN_WIDTH: u16 = 50;

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
        if let Some(ref mut diff) = self.diff_view {
            // Compute diff for selected file if not cached
            let _ = diff.get_current_diff();

            diff.render(frame, area, theme);
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

        // Layout: left panel (list) and right panel (preview)
        // On small screens, cap list width so the preview pane gets adequate space
        let available_width = main_chunks[0].width;
        let effective_list_width = self
            .list_width
            .min(available_width.saturating_sub(40))
            .max(10);
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(effective_list_width),
                Constraint::Min(40),
            ])
            .split(main_chunks[0]);

        self.render_list(frame, chunks[0], theme);
        self.render_preview(frame, chunks[1], theme);
        self.render_status_bar(frame, main_chunks[1], theme);

        if let Some(info) = update_info {
            self.render_update_bar(frame, main_chunks[2], theme, info);
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
            changelog_dialog,
            info_dialog,
            profile_picker_dialog,
            send_message_dialog,
        );

        #[cfg(feature = "serve")]
        if let Some(dialog) = &self.serve_dialog {
            dialog.render(frame, area, theme);
        }
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
                Line::from("or 'agent-of-empires add .'").style(Style::default().fg(theme.hint)),
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

        let session_changed = match &self.selected_session {
            Some(id) => self.preview_cache.session_id.as_ref() != Some(id),
            None => false,
        };
        let dims_changed = self.preview_cache.dimensions != (width, height);
        let timer_expired =
            self.preview_cache.last_refresh.elapsed().as_millis() > PREVIEW_REFRESH_MS;

        let needs_refresh =
            self.selected_session.is_some() && (session_changed || dims_changed || timer_expired);

        if needs_refresh {
            if let Some(id) = &self.selected_session {
                if let Some(inst) = self.get_instance(id) {
                    self.preview_cache.content = inst
                        .capture_output_with_size(height as usize, width, height)
                        .unwrap_or_default();
                    self.preview_cache.session_id = Some(id.clone());
                    self.preview_cache.dimensions = (width, height);
                    self.preview_cache.last_refresh = Instant::now();
                }
            }
        }
    }

    /// Refresh terminal preview cache if needed (for host terminals)
    fn refresh_terminal_preview_cache_if_needed(&mut self, width: u16, height: u16) {
        const PREVIEW_REFRESH_MS: u128 = 250;

        let needs_refresh = match &self.selected_session {
            Some(id) => {
                self.terminal_preview_cache.session_id.as_ref() != Some(id)
                    || self.terminal_preview_cache.dimensions != (width, height)
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
                    self.terminal_preview_cache.content = inst
                        .terminal_tmux_session()
                        .and_then(|s| s.capture_pane(height as usize))
                        .unwrap_or_default();
                    self.terminal_preview_cache.session_id = Some(id.clone());
                    self.terminal_preview_cache.dimensions = (width, height);
                    self.terminal_preview_cache.last_refresh = Instant::now();
                }
            }
        }
    }

    /// Refresh container terminal preview cache if needed
    fn refresh_container_terminal_preview_cache_if_needed(&mut self, width: u16, height: u16) {
        const PREVIEW_REFRESH_MS: u128 = 250;

        let needs_refresh = match &self.selected_session {
            Some(id) => {
                self.container_terminal_preview_cache.session_id.as_ref() != Some(id)
                    || self.container_terminal_preview_cache.dimensions != (width, height)
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
                    self.container_terminal_preview_cache.content = inst
                        .container_terminal_tmux_session()
                        .and_then(|s| s.capture_pane(height as usize))
                        .unwrap_or_default();
                    self.container_terminal_preview_cache.session_id = Some(id.clone());
                    self.container_terminal_preview_cache.dimensions = (width, height);
                    self.container_terminal_preview_cache.last_refresh = Instant::now();
                }
            }
        }
    }

    fn render_preview(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let title = match self.view_mode {
            ViewMode::Agent => " Preview ",
            ViewMode::Terminal => " Terminal Preview ",
        };
        let (border_color, title_color) = match self.view_mode {
            ViewMode::Agent => (theme.border, theme.title),
            ViewMode::Terminal => (theme.terminal_border, theme.terminal_border),
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color))
            .title(title)
            .title_style(Style::default().fg(title_color))
            .padding(Padding::horizontal(1));

        let inner = block.inner(area);
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
                                theme,
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
                            theme,
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

        let mut spans: Vec<Span> = Vec::new();

        // Serve indicator: shown only when the `aoe serve` daemon is live.
        // The TUI does not own the daemon, so we probe the PID file each
        // render. Mode comes from a PID-keyed cache so we don't read the
        // serve.mode file from disk on every frame; the cache invalidates
        // whenever the daemon PID changes (restart / fresh spawn).
        #[cfg(feature = "serve")]
        {
            let mode_label = crate::cli::serve::cached_serve_mode_label();
            // cached_serve_mode_label() returns None both for "no daemon"
            // and "daemon but mode unknown", so check the daemon PID to
            // distinguish — only render the indicator when there's a
            // daemon, with the mode tag if we have it.
            if crate::cli::serve::daemon_pid().is_some() {
                let label = match mode_label {
                    Some(m) => format!(" \u{25CF} Serving ({}) ", m),
                    None => " \u{25CF} Serving ".to_string(),
                };
                spans.extend([
                    Span::styled(label, Style::default().fg(theme.running).bold()),
                    Span::styled("│", sep_style),
                ]);
            }
        }

        spans.extend([
            Span::styled(" j/k", key_style),
            Span::styled(" Nav ", desc_style),
        ]);
        if let Some(enter_action_text) = match self.flat_items.get(self.cursor) {
            Some(Item::Group {
                collapsed: true, ..
            }) => Some(" Expand "),
            Some(Item::Group {
                collapsed: false, ..
            }) => Some(" Collapse "),
            Some(Item::Session { .. }) => Some(" Attach "),
            None => None,
        } {
            spans.extend([
                Span::styled("│", sep_style),
                Span::styled(" Enter", key_style),
                Span::styled(enter_action_text, desc_style),
            ])
        }
        let strict = self.strict_hotkeys;
        spans.extend([
            Span::styled("│", sep_style),
            Span::styled(if strict { " T" } else { " t" }, key_style),
            Span::styled(" View ", desc_style),
            Span::styled("│", sep_style),
            Span::styled(if strict { " ^G" } else { " g" }, key_style),
            Span::styled(" Group ", desc_style),
        ]);

        // Show c: container/host hint for sandboxed sessions in Terminal view
        if self.view_mode == ViewMode::Terminal {
            if let Some(id) = &self.selected_session {
                if let Some(inst) = self.get_instance(id) {
                    if inst.is_sandboxed() {
                        spans.extend([
                            Span::styled("│", sep_style),
                            Span::styled(if strict { " C" } else { " c" }, key_style),
                            Span::styled(" Mode ", desc_style),
                        ]);
                    }
                }
            }
        }

        spans.extend([
            Span::styled("│", sep_style),
            Span::styled(if strict { " N" } else { " n" }, key_style),
            Span::styled(" New ", desc_style),
        ]);

        if self.selected_session.is_some() {
            spans.extend([
                Span::styled("│", sep_style),
                Span::styled(if strict { " M" } else { " m" }, key_style),
                Span::styled(" Msg ", desc_style),
            ]);
        }

        if !self.flat_items.is_empty() {
            spans.extend([
                Span::styled("│", sep_style),
                Span::styled(if strict { " D" } else { " d" }, key_style),
                Span::styled(" Del ", desc_style),
            ]);
        }

        spans.extend([
            Span::styled("│", sep_style),
            Span::styled(" /", key_style),
            Span::styled(" Search ", desc_style),
            Span::styled("│", sep_style),
            Span::styled(if strict { " ^D" } else { " D" }, key_style),
            Span::styled(" Diff ", desc_style),
            Span::styled("│", sep_style),
            Span::styled(" ?", key_style),
            Span::styled(" Help ", desc_style),
            Span::styled("│", sep_style),
            Span::styled(if strict { " Q" } else { " q" }, key_style),
            Span::styled(" Quit", desc_style),
        ]);

        let status = Paragraph::new(Line::from(spans)).style(Style::default().bg(theme.selection));
        frame.render_widget(status, area);
    }

    fn render_update_bar(&self, frame: &mut Frame, area: Rect, theme: &Theme, info: &UpdateInfo) {
        let update_style = Style::default().fg(theme.waiting).bold();
        let text = format!(
            " update available {} -> {}",
            info.current_version, info.latest_version
        );
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
}
