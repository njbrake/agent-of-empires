//! Confirmation dialog

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::*;

use super::DialogResult;
use crate::tui::components::buttons::render_yes_no;
use crate::tui::styles::Theme;

pub struct ConfirmDialog {
    title: String,
    message: String,
    action: String,
    selected: bool, // true = Yes, false = No
    /// When set, the dialog shows a "don't warn me again" checkbox the
    /// user can toggle with Space. The caller reads `dont_ask_again()`
    /// on Submit to persist the opt-out. `None` hides the checkbox.
    dont_ask_again: Option<bool>,
    yes_button_area: Rect,
    no_button_area: Rect,
}

impl ConfirmDialog {
    pub fn new(title: &str, message: &str, action: &str) -> Self {
        Self {
            title: title.to_string(),
            message: message.to_string(),
            action: action.to_string(),
            selected: false,
            dont_ask_again: None,
            yes_button_area: Rect::default(),
            no_button_area: Rect::default(),
        }
    }

    /// Offer a "don't warn me again" checkbox (unchecked to start). The
    /// caller inspects `dont_ask_again()` after a Submit to act on it.
    pub fn offering_dont_ask_again(mut self) -> Self {
        self.dont_ask_again = Some(false);
        self
    }

    /// Whether the user ticked "don't warn me again". Always false when
    /// the checkbox wasn't offered.
    pub fn dont_ask_again(&self) -> bool {
        self.dont_ask_again.unwrap_or(false)
    }

    /// Route a left-click. `Some(Submit)` for `[Yes]`, `Some(Cancel)`
    /// for `[No]`, `None` for clicks that hit elsewhere inside the
    /// dialog. Mirrors UnifiedDeleteDialog so the home view's
    /// `handle_dialog_click` can fan out the same way.
    pub fn handle_click(&self, col: u16, row: u16) -> Option<DialogResult<()>> {
        let pos = ratatui::layout::Position::from((col, row));
        if self.yes_button_area.contains(pos) {
            return Some(DialogResult::Submit(()));
        }
        if self.no_button_area.contains(pos) {
            return Some(DialogResult::Cancel);
        }
        None
    }

    /// Hover does not change the Yes/No selection. Otherwise the mouse
    /// drifting over the opposite button between the user reading the
    /// prompt and pressing Enter would silently flip which action
    /// fires. Click commits explicitly via `handle_click`.
    pub fn handle_hover(&mut self, _col: u16, _row: u16) -> bool {
        false
    }

    pub fn action(&self) -> &str {
        &self.action
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult<()> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => DialogResult::Cancel,
            KeyCode::Enter => {
                if self.selected {
                    DialogResult::Submit(())
                } else {
                    DialogResult::Cancel
                }
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => DialogResult::Submit(()),
            KeyCode::Char(' ') if self.dont_ask_again.is_some() => {
                self.dont_ask_again = Some(!self.dont_ask_again.unwrap_or(false));
                DialogResult::Continue
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
            _ => DialogResult::Continue,
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        // The checkbox row adds a line, so grow the dialog when offered.
        let height = if self.dont_ask_again.is_some() { 9 } else { 8 };
        let dialog_area = super::centered_rect(area, 50, height);

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.error))
            .title(format!(" {} ", self.title))
            .title_style(Style::default().fg(theme.error).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let constraints: &[Constraint] = if self.dont_ask_again.is_some() {
            &[
                Constraint::Min(1),
                Constraint::Length(1),
                Constraint::Length(2),
            ]
        } else {
            &[Constraint::Min(1), Constraint::Length(2)]
        };
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints(constraints)
            .split(inner);

        // Message
        let message = Paragraph::new(&*self.message)
            .style(Style::default().fg(theme.text))
            .wrap(Wrap { trim: true });
        frame.render_widget(message, chunks[0]);

        let buttons_chunk = if let Some(checked) = self.dont_ask_again {
            let mark = if checked { "x" } else { " " };
            let checkbox = Paragraph::new(format!("[{mark}] Don't warn me again (space)"))
                .style(Style::default().fg(theme.dimmed));
            frame.render_widget(checkbox, chunks[1]);
            chunks[2]
        } else {
            chunks[1]
        };

        let (yes, no) = render_yes_no(frame, buttons_chunk, theme, self.selected);
        self.yes_button_area = yes;
        self.no_button_area = no;
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
    fn test_default_selection_is_no() {
        let dialog = ConfirmDialog::new("Test", "Are you sure?", "test_action");
        assert!(!dialog.selected);
    }

    #[test]
    fn test_action_accessor() {
        let dialog = ConfirmDialog::new("Title", "Message", "delete");
        assert_eq!(dialog.action(), "delete");
    }

    #[test]
    fn test_esc_cancels() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        let result = dialog.handle_key(key(KeyCode::Esc));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_n_cancels() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        let result = dialog.handle_key(key(KeyCode::Char('n')));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_uppercase_n_cancels() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        let result = dialog.handle_key(key(KeyCode::Char('N')));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_y_confirms() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        let result = dialog.handle_key(key(KeyCode::Char('y')));
        assert!(matches!(result, DialogResult::Submit(())));
    }

    #[test]
    fn test_uppercase_y_confirms() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        let result = dialog.handle_key(key(KeyCode::Char('Y')));
        assert!(matches!(result, DialogResult::Submit(())));
    }

    #[test]
    fn test_enter_with_no_selected_cancels() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_enter_with_yes_selected_submits() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        dialog.selected = true;
        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, DialogResult::Submit(())));
    }

    #[test]
    fn test_tab_toggles_selection() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        assert!(!dialog.selected);

        dialog.handle_key(key(KeyCode::Tab));
        assert!(dialog.selected);

        dialog.handle_key(key(KeyCode::Tab));
        assert!(!dialog.selected);
    }

    #[test]
    fn test_left_selects_yes() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        dialog.handle_key(key(KeyCode::Left));
        assert!(dialog.selected);
    }

    #[test]
    fn test_right_selects_no() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        dialog.selected = true;
        dialog.handle_key(key(KeyCode::Right));
        assert!(!dialog.selected);
    }

    #[test]
    fn test_h_selects_yes() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        dialog.handle_key(key(KeyCode::Char('h')));
        assert!(dialog.selected);
    }

    #[test]
    fn test_l_selects_no() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        dialog.selected = true;
        dialog.handle_key(key(KeyCode::Char('l')));
        assert!(!dialog.selected);
    }

    #[test]
    fn test_unknown_key_continues() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        let result = dialog.handle_key(key(KeyCode::Char('x')));
        assert!(matches!(result, DialogResult::Continue));
    }

    #[test]
    fn dont_ask_again_defaults_false_when_not_offered() {
        let mut dialog = ConfirmDialog::new("Test", "Message", "action");
        assert!(!dialog.dont_ask_again());
        // Space is inert when the checkbox isn't offered.
        let result = dialog.handle_key(key(KeyCode::Char(' ')));
        assert!(matches!(result, DialogResult::Continue));
        assert!(!dialog.dont_ask_again());
    }

    #[test]
    fn space_toggles_dont_ask_again_when_offered() {
        let mut dialog = ConfirmDialog::new("Quit", "Quit?", "quit").offering_dont_ask_again();
        assert!(!dialog.dont_ask_again());

        let result = dialog.handle_key(key(KeyCode::Char(' ')));
        assert!(matches!(result, DialogResult::Continue));
        assert!(dialog.dont_ask_again());

        dialog.handle_key(key(KeyCode::Char(' ')));
        assert!(!dialog.dont_ask_again());
    }

    #[test]
    fn dont_ask_again_survives_into_submit() {
        let mut dialog = ConfirmDialog::new("Quit", "Quit?", "quit").offering_dont_ask_again();
        dialog.handle_key(key(KeyCode::Char(' ')));
        let result = dialog.handle_key(key(KeyCode::Char('y')));
        assert!(matches!(result, DialogResult::Submit(())));
        assert!(dialog.dont_ask_again());
    }
}
