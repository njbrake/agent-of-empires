//! Preview panel component

use ratatui::prelude::*;
use ratatui::widgets::*;

use crate::session::Instance;
use crate::tui::styles::Theme;

pub struct Preview;

impl Preview {
    pub fn render(frame: &mut Frame, area: Rect, instance: &Instance, theme: &Theme) {
        // Adjust height based on whether worktree info is present
        let info_height = if instance.worktree_info.is_some() {
            10 // Expanded to show worktree details
        } else {
            6 // Standard height
        };

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(info_height), // Info section
                Constraint::Min(1),              // Output section
            ])
            .split(area);

        Self::render_info(frame, chunks[0], instance, theme);
        Self::render_output(frame, chunks[1], instance, theme);
    }

    fn render_info(frame: &mut Frame, area: Rect, instance: &Instance, theme: &Theme) {
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
                Span::styled("Tool:    ", Style::default().fg(theme.dimmed)),
                Span::styled(&instance.tool, Style::default().fg(theme.accent)),
            ]),
            Line::from(vec![
                Span::styled("Status:  ", Style::default().fg(theme.dimmed)),
                Span::styled(
                    format!("{:?}", instance.status),
                    Style::default().fg(match instance.status {
                        crate::session::Status::Running => theme.running,
                        crate::session::Status::Waiting => theme.waiting,
                        crate::session::Status::Idle => theme.idle,
                        crate::session::Status::Error => theme.error,
                        crate::session::Status::Starting => theme.dimmed,
                    }),
                ),
            ]),
            Line::from(vec![
                Span::styled("Group:   ", Style::default().fg(theme.dimmed)),
                Span::styled(
                    if instance.group_path.is_empty() {
                        "(none)"
                    } else {
                        &instance.group_path
                    },
                    Style::default().fg(theme.group),
                ),
            ]),
        ];

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
                Span::styled(&wt_info.branch, Style::default().fg(Color::Cyan)),
            ]));
            info_lines.push(Line::from(vec![
                Span::styled("Main:    ", Style::default().fg(theme.dimmed)),
                Span::styled(
                    shorten_path(&wt_info.main_repo_path),
                    Style::default().fg(theme.text),
                ),
            ]));

            let managed_text = if wt_info.managed_by_aoe {
                "Yes (delete branch on aoe session delete)"
            } else {
                "No (manual worktree)"
            };
            info_lines.push(Line::from(vec![
                Span::styled("Managed: ", Style::default().fg(theme.dimmed)),
                Span::styled(
                    managed_text,
                    Style::default().fg(if wt_info.managed_by_aoe {
                        Color::Green
                    } else {
                        Color::Yellow
                    }),
                ),
            ]));
        }

        let paragraph = Paragraph::new(info_lines);
        frame.render_widget(paragraph, area);
    }

    fn render_output(frame: &mut Frame, area: Rect, instance: &Instance, theme: &Theme) {
        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(theme.border))
            .title(" Output ")
            .title_style(Style::default().fg(theme.dimmed));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        if let Some(error) = &instance.last_error {
            let error_lines: Vec<Line> = vec![
                Line::from(Span::styled(
                    "Error:",
                    Style::default().fg(theme.error).bold(),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    error.as_str(),
                    Style::default().fg(theme.error),
                )),
            ];
            let paragraph = Paragraph::new(error_lines).wrap(Wrap { trim: false });
            frame.render_widget(paragraph, inner);
            return;
        }

        let output = instance
            .capture_output_with_size(inner.height as usize, inner.width, inner.height)
            .unwrap_or_default();

        if output.is_empty() {
            let hint = Paragraph::new("No output available")
                .style(Style::default().fg(theme.dimmed))
                .alignment(Alignment::Center);
            frame.render_widget(hint, inner);
        } else {
            let output_lines: Vec<Line> = output
                .lines()
                .map(|line| Line::from(Span::raw(line)))
                .collect();

            let line_count = output_lines.len();
            let visible_height = inner.height as usize;

            // Scroll to show the bottom of the content
            let scroll_offset = if line_count > visible_height {
                (line_count - visible_height) as u16
            } else {
                0
            };

            let paragraph = Paragraph::new(output_lines)
                .style(Style::default().fg(theme.text))
                .scroll((scroll_offset, 0));

            frame.render_widget(paragraph, inner);
        }
    }
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
}
