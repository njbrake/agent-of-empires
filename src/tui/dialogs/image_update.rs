//! Dialog for Docker image update notifications.
//!
//! Follows the HookTrustDialog pattern: navigable buttons with parenthesized
//! key hints, Tab/arrow navigation, and direct key shortcuts.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::*;

use super::DialogResult;
use crate::tui::styles::Theme;

/// User's choice in the image update dialog.
pub enum ImageUpdateAction {
    /// Pull the image now
    Pull,
    /// Snooze for 24 hours
    Snooze,
    /// Never ask again
    Dismiss,
}

/// Which button is focused: Pull, Skip, or Never.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Selection {
    Pull,
    Skip,
    Never,
}

pub struct ImageUpdateDialog {
    selected: Selection,
}

impl Default for ImageUpdateDialog {
    fn default() -> Self {
        Self::new()
    }
}

impl ImageUpdateDialog {
    pub fn new() -> Self {
        Self {
            selected: Selection::Skip,
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult<ImageUpdateAction> {
        match key.code {
            // Direct key shortcuts
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                DialogResult::Submit(ImageUpdateAction::Pull)
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                DialogResult::Submit(ImageUpdateAction::Snooze)
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                DialogResult::Submit(ImageUpdateAction::Dismiss)
            }
            KeyCode::Esc => DialogResult::Submit(ImageUpdateAction::Snooze),
            // Enter confirms the focused button
            KeyCode::Enter => match self.selected {
                Selection::Pull => DialogResult::Submit(ImageUpdateAction::Pull),
                Selection::Skip => DialogResult::Submit(ImageUpdateAction::Snooze),
                Selection::Never => DialogResult::Submit(ImageUpdateAction::Dismiss),
            },
            // Navigation
            KeyCode::Tab => {
                self.selected = match self.selected {
                    Selection::Pull => Selection::Skip,
                    Selection::Skip => Selection::Never,
                    Selection::Never => Selection::Pull,
                };
                DialogResult::Continue
            }
            KeyCode::BackTab => {
                self.selected = match self.selected {
                    Selection::Pull => Selection::Never,
                    Selection::Skip => Selection::Pull,
                    Selection::Never => Selection::Skip,
                };
                DialogResult::Continue
            }
            KeyCode::Left | KeyCode::Char('h') | KeyCode::Up | KeyCode::Char('k') => {
                self.selected = match self.selected {
                    Selection::Pull => Selection::Pull,
                    Selection::Skip => Selection::Pull,
                    Selection::Never => Selection::Skip,
                };
                DialogResult::Continue
            }
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Down | KeyCode::Char('j') => {
                self.selected = match self.selected {
                    Selection::Pull => Selection::Skip,
                    Selection::Skip => Selection::Never,
                    Selection::Never => Selection::Never,
                };
                DialogResult::Continue
            }
            _ => DialogResult::Continue,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_area = super::centered_rect(area, 60, 10);

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.border))
            .title(" Image Update Available ")
            .title_style(Style::default().fg(theme.title).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([Constraint::Min(1), Constraint::Length(2)])
            .split(inner);

        let message = Paragraph::new("A newer sandbox image is available.")
            .style(Style::default().fg(theme.text))
            .wrap(Wrap { trim: true });
        frame.render_widget(message, chunks[0]);

        // Buttons following the HookTrustDialog pattern
        let pull_style = if self.selected == Selection::Pull {
            Style::default().fg(theme.running).bold()
        } else {
            Style::default().fg(theme.dimmed)
        };
        let skip_style = if self.selected == Selection::Skip {
            Style::default().fg(theme.accent).bold()
        } else {
            Style::default().fg(theme.dimmed)
        };
        let never_style = if self.selected == Selection::Never {
            Style::default().fg(theme.error).bold()
        } else {
            Style::default().fg(theme.dimmed)
        };

        let buttons = Line::from(vec![
            Span::raw(" "),
            Span::styled("[Pull (y)]", pull_style),
            Span::raw("  "),
            Span::styled("[Skip (n)]", skip_style),
            Span::raw("  "),
            Span::styled("[Never (d)]", never_style),
        ]);

        frame.render_widget(
            Paragraph::new(buttons).alignment(Alignment::Center),
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
    fn test_default_selection_is_skip() {
        let dialog = ImageUpdateDialog::new();
        assert_eq!(dialog.selected, Selection::Skip);
    }

    #[test]
    fn test_y_pulls() {
        let mut dialog = ImageUpdateDialog::new();
        assert!(matches!(
            dialog.handle_key(key(KeyCode::Char('y'))),
            DialogResult::Submit(ImageUpdateAction::Pull)
        ));
    }

    #[test]
    fn test_n_snoozes() {
        let mut dialog = ImageUpdateDialog::new();
        assert!(matches!(
            dialog.handle_key(key(KeyCode::Char('n'))),
            DialogResult::Submit(ImageUpdateAction::Snooze)
        ));
    }

    #[test]
    fn test_d_dismisses() {
        let mut dialog = ImageUpdateDialog::new();
        assert!(matches!(
            dialog.handle_key(key(KeyCode::Char('d'))),
            DialogResult::Submit(ImageUpdateAction::Dismiss)
        ));
    }

    #[test]
    fn test_esc_snoozes() {
        let mut dialog = ImageUpdateDialog::new();
        assert!(matches!(
            dialog.handle_key(key(KeyCode::Esc)),
            DialogResult::Submit(ImageUpdateAction::Snooze)
        ));
    }

    #[test]
    fn test_enter_confirms_selection() {
        let mut dialog = ImageUpdateDialog::new();
        // Default is Skip
        assert!(matches!(
            dialog.handle_key(key(KeyCode::Enter)),
            DialogResult::Submit(ImageUpdateAction::Snooze)
        ));

        // Navigate to Pull
        dialog.selected = Selection::Pull;
        assert!(matches!(
            dialog.handle_key(key(KeyCode::Enter)),
            DialogResult::Submit(ImageUpdateAction::Pull)
        ));

        // Navigate to Never
        dialog.selected = Selection::Never;
        assert!(matches!(
            dialog.handle_key(key(KeyCode::Enter)),
            DialogResult::Submit(ImageUpdateAction::Dismiss)
        ));
    }

    #[test]
    fn test_tab_cycles_forward() {
        let mut dialog = ImageUpdateDialog::new();
        assert_eq!(dialog.selected, Selection::Skip);

        dialog.handle_key(key(KeyCode::Tab));
        assert_eq!(dialog.selected, Selection::Never);

        dialog.handle_key(key(KeyCode::Tab));
        assert_eq!(dialog.selected, Selection::Pull);

        dialog.handle_key(key(KeyCode::Tab));
        assert_eq!(dialog.selected, Selection::Skip);
    }

    #[test]
    fn test_left_moves_selection() {
        let mut dialog = ImageUpdateDialog::new();
        dialog.selected = Selection::Never;

        dialog.handle_key(key(KeyCode::Left));
        assert_eq!(dialog.selected, Selection::Skip);

        dialog.handle_key(key(KeyCode::Left));
        assert_eq!(dialog.selected, Selection::Pull);

        // At leftmost, stays
        dialog.handle_key(key(KeyCode::Left));
        assert_eq!(dialog.selected, Selection::Pull);
    }

    #[test]
    fn test_right_moves_selection() {
        let mut dialog = ImageUpdateDialog::new();
        dialog.selected = Selection::Pull;

        dialog.handle_key(key(KeyCode::Right));
        assert_eq!(dialog.selected, Selection::Skip);

        dialog.handle_key(key(KeyCode::Right));
        assert_eq!(dialog.selected, Selection::Never);

        // At rightmost, stays
        dialog.handle_key(key(KeyCode::Right));
        assert_eq!(dialog.selected, Selection::Never);
    }

    #[test]
    fn test_unknown_key_continues() {
        let mut dialog = ImageUpdateDialog::new();
        assert!(matches!(
            dialog.handle_key(key(KeyCode::Char('x'))),
            DialogResult::Continue
        ));
    }
}
