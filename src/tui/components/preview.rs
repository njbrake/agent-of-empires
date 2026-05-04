//! Preview panel component

use std::time::Duration;

use ansi_to_tui::IntoText;
use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::session::Instance;
use crate::tui::styles::Theme;

pub struct Preview;

impl Preview {
    #[allow(clippy::too_many_arguments)]
    pub fn render_terminal_preview(
        frame: &mut Frame,
        area: Rect,
        instance: &Instance,
        terminal_running: bool,
        cached_output: &str,
        scroll_offset: u16,
        theme: &Theme,
        compact: bool,
    ) {
        // Compact mode (narrow viewports) skips the info header entirely:
        // the outer block title already carries the session name + status,
        // and on a phone every row of vertical space matters.
        let output_area = if compact {
            area
        } else {
            let info_height = if instance.sandbox_info.as_ref().is_some_and(|s| s.enabled) {
                4 // title + path + status + sandbox
            } else {
                3 // title + path + status
            };
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(info_height), // Minimal info section
                    Constraint::Min(1),              // Output section
                ])
                .split(area);

            // Minimal info for terminal view
            let mut info_lines = vec![
                Line::from(vec![
                    Span::styled("Title:   ", Style::default().fg(theme.dimmed)),
                    Span::styled(&instance.title, Style::default().fg(theme.text).bold()),
                ]),
                Line::from(vec![
                    Span::styled("Path:    ", Style::default().fg(theme.dimmed)),
                    Span::styled(
                        shorten_path(&instance.project_path),
                        Style::default().fg(theme.text),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("Status:  ", Style::default().fg(theme.dimmed)),
                    Span::styled(
                        if terminal_running {
                            "Running"
                        } else {
                            "Not started"
                        },
                        Style::default().fg(if terminal_running {
                            theme.terminal_active
                        } else {
                            theme.dimmed
                        }),
                    ),
                ]),
            ];
            if let Some(sandbox) = &instance.sandbox_info {
                if sandbox.enabled {
                    info_lines.push(Line::from(vec![
                        Span::styled("Sandbox: ", Style::default().fg(theme.dimmed)),
                        Span::styled(&sandbox.container_name, Style::default().fg(theme.sandbox)),
                    ]));
                }
            }
            let paragraph = Paragraph::new(info_lines);
            frame.render_widget(paragraph, chunks[0]);
            chunks[1]
        };

        // Output section
        let visible_height = output_area.height.saturating_sub(1) as usize;
        let parsed_output = if terminal_running && !cached_output.is_empty() {
            Some(parse_output_text(cached_output))
        } else {
            None
        };
        let line_count = parsed_output.as_ref().map_or(0, |t| t.lines.len());

        // Compact mode: no inner separator/title; the outer block already
        // names the session. Scroll indicator is dropped to save a row.
        let inner = if compact {
            output_area
        } else {
            let mut block = Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(theme.border))
                .title(" Terminal Output ")
                .title_style(Style::default().fg(theme.dimmed));
            if let Some(indicator) =
                format_scroll_indicator(line_count, visible_height, scroll_offset)
            {
                block = block.title_top(
                    Line::from(indicator)
                        .right_aligned()
                        .style(Style::default().fg(theme.dimmed)),
                );
            }
            let inner = block.inner(output_area);
            frame.render_widget(block, output_area);
            inner
        };

        if !terminal_running {
            let hint = Paragraph::new("Press Enter to start terminal")
                .style(Style::default().fg(theme.dimmed))
                .alignment(Alignment::Center);
            frame.render_widget(hint, inner);
        } else if let Some(output_text) = parsed_output {
            let paragraph_scroll = compute_scroll(line_count, visible_height, scroll_offset);

            let paragraph = Paragraph::new(output_text)
                .style(Style::default().fg(theme.text))
                .scroll((paragraph_scroll, 0));

            frame.render_widget(paragraph, inner);
        } else {
            let hint = Paragraph::new("No output available")
                .style(Style::default().fg(theme.dimmed))
                .alignment(Alignment::Center);
            frame.render_widget(hint, inner);
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render_with_cache(
        frame: &mut Frame,
        area: Rect,
        instance: &Instance,
        cached_output: &str,
        scroll_offset: u16,
        theme: &Theme,
        idle_decay_window: Duration,
        compact: bool,
    ) {
        if compact {
            Self::render_output_cached(
                frame,
                area,
                instance,
                cached_output,
                scroll_offset,
                theme,
                true,
            );
            return;
        }

        // 3 base lines (profile+tool / path / status) + optional sandbox + optional worktree block
        let base = 3;
        let sandbox_lines = if instance.is_sandboxed() { 1 } else { 0 };
        let info_height = if instance.worktree_info.is_some() {
            base + sandbox_lines + 4 // blank + header + branch + main
        } else {
            base + sandbox_lines
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(info_height), // Info section
                Constraint::Min(1),              // Output section
            ])
            .split(area);

        Self::render_info(frame, chunks[0], instance, theme, idle_decay_window);
        Self::render_output_cached(
            frame,
            chunks[1],
            instance,
            cached_output,
            scroll_offset,
            theme,
            false,
        );
    }

    fn render_info(
        frame: &mut Frame,
        area: Rect,
        instance: &Instance,
        theme: &Theme,
        idle_decay_window: Duration,
    ) {
        let mut info_lines = Vec::new();

        // Profile and Tool on the same row to save vertical space
        let mut profile_tool_spans = Vec::new();
        if !instance.source_profile.is_empty() {
            profile_tool_spans.push(Span::styled("Profile: ", Style::default().fg(theme.dimmed)));
            profile_tool_spans.push(Span::styled(
                &instance.source_profile,
                Style::default().fg(theme.accent),
            ));
            profile_tool_spans.push(Span::raw("  "));
        }
        profile_tool_spans.push(Span::styled("Tool: ", Style::default().fg(theme.dimmed)));
        profile_tool_spans.push(Span::styled(
            &instance.tool,
            Style::default().fg(theme.accent),
        ));
        info_lines.push(Line::from(profile_tool_spans));

        info_lines.extend([
            Line::from(vec![
                Span::styled("Path:    ", Style::default().fg(theme.dimmed)),
                Span::styled(
                    shorten_path(&instance.project_path),
                    Style::default().fg(theme.text),
                ),
            ]),
            Line::from(vec![
                Span::styled("Status:  ", Style::default().fg(theme.dimmed)),
                Span::styled(
                    format!("{:?}", instance.status),
                    Style::default().fg(match instance.status {
                        crate::session::Status::Running => theme.running,
                        crate::session::Status::Waiting => theme.waiting,
                        crate::session::Status::Idle => {
                            theme.idle_color_at_age(instance.idle_age(), idle_decay_window)
                        }
                        crate::session::Status::Unknown => theme.waiting,
                        crate::session::Status::Stopped => theme.dimmed,
                        crate::session::Status::Error => theme.error,
                        crate::session::Status::Starting => theme.dimmed,
                        crate::session::Status::Deleting => theme.waiting,
                        crate::session::Status::Creating => theme.accent,
                    }),
                ),
            ]),
        ]);

        // Add sandbox information if present
        if let Some(sandbox) = &instance.sandbox_info {
            if sandbox.enabled {
                info_lines.push(Line::from(vec![
                    Span::styled("Sandbox: ", Style::default().fg(theme.dimmed)),
                    Span::styled(&sandbox.container_name, Style::default().fg(theme.sandbox)),
                ]));
            }
        }

        // Add worktree information if present
        if let Some(wt_info) = &instance.worktree_info {
            info_lines.push(Line::from(""));
            info_lines.push(Line::from(vec![
                Span::styled("─", Style::default().fg(theme.border)),
                Span::styled(" Worktree ", Style::default().fg(theme.dimmed)),
                Span::styled("─", Style::default().fg(theme.border)),
            ]));
            info_lines.push(Line::from(vec![
                Span::styled("Branch:  ", Style::default().fg(theme.dimmed)),
                Span::styled(&wt_info.branch, Style::default().fg(theme.branch)),
            ]));
            info_lines.push(Line::from(vec![
                Span::styled("Main:    ", Style::default().fg(theme.dimmed)),
                Span::styled(
                    shorten_path(&wt_info.main_repo_path),
                    Style::default().fg(theme.text),
                ),
            ]));
        }

        let paragraph = Paragraph::new(info_lines);
        frame.render_widget(paragraph, area);
    }

    fn render_output_cached(
        frame: &mut Frame,
        area: Rect,
        instance: &Instance,
        cached_output: &str,
        scroll_offset: u16,
        theme: &Theme,
        compact: bool,
    ) {
        let visible_height = area.height.saturating_sub(1) as usize;
        let parsed_output = if instance.last_error.is_none() && !cached_output.is_empty() {
            Some(parse_output_text(cached_output))
        } else {
            None
        };
        let line_count = parsed_output.as_ref().map_or(0, |t| t.lines.len());

        // Compact mode skips the inner separator/title; the outer block
        // already names the session and scroll indicator is omitted to
        // free a row.
        let inner = if compact {
            area
        } else {
            let mut block = Block::default()
                .borders(Borders::TOP)
                .border_style(Style::default().fg(theme.border))
                .title(" Output ")
                .title_style(Style::default().fg(theme.dimmed));
            if let Some(indicator) =
                format_scroll_indicator(line_count, visible_height, scroll_offset)
            {
                block = block.title_top(
                    Line::from(indicator)
                        .right_aligned()
                        .style(Style::default().fg(theme.dimmed)),
                );
            }
            let inner = block.inner(area);
            frame.render_widget(block, area);
            inner
        };

        if let Some(error) = &instance.last_error {
            let mut error_lines: Vec<Line> = vec![
                Line::from(Span::styled(
                    "Error:",
                    Style::default().fg(theme.error).bold(),
                )),
                Line::from(""),
            ];
            for line in error.split('\n') {
                error_lines.push(Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(theme.error),
                )));
            }
            let paragraph = Paragraph::new(error_lines).wrap(Wrap { trim: false });
            frame.render_widget(paragraph, inner);
            return;
        }

        if let Some(output_text) = parsed_output {
            let paragraph_scroll = compute_scroll(line_count, visible_height, scroll_offset);

            let paragraph = Paragraph::new(output_text)
                .style(Style::default().fg(theme.text))
                .scroll((paragraph_scroll, 0));

            frame.render_widget(paragraph, inner);
        } else {
            let hint = Paragraph::new("No output available")
                .style(Style::default().fg(theme.dimmed))
                .alignment(Alignment::Center);
            frame.render_widget(hint, inner);
        }
    }
}

/// Pick the row offset passed to `Paragraph::scroll`. Zero user offset shows
/// the bottom of the cached pane (live-follow). A positive offset scrolls the
/// same number of lines back, saturating at the top of the capture.
fn compute_scroll(line_count: usize, visible_height: usize, user_offset: u16) -> u16 {
    if line_count <= visible_height {
        return 0;
    }
    let bottom = (line_count - visible_height) as u16;
    bottom.saturating_sub(user_offset)
}

/// Render a tmux-style ` [offset/max] ` indicator when the user has scrolled
/// back. Returns `None` while live-following or when the content fits in view.
fn format_scroll_indicator(
    line_count: usize,
    visible_height: usize,
    user_offset: u16,
) -> Option<String> {
    if user_offset == 0 || line_count <= visible_height {
        return None;
    }
    let max_offset = (line_count - visible_height) as u16;
    let clamped = user_offset.min(max_offset);
    Some(format!(" [{}/{}] ", clamped, max_offset))
}

fn parse_output_text(content: &str) -> Text<'static> {
    content
        .into_text()
        .unwrap_or_else(|_| Text::from(content.to_string()))
}

fn shorten_path(path: &str) -> String {
    let path_buf = std::path::PathBuf::from(path);

    if let Some(home) = dirs::home_dir() {
        if let (Ok(canonical_path), Ok(canonical_home)) =
            (path_buf.canonicalize(), home.canonicalize())
        {
            let path_str = canonical_path.to_string_lossy();
            if let Some(home_str) = canonical_home.to_str() {
                if let Some(stripped) = path_str.strip_prefix(home_str) {
                    return format!("~{}", stripped);
                }
            }
            return path_str.into_owned();
        }

        if let Some(home_str) = home.to_str() {
            if let Some(stripped) = path.strip_prefix(home_str) {
                return format!("~{}", stripped);
            }
        }
    }
    path.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shorten_path_with_home() {
        if let Some(home) = dirs::home_dir() {
            if let Some(home_str) = home.to_str() {
                let path = format!("{}/projects/myapp", home_str);
                let shortened = shorten_path(&path);
                assert_eq!(shortened, "~/projects/myapp");
            }
        }
    }

    #[test]
    fn test_shorten_path_without_home_prefix() {
        let path = "/tmp/some/path";
        let shortened = shorten_path(path);
        assert_eq!(shortened, "/tmp/some/path");
    }

    #[test]
    fn test_shorten_path_exact_home() {
        if let Some(home) = dirs::home_dir() {
            if let Some(home_str) = home.to_str() {
                let shortened = shorten_path(home_str);
                assert_eq!(shortened, "~");
            }
        }
    }

    #[test]
    fn test_shorten_path_relative() {
        let path = "relative/path";
        let shortened = shorten_path(path);
        assert_eq!(shortened, "relative/path");
    }

    #[test]
    fn test_shorten_path_empty() {
        let path = "";
        let shortened = shorten_path(path);
        assert_eq!(shortened, "");
    }

    #[test]
    fn test_shorten_path_similar_prefix_not_home() {
        if let Some(home) = dirs::home_dir() {
            if let Some(home_str) = home.to_str() {
                let path = format!("{}extra/not/home", home_str);
                let shortened = shorten_path(&path);
                assert_eq!(shortened, format!("~extra/not/home"));
            }
        }
    }

    #[test]
    fn test_shorten_path_preserves_trailing_slash() {
        if let Some(home) = dirs::home_dir() {
            if let Some(home_str) = home.to_str() {
                let path = format!("{}/projects/", home_str);
                let shortened = shorten_path(&path);
                assert_eq!(shortened, "~/projects/");
            }
        }
    }

    #[test]
    fn compute_scroll_live_follow_when_content_fits() {
        assert_eq!(compute_scroll(5, 10, 0), 0);
        assert_eq!(compute_scroll(5, 10, 20), 0);
    }

    #[test]
    fn compute_scroll_sticks_to_bottom_with_zero_offset() {
        assert_eq!(compute_scroll(100, 20, 0), 80);
    }

    #[test]
    fn compute_scroll_walks_back_by_offset() {
        assert_eq!(compute_scroll(100, 20, 15), 65);
    }

    #[test]
    fn compute_scroll_saturates_at_top() {
        assert_eq!(compute_scroll(100, 20, 500), 0);
    }

    #[test]
    fn scroll_indicator_hidden_when_live() {
        assert_eq!(format_scroll_indicator(100, 20, 0), None);
    }

    #[test]
    fn scroll_indicator_hidden_when_content_fits() {
        assert_eq!(format_scroll_indicator(10, 20, 5), None);
    }

    #[test]
    fn scroll_indicator_reports_position_and_max() {
        assert_eq!(
            format_scroll_indicator(100, 20, 15),
            Some(" [15/80] ".to_string())
        );
    }

    #[test]
    fn scroll_indicator_clamps_to_max() {
        assert_eq!(
            format_scroll_indicator(100, 20, 500),
            Some(" [80/80] ".to_string())
        );
    }
}
