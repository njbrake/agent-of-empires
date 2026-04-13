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
            HelpOverlay::render(frame, area, theme, self.sort_order);
        }

        if let Some(dialog) = &self.new_dialog {
            dialog.render(frame, area, theme);
        }

        if let Some(dialog) = &self.confirm_dialog {
            dialog.render(frame, area, theme);
        }

        if let Some(dialog) = &self.unified_delete_dialog {
            dialog.render(frame, area, theme);
        }

        if let Some(dialog) = &self.group_delete_options_dialog {
            dialog.render(frame, area, theme);
        }

        if let Some(dialog) = &self.rename_dialog {
            dialog.render(frame, area, theme);
        }

        if let Some(dialog) = &self.hooks_install_dialog {
            dialog.render(frame, area, theme);
        }

        if let Some(dialog) = &self.hook_trust_dialog {
            dialog.render(frame, area, theme);
        }

        if let Some(dialog) = &self.welcome_dialog {
            dialog.render(frame, area, theme);
        }

        if let Some(dialog) = &self.changelog_dialog {
            dialog.render(frame, area, theme);
        }

        if let Some(dialog) = &self.info_dialog {
            dialog.render(frame, area, theme);
        }

        if let Some(dialog) = &self.profile_picker_dialog {
            dialog.render(frame, area, theme);
        }

        if let Some(dialog) = &self.send_message_dialog {
            dialog.render(frame, area, theme);
        }
    }

    fn render_list(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let title = match self.view_mode {
            ViewMode::Agent => format!(" Agent of Empires [{}] ", self.active_profile_display()),
            ViewMode::Terminal => format!(" Terminals [{}] ", self.active_profile_display()),
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
            let mut line = self.render_item_line(item, is_selected, is_match, theme);
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
                    line_spans.push(Span::styled(
                        format!("  {}", wt_info.branch),
                        Style::default().fg(theme.branch),
                    ));
                }
                if inst.is_sandboxed() {
                    match self.view_mode {
                        ViewMode::Agent => {
                            line_spans.push(Span::styled(
                                " [sandbox]",
                                Style::default().fg(theme.sandbox),
                            ));
                        }
                        ViewMode::Terminal => {
                            let mode = self.get_terminal_mode(id);
                            let mode_text = match mode {
                                TerminalMode::Container => " [container]",
                                TerminalMode::Host => " [host]",
                            };
                            line_spans
                                .push(Span::styled(mode_text, Style::default().fg(theme.sandbox)));
                        }
                    }
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

    fn render_status_bar(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let key_style = Style::default().fg(theme.accent).bold();
        let desc_style = Style::default().fg(theme.dimmed);
        let sep_style = Style::default().fg(theme.border);

        let (mode_indicator, mode_color) = match self.view_mode {
            ViewMode::Agent => ("[Agent]", theme.waiting),
            ViewMode::Terminal => ("[Term]", theme.terminal_border),
        };
        let mode_style = Style::default().fg(mode_color).bold();

        let mut spans = vec![
            Span::styled(format!(" {} ", mode_indicator), mode_style),
            Span::styled("│", sep_style),
            Span::styled(" j/k", key_style),
            Span::styled(" Nav ", desc_style),
        ];
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
        spans.extend([
            Span::styled("│", sep_style),
            Span::styled(" t", key_style),
            Span::styled(" View ", desc_style),
        ]);

        // Show c: container/host hint for sandboxed sessions in Terminal view
        if self.view_mode == ViewMode::Terminal {
            if let Some(id) = &self.selected_session {
                if let Some(inst) = self.get_instance(id) {
                    if inst.is_sandboxed() {
                        spans.extend([
                            Span::styled("│", sep_style),
                            Span::styled(" c", key_style),
                            Span::styled(" Mode ", desc_style),
                        ]);
                    }
                }
            }
        }

        spans.extend([
            Span::styled("│", sep_style),
            Span::styled(" n", key_style),
            Span::styled(" New ", desc_style),
        ]);

        if self.selected_session.is_some() {
            spans.extend([
                Span::styled("│", sep_style),
                Span::styled(" m", key_style),
                Span::styled(" Msg ", desc_style),
            ]);
        }

        if !self.flat_items.is_empty() {
            spans.extend([
                Span::styled("│", sep_style),
                Span::styled(" d", key_style),
                Span::styled(" Del ", desc_style),
            ]);
        }

        spans.extend([
            Span::styled("│", sep_style),
            Span::styled(" /", key_style),
            Span::styled(" Search ", desc_style),
            Span::styled("│", sep_style),
            Span::styled(" D", key_style),
            Span::styled(" Diff ", desc_style),
            Span::styled("│", sep_style),
            Span::styled(" ?", key_style),
            Span::styled(" Help ", desc_style),
            Span::styled("│", sep_style),
            Span::styled(" q", key_style),
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
