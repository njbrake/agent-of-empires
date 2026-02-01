//! Changelog dialog for showing updates after version changes

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::*;

use super::DialogResult;
use crate::tui::styles::Theme;
use crate::update::{get_cached_releases, ReleaseInfo};

pub struct ChangelogDialog {
    scroll_offset: usize,
    releases: Vec<ReleaseInfo>,
}

impl ChangelogDialog {
    pub fn new(from_version: Option<String>) -> Self {
        let releases = get_cached_releases(from_version.as_deref());
        Self {
            scroll_offset: 0,
            releases,
        }
    }

    fn content_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();

        if self.releases.is_empty() {
            lines.push("No release notes available.".to_string());
            return lines;
        }

        for release in &self.releases {
            lines.push(format!("v{}", release.version));

            // Parse the release body - split by lines and add as bullet points
            for line in release.body.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                // Skip markdown headers but keep their content
                let content = trimmed.trim_start_matches('#').trim();
                if content.is_empty() {
                    continue;
                }
                // Format as indented content
                if trimmed.starts_with('-') || trimmed.starts_with('*') {
                    lines.push(format!("  {}", trimmed));
                } else if !trimmed.starts_with('#') {
                    lines.push(format!("  {}", content));
                }
            }
            lines.push(String::new());
        }

        lines
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult<()> {
        let content_len = self.content_lines().len();

        match key.code {
            KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char(' ') => {
                DialogResult::Submit(())
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.scroll_offset < content_len.saturating_sub(5) {
                    self.scroll_offset += 1;
                }
                DialogResult::Continue
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
                DialogResult::Continue
            }
            KeyCode::PageDown => {
                self.scroll_offset = (self.scroll_offset + 5).min(content_len.saturating_sub(5));
                DialogResult::Continue
            }
            KeyCode::PageUp => {
                self.scroll_offset = self.scroll_offset.saturating_sub(5);
                DialogResult::Continue
            }
            _ => DialogResult::Continue,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        // Use 80% of terminal width/height, with min/max bounds
        let dialog_width = (area.width * 80 / 100).clamp(60, 100);
        let dialog_height = (area.height * 80 / 100).clamp(16, 40);
        let dialog_area = super::centered_rect(area, dialog_width, dialog_height);

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .title(" What's New ")
            .title_style(Style::default().fg(theme.accent).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([Constraint::Min(1), Constraint::Length(2)])
            .split(inner);

        let content_area = chunks[0];
        let visible_height = content_area.height as usize;

        let all_lines = self.content_lines();
        let styled_lines: Vec<Line> = all_lines
            .into_iter()
            .skip(self.scroll_offset)
            .take(visible_height)
            .map(|text| {
                if text.starts_with('v') {
                    Line::from(Span::styled(text, Style::default().fg(theme.title).bold()))
                } else if text.starts_with("  ") {
                    Line::from(Span::styled(text, Style::default().fg(theme.text)))
                } else {
                    Line::from(text)
                }
            })
            .collect();

        let paragraph = Paragraph::new(styled_lines).wrap(Wrap { trim: false });
        frame.render_widget(paragraph, content_area);

        let button = Line::from(vec![
            Span::styled("[Got it]", Style::default().fg(theme.accent).bold()),
            Span::styled("  j/k to scroll", Style::default().fg(theme.dimmed)),
        ]);
        frame.render_widget(
            Paragraph::new(button).alignment(Alignment::Center),
            chunks[1],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn test_enter_submits() {
        let mut dialog = ChangelogDialog {
            scroll_offset: 0,
            releases: vec![],
        };
        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, DialogResult::Submit(())));
    }

    #[test]
    fn test_esc_submits() {
        let mut dialog = ChangelogDialog {
            scroll_offset: 0,
            releases: vec![],
        };
        let result = dialog.handle_key(key(KeyCode::Esc));
        assert!(matches!(result, DialogResult::Submit(())));
    }

    #[test]
    fn test_q_submits() {
        let mut dialog = ChangelogDialog {
            scroll_offset: 0,
            releases: vec![],
        };
        let result = dialog.handle_key(key(KeyCode::Char('q')));
        assert!(matches!(result, DialogResult::Submit(())));
    }

    #[test]
    fn test_space_submits() {
        let mut dialog = ChangelogDialog {
            scroll_offset: 0,
            releases: vec![],
        };
        let result = dialog.handle_key(key(KeyCode::Char(' ')));
        assert!(matches!(result, DialogResult::Submit(())));
    }

    #[test]
    fn test_scroll_down() {
        let mut dialog = ChangelogDialog {
            scroll_offset: 0,
            releases: vec![ReleaseInfo {
                version: "1.0.0".to_string(),
                body: "- Change 1\n- Change 2\n- Change 3\n- Change 4\n- Change 5\n- Change 6"
                    .to_string(),
                published_at: None,
            }],
        };
        assert_eq!(dialog.scroll_offset, 0);
        dialog.handle_key(key(KeyCode::Down));
        assert_eq!(dialog.scroll_offset, 1);
    }

    #[test]
    fn test_scroll_up() {
        let mut dialog = ChangelogDialog {
            scroll_offset: 2,
            releases: vec![],
        };
        dialog.handle_key(key(KeyCode::Up));
        assert_eq!(dialog.scroll_offset, 1);
    }

    #[test]
    fn test_scroll_up_at_zero() {
        let mut dialog = ChangelogDialog {
            scroll_offset: 0,
            releases: vec![],
        };
        dialog.handle_key(key(KeyCode::Up));
        assert_eq!(dialog.scroll_offset, 0);
    }

    #[test]
    fn test_content_lines_with_releases() {
        let dialog = ChangelogDialog {
            scroll_offset: 0,
            releases: vec![ReleaseInfo {
                version: "1.0.0".to_string(),
                body: "- Added feature X\n- Fixed bug Y".to_string(),
                published_at: None,
            }],
        };
        let lines = dialog.content_lines();
        assert!(lines.iter().any(|l| l.contains("v1.0.0")));
        assert!(lines.iter().any(|l| l.contains("Added feature X")));
    }

    #[test]
    fn test_content_lines_empty_releases() {
        let dialog = ChangelogDialog {
            scroll_offset: 0,
            releases: vec![],
        };
        let lines = dialog.content_lines();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("No release notes"));
    }
}
