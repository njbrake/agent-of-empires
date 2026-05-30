//! Capture-key dialog: the modal that prompts the user to type a
//! single chord and returns it via `DialogResult::Submit(KeyBinding)`.
//!
//! Used by the Keybinds settings tab. Esc cancels. Bare modifier keys
//! (`Shift`, `Ctrl`, `Alt` released alone) are ignored so the dialog
//! stays open until the user types the actual key. Unrepresentable
//! codes (caps lock, media keys, etc.) surface a status-line error
//! and the dialog stays open for retry.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::*;

use super::DialogResult;
use crate::session::config::{KeyBinding, KeyCodeRepr};
use crate::tui::settings::HomeKeybindCmd;
use crate::tui::styles::Theme;

/// Dialog that captures one keystroke and returns it as a `KeyBinding`.
/// The owning view runs the conflict scan on `Submit`; if a conflict
/// is found, the view sets an error via `set_error` and the dialog
/// stays open for retry.
pub struct CaptureKeyDialog {
    command: HomeKeybindCmd,
    error: Option<String>,
}

impl CaptureKeyDialog {
    /// Open a dialog scoped to one command. The command is carried so
    /// the conflict scan in the parent view can skip the row currently
    /// being edited.
    pub fn new(command: HomeKeybindCmd) -> Self {
        Self {
            command,
            error: None,
        }
    }

    /// The command this dialog is rebinding. Used by the conflict scan
    /// to skip the row currently being edited.
    pub fn command(&self) -> HomeKeybindCmd {
        self.command
    }

    /// Surface a one-line error in the dialog. Called by the parent
    /// view after a conflict-scan rejection.
    pub fn set_error(&mut self, message: String) {
        self.error = Some(message);
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult<KeyBinding> {
        // Esc cancels. Treat Esc-with-modifiers the same so a stuck
        // Shift doesn't trap the user in the dialog.
        if key.code == KeyCode::Esc {
            return DialogResult::Cancel;
        }

        // Bare modifier keys (the user pressed Shift on its own while
        // reaching for the real key). Ignore so the dialog stays open
        // waiting for the actual chord.
        if matches!(
            key.code,
            KeyCode::Null
                | KeyCode::CapsLock
                | KeyCode::ScrollLock
                | KeyCode::NumLock
                | KeyCode::Menu
                | KeyCode::KeypadBegin
                | KeyCode::Modifier(_)
        ) {
            return DialogResult::Continue;
        }

        // Reject codes we cannot serialize through `KeyCodeRepr`.
        // Surface the error and stay open for retry.
        let Some(repr) = KeyCodeRepr::from_code(key.code) else {
            self.error = Some(format!(
                "key not supported: {:?} (try a printable key or Fn)",
                key.code
            ));
            return DialogResult::Continue;
        };

        DialogResult::Submit(KeyBinding {
            key: repr,
            modifiers: sanitize_modifiers(key.modifiers, &key.code),
        })
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let (width, height) = (60, 9);
        let dialog_area = super::centered_rect(area, width, height);
        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(theme.accent))
            .title(format!(" Rebind: {} ", self.command.label()))
            .title_style(Style::default().fg(theme.accent).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(1), // prompt
                Constraint::Length(1), // spacer
                Constraint::Length(1), // error or hint
                Constraint::Min(0),    // spacer
                Constraint::Length(1), // footer
            ])
            .split(inner);

        let prompt = Paragraph::new("Press the key combination to bind, or Esc to cancel.")
            .style(Style::default().fg(theme.text));
        frame.render_widget(prompt, chunks[0]);

        let message = match &self.error {
            Some(err) => Line::from(Span::styled(err.clone(), Style::default().fg(theme.error))),
            None => Line::from(Span::styled(
                format!("Default: {}", self.command.default_binding().display()),
                Style::default().fg(theme.dimmed),
            )),
        };
        frame.render_widget(Paragraph::new(message), chunks[2]);

        let footer = Line::from(vec![
            Span::styled("Esc", Style::default().fg(theme.accent)),
            Span::styled(" cancel", Style::default().fg(theme.dimmed)),
        ]);
        frame.render_widget(Paragraph::new(footer), chunks[4]);
    }
}

/// Drop modifier flags that don't disambiguate a chord. Specifically:
/// for a `KeyCode::Char(uppercase)` event, terminals deliver the Shift
/// flag alongside the uppercase code (or, on some legacy paths, only
/// the uppercase code without the flag). Stripping the redundant
/// Shift keeps the on-disk form stable across both paths so two users
/// with different terminals don't see different display strings for
/// the same logical binding.
fn sanitize_modifiers(mods: KeyModifiers, code: &KeyCode) -> KeyModifiers {
    let mut out = mods;
    if let KeyCode::Char(c) = code {
        if c.is_uppercase() {
            out.remove(KeyModifiers::SHIFT);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::settings::HomeKeybindCmd;

    fn key(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)
    }

    #[test]
    fn esc_cancels() {
        let mut dialog = CaptureKeyDialog::new(HomeKeybindCmd::Search);
        let result = dialog.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn printable_submits() {
        let mut dialog = CaptureKeyDialog::new(HomeKeybindCmd::Search);
        let result = dialog.handle_key(key('\\'));
        match result {
            DialogResult::Submit(binding) => {
                assert_eq!(binding.key, KeyCodeRepr::Char('\\'));
                assert_eq!(binding.modifiers, KeyModifiers::NONE);
            }
            _ => panic!("expected Submit"),
        }
    }

    #[test]
    fn function_key_submits() {
        let mut dialog = CaptureKeyDialog::new(HomeKeybindCmd::Search);
        let result = dialog.handle_key(KeyEvent::new(KeyCode::F(5), KeyModifiers::NONE));
        match result {
            DialogResult::Submit(binding) => {
                assert_eq!(binding.key, KeyCodeRepr::F(5));
            }
            _ => panic!("expected Submit"),
        }
    }

    #[test]
    fn ctrl_combination_submits_with_modifier() {
        let mut dialog = CaptureKeyDialog::new(HomeKeybindCmd::Search);
        let result = dialog.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        match result {
            DialogResult::Submit(binding) => {
                assert_eq!(binding.key, KeyCodeRepr::Char('s'));
                assert!(binding.modifiers.contains(KeyModifiers::CONTROL));
            }
            _ => panic!("expected Submit"),
        }
    }

    #[test]
    fn uppercase_strips_shift_for_stable_round_trip() {
        let mut dialog = CaptureKeyDialog::new(HomeKeybindCmd::Search);
        let result = dialog.handle_key(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::SHIFT));
        match result {
            DialogResult::Submit(binding) => {
                assert_eq!(binding.key, KeyCodeRepr::Char('A'));
                assert!(!binding.modifiers.contains(KeyModifiers::SHIFT));
            }
            _ => panic!("expected Submit"),
        }
    }
}
