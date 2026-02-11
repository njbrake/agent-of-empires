//! Rename session dialog

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::*;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

use super::DialogResult;
use crate::tui::components::render_text_field;
use crate::tui::styles::Theme;

/// Data returned when the rename dialog is submitted
#[derive(Debug, Clone)]
pub struct RenameData {
    /// New title (empty string means keep current)
    pub title: String,
    /// New group path (None means keep current, Some("") means remove from group)
    pub group: Option<String>,
    /// New profile (None means keep current, Some(name) means move to that profile)
    pub profile: Option<String>,
    /// YOLO mode (None means keep current, Some(bool) means change)
    pub yolo_mode: Option<bool>,
}

pub struct RenameDialog {
    current_title: String,
    current_group: String,
    current_profile: String,
    available_profiles: Vec<String>,
    new_title: Input,
    new_group: Input,
    profile_index: usize,
    /// Whether session is sandboxed (YOLO only applies to sandboxed sessions)
    is_sandboxed: bool,
    /// Current YOLO mode state
    current_yolo: bool,
    /// New YOLO mode state
    new_yolo: bool,
    focused_field: usize, // 0 = title, 1 = group, 2 = profile, 3 = yolo (if sandboxed)
}

impl RenameDialog {
    pub fn new(
        current_title: &str,
        current_group: &str,
        current_profile: &str,
        available_profiles: Vec<String>,
        is_sandboxed: bool,
        current_yolo: bool,
    ) -> Self {
        let profile_index = available_profiles
            .iter()
            .position(|p| p == current_profile)
            .unwrap_or(0);

        Self {
            current_title: current_title.to_string(),
            current_group: current_group.to_string(),
            current_profile: current_profile.to_string(),
            available_profiles,
            new_title: Input::default(),
            new_group: Input::new(current_group.to_string()),
            profile_index,
            is_sandboxed,
            current_yolo,
            new_yolo: current_yolo,
            focused_field: 0,
        }
    }

    fn num_fields(&self) -> usize {
        if self.is_sandboxed {
            4 // title, group, profile, yolo
        } else {
            3 // title, group, profile
        }
    }

    fn focused_input(&mut self) -> Option<&mut Input> {
        match self.focused_field {
            0 => Some(&mut self.new_title),
            1 => Some(&mut self.new_group),
            _ => None, // Profile and YOLO fields don't use text input
        }
    }

    fn next_field(&mut self) {
        self.focused_field = (self.focused_field + 1) % self.num_fields();
    }

    fn prev_field(&mut self) {
        self.focused_field = if self.focused_field == 0 {
            self.num_fields() - 1
        } else {
            self.focused_field - 1
        };
    }

    fn is_yolo_field(&self) -> bool {
        self.is_sandboxed && self.focused_field == 3
    }

    fn selected_profile(&self) -> &str {
        &self.available_profiles[self.profile_index]
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult<RenameData> {
        match key.code {
            KeyCode::Esc => DialogResult::Cancel,
            KeyCode::Enter => {
                let title_value = self.new_title.value().trim().to_string();
                let group_value = self.new_group.value().trim();
                let selected_profile = self.selected_profile();
                let profile_changed = selected_profile != self.current_profile;
                let yolo_changed = self.is_sandboxed && self.new_yolo != self.current_yolo;

                // If nothing has changed, cancel
                if title_value.is_empty()
                    && group_value == self.current_group
                    && !profile_changed
                    && !yolo_changed
                {
                    return DialogResult::Cancel;
                }

                // Determine the group value:
                // - Same as current means keep current group (None)
                // - Empty (and was non-empty) means remove from group (Some(""))
                // - Any other changed value means set new group
                let group = if group_value == self.current_group {
                    None
                } else if group_value.is_empty() {
                    Some(String::new())
                } else {
                    Some(group_value.to_string())
                };

                // Determine profile value
                let profile = if profile_changed {
                    Some(selected_profile.to_string())
                } else {
                    None
                };

                // Determine YOLO mode value
                let yolo_mode = if yolo_changed {
                    Some(self.new_yolo)
                } else {
                    None
                };

                DialogResult::Submit(RenameData {
                    title: title_value,
                    group,
                    profile,
                    yolo_mode,
                })
            }
            KeyCode::Tab => {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    self.prev_field();
                } else {
                    self.next_field();
                }
                DialogResult::Continue
            }
            KeyCode::Down => {
                self.next_field();
                DialogResult::Continue
            }
            KeyCode::Up => {
                self.prev_field();
                DialogResult::Continue
            }
            KeyCode::Left if self.focused_field == 2 => {
                // Cycle profile backwards
                if self.profile_index == 0 {
                    self.profile_index = self.available_profiles.len().saturating_sub(1);
                } else {
                    self.profile_index -= 1;
                }
                DialogResult::Continue
            }
            KeyCode::Right | KeyCode::Char(' ') if self.focused_field == 2 => {
                // Cycle profile forwards
                self.profile_index = (self.profile_index + 1) % self.available_profiles.len();
                DialogResult::Continue
            }
            KeyCode::Char(' ') if self.is_yolo_field() => {
                // Toggle YOLO mode
                self.new_yolo = !self.new_yolo;
                DialogResult::Continue
            }
            _ => {
                if let Some(input) = self.focused_input() {
                    input.handle_event(&crossterm::event::Event::Key(key));
                }
                DialogResult::Continue
            }
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_width = 50;
        let dialog_height = if self.is_sandboxed { 17 } else { 15 };
        let dialog_area = super::centered_rect(area, dialog_width, dialog_height);

        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .title(" Edit Session ")
            .title_style(Style::default().fg(theme.title).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let mut constraints = vec![
            Constraint::Length(1), // Current title
            Constraint::Length(1), // Current group
            Constraint::Length(1), // Current profile
            Constraint::Length(1), // Spacer
            Constraint::Length(1), // New title field
            Constraint::Length(1), // New group field
            Constraint::Length(1), // Profile selector
        ];
        if self.is_sandboxed {
            constraints.push(Constraint::Length(1)); // YOLO mode
        }
        constraints.push(Constraint::Length(1)); // Spacer
        constraints.push(Constraint::Min(1)); // Hint

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints(constraints)
            .split(inner);

        // Current title
        let current_title_line = Line::from(vec![
            Span::styled("Current title: ", Style::default().fg(theme.dimmed)),
            Span::styled(&self.current_title, Style::default().fg(theme.text)),
        ]);
        frame.render_widget(Paragraph::new(current_title_line), chunks[0]);

        // Current group
        let group_display = if self.current_group.is_empty() {
            "(none)".to_string()
        } else {
            self.current_group.clone()
        };
        let current_group_line = Line::from(vec![
            Span::styled("Current group: ", Style::default().fg(theme.dimmed)),
            Span::styled(group_display, Style::default().fg(theme.text)),
        ]);
        frame.render_widget(Paragraph::new(current_group_line), chunks[1]);

        // Current profile
        let current_profile_line = Line::from(vec![
            Span::styled("Current profile: ", Style::default().fg(theme.dimmed)),
            Span::styled(&self.current_profile, Style::default().fg(theme.text)),
        ]);
        frame.render_widget(Paragraph::new(current_profile_line), chunks[2]);

        // New title field
        render_text_field(
            frame,
            chunks[4],
            "New title:",
            &self.new_title,
            self.focused_field == 0,
            None,
            theme,
        );

        // New group field
        render_text_field(
            frame,
            chunks[5],
            "New group:",
            &self.new_group,
            self.focused_field == 1,
            None,
            theme,
        );

        // Profile selector
        let profile_focused = self.focused_field == 2;
        let selected_profile = self.selected_profile();
        let profile_style = if profile_focused {
            Style::default().fg(theme.accent)
        } else {
            Style::default().fg(theme.text)
        };

        let profile_line = Line::from(vec![
            Span::styled(
                "Profile:    ",
                if profile_focused {
                    Style::default().fg(theme.accent)
                } else {
                    Style::default().fg(theme.dimmed)
                },
            ),
            Span::styled("< ", Style::default().fg(theme.dimmed)),
            Span::styled(selected_profile, profile_style),
            Span::styled(" >", Style::default().fg(theme.dimmed)),
        ]);
        frame.render_widget(Paragraph::new(profile_line), chunks[6]);

        // YOLO mode checkbox (only for sandboxed sessions)
        let hint_chunk = if self.is_sandboxed {
            let yolo_focused = self.focused_field == 3;
            let yolo_label_style = if yolo_focused {
                Style::default().fg(theme.accent)
            } else {
                Style::default().fg(theme.dimmed)
            };
            let yolo_checkbox = if self.new_yolo { "[x]" } else { "[ ]" };
            let yolo_checkbox_style = if self.new_yolo {
                Style::default().fg(theme.running)
            } else {
                Style::default().fg(theme.text)
            };

            let yolo_line = Line::from(vec![
                Span::styled("YOLO Mode:  ", yolo_label_style),
                Span::styled(yolo_checkbox, yolo_checkbox_style),
                Span::styled(
                    if self.new_yolo {
                        " (enabled)"
                    } else {
                        " (disabled)"
                    },
                    Style::default().fg(theme.dimmed),
                ),
            ]);
            frame.render_widget(Paragraph::new(yolo_line), chunks[7]);
            9 // Hint is at chunk 9 when sandboxed
        } else {
            8 // Hint is at chunk 8 when not sandboxed
        };

        // Hint
        let hint = Line::from(vec![
            Span::styled("Tab", Style::default().fg(theme.hint)),
            Span::raw(" switch  "),
            Span::styled("Enter", Style::default().fg(theme.hint)),
            Span::raw(" save  "),
            Span::styled("Esc", Style::default().fg(theme.hint)),
            Span::raw(" cancel"),
        ]);
        frame.render_widget(Paragraph::new(hint), chunks[hint_chunk]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::empty())
    }

    fn shift_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::SHIFT)
    }

    fn default_profiles() -> Vec<String> {
        vec!["default".to_string()]
    }

    fn multi_profiles() -> Vec<String> {
        vec![
            "default".to_string(),
            "work".to_string(),
            "personal".to_string(),
        ]
    }

    #[test]
    fn test_new_dialog() {
        let dialog = RenameDialog::new(
            "Original Title",
            "work/frontend",
            "default",
            default_profiles(),
            false,
            false,
        );
        assert_eq!(dialog.current_title, "Original Title");
        assert_eq!(dialog.current_group, "work/frontend");
        assert_eq!(dialog.current_profile, "default");
        assert_eq!(dialog.new_title.value(), "");
        assert_eq!(dialog.new_group.value(), "work/frontend"); // Pre-populated with current group
        assert_eq!(dialog.profile_index, 0);
        assert_eq!(dialog.focused_field, 0);
        assert!(!dialog.is_sandboxed);
        assert!(!dialog.current_yolo);
    }

    #[test]
    fn test_new_dialog_empty_group() {
        let dialog = RenameDialog::new("Title", "", "default", default_profiles(), false, false);
        assert_eq!(dialog.current_group, "");
    }

    #[test]
    fn test_new_dialog_with_non_default_profile() {
        let dialog = RenameDialog::new("Title", "group", "work", multi_profiles(), false, false);
        assert_eq!(dialog.current_profile, "work");
        assert_eq!(dialog.profile_index, 1); // "work" is at index 1
    }

    #[test]
    fn test_esc_cancels() {
        let mut dialog =
            RenameDialog::new("Test", "group", "default", default_profiles(), false, false);
        let result = dialog.handle_key(key(KeyCode::Esc));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_enter_with_unchanged_fields_cancels() {
        let mut dialog =
            RenameDialog::new("Test", "group", "default", default_profiles(), false, false);
        // Title is empty, group is pre-populated but unchanged, profile unchanged - should cancel
        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_enter_with_title_only_submits() {
        let mut dialog = RenameDialog::new(
            "Old Title",
            "group",
            "default",
            default_profiles(),
            false,
            false,
        );
        dialog.handle_key(key(KeyCode::Char('N')));
        dialog.handle_key(key(KeyCode::Char('e')));
        dialog.handle_key(key(KeyCode::Char('w')));

        let result = dialog.handle_key(key(KeyCode::Enter));
        match result {
            DialogResult::Submit(data) => {
                assert_eq!(data.title, "New");
                assert_eq!(data.group, None); // Group unchanged
                assert_eq!(data.profile, None); // Profile unchanged
            }
            _ => panic!("Expected Submit result"),
        }
    }

    #[test]
    fn test_enter_with_group_only_submits() {
        let mut dialog = RenameDialog::new(
            "Title",
            "old-group",
            "default",
            default_profiles(),
            false,
            false,
        );
        // Switch to group field and clear it
        dialog.handle_key(key(KeyCode::Tab));
        for _ in 0.."old-group".len() {
            dialog.handle_key(key(KeyCode::Backspace));
        }
        // Type new group
        for c in "new-group".chars() {
            dialog.handle_key(key(KeyCode::Char(c)));
        }

        let result = dialog.handle_key(key(KeyCode::Enter));
        match result {
            DialogResult::Submit(data) => {
                assert_eq!(data.title, ""); // Title unchanged
                assert_eq!(data.group, Some("new-group".to_string()));
                assert_eq!(data.profile, None); // Profile unchanged
            }
            _ => panic!("Expected Submit result"),
        }
    }

    #[test]
    fn test_enter_with_both_fields_submits() {
        let mut dialog = RenameDialog::new(
            "Old Title",
            "old-group",
            "default",
            default_profiles(),
            false,
            false,
        );
        // Type title
        for c in "New Title".chars() {
            dialog.handle_key(key(KeyCode::Char(c)));
        }
        // Switch to group field and clear it
        dialog.handle_key(key(KeyCode::Tab));
        for _ in 0.."old-group".len() {
            dialog.handle_key(key(KeyCode::Backspace));
        }
        // Type new group
        for c in "new-group".chars() {
            dialog.handle_key(key(KeyCode::Char(c)));
        }

        let result = dialog.handle_key(key(KeyCode::Enter));
        match result {
            DialogResult::Submit(data) => {
                assert_eq!(data.title, "New Title");
                assert_eq!(data.group, Some("new-group".to_string()));
                assert_eq!(data.profile, None); // Profile unchanged
            }
            _ => panic!("Expected Submit result"),
        }
    }

    #[test]
    fn test_clearing_group_removes_from_group() {
        let mut dialog = RenameDialog::new(
            "Title",
            "some-group",
            "default",
            default_profiles(),
            false,
            false,
        );
        // Switch to group field and clear it
        dialog.handle_key(key(KeyCode::Tab));
        // Clear the pre-populated value
        for _ in 0.."some-group".len() {
            dialog.handle_key(key(KeyCode::Backspace));
        }

        let result = dialog.handle_key(key(KeyCode::Enter));
        match result {
            DialogResult::Submit(data) => {
                assert_eq!(data.title, "");
                assert_eq!(data.group, Some(String::new())); // Empty string means ungroup
                assert_eq!(data.profile, None);
            }
            _ => panic!("Expected Submit result"),
        }
    }

    #[test]
    fn test_tab_switches_fields() {
        let mut dialog =
            RenameDialog::new("Test", "group", "default", default_profiles(), false, false);
        assert_eq!(dialog.focused_field, 0);

        dialog.handle_key(key(KeyCode::Tab));
        assert_eq!(dialog.focused_field, 1);

        dialog.handle_key(key(KeyCode::Tab));
        assert_eq!(dialog.focused_field, 2);

        dialog.handle_key(key(KeyCode::Tab));
        assert_eq!(dialog.focused_field, 0);
    }

    #[test]
    fn test_shift_tab_switches_fields_backwards() {
        let mut dialog =
            RenameDialog::new("Test", "group", "default", default_profiles(), false, false);
        assert_eq!(dialog.focused_field, 0);

        dialog.handle_key(shift_key(KeyCode::Tab));
        assert_eq!(dialog.focused_field, 2);

        dialog.handle_key(shift_key(KeyCode::Tab));
        assert_eq!(dialog.focused_field, 1);

        dialog.handle_key(shift_key(KeyCode::Tab));
        assert_eq!(dialog.focused_field, 0);
    }

    #[test]
    fn test_down_switches_to_next_field() {
        let mut dialog =
            RenameDialog::new("Test", "group", "default", default_profiles(), false, false);
        assert_eq!(dialog.focused_field, 0);

        dialog.handle_key(key(KeyCode::Down));
        assert_eq!(dialog.focused_field, 1);

        dialog.handle_key(key(KeyCode::Down));
        assert_eq!(dialog.focused_field, 2);
    }

    #[test]
    fn test_up_switches_to_previous_field() {
        let mut dialog =
            RenameDialog::new("Test", "group", "default", default_profiles(), false, false);
        dialog.focused_field = 2;

        dialog.handle_key(key(KeyCode::Up));
        assert_eq!(dialog.focused_field, 1);

        dialog.handle_key(key(KeyCode::Up));
        assert_eq!(dialog.focused_field, 0);
    }

    #[test]
    fn test_char_input_goes_to_focused_field() {
        let mut dialog =
            RenameDialog::new("Test", "group", "default", default_profiles(), false, false);

        // Type in title field
        dialog.handle_key(key(KeyCode::Char('a')));
        assert_eq!(dialog.new_title.value(), "a");
        assert_eq!(dialog.new_group.value(), "group"); // Pre-populated

        // Switch to group and type (appends to pre-populated value)
        dialog.handle_key(key(KeyCode::Tab));
        dialog.handle_key(key(KeyCode::Char('b')));
        assert_eq!(dialog.new_title.value(), "a");
        assert_eq!(dialog.new_group.value(), "groupb");
    }

    #[test]
    fn test_char_input_ignored_on_profile_field() {
        let mut dialog =
            RenameDialog::new("Test", "group", "default", multi_profiles(), false, false);
        dialog.focused_field = 2; // Profile field

        // Typing should not affect anything
        dialog.handle_key(key(KeyCode::Char('a')));
        assert_eq!(dialog.profile_index, 0);
        assert_eq!(dialog.new_title.value(), "");
        assert_eq!(dialog.new_group.value(), "group");
    }

    #[test]
    fn test_backspace_removes_char_from_focused_field() {
        let mut dialog =
            RenameDialog::new("Test", "group", "default", default_profiles(), false, false);
        dialog.handle_key(key(KeyCode::Char('a')));
        dialog.handle_key(key(KeyCode::Char('b')));
        dialog.handle_key(key(KeyCode::Char('c')));

        dialog.handle_key(key(KeyCode::Backspace));
        assert_eq!(dialog.new_title.value(), "ab");
    }

    #[test]
    fn test_current_values_preserved() {
        let mut dialog = RenameDialog::new(
            "Original",
            "original-group",
            "default",
            default_profiles(),
            false,
            false,
        );
        dialog.handle_key(key(KeyCode::Char('N')));
        dialog.handle_key(key(KeyCode::Char('e')));
        dialog.handle_key(key(KeyCode::Char('w')));

        assert_eq!(dialog.current_title, "Original");
        assert_eq!(dialog.current_group, "original-group");
        assert_eq!(dialog.current_profile, "default");
        assert_eq!(dialog.new_title.value(), "New");
    }

    #[test]
    fn test_full_workflow_type_both_and_submit() {
        let mut dialog = RenameDialog::new(
            "Old Name",
            "old/group",
            "default",
            default_profiles(),
            false,
            false,
        );

        // Type new title
        for c in "Renamed Project".chars() {
            dialog.handle_key(key(KeyCode::Char(c)));
        }

        // Switch to group and clear it, then type new group
        dialog.handle_key(key(KeyCode::Tab));
        for _ in 0.."old/group".len() {
            dialog.handle_key(key(KeyCode::Backspace));
        }
        for c in "new/group".chars() {
            dialog.handle_key(key(KeyCode::Char(c)));
        }

        let result = dialog.handle_key(key(KeyCode::Enter));
        match result {
            DialogResult::Submit(data) => {
                assert_eq!(data.title, "Renamed Project");
                assert_eq!(data.group, Some("new/group".to_string()));
                assert_eq!(data.profile, None);
            }
            _ => panic!("Expected Submit"),
        }
    }

    #[test]
    fn test_full_workflow_type_and_cancel() {
        let mut dialog = RenameDialog::new(
            "Old Name",
            "group",
            "default",
            default_profiles(),
            false,
            false,
        );

        dialog.handle_key(key(KeyCode::Char('N')));
        dialog.handle_key(key(KeyCode::Char('e')));
        dialog.handle_key(key(KeyCode::Char('w')));

        let result = dialog.handle_key(key(KeyCode::Esc));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_whitespace_is_trimmed() {
        let mut dialog =
            RenameDialog::new("Test", "group", "default", default_profiles(), false, false);
        for c in "  New Title  ".chars() {
            dialog.handle_key(key(KeyCode::Char(c)));
        }
        dialog.handle_key(key(KeyCode::Tab));
        // Clear pre-populated value first
        for _ in 0.."group".len() {
            dialog.handle_key(key(KeyCode::Backspace));
        }
        for c in "  new-group  ".chars() {
            dialog.handle_key(key(KeyCode::Char(c)));
        }

        let result = dialog.handle_key(key(KeyCode::Enter));
        match result {
            DialogResult::Submit(data) => {
                assert_eq!(data.title, "New Title");
                assert_eq!(data.group, Some("new-group".to_string()));
            }
            _ => panic!("Expected Submit"),
        }
    }

    #[test]
    fn test_left_right_arrow_moves_cursor_in_input() {
        let mut dialog =
            RenameDialog::new("Test", "group", "default", default_profiles(), false, false);
        dialog.handle_key(key(KeyCode::Char('a')));
        dialog.handle_key(key(KeyCode::Char('b')));
        dialog.handle_key(key(KeyCode::Char('c')));

        // Move cursor left and insert
        dialog.handle_key(key(KeyCode::Left));
        dialog.handle_key(key(KeyCode::Char('X')));

        assert_eq!(dialog.new_title.value(), "abXc");
    }

    #[test]
    fn test_profile_selection_with_right_arrow() {
        let mut dialog =
            RenameDialog::new("Test", "group", "default", multi_profiles(), false, false);
        assert_eq!(dialog.profile_index, 0);
        assert_eq!(dialog.selected_profile(), "default");

        // Move to profile field
        dialog.focused_field = 2;

        // Cycle forward
        dialog.handle_key(key(KeyCode::Right));
        assert_eq!(dialog.profile_index, 1);
        assert_eq!(dialog.selected_profile(), "work");

        dialog.handle_key(key(KeyCode::Right));
        assert_eq!(dialog.profile_index, 2);
        assert_eq!(dialog.selected_profile(), "personal");

        // Wrap around
        dialog.handle_key(key(KeyCode::Right));
        assert_eq!(dialog.profile_index, 0);
        assert_eq!(dialog.selected_profile(), "default");
    }

    #[test]
    fn test_profile_selection_with_space_key() {
        let mut dialog =
            RenameDialog::new("Test", "group", "default", multi_profiles(), false, false);
        dialog.focused_field = 2;

        // Space cycles forward like Right arrow
        dialog.handle_key(key(KeyCode::Char(' ')));
        assert_eq!(dialog.profile_index, 1);
        assert_eq!(dialog.selected_profile(), "work");

        dialog.handle_key(key(KeyCode::Char(' ')));
        assert_eq!(dialog.profile_index, 2);
        assert_eq!(dialog.selected_profile(), "personal");

        // Wrap around
        dialog.handle_key(key(KeyCode::Char(' ')));
        assert_eq!(dialog.profile_index, 0);
        assert_eq!(dialog.selected_profile(), "default");
    }

    #[test]
    fn test_profile_selection_with_left_arrow() {
        let mut dialog =
            RenameDialog::new("Test", "group", "default", multi_profiles(), false, false);
        dialog.focused_field = 2;

        // Cycle backward (should wrap to end)
        dialog.handle_key(key(KeyCode::Left));
        assert_eq!(dialog.profile_index, 2);
        assert_eq!(dialog.selected_profile(), "personal");

        dialog.handle_key(key(KeyCode::Left));
        assert_eq!(dialog.profile_index, 1);
        assert_eq!(dialog.selected_profile(), "work");

        dialog.handle_key(key(KeyCode::Left));
        assert_eq!(dialog.profile_index, 0);
        assert_eq!(dialog.selected_profile(), "default");
    }

    #[test]
    fn test_profile_arrows_only_work_on_profile_field() {
        let mut dialog =
            RenameDialog::new("Test", "group", "default", multi_profiles(), false, false);
        assert_eq!(dialog.focused_field, 0); // Title field

        // Right arrow on title field should move cursor, not change profile
        dialog.handle_key(key(KeyCode::Char('a')));
        dialog.handle_key(key(KeyCode::Char('b')));
        let initial_profile = dialog.profile_index;
        dialog.handle_key(key(KeyCode::Right));
        assert_eq!(dialog.profile_index, initial_profile);
    }

    #[test]
    fn test_submit_with_profile_change() {
        let mut dialog =
            RenameDialog::new("Test", "group", "default", multi_profiles(), false, false);

        // Change profile
        dialog.focused_field = 2;
        dialog.handle_key(key(KeyCode::Right)); // Select "work"

        let result = dialog.handle_key(key(KeyCode::Enter));
        match result {
            DialogResult::Submit(data) => {
                assert_eq!(data.title, "");
                assert_eq!(data.group, None);
                assert_eq!(data.profile, Some("work".to_string()));
            }
            _ => panic!("Expected Submit"),
        }
    }

    #[test]
    fn test_submit_with_all_changes() {
        let mut dialog = RenameDialog::new(
            "Old Title",
            "old-group",
            "default",
            multi_profiles(),
            false,
            false,
        );

        // Change title
        for c in "New Title".chars() {
            dialog.handle_key(key(KeyCode::Char(c)));
        }

        // Change group
        dialog.handle_key(key(KeyCode::Tab));
        for _ in 0.."old-group".len() {
            dialog.handle_key(key(KeyCode::Backspace));
        }
        for c in "new-group".chars() {
            dialog.handle_key(key(KeyCode::Char(c)));
        }

        // Change profile
        dialog.handle_key(key(KeyCode::Tab));
        dialog.handle_key(key(KeyCode::Right)); // Select "work"

        let result = dialog.handle_key(key(KeyCode::Enter));
        match result {
            DialogResult::Submit(data) => {
                assert_eq!(data.title, "New Title");
                assert_eq!(data.group, Some("new-group".to_string()));
                assert_eq!(data.profile, Some("work".to_string()));
            }
            _ => panic!("Expected Submit"),
        }
    }

    #[test]
    fn test_same_profile_returns_none() {
        let mut dialog = RenameDialog::new("Test", "group", "work", multi_profiles(), false, false);

        // Change title to trigger submit
        dialog.handle_key(key(KeyCode::Char('X')));

        // Profile stays at "work" (don't change it)
        let result = dialog.handle_key(key(KeyCode::Enter));
        match result {
            DialogResult::Submit(data) => {
                assert_eq!(data.profile, None); // Same profile, returns None
            }
            _ => panic!("Expected Submit"),
        }
    }
}
