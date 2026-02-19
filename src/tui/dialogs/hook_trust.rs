//! Trust confirmation dialog for repository hooks

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::*;

use super::DialogResult;
use crate::session::HooksConfig;
use crate::tui::styles::Theme;

pub struct HookTrustDialog {
    hooks: HooksConfig,
    hooks_hash: String,
    project_path: String,
    selected: bool, // true = Trust, false = Skip
    scroll_offset: u16,
}

/// Result from the hook trust dialog.
pub enum HookTrustAction {
    /// User trusts the hooks; proceed with execution.
    Trust {
        hooks: HooksConfig,
        hooks_hash: String,
        project_path: String,
    },
    /// User chose to skip hooks but still create the session.
    Skip,
}

impl HookTrustDialog {
    pub fn new(hooks: HooksConfig, hooks_hash: String, project_path: String) -> Self {
        Self {
            hooks,
            hooks_hash,
            project_path,
            selected: false,
            scroll_offset: 0,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult<HookTrustAction> {
        match key.code {
            KeyCode::Esc => DialogResult::Cancel,
            KeyCode::Char('n') | KeyCode::Char('N') => DialogResult::Submit(HookTrustAction::Skip),
            KeyCode::Enter => {
                if self.selected {
                    DialogResult::Submit(HookTrustAction::Trust {
                        hooks: self.hooks.clone(),
                        hooks_hash: self.hooks_hash.clone(),
                        project_path: self.project_path.clone(),
                    })
                } else {
                    DialogResult::Submit(HookTrustAction::Skip)
                }
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                DialogResult::Submit(HookTrustAction::Trust {
                    hooks: self.hooks.clone(),
                    hooks_hash: self.hooks_hash.clone(),
                    project_path: self.project_path.clone(),
                })
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
                let total_lines = self.build_hook_lines().len() as u16;
                if self.scroll_offset + 1 < total_lines {
                    self.scroll_offset += 1;
                }
                DialogResult::Continue
            }
            _ => DialogResult::Continue,
        }
    }

    fn build_hook_lines(&self) -> Vec<Line<'_>> {
        let mut lines = Vec::new();

        if !self.hooks.on_create.is_empty() {
            lines.push(Line::from(Span::styled(
                "on_create:",
                Style::default().bold(),
            )));
            for cmd in &self.hooks.on_create {
                lines.push(Line::from(format!("  {}", cmd)));
            }
            lines.push(Line::from(""));
        }

        if !self.hooks.on_launch.is_empty() {
            lines.push(Line::from(Span::styled(
                "on_launch:",
                Style::default().bold(),
            )));
            for cmd in &self.hooks.on_launch {
                lines.push(Line::from(format!("  {}", cmd)));
            }
        }

        lines
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let hook_lines = self.build_hook_lines();
        let content_height = hook_lines.len() as u16 + 4; // +4 for header, spacing, buttons

        let dialog_width = 60.min(area.width.saturating_sub(4));
        let dialog_height = (content_height + 6).min(area.height.saturating_sub(4));
        let dialog_area = super::centered_rect(area, dialog_width, dialog_height);

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .title(" Repository Hooks ")
            .title_style(Style::default().fg(theme.accent).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(2), // header
                Constraint::Min(1),    // hook commands
                Constraint::Length(2), // buttons
            ])
            .split(inner);

        // Header
        let header = Paragraph::new(
            "This repo has hooks defined in .aoe/config.toml.\nAllow these commands to run?",
        )
        .style(Style::default().fg(theme.text))
        .wrap(Wrap { trim: true });
        frame.render_widget(header, chunks[0]);

        // Hook commands (scrollable)
        let visible_lines: Vec<Line> = hook_lines
            .into_iter()
            .skip(self.scroll_offset as usize)
            .collect();
        let hooks_paragraph = Paragraph::new(visible_lines)
            .style(Style::default().fg(theme.dimmed))
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(theme.border)),
            );
        frame.render_widget(hooks_paragraph, chunks[1]);

        // Buttons
        let trust_style = if self.selected {
            Style::default().fg(theme.running).bold()
        } else {
            Style::default().fg(theme.dimmed)
        };
        let skip_style = if !self.selected {
            Style::default().fg(theme.accent).bold()
        } else {
            Style::default().fg(theme.dimmed)
        };

        let buttons = Line::from(vec![
            Span::raw("  "),
            Span::styled("[Trust & Run (y)]", trust_style),
            Span::raw("    "),
            Span::styled("[Skip (n)]", skip_style),
            Span::raw("    "),
            Span::styled("[Cancel (Esc)]", Style::default().fg(theme.dimmed)),
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

    fn test_dialog() -> HookTrustDialog {
        HookTrustDialog::new(
            HooksConfig {
                on_create: vec!["npm install".to_string()],
                on_launch: vec!["echo start".to_string()],
            },
            "abc123".to_string(),
            "/home/user/project".to_string(),
        )
    }

    #[test]
    fn test_default_selection_is_skip() {
        let dialog = test_dialog();
        assert!(!dialog.selected);
    }

    #[test]
    fn test_y_trusts() {
        let mut dialog = test_dialog();
        let result = dialog.handle_key(key(KeyCode::Char('y')));
        assert!(matches!(
            result,
            DialogResult::Submit(HookTrustAction::Trust { .. })
        ));
    }

    #[test]
    fn test_n_skips() {
        let mut dialog = test_dialog();
        let result = dialog.handle_key(key(KeyCode::Char('n')));
        assert!(matches!(
            result,
            DialogResult::Submit(HookTrustAction::Skip)
        ));
    }

    #[test]
    fn test_esc_cancels() {
        let mut dialog = test_dialog();
        let result = dialog.handle_key(key(KeyCode::Esc));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_enter_with_trust_selected() {
        let mut dialog = test_dialog();
        dialog.selected = true;
        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(
            result,
            DialogResult::Submit(HookTrustAction::Trust { .. })
        ));
    }

    #[test]
    fn test_enter_with_skip_selected() {
        let mut dialog = test_dialog();
        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(
            result,
            DialogResult::Submit(HookTrustAction::Skip)
        ));
    }

    #[test]
    fn test_tab_toggles() {
        let mut dialog = test_dialog();
        assert!(!dialog.selected);
        dialog.handle_key(key(KeyCode::Tab));
        assert!(dialog.selected);
        dialog.handle_key(key(KeyCode::Tab));
        assert!(!dialog.selected);
    }

    #[test]
    fn test_empty_hooks_dialog() {
        let dialog = HookTrustDialog::new(
            HooksConfig::default(),
            "empty_hash".to_string(),
            "/some/path".to_string(),
        );
        // Should build with no lines
        let lines = dialog.build_hook_lines();
        assert!(lines.is_empty());
    }
}
