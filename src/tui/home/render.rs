//! Rendering for HomeView

use ratatui::prelude::*;
use ratatui::widgets::*;
use std::time::Instant;

use super::{
    get_indent, HomeView, TerminalMode, ViewMode, ICON_COLLAPSED, ICON_DELETING, ICON_ERROR,
    ICON_EXPANDED, ICON_IDLE, ICON_INDICATOR, ICON_RUNNING, ICON_STARTING, ICON_WAITING,
    TREE_BRANCH, TREE_LAST,
};
use crate::session::{Item, Status};
use crate::tui::components::{HelpOverlay, Preview};
use crate::tui::styles::{tint_background, Theme};
use crate::update::UpdateInfo;

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

        if self.sidebar_mode {
            // Sidebar mode: session list fills entire width, no preview
            self.render_list(frame, main_chunks[0], theme);
        } else {
            // Layout: left panel (list) and right panel (preview)
            let chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(self.list_width), Constraint::Min(40)])
                .split(main_chunks[0]);

            self.render_list(frame, chunks[0], theme);
            self.render_preview(frame, chunks[1], theme);
        }
        self.render_status_bar(frame, main_chunks[1], theme);

        if let Some(info) = update_info {
            self.render_update_bar(frame, main_chunks[2], theme, info);
        }

        // Render dialogs on top
        if self.show_help {
            HelpOverlay::render(frame, area, theme);
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
    }

    fn render_list(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let title = match self.view_mode {
            ViewMode::Agent => format!(" ▨ kokorro [{}] ", self.storage.profile()),
            ViewMode::Terminal => format!(" Terminals [{}] ", self.storage.profile()),
        };
        let (border_color, title_color) = match self.view_mode {
            ViewMode::Agent => (theme.border, theme.title),
            ViewMode::Terminal => (theme.terminal_border, theme.terminal_border),
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .title(title)
            .title_style(Style::default().fg(title_color).bold());

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if self.instances.is_empty() && self.groups.is_empty() {
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

        let indices: Vec<usize> = if let Some(ref filtered) = self.filtered_items {
            filtered.clone()
        } else {
            (0..self.flat_items.len()).collect()
        };

        let list_items: Vec<ListItem> = indices
            .iter()
            .enumerate()
            .filter_map(|(display_idx, &item_idx)| {
                self.flat_items.get(item_idx).map(|item| {
                    let is_selected = display_idx == self.cursor;
                    self.render_item(item, is_selected, theme)
                })
            })
            .collect();

        let mut list_state = std::mem::take(&mut self.list_state);
        list_state.select(Some(self.cursor));

        let list = List::new(list_items).highlight_style(Style::default());

        frame.render_stateful_widget(list, inner, &mut list_state);
        self.list_state = list_state;

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

            frame.render_widget(Paragraph::new(Line::from(spans)), search_area);
        }
    }

    fn render_item(&self, item: &Item, is_selected: bool, theme: &Theme) -> ListItem<'static> {
        let indent = get_indent(item.depth());

        // Groups remain single-line
        if let Item::Group {
            name,
            collapsed,
            session_count,
            ..
        } = item
        {
            let icon = if *collapsed {
                ICON_COLLAPSED
            } else {
                ICON_EXPANDED
            };
            let style = Style::default().fg(theme.group).bold();
            let line = Line::from(vec![
                Span::raw(indent),
                Span::styled(format!("{} ", icon), style),
                Span::styled(format!("{} ({})", name, session_count), style),
            ]);
            return if is_selected {
                ListItem::new(line).style(Style::default().bg(theme.session_selection))
            } else {
                ListItem::new(line)
            };
        }

        // Session rendering
        let Item::Session { id, .. } = item else {
            unreachable!()
        };

        let Some(inst) = self.instance_map.get(id) else {
            let line = Line::from(vec![
                Span::raw(indent),
                Span::styled("? ", Style::default().fg(theme.dimmed)),
                Span::styled(id.clone(), Style::default().fg(theme.dimmed)),
            ]);
            return ListItem::new(line);
        };

        // Resolve icon and status color based on view mode
        let (icon, status_color) = match self.view_mode {
            ViewMode::Agent => {
                let icon = match inst.status {
                    Status::Running => ICON_RUNNING,
                    Status::Waiting => ICON_WAITING,
                    Status::Idle => ICON_IDLE,
                    Status::Error => ICON_ERROR,
                    Status::Starting => ICON_STARTING,
                    Status::Deleting => ICON_DELETING,
                };
                let color = match inst.status {
                    Status::Running => theme.running,
                    Status::Waiting => theme.waiting,
                    Status::Idle => theme.idle,
                    Status::Error => theme.error,
                    Status::Starting => theme.dimmed,
                    Status::Deleting => theme.waiting,
                };
                (icon, color)
            }
            ViewMode::Terminal => {
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
                if terminal_running {
                    (ICON_RUNNING, theme.terminal_active)
                } else {
                    (ICON_IDLE, theme.dimmed)
                }
            }
        };
        let style = Style::default().fg(status_color);
        let title_style = if is_selected { style.bold() } else { style };

        let is_collapsed = self.collapsed_sessions.contains(id);
        let has_worktree = inst.worktree_info.is_some();
        let has_sandbox = inst.is_sandboxed();

        if is_collapsed {
            // Collapsed: single line with indicator dots
            let mut spans = Vec::with_capacity(6);
            spans.push(Span::raw(indent));
            spans.push(Span::styled(format!("{} ", icon), style));
            spans.push(Span::styled(inst.title.clone(), title_style));

            if has_worktree {
                spans.push(Span::styled(
                    format!(" {}", ICON_INDICATOR),
                    Style::default().fg(theme.worktree_indicator),
                ));
            }
            if has_sandbox {
                spans.push(Span::styled(
                    format!(" {}", ICON_INDICATOR),
                    Style::default().fg(theme.sandbox_indicator),
                ));
            }

            let line = Line::from(spans);
            return if is_selected {
                ListItem::new(line)
                    .style(Style::default().bg(tint_background(status_color, theme.background)))
            } else {
                ListItem::new(line)
            };
        }

        // Expanded: multi-line with tree structure
        let mut lines = Vec::with_capacity(4);

        // Title line
        let title_line = Line::from(vec![
            Span::raw(indent),
            Span::styled(format!("{} ", icon), style),
            Span::styled(inst.title.clone(), title_style),
        ]);
        lines.push(title_line);

        // Collect detail entries: (text, style)
        let mut details: Vec<(String, Style)> = Vec::new();

        if let Some(wt_info) = &inst.worktree_info {
            details.push((wt_info.branch.clone(), Style::default().fg(Color::Cyan)));

            // Worktree directory name -- skip if it matches the branch (dedup)
            if let Some(dir_name) = std::path::Path::new(&inst.project_path)
                .file_name()
                .and_then(|n| n.to_str())
            {
                if dir_name != crate::git::template::sanitize_branch_name(&wt_info.branch) {
                    details.push((dir_name.to_string(), Style::default().fg(theme.dimmed)));
                }
            }
        }

        if has_sandbox {
            match self.view_mode {
                ViewMode::Agent => {
                    if let Some(sandbox) = &inst.sandbox_info {
                        details.push((
                            sandbox.container_name.clone(),
                            Style::default().fg(Color::Magenta),
                        ));
                    }
                }
                ViewMode::Terminal => {
                    let mode = self.get_terminal_mode(id);
                    let mode_text = match mode {
                        TerminalMode::Container => "container",
                        TerminalMode::Host => "host",
                    };
                    details.push((mode_text.to_string(), Style::default().fg(Color::Magenta)));
                }
            }
        }

        // Build detail lines with tree connectors
        let detail_count = details.len();
        let tree_style = Style::default().fg(status_color);
        let detail_indent = format!("{}  ", indent);

        for (i, (text, text_style)) in details.into_iter().enumerate() {
            let connector = if i == detail_count - 1 {
                TREE_LAST
            } else {
                TREE_BRANCH
            };
            let detail_line = Line::from(vec![
                Span::raw(detail_indent.clone()),
                Span::styled(connector, tree_style),
                Span::styled(text, text_style),
            ]);
            lines.push(detail_line);
        }

        if is_selected {
            ListItem::new(lines)
                .style(Style::default().bg(tint_background(status_color, theme.background)))
        } else {
            ListItem::new(lines)
        }
    }

    /// Refresh preview cache if needed (session changed, dimensions changed, or timer expired)
    fn refresh_preview_cache_if_needed(&mut self, width: u16, height: u16) {
        const PREVIEW_REFRESH_MS: u128 = 250; // Refresh preview 4x/second max

        let needs_refresh = match &self.selected_session {
            Some(id) => {
                self.preview_cache.session_id.as_ref() != Some(id)
                    || self.preview_cache.dimensions != (width, height)
                    || self.preview_cache.last_refresh.elapsed().as_millis() > PREVIEW_REFRESH_MS
            }
            None => false,
        };

        if needs_refresh {
            if let Some(id) = &self.selected_session {
                if let Some(inst) = self.instance_map.get(id) {
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
                if let Some(inst) = self.instance_map.get(id) {
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
                if let Some(inst) = self.instance_map.get(id) {
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
            .border_style(Style::default().fg(border_color))
            .title(title)
            .title_style(Style::default().fg(title_color));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        match self.view_mode {
            ViewMode::Agent => {
                // Refresh cache before borrowing from instance_map to avoid borrow conflicts
                self.refresh_preview_cache_if_needed(inner.width, inner.height);

                if let Some(id) = &self.selected_session {
                    if let Some(inst) = self.instance_map.get(id) {
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
                    let terminal_mode = if let Some(inst) = self.instance_map.get(&id) {
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
                    if let Some(inst) = self.instance_map.get(&id) {
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
        // Show h/l Fold hint when a session with details is selected
        if let Some(id) = &self.selected_session {
            if let Some(inst) = self.instance_map.get(id) {
                if inst.worktree_info.is_some() || inst.is_sandboxed() {
                    spans.extend([
                        Span::styled("│", sep_style),
                        Span::styled(" h/l", key_style),
                        Span::styled(" Fold ", desc_style),
                    ]);
                }
            }
        }
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
        spans.extend([
            Span::styled("│", sep_style),
            Span::styled(" Tab", key_style),
            Span::styled(
                if self.sidebar_mode {
                    " Preview "
                } else {
                    " Sidebar "
                },
                desc_style,
            ),
        ]);

        // Show c: container/host hint for sandboxed sessions in Terminal view
        if self.view_mode == ViewMode::Terminal {
            if let Some(id) = &self.selected_session {
                if let Some(inst) = self.instance_map.get(id) {
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
