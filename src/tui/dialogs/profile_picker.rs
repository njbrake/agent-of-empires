//! Profile picker dialog - list, create, and delete profiles

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::*;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

use super::DialogResult;
use crate::tui::styles::Theme;

/// Result when profile picker submits
pub enum ProfilePickerAction {
    Switch(String),
    Created(String),
    Deleted(String),
}

/// Sub-mode of the profile picker
enum Mode {
    /// Browsing the profile list
    List,
    /// Entering a name for a new profile
    CreateInput,
    /// Confirming deletion of the selected profile
    ConfirmDelete,
}

/// Info about a single profile entry
pub struct ProfileEntry {
    pub name: String,
    pub session_count: usize,
    pub is_active: bool,
}

pub struct ProfilePickerDialog {
    mode: Mode,
    profiles: Vec<ProfileEntry>,
    selected: usize,
    /// Input for new profile name
    name_input: Input,
    /// Error/validation message
    error: Option<String>,
    /// Confirmation selection: true = Yes, false = No
    confirm_selected: bool,
}

impl ProfilePickerDialog {
    pub fn new(profiles: Vec<ProfileEntry>, active_profile: &str) -> Self {
        let selected = profiles
            .iter()
            .position(|p| p.name == active_profile)
            .unwrap_or(0);
        Self {
            mode: Mode::List,
            profiles,
            selected,
            name_input: Input::default(),
            error: None,
            confirm_selected: false,
        }
    }

    fn selected_profile(&self) -> Option<&ProfileEntry> {
        self.profiles.get(self.selected)
    }

    fn can_delete_selected(&self) -> bool {
        self.selected_profile()
            .is_some_and(|p| !p.is_active && p.name != "default")
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult<ProfilePickerAction> {
        match self.mode {
            Mode::List => self.handle_list_key(key),
            Mode::CreateInput => self.handle_create_key(key),
            Mode::ConfirmDelete => self.handle_confirm_delete_key(key),
        }
    }

    fn handle_list_key(&mut self, key: KeyEvent) -> DialogResult<ProfilePickerAction> {
        match key.code {
            KeyCode::Esc => DialogResult::Cancel,
            KeyCode::Up | KeyCode::Char('k') => {
                if self.selected > 0 {
                    self.selected -= 1;
                }
                DialogResult::Continue
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.profiles.is_empty() && self.selected < self.profiles.len() - 1 {
                    self.selected += 1;
                }
                DialogResult::Continue
            }
            KeyCode::Enter => {
                if let Some(profile) = self.selected_profile() {
                    if profile.is_active {
                        DialogResult::Cancel
                    } else {
                        DialogResult::Submit(ProfilePickerAction::Switch(profile.name.clone()))
                    }
                } else {
                    DialogResult::Cancel
                }
            }
            KeyCode::Char('n') => {
                self.mode = Mode::CreateInput;
                self.name_input = Input::default();
                self.error = None;
                DialogResult::Continue
            }
            KeyCode::Char('d') => {
                if self.can_delete_selected() {
                    self.mode = Mode::ConfirmDelete;
                    self.confirm_selected = false;
                }
                DialogResult::Continue
            }
            _ => DialogResult::Continue,
        }
    }

    fn handle_create_key(&mut self, key: KeyEvent) -> DialogResult<ProfilePickerAction> {
        match key.code {
            KeyCode::Esc => {
                self.mode = Mode::List;
                self.error = None;
                DialogResult::Continue
            }
            KeyCode::Enter => {
                let name = self.name_input.value().trim().to_string();
                if let Some(err) = self.validate_name(&name) {
                    self.error = Some(err);
                    return DialogResult::Continue;
                }
                DialogResult::Submit(ProfilePickerAction::Created(name))
            }
            _ => {
                self.name_input
                    .handle_event(&crossterm::event::Event::Key(key));
                self.error = None;
                DialogResult::Continue
            }
        }
    }

    fn handle_confirm_delete_key(&mut self, key: KeyEvent) -> DialogResult<ProfilePickerAction> {
        match key.code {
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                self.mode = Mode::List;
                DialogResult::Continue
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                if let Some(profile) = self.selected_profile() {
                    let name = profile.name.clone();
                    DialogResult::Submit(ProfilePickerAction::Deleted(name))
                } else {
                    self.mode = Mode::List;
                    DialogResult::Continue
                }
            }
            KeyCode::Enter => {
                if self.confirm_selected {
                    if let Some(profile) = self.selected_profile() {
                        let name = profile.name.clone();
                        return DialogResult::Submit(ProfilePickerAction::Deleted(name));
                    }
                }
                self.mode = Mode::List;
                DialogResult::Continue
            }
            KeyCode::Left | KeyCode::Char('h') => {
                self.confirm_selected = true;
                DialogResult::Continue
            }
            KeyCode::Right | KeyCode::Char('l') => {
                self.confirm_selected = false;
                DialogResult::Continue
            }
            KeyCode::Tab => {
                self.confirm_selected = !self.confirm_selected;
                DialogResult::Continue
            }
            _ => DialogResult::Continue,
        }
    }

    fn validate_name(&self, name: &str) -> Option<String> {
        if name.is_empty() {
            return Some("Profile name cannot be empty".to_string());
        }
        if name.contains('/') || name.contains('\\') {
            return Some("Profile name cannot contain path separators".to_string());
        }
        if self.profiles.iter().any(|p| p.name == name) {
            return Some(format!("Profile '{}' already exists", name));
        }
        None
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        match self.mode {
            Mode::List => self.render_list(frame, area, theme),
            Mode::CreateInput => self.render_create(frame, area, theme),
            Mode::ConfirmDelete => self.render_confirm_delete(frame, area, theme),
        }
    }

    fn render_list(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let max_visible: usize = 8;
        let list_height = self.profiles.len().min(max_visible) as u16;
        // list + hint (1) + borders (2) + margin (2)
        let dialog_height = (list_height + 5).min(area.height);
        let dialog_width: u16 = 40;

        let dialog_area = super::centered_rect(area, dialog_width, dialog_height);
        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .title(" Profiles ")
            .title_style(Style::default().fg(theme.title).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Min(1),    // profile list
                Constraint::Length(1), // hint
            ])
            .split(inner);

        // Profile list with scrolling
        let visible_height = chunks[0].height as usize;
        let scroll_offset = if self.selected >= visible_height {
            self.selected - visible_height + 1
        } else {
            0
        };

        let mut lines: Vec<Line> = Vec::new();
        for (i, profile) in self
            .profiles
            .iter()
            .skip(scroll_offset)
            .take(visible_height)
            .enumerate()
        {
            let abs_idx = i + scroll_offset;
            let is_selected = abs_idx == self.selected;
            let prefix = if is_selected { "> " } else { "  " };

            let mut spans = Vec::new();
            let name_style = if is_selected {
                Style::default().fg(theme.accent).bold()
            } else {
                Style::default().fg(theme.text)
            };
            spans.push(Span::styled(prefix, name_style));
            spans.push(Span::styled(&profile.name, name_style));

            if profile.is_active {
                spans.push(Span::styled(
                    "  (active)",
                    Style::default().fg(theme.running),
                ));
            } else {
                let count_text = format!(
                    "  {} session{}",
                    profile.session_count,
                    if profile.session_count == 1 { "" } else { "s" }
                );
                spans.push(Span::styled(count_text, Style::default().fg(theme.dimmed)));
            }

            lines.push(Line::from(spans));
        }

        frame.render_widget(Paragraph::new(lines), chunks[0]);

        // Hint line
        let mut hint_spans = vec![
            Span::styled("n", Style::default().fg(theme.hint)),
            Span::raw(" new  "),
        ];
        if self.can_delete_selected() {
            hint_spans.extend([
                Span::styled("d", Style::default().fg(theme.hint)),
                Span::raw(" delete  "),
            ]);
        }
        hint_spans.extend([
            Span::styled("Enter", Style::default().fg(theme.hint)),
            Span::raw(" switch  "),
            Span::styled("Esc", Style::default().fg(theme.hint)),
            Span::raw(" close"),
        ]);
        frame.render_widget(Paragraph::new(Line::from(hint_spans)), chunks[1]);
    }

    fn render_create(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let has_error = self.error.is_some();
        let dialog_width: u16 = 40;
        // inner width = dialog_width - borders(2) - margin(2) = 36
        let error_lines: u16 = if let Some(err) = &self.error {
            let inner_width = dialog_width.saturating_sub(6) as usize;
            if inner_width == 0 {
                1
            } else {
                err.len().div_ceil(inner_width) as u16
            }
        } else {
            0
        };
        // name(1) + spacer(1) + error_lines + hint(1) + borders(2) + margin(2)
        let dialog_height: u16 = if has_error { 7 + error_lines } else { 7 };

        let dialog_area = super::centered_rect(area, dialog_width, dialog_height);
        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .title(" New Profile ")
            .title_style(Style::default().fg(theme.title).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let mut constraints = vec![
            Constraint::Length(1), // "Name:" label + input
            Constraint::Length(1), // spacer
        ];
        if has_error {
            constraints.push(Constraint::Length(error_lines)); // error message
        }
        constraints.push(Constraint::Length(1)); // hint

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints(constraints)
            .split(inner);

        // Name input
        let value = self.name_input.value();
        let input_line = Line::from(vec![
            Span::styled("Name: ", Style::default().fg(theme.text)),
            Span::styled(value, Style::default().fg(theme.accent).bold()),
            Span::styled("_", Style::default().fg(theme.accent)),
        ]);
        frame.render_widget(Paragraph::new(input_line), chunks[0]);

        let mut chunk_idx = 2;

        // Error message
        if let Some(err) = &self.error {
            frame.render_widget(
                Paragraph::new(err.as_str())
                    .style(Style::default().fg(theme.error))
                    .wrap(Wrap { trim: true }),
                chunks[chunk_idx],
            );
            chunk_idx += 1;
        }

        // Hint
        let hint_line = Line::from(vec![
            Span::styled("Enter", Style::default().fg(theme.hint)),
            Span::raw(" confirm  "),
            Span::styled("Esc", Style::default().fg(theme.hint)),
            Span::raw(" cancel"),
        ]);
        frame.render_widget(Paragraph::new(hint_line), chunks[chunk_idx]);
    }

    fn render_confirm_delete(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let dialog_height: u16 = 8;
        let dialog_width: u16 = 40;

        let dialog_area = super::centered_rect(area, dialog_width, dialog_height);
        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.error))
            .title(" Delete Profile ")
            .title_style(Style::default().fg(theme.error).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(inner);

        // Message
        if let Some(profile) = self.selected_profile() {
            let msg = format!(
                "Delete '{}' ({} session{})?",
                profile.name,
                profile.session_count,
                if profile.session_count == 1 { "" } else { "s" }
            );
            frame.render_widget(
                Paragraph::new(msg)
                    .style(Style::default().fg(theme.text))
                    .wrap(Wrap { trim: true }),
                chunks[0],
            );
        }

        // Buttons
        let yes_style = if self.confirm_selected {
            Style::default().fg(theme.error).bold()
        } else {
            Style::default().fg(theme.dimmed)
        };
        let no_style = if !self.confirm_selected {
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

    fn sample_profiles() -> Vec<ProfileEntry> {
        vec![
            ProfileEntry {
                name: "default".to_string(),
                session_count: 2,
                is_active: true,
            },
            ProfileEntry {
                name: "work".to_string(),
                session_count: 3,
                is_active: false,
            },
            ProfileEntry {
                name: "personal".to_string(),
                session_count: 0,
                is_active: false,
            },
        ]
    }

    #[test]
    fn test_new_selects_active_profile() {
        let dialog = ProfilePickerDialog::new(sample_profiles(), "default");
        assert_eq!(dialog.selected, 0);

        let dialog = ProfilePickerDialog::new(sample_profiles(), "work");
        assert_eq!(dialog.selected, 1);
    }

    #[test]
    fn test_esc_cancels() {
        let mut dialog = ProfilePickerDialog::new(sample_profiles(), "default");
        let result = dialog.handle_key(key(KeyCode::Esc));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_navigate_and_select() {
        let mut dialog = ProfilePickerDialog::new(sample_profiles(), "default");

        // Move down to "work"
        dialog.handle_key(key(KeyCode::Down));
        assert_eq!(dialog.selected, 1);

        // Select it
        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(
            result,
            DialogResult::Submit(ProfilePickerAction::Switch(name)) if name == "work"
        ));
    }

    #[test]
    fn test_enter_on_active_cancels() {
        let mut dialog = ProfilePickerDialog::new(sample_profiles(), "default");
        // "default" is active, Enter should cancel
        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_create_flow() {
        let mut dialog = ProfilePickerDialog::new(sample_profiles(), "default");

        // Press 'n' to enter create mode
        dialog.handle_key(key(KeyCode::Char('n')));
        assert!(matches!(dialog.mode, Mode::CreateInput));

        // Type a name
        dialog.handle_key(key(KeyCode::Char('t')));
        dialog.handle_key(key(KeyCode::Char('e')));
        dialog.handle_key(key(KeyCode::Char('s')));
        dialog.handle_key(key(KeyCode::Char('t')));

        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(
            result,
            DialogResult::Submit(ProfilePickerAction::Created(name)) if name == "test"
        ));
    }

    #[test]
    fn test_create_empty_name_error() {
        let mut dialog = ProfilePickerDialog::new(sample_profiles(), "default");
        dialog.handle_key(key(KeyCode::Char('n')));

        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, DialogResult::Continue));
        assert!(dialog.error.is_some());
        assert!(dialog.error.as_ref().unwrap().contains("empty"));
    }

    #[test]
    fn test_create_duplicate_name_error() {
        let mut dialog = ProfilePickerDialog::new(sample_profiles(), "default");
        dialog.handle_key(key(KeyCode::Char('n')));

        // Type "work" which already exists
        for c in "work".chars() {
            dialog.handle_key(key(KeyCode::Char(c)));
        }

        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, DialogResult::Continue));
        assert!(dialog.error.as_ref().unwrap().contains("already exists"));
    }

    #[test]
    fn test_create_path_separator_error() {
        let mut dialog = ProfilePickerDialog::new(sample_profiles(), "default");
        dialog.handle_key(key(KeyCode::Char('n')));

        for c in "a/b".chars() {
            dialog.handle_key(key(KeyCode::Char(c)));
        }

        let result = dialog.handle_key(key(KeyCode::Enter));
        assert!(matches!(result, DialogResult::Continue));
        assert!(dialog.error.as_ref().unwrap().contains("path separators"));
    }

    #[test]
    fn test_create_esc_returns_to_list() {
        let mut dialog = ProfilePickerDialog::new(sample_profiles(), "default");
        dialog.handle_key(key(KeyCode::Char('n')));
        assert!(matches!(dialog.mode, Mode::CreateInput));

        dialog.handle_key(key(KeyCode::Esc));
        assert!(matches!(dialog.mode, Mode::List));
    }

    #[test]
    fn test_delete_not_allowed_on_active() {
        let mut dialog = ProfilePickerDialog::new(sample_profiles(), "default");
        // "default" is active, 'd' should not enter confirm mode
        dialog.handle_key(key(KeyCode::Char('d')));
        assert!(matches!(dialog.mode, Mode::List));
    }

    #[test]
    fn test_delete_not_allowed_on_default() {
        let profiles = vec![
            ProfileEntry {
                name: "default".to_string(),
                session_count: 0,
                is_active: false,
            },
            ProfileEntry {
                name: "work".to_string(),
                session_count: 0,
                is_active: true,
            },
        ];
        let mut dialog = ProfilePickerDialog::new(profiles, "work");
        // Select "default" (index 0)
        dialog.handle_key(key(KeyCode::Up));
        assert_eq!(dialog.selected, 0);

        dialog.handle_key(key(KeyCode::Char('d')));
        assert!(matches!(dialog.mode, Mode::List));
    }

    #[test]
    fn test_delete_confirm_flow() {
        let mut dialog = ProfilePickerDialog::new(sample_profiles(), "default");
        // Select "work"
        dialog.handle_key(key(KeyCode::Down));
        assert_eq!(dialog.selected, 1);

        // Press 'd' to enter confirm
        dialog.handle_key(key(KeyCode::Char('d')));
        assert!(matches!(dialog.mode, Mode::ConfirmDelete));

        // Press 'y' to confirm
        let result = dialog.handle_key(key(KeyCode::Char('y')));
        assert!(matches!(
            result,
            DialogResult::Submit(ProfilePickerAction::Deleted(name)) if name == "work"
        ));
    }

    #[test]
    fn test_delete_cancel_returns_to_list() {
        let mut dialog = ProfilePickerDialog::new(sample_profiles(), "default");
        dialog.handle_key(key(KeyCode::Down));
        dialog.handle_key(key(KeyCode::Char('d')));
        assert!(matches!(dialog.mode, Mode::ConfirmDelete));

        dialog.handle_key(key(KeyCode::Esc));
        assert!(matches!(dialog.mode, Mode::List));
    }

    #[test]
    fn test_navigation_clamps() {
        let mut dialog = ProfilePickerDialog::new(sample_profiles(), "default");

        // Can't go above 0
        dialog.handle_key(key(KeyCode::Up));
        assert_eq!(dialog.selected, 0);

        // Go to last
        dialog.handle_key(key(KeyCode::Down));
        dialog.handle_key(key(KeyCode::Down));
        assert_eq!(dialog.selected, 2);

        // Can't go past last
        dialog.handle_key(key(KeyCode::Down));
        assert_eq!(dialog.selected, 2);
    }

    #[test]
    fn test_k_j_navigation() {
        let mut dialog = ProfilePickerDialog::new(sample_profiles(), "default");

        dialog.handle_key(key(KeyCode::Char('j')));
        assert_eq!(dialog.selected, 1);

        dialog.handle_key(key(KeyCode::Char('k')));
        assert_eq!(dialog.selected, 0);
    }
}
