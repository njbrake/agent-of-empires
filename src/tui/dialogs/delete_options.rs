//! Delete options dialog

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::*;

use super::DialogResult;
use crate::tui::styles::Theme;

/// Options for what to clean up when deleting a session
#[derive(Clone, Debug, Default)]
pub struct DeleteOptions {
    pub delete_worktree: bool,
}

/// Dialog for configuring delete options
pub struct DeleteOptionsDialog {
    session_title: String,
    options: DeleteOptions,
    worktree_branch: String,
}

impl DeleteOptionsDialog {
    pub fn new(session_title: String, worktree_branch: String) -> Self {
        Self {
            session_title,
            options: DeleteOptions::default(),
            worktree_branch,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult<DeleteOptions> {
        match key.code {
            KeyCode::Esc => DialogResult::Cancel,
            KeyCode::Enter => DialogResult::Submit(self.options.clone()),
            KeyCode::Char(' ') => {
                self.options.delete_worktree = !self.options.delete_worktree;
                DialogResult::Continue
            }
            _ => DialogResult::Continue,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_width = 55;
        let dialog_height = 11;

        let x = area.x + (area.width.saturating_sub(dialog_width)) / 2;
        let y = area.y + (area.height.saturating_sub(dialog_height)) / 2;

        let dialog_area = Rect {
            x,
            y,
            width: dialog_width.min(area.width),
            height: dialog_height.min(area.height),
        };

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.error))
            .title(" Delete Session ")
            .title_style(Style::default().fg(theme.error).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(2), // Session title
                Constraint::Length(1), // "Cleanup options:" label
                Constraint::Length(2), // Checkbox
                Constraint::Min(1),    // Hints
            ])
            .split(inner);

        let title_line = Line::from(vec![
            Span::styled("Session: ", Style::default().fg(theme.text)),
            Span::styled(
                format!("\"{}\"", self.session_title),
                Style::default().fg(theme.accent).bold(),
            ),
        ]);
        frame.render_widget(Paragraph::new(title_line), chunks[0]);

        let label = Paragraph::new(Line::from(Span::styled(
            "Cleanup options:",
            Style::default().fg(theme.text),
        )));
        frame.render_widget(label, chunks[1]);

        let checkbox = if self.options.delete_worktree {
            "[x]"
        } else {
            "[ ]"
        };
        let checkbox_style = if self.options.delete_worktree {
            Style::default().fg(theme.error).bold()
        } else {
            Style::default().fg(theme.dimmed)
        };

        let wt_line = Line::from(vec![
            Span::styled(checkbox, checkbox_style),
            Span::raw(" "),
            Span::styled(
                "Delete worktree",
                Style::default().fg(theme.accent).underlined(),
            ),
            Span::raw(" "),
            Span::styled(
                format!("({})", self.worktree_branch),
                Style::default().fg(theme.dimmed),
            ),
        ]);
        frame.render_widget(Paragraph::new(wt_line), chunks[2]);

        let hints = Line::from(vec![
            Span::styled("Space", Style::default().fg(theme.hint)),
            Span::raw(" toggle  "),
            Span::styled("Enter", Style::default().fg(theme.hint)),
            Span::raw(" delete  "),
            Span::styled("Esc", Style::default().fg(theme.hint)),
            Span::raw(" cancel"),
        ]);
        frame.render_widget(Paragraph::new(hints), chunks[3]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn dialog() -> DeleteOptionsDialog {
        DeleteOptionsDialog::new("Test Session".to_string(), "feature-branch".to_string())
    }

    #[test]
    fn test_default_options() {
        let options = DeleteOptions::default();
        assert!(!options.delete_worktree);
    }

    #[test]
    fn test_esc_cancels() {
        let mut dialog = dialog();
        let result = dialog.handle_key(key(KeyCode::Esc));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_enter_confirms() {
        let mut dialog = dialog();
        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, DialogResult::Submit(_)));
    }

    #[test]
    fn test_space_toggles_worktree() {
        let mut dialog = dialog();
        assert!(!dialog.options.delete_worktree);

        dialog.handle_key(key(KeyCode::Char(' ')));
        assert!(dialog.options.delete_worktree);

        dialog.handle_key(key(KeyCode::Char(' ')));
        assert!(!dialog.options.delete_worktree);
    }

    #[test]
    fn test_submit_returns_options() {
        let mut dialog = dialog();
        dialog.options.delete_worktree = true;

        let result = dialog.handle_key(key(KeyCode::Enter));
        match result {
            DialogResult::Submit(opts) => {
                assert!(opts.delete_worktree);
            }
            _ => panic!("Expected Submit"),
        }
    }
}
