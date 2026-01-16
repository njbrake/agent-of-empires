//! Unified delete dialog

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::*;

use super::DialogResult;
use crate::tui::styles::Theme;

/// Options for what to clean up when deleting a session
#[derive(Clone, Debug, Default)]
pub struct DeleteOptions {
    pub delete_worktree: bool,
    pub delete_sandbox: bool,
}

/// Configuration for what cleanup options to show in the dialog
#[derive(Clone, Debug, Default)]
pub struct DeleteDialogConfig {
    pub worktree_branch: Option<String>,
    pub has_sandbox: bool,
}

/// Focus states for navigation
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FocusElement {
    WorktreeCheckbox,
    SandboxCheckbox,
    YesButton,
    NoButton,
}

/// Unified delete dialog that adapts based on available cleanup options
pub struct UnifiedDeleteDialog {
    session_title: String,
    config: DeleteDialogConfig,
    options: DeleteOptions,
    focus: FocusElement,
    focusable_elements: Vec<FocusElement>,
}

impl UnifiedDeleteDialog {
    pub fn new(session_title: String, config: DeleteDialogConfig) -> Self {
        let user_config = crate::session::Config::load().ok().unwrap_or_default();

        let mut focusable_elements = Vec::new();

        if config.worktree_branch.is_some() {
            focusable_elements.push(FocusElement::WorktreeCheckbox);
        }
        if config.has_sandbox {
            focusable_elements.push(FocusElement::SandboxCheckbox);
        }

        focusable_elements.push(FocusElement::YesButton);
        focusable_elements.push(FocusElement::NoButton);

        let initial_focus = if config.worktree_branch.is_some() {
            FocusElement::WorktreeCheckbox
        } else if config.has_sandbox {
            FocusElement::SandboxCheckbox
        } else {
            FocusElement::NoButton
        };

        let options = DeleteOptions {
            delete_worktree: config.worktree_branch.is_some() && user_config.worktree.auto_cleanup,
            delete_sandbox: config.has_sandbox && user_config.sandbox.auto_cleanup,
        };

        Self {
            session_title,
            config,
            options,
            focus: initial_focus,
            focusable_elements,
        }
    }

    pub fn options(&self) -> &DeleteOptions {
        &self.options
    }

    fn focus_index(&self) -> usize {
        self.focusable_elements
            .iter()
            .position(|&e| e == self.focus)
            .unwrap_or(0)
    }

    fn focus_next(&mut self) {
        let idx = self.focus_index();
        let next_idx = (idx + 1) % self.focusable_elements.len();
        self.focus = self.focusable_elements[next_idx];
    }

    fn focus_prev(&mut self) {
        let idx = self.focus_index();
        let prev_idx = if idx == 0 {
            self.focusable_elements.len() - 1
        } else {
            idx - 1
        };
        self.focus = self.focusable_elements[prev_idx];
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult<DeleteOptions> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => DialogResult::Cancel,

            KeyCode::Char('y') | KeyCode::Char('Y') => DialogResult::Submit(self.options.clone()),

            KeyCode::Enter => match self.focus {
                FocusElement::YesButton => DialogResult::Submit(self.options.clone()),
                FocusElement::NoButton => DialogResult::Cancel,
                _ => DialogResult::Submit(self.options.clone()),
            },

            KeyCode::Char(' ') => {
                match self.focus {
                    FocusElement::WorktreeCheckbox => {
                        self.options.delete_worktree = !self.options.delete_worktree;
                    }
                    FocusElement::SandboxCheckbox => {
                        self.options.delete_sandbox = !self.options.delete_sandbox;
                    }
                    FocusElement::YesButton | FocusElement::NoButton => {}
                }
                DialogResult::Continue
            }

            KeyCode::Tab => {
                self.focus_next();
                DialogResult::Continue
            }

            KeyCode::BackTab => {
                self.focus_prev();
                DialogResult::Continue
            }

            KeyCode::Up | KeyCode::Char('k') => {
                self.focus_prev();
                DialogResult::Continue
            }

            KeyCode::Down | KeyCode::Char('j') => {
                self.focus_next();
                DialogResult::Continue
            }

            KeyCode::Left | KeyCode::Char('h') => {
                self.focus = FocusElement::YesButton;
                DialogResult::Continue
            }

            KeyCode::Right | KeyCode::Char('l') => {
                self.focus = FocusElement::NoButton;
                DialogResult::Continue
            }

            _ => DialogResult::Continue,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let has_worktree = self.config.worktree_branch.is_some();
        let has_sandbox = self.config.has_sandbox;
        let checkbox_count = (has_worktree as u16) + (has_sandbox as u16);

        let dialog_width = 55;
        let dialog_height = if checkbox_count > 0 {
            6 + checkbox_count
        } else {
            5
        };

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

        let mut constraints = vec![Constraint::Length(1)];

        if checkbox_count > 0 {
            for _ in 0..checkbox_count {
                constraints.push(Constraint::Length(1));
            }
        }

        constraints.push(Constraint::Length(1));
        constraints.push(Constraint::Length(1));

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(inner);

        let mut chunk_idx = 0;

        let message = format!("Delete \"{}\"?", self.session_title);
        frame.render_widget(
            Paragraph::new(message)
                .style(Style::default().fg(theme.text))
                .alignment(Alignment::Center),
            chunks[chunk_idx],
        );
        chunk_idx += 1;

        if checkbox_count > 0 {
            if let Some(branch) = &self.config.worktree_branch {
                let focused = self.focus == FocusElement::WorktreeCheckbox;
                self.render_checkbox(
                    frame,
                    chunks[chunk_idx],
                    theme,
                    "Delete worktree",
                    Some(branch),
                    self.options.delete_worktree,
                    focused,
                );
                chunk_idx += 1;
            }

            if has_sandbox {
                let focused = self.focus == FocusElement::SandboxCheckbox;
                self.render_checkbox(
                    frame,
                    chunks[chunk_idx],
                    theme,
                    "Delete container",
                    None,
                    self.options.delete_sandbox,
                    focused,
                );
                chunk_idx += 1;
            }
        }

        self.render_buttons(frame, chunks[chunk_idx], theme);
        chunk_idx += 1;

        self.render_hints(frame, chunks[chunk_idx], theme, checkbox_count > 0);
    }

    #[allow(clippy::too_many_arguments)]
    fn render_checkbox(
        &self,
        frame: &mut Frame,
        area: Rect,
        theme: &Theme,
        label: &str,
        detail: Option<&str>,
        checked: bool,
        focused: bool,
    ) {
        let checkbox = if checked { "[x]" } else { "[ ]" };

        let checkbox_style = if focused {
            Style::default().fg(theme.accent).bold()
        } else if checked {
            Style::default().fg(theme.error).bold()
        } else {
            Style::default().fg(theme.dimmed)
        };

        let label_style = if focused {
            Style::default().fg(theme.accent).underlined()
        } else {
            Style::default().fg(theme.text)
        };

        let mut spans = vec![
            Span::styled(checkbox, checkbox_style),
            Span::raw(" "),
            Span::styled(label, label_style),
        ];

        if let Some(detail_text) = detail {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(
                format!("({})", detail_text),
                Style::default().fg(theme.dimmed),
            ));
        }

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }

    fn render_buttons(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let yes_focused = self.focus == FocusElement::YesButton;
        let no_focused = self.focus == FocusElement::NoButton;

        let yes_style = if yes_focused {
            Style::default().fg(theme.error).bold()
        } else {
            Style::default().fg(theme.dimmed)
        };

        let no_style = if no_focused {
            Style::default().fg(theme.running).bold()
        } else {
            Style::default().fg(theme.dimmed)
        };

        let buttons = Line::from(vec![
            Span::raw("  "),
            Span::styled("[Yes]", yes_style),
            Span::raw("    "),
            Span::styled("[No]", no_style),
        ]);

        frame.render_widget(Paragraph::new(buttons).alignment(Alignment::Center), area);
    }

    fn render_hints(&self, frame: &mut Frame, area: Rect, theme: &Theme, has_checkboxes: bool) {
        let mut hints = vec![
            Span::styled("Tab", Style::default().fg(theme.hint)),
            Span::raw(" navigate  "),
        ];

        if has_checkboxes {
            hints.extend([
                Span::styled("Space", Style::default().fg(theme.hint)),
                Span::raw(" toggle  "),
            ]);
        }

        hints.extend([
            Span::styled("Enter", Style::default().fg(theme.hint)),
            Span::raw(" confirm  "),
            Span::styled("Esc", Style::default().fg(theme.hint)),
            Span::raw(" cancel"),
        ]);

        frame.render_widget(Paragraph::new(Line::from(hints)), area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn simple_dialog() -> UnifiedDeleteDialog {
        UnifiedDeleteDialog::new("Test Session".to_string(), DeleteDialogConfig::default())
    }

    fn full_dialog() -> UnifiedDeleteDialog {
        UnifiedDeleteDialog::new(
            "Test Session".to_string(),
            DeleteDialogConfig {
                worktree_branch: Some("feature-branch".to_string()),
                has_sandbox: true,
            },
        )
    }

    #[test]
    fn test_default_options() {
        let options = DeleteOptions::default();
        assert!(!options.delete_worktree);
        assert!(!options.delete_sandbox);
    }

    #[test]
    fn test_simple_dialog_focuses_no_button() {
        let dialog = simple_dialog();
        assert_eq!(dialog.focus, FocusElement::NoButton);
    }

    #[test]
    fn test_full_dialog_focuses_first_checkbox() {
        let dialog = full_dialog();
        assert_eq!(dialog.focus, FocusElement::WorktreeCheckbox);
    }

    #[test]
    fn test_full_dialog_respects_config_defaults() {
        let dialog = full_dialog();
        assert!(
            dialog.options.delete_worktree,
            "With default config (auto_cleanup: true), delete_worktree should be true"
        );
        assert!(
            dialog.options.delete_sandbox,
            "With default config (auto_cleanup: true), delete_sandbox should be true"
        );
    }

    #[test]
    fn test_tab_cycles_through_elements() {
        let mut dialog = full_dialog();
        assert_eq!(dialog.focus, FocusElement::WorktreeCheckbox);

        dialog.handle_key(key(KeyCode::Tab));
        assert_eq!(dialog.focus, FocusElement::SandboxCheckbox);

        dialog.handle_key(key(KeyCode::Tab));
        assert_eq!(dialog.focus, FocusElement::YesButton);

        dialog.handle_key(key(KeyCode::Tab));
        assert_eq!(dialog.focus, FocusElement::NoButton);

        dialog.handle_key(key(KeyCode::Tab));
        assert_eq!(dialog.focus, FocusElement::WorktreeCheckbox);
    }

    #[test]
    fn test_space_toggles_checkbox() {
        let mut dialog = full_dialog();
        let initial = dialog.options.delete_worktree;

        dialog.handle_key(key(KeyCode::Char(' ')));
        assert_eq!(dialog.options.delete_worktree, !initial);

        dialog.handle_key(key(KeyCode::Char(' ')));
        assert_eq!(dialog.options.delete_worktree, initial);
    }

    #[test]
    fn test_esc_cancels() {
        let mut dialog = full_dialog();
        let result = dialog.handle_key(key(KeyCode::Esc));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_n_cancels() {
        let mut dialog = full_dialog();
        let result = dialog.handle_key(key(KeyCode::Char('n')));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_y_confirms() {
        let mut dialog = full_dialog();
        let result = dialog.handle_key(key(KeyCode::Char('y')));
        assert!(matches!(result, DialogResult::Submit(_)));
    }

    #[test]
    fn test_enter_on_no_cancels() {
        let mut dialog = simple_dialog();
        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_enter_on_yes_submits() {
        let mut dialog = simple_dialog();
        dialog.focus = FocusElement::YesButton;
        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, DialogResult::Submit(_)));
    }

    #[test]
    fn test_left_focuses_yes() {
        let mut dialog = simple_dialog();
        dialog.handle_key(key(KeyCode::Left));
        assert_eq!(dialog.focus, FocusElement::YesButton);
    }

    #[test]
    fn test_right_focuses_no() {
        let mut dialog = simple_dialog();
        dialog.focus = FocusElement::YesButton;
        dialog.handle_key(key(KeyCode::Right));
        assert_eq!(dialog.focus, FocusElement::NoButton);
    }

    #[test]
    fn test_submit_returns_options() {
        let mut dialog = full_dialog();
        dialog.options.delete_worktree = true;
        dialog.options.delete_sandbox = true;

        let result = dialog.handle_key(key(KeyCode::Char('y')));
        match result {
            DialogResult::Submit(opts) => {
                assert!(opts.delete_worktree);
                assert!(opts.delete_sandbox);
            }
            _ => panic!("Expected Submit"),
        }
    }
}
