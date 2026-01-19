//! Rendering for HomeView

use ratatui::prelude::*;
use ratatui::widgets::*;
use std::time::Instant;

use super::{
    get_indent, HomeView, ViewMode, ICON_COLLAPSED, ICON_DELETING, ICON_ERROR, ICON_EXPANDED,
    ICON_IDLE, ICON_RUNNING, ICON_STARTING, ICON_WAITING,
};
use crate::session::{Item, Status};
use crate::tui::components::{HelpOverlay, Preview};
use crate::tui::styles::Theme;
use crate::update::UpdateInfo;

impl HomeView {
    pub fn render(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
        update_info: Option<&UpdateInfo>,
    ) {
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
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
            .split(main_chunks[0]);

        self.render_list(frame, chunks[0], theme);
        self.render_preview(frame, chunks[1], theme);
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
            ViewMode::Agent => format!(" Agent of Empires [{}] ", self.storage.profile()),
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

        let list =
            List::new(list_items).highlight_style(Style::default().bg(theme.session_selection));

        frame.render_widget(list, inner);

        // Render search bar if active
        if self.search_active {
            let search_area = Rect {
                x: inner.x,
                y: inner.y + inner.height.saturating_sub(1),
                width: inner.width,
                height: 1,
            };
            let search_text = format!("/{}", self.search_query);
            let search_para = Paragraph::new(search_text).style(Style::default().fg(theme.search));
            frame.render_widget(search_para, search_area);
        }
    }

    fn render_item(&self, item: &Item, is_selected: bool, theme: &Theme) -> ListItem<'_> {
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
                if let Some(inst) = self.instance_map.get(id) {
                    match self.view_mode {
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
                            let style = Style::default().fg(color);
                            (icon, Cow::Borrowed(&inst.title), style)
                        }
                        ViewMode::Terminal => {
                            let terminal_running = inst
                                .terminal_tmux_session()
                                .map(|s| s.exists())
                                .unwrap_or(false);
                            let (icon, color) = if terminal_running {
                                (ICON_RUNNING, theme.running)
                            } else {
                                (ICON_IDLE, theme.dimmed)
                            };
                            let style = Style::default().fg(color);
                            (icon, Cow::Borrowed(&inst.title), style)
                        }
                    }
                } else {
                    (
                        "?",
                        Cow::Borrowed(id.as_str()),
                        Style::default().fg(theme.dimmed),
                    )
                }
            }
        };

        let mut line_spans = Vec::with_capacity(5);
        line_spans.push(Span::raw(indent));
        line_spans.push(Span::styled(format!("{} ", icon), style));
        line_spans.push(Span::styled(
            text.into_owned(),
            if is_selected { style.bold() } else { style },
        ));

        if let Item::Session { id, .. } = item {
            if let Some(inst) = self.instance_map.get(id) {
                if let Some(wt_info) = &inst.worktree_info {
                    line_spans.push(Span::styled(
                        format!("  {}", wt_info.branch),
                        Style::default().fg(Color::Cyan),
                    ));
                }
                if inst.is_sandboxed() && self.view_mode == ViewMode::Agent {
                    line_spans.push(Span::styled(
                        " [sandbox]",
                        Style::default().fg(Color::Magenta),
                    ));
                }
            }
        }

        let line = Line::from(line_spans);

        if is_selected {
            ListItem::new(line).style(Style::default().bg(theme.session_selection))
        } else {
            ListItem::new(line)
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

    /// Refresh terminal preview cache if needed
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
                self.refresh_terminal_preview_cache_if_needed(inner.width, inner.height);

                if let Some(id) = &self.selected_session {
                    if let Some(inst) = self.instance_map.get(id) {
                        let terminal_running = inst
                            .terminal_tmux_session()
                            .map(|s| s.exists())
                            .unwrap_or(false);
                        Preview::render_terminal_preview(
                            frame,
                            inner,
                            inst,
                            terminal_running,
                            &self.terminal_preview_cache.content,
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

        let spans = vec![
            Span::styled(format!(" {} ", mode_indicator), mode_style),
            Span::styled("│", sep_style),
            Span::styled(" j/k", key_style),
            Span::styled(" Nav ", desc_style),
            Span::styled("│", sep_style),
            Span::styled(" Enter", key_style),
            Span::styled(" Attach ", desc_style),
            Span::styled("│", sep_style),
            Span::styled(" t", key_style),
            Span::styled(" View ", desc_style),
            Span::styled("│", sep_style),
            Span::styled(" n", key_style),
            Span::styled(" New ", desc_style),
            Span::styled("│", sep_style),
            Span::styled(" d", key_style),
            Span::styled(" Del ", desc_style),
            Span::styled("│", sep_style),
            Span::styled(" /", key_style),
            Span::styled(" Search ", desc_style),
            Span::styled("│", sep_style),
            Span::styled(" ?", key_style),
            Span::styled(" Help ", desc_style),
            Span::styled("│", sep_style),
            Span::styled(" q", key_style),
            Span::styled(" Quit", desc_style),
        ];

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
