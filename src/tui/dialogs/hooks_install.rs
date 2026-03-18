//! Acknowledgment dialog for first-time agent status hook installation

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::*;

use super::DialogResult;
use crate::tui::styles::Theme;

pub struct HooksInstallDialog {
    settings_paths: Vec<String>,
    hook_commands: Vec<(String, String)>,
    selected: bool, // true = Accept, false = Cancel
    scroll_offset: u16,
}

impl HooksInstallDialog {
    pub fn new(tool_name: &str) -> Self {
        let mut settings_paths = Vec::new();
        let mut hook_commands = Vec::new();

        if let Some(agent) = crate::agents::get_agent(tool_name) {
            if let Some(hook_cfg) = &agent.hook_config {
                settings_paths.push(format!("~/{}", hook_cfg.settings_rel_path));
                for event in hook_cfg.events {
                    let label = match event.status {
                        Some(s) => format!("writes \"{}\"", s),
                        None => "session lifecycle".to_string(),
                    };
                    hook_commands.push((event.name.to_string(), label));
                }
            }
        }

        Self {
            settings_paths,
            hook_commands,
            selected: true,
            scroll_offset: 0,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult<bool> {
        match key.code {
            KeyCode::Esc => DialogResult::Cancel,
            KeyCode::Char('y') | KeyCode::Char('Y') => DialogResult::Submit(true),
            KeyCode::Char('n') | KeyCode::Char('N') => DialogResult::Cancel,
            KeyCode::Enter => {
                if self.selected {
                    DialogResult::Submit(true)
                } else {
                    DialogResult::Cancel
                }
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.selected = true;
                DialogResult::Continue
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.selected = false;
                DialogResult::Continue
            }
            KeyCode::Tab => {
                self.selected = !self.selected;
                DialogResult::Continue
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.scroll_offset = self.scroll_offset.saturating_sub(1);
                DialogResult::Continue
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let total_lines = self.build_content_lines().len() as u16;
                if self.scroll_offset + 1 < total_lines {
                    self.scroll_offset += 1;
                }
                DialogResult::Continue
            }
            _ => DialogResult::Continue,
        }
    }

    fn build_content_lines(&self) -> Vec<Line<'_>> {
        let mut lines = Vec::new();

        lines.push(Line::from(Span::styled(
            "Modified files:",
            Style::default().bold(),
        )));
        for path in &self.settings_paths {
            lines.push(Line::from(format!("  {}", path)));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Hook events added:",
            Style::default().bold(),
        )));
        for (event, status) in &self.hook_commands {
            lines.push(Line::from(format!("  {} -> {}", event, status)));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Each hook runs:",
            Style::default().bold(),
        )));
        lines.push(Line::from("  printf {status} > /tmp/aoe-hooks/$ID/status"));

        lines.push(Line::from(""));
        lines.push(Line::from(
            "Hooks are guarded by $AOE_INSTANCE_ID and are a",
        ));
        lines.push(Line::from("no-op outside of AoE sessions."));

        lines
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let content_lines = self.build_content_lines();
        let content_height = content_lines.len() as u16 + 6; // header + spacing + buttons

        let dialog_width = 64.min(area.width.saturating_sub(4));
        let dialog_height = (content_height + 6).min(area.height.saturating_sub(4));
        let dialog_area = super::centered_rect(area, dialog_width, dialog_height);

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .title(" Agent Status Hooks ")
            .title_style(Style::default().fg(theme.accent).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // header
                Constraint::Min(1),    // content
                Constraint::Length(2), // buttons
            ])
            .split(inner);

        // Header
        let header = Paragraph::new(
            "AoE needs to install hooks into your agent's settings\nto detect session status (running/waiting/idle).",
        )
        .style(Style::default().fg(theme.text))
        .wrap(Wrap { trim: true });
        frame.render_widget(header, chunks[0]);

        // Scrollable content
        let visible_lines: Vec<Line> = content_lines
            .into_iter()
            .skip(self.scroll_offset as usize)
            .collect();
        let content_paragraph = Paragraph::new(visible_lines)
            .style(Style::default().fg(theme.dimmed))
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(theme.border)),
            );
        frame.render_widget(content_paragraph, chunks[1]);

        // Buttons
        let accept_style = if self.selected {
            Style::default().fg(theme.running).bold()
        } else {
            Style::default().fg(theme.dimmed)
        };
        let cancel_style = if !self.selected {
            Style::default().fg(theme.accent).bold()
        } else {
            Style::default().fg(theme.dimmed)
        };

        let buttons = Line::from(vec![
            Span::raw("  "),
            Span::styled("[Accept (y)]", accept_style),
            Span::raw("    "),
            Span::styled("[Cancel (Esc)]", cancel_style),
        ]);

        frame.render_widget(
            Paragraph::new(buttons).alignment(Alignment::Center),
            chunks[2],
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
    fn test_default_selection_is_accept() {
        let dialog = HooksInstallDialog::new("claude");
        assert!(dialog.selected);
    }

    #[test]
    fn test_y_accepts() {
        let mut dialog = HooksInstallDialog::new("claude");
        let result = dialog.handle_key(key(KeyCode::Char('y')));
        assert!(matches!(result, DialogResult::Submit(true)));
    }

    #[test]
    fn test_n_cancels() {
        let mut dialog = HooksInstallDialog::new("claude");
        let result = dialog.handle_key(key(KeyCode::Char('n')));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_esc_cancels() {
        let mut dialog = HooksInstallDialog::new("claude");
        let result = dialog.handle_key(key(KeyCode::Esc));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_enter_with_accept_selected() {
        let mut dialog = HooksInstallDialog::new("claude");
        dialog.selected = true;
        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, DialogResult::Submit(true)));
    }

    #[test]
    fn test_enter_with_cancel_selected() {
        let mut dialog = HooksInstallDialog::new("claude");
        dialog.selected = false;
        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_tab_toggles() {
        let mut dialog = HooksInstallDialog::new("claude");
        assert!(dialog.selected);
        dialog.handle_key(key(KeyCode::Tab));
        assert!(!dialog.selected);
        dialog.handle_key(key(KeyCode::Tab));
        assert!(dialog.selected);
    }

    #[test]
    fn test_content_shows_settings_path() {
        let dialog = HooksInstallDialog::new("claude");
        let lines = dialog.build_content_lines();
        let text: String = lines
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains(".claude/settings.json"));
    }

    #[test]
    fn test_content_shows_hook_events() {
        let dialog = HooksInstallDialog::new("claude");
        let lines = dialog.build_content_lines();
        let text: String = lines
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("PreToolUse"));
        assert!(text.contains("Stop"));
        assert!(text.contains("Notification"));
    }

    #[test]
    fn test_cursor_agent_shows_cursor_path() {
        let dialog = HooksInstallDialog::new("cursor");
        let lines = dialog.build_content_lines();
        let text: String = lines
            .iter()
            .map(|l| l.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains(".cursor/settings.json"));
    }
}
