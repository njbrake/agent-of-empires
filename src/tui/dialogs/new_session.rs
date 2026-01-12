//! New session dialog

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::*;

use super::DialogResult;
use crate::session::civilizations;
use crate::tmux::AvailableTools;
use crate::tui::styles::Theme;

#[derive(Clone)]
pub struct NewSessionData {
    pub title: String,
    pub path: String,
    pub group: String,
    pub tool: String,
    pub worktree_branch: Option<String>,
    pub create_new_branch: bool,
}

pub struct NewSessionDialog {
    title: String,
    path: String,
    group: String,
    tool_index: usize,
    focused_field: usize,
    available_tools: Vec<&'static str>,
    existing_titles: Vec<String>,
    worktree_branch: String,
    error_message: Option<String>,
}

impl NewSessionDialog {
    pub fn new(tools: AvailableTools, existing_titles: Vec<String>) -> Self {
        let current_dir = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let available_tools = tools.available_list();

        Self {
            title: String::new(),
            path: current_dir,
            group: String::new(),
            tool_index: 0,
            focused_field: 0,
            available_tools,
            existing_titles,
            worktree_branch: String::new(),
            error_message: None,
        }
    }

    #[cfg(test)]
    fn new_with_tools(tools: Vec<&'static str>, path: String) -> Self {
        Self {
            title: String::new(),
            path,
            group: String::new(),
            tool_index: 0,
            focused_field: 0,
            available_tools: tools,
            existing_titles: Vec::new(),
            worktree_branch: String::new(),
            error_message: None,
        }
    }

    pub fn set_error(&mut self, error: String) {
        self.error_message = Some(error);
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult<NewSessionData> {
        let has_tool_selection = self.available_tools.len() > 1;
        let max_field = if has_tool_selection { 5 } else { 4 };
        let tool_field = if has_tool_selection { 3 } else { usize::MAX };

        match key.code {
            KeyCode::Esc => {
                self.error_message = None;
                DialogResult::Cancel
            }
            KeyCode::Enter => {
                self.error_message = None;
                let final_title = if self.title.is_empty() {
                    let refs: Vec<&str> = self.existing_titles.iter().map(|s| s.as_str()).collect();
                    civilizations::generate_random_title(&refs)
                } else {
                    self.title.clone()
                };
                let worktree_branch = if self.worktree_branch.is_empty() {
                    None
                } else {
                    Some(self.worktree_branch.clone())
                };
                DialogResult::Submit(NewSessionData {
                    title: final_title,
                    path: self.path.clone(),
                    group: self.group.clone(),
                    tool: self.available_tools[self.tool_index].to_string(),
                    worktree_branch,
                    create_new_branch: true, // Always create new branch when worktree specified
                })
            }
            KeyCode::Tab => {
                self.focused_field = (self.focused_field + 1) % max_field;
                DialogResult::Continue
            }
            KeyCode::BackTab => {
                self.focused_field = if self.focused_field == 0 {
                    max_field - 1
                } else {
                    self.focused_field - 1
                };
                DialogResult::Continue
            }
            KeyCode::Left | KeyCode::Right if self.focused_field == tool_field => {
                self.tool_index = (self.tool_index + 1) % self.available_tools.len();
                DialogResult::Continue
            }
            KeyCode::Char(' ') if self.focused_field == tool_field => {
                self.tool_index = (self.tool_index + 1) % self.available_tools.len();
                DialogResult::Continue
            }
            KeyCode::Backspace => {
                if self.focused_field != tool_field {
                    self.current_field_mut().pop();
                    self.error_message = None;
                }
                DialogResult::Continue
            }
            KeyCode::Char(c) => {
                if self.focused_field != tool_field {
                    self.current_field_mut().push(c);
                    self.error_message = None;
                }
                DialogResult::Continue
            }
            _ => DialogResult::Continue,
        }
    }

    fn current_field_mut(&mut self) -> &mut String {
        let has_tool_selection = self.available_tools.len() > 1;
        let worktree_field = if has_tool_selection { 4 } else { 3 };

        match self.focused_field {
            0 => &mut self.title,
            1 => &mut self.path,
            2 => &mut self.group,
            n if n == worktree_field => &mut self.worktree_branch,
            _ => &mut self.title,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let has_tool_selection = self.available_tools.len() > 1;
        let dialog_width = 60;
        let dialog_height = 16;
        let x = area.x + (area.width.saturating_sub(dialog_width)) / 2;
        let y = area.y + (area.height.saturating_sub(dialog_height)) / 2;

        let dialog_area = Rect {
            x,
            y,
            width: dialog_width.min(area.width),
            height: dialog_height.min(area.height),
        };

        let clear = Clear;
        frame.render_widget(clear, dialog_area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.accent))
            .title(" New Session ")
            .title_style(Style::default().fg(theme.title).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(2),
                Constraint::Length(2),
                Constraint::Length(2),
                Constraint::Length(2),
                Constraint::Length(2),
                Constraint::Min(1),
            ])
            .split(inner);

        let text_fields = [
            ("Title:", &self.title),
            ("Path:", &self.path),
            ("Group:", &self.group),
        ];

        for (idx, (label, value)) in text_fields.iter().enumerate() {
            let is_focused = idx == self.focused_field;
            let label_style = if is_focused {
                Style::default().fg(theme.accent).underlined()
            } else {
                Style::default().fg(theme.text)
            };
            let value_style = if is_focused {
                Style::default().fg(theme.accent)
            } else {
                Style::default().fg(theme.text)
            };

            let display_value = if value.is_empty() && idx == 0 {
                "(random civ)"
            } else {
                value.as_str()
            };

            let cursor = if is_focused { "█" } else { "" };
            let line = Line::from(vec![
                Span::styled(*label, label_style),
                Span::styled(format!(" {}", display_value), value_style),
                Span::styled(cursor, Style::default().fg(theme.accent).bg(theme.text)),
            ]);

            frame.render_widget(Paragraph::new(line), chunks[idx]);
        }

        let is_tool_focused = self.focused_field == 3;
        let tool_style = if is_tool_focused && has_tool_selection {
            Style::default().fg(theme.accent).underlined()
        } else {
            Style::default().fg(theme.text)
        };

        if has_tool_selection {
            let label_style = if is_tool_focused && has_tool_selection {
                Style::default().fg(theme.accent).underlined()
            } else {
                Style::default().fg(theme.text)
            };

            let mut tool_spans = vec![Span::styled("Tool:", label_style), Span::raw(" ")];

            for (idx, tool_name) in self.available_tools.iter().enumerate() {
                let is_selected = idx == self.tool_index;
                let style = if is_selected {
                    Style::default().fg(theme.accent).bold()
                } else {
                    Style::default().fg(theme.dimmed)
                };

                if idx > 0 {
                    tool_spans.push(Span::raw("  "));
                }
                tool_spans.push(Span::styled(if is_selected { "● " } else { "○ " }, style));
                tool_spans.push(Span::styled(*tool_name, style));
            }

            let tool_line = Line::from(tool_spans);
            frame.render_widget(Paragraph::new(tool_line), chunks[3]);
        } else {
            let tool_line = Line::from(vec![
                Span::styled("Tool:", tool_style),
                Span::raw(" "),
                Span::styled(self.available_tools[0], Style::default().fg(theme.accent)),
            ]);
            frame.render_widget(Paragraph::new(tool_line), chunks[3]);
        }

        let worktree_field = if has_tool_selection { 4 } else { 3 };

        let is_wt_focused = self.focused_field == worktree_field;
        let wt_label_style = if is_wt_focused {
            Style::default().fg(theme.accent).underlined()
        } else {
            Style::default().fg(theme.text)
        };
        let wt_value_style = if is_wt_focused {
            Style::default().fg(theme.accent)
        } else {
            Style::default().fg(theme.text)
        };

        let wt_display = if self.worktree_branch.is_empty() {
            "(leave empty to skip worktree creation)"
        } else {
            &self.worktree_branch
        };
        let wt_cursor = if is_wt_focused { "█" } else { "" };
        let wt_line = Line::from(vec![
            Span::styled("Worktree Name:", wt_label_style),
            Span::styled(format!(" {}", wt_display), wt_value_style),
            Span::styled(wt_cursor, Style::default().fg(theme.accent).bg(theme.text)),
        ]);
        frame.render_widget(Paragraph::new(wt_line), chunks[4]);

        if let Some(error) = &self.error_message {
            let error_line = Line::from(vec![
                Span::styled("✗ Error: ", Style::default().fg(Color::Red).bold()),
                Span::styled(error, Style::default().fg(Color::Red)),
            ]);
            frame.render_widget(Paragraph::new(error_line), chunks[5]);
        } else {
            let hint = if has_tool_selection {
                Line::from(vec![
                    Span::styled("Tab", Style::default().fg(theme.hint)),
                    Span::raw(" next  "),
                    Span::styled("←/→", Style::default().fg(theme.hint)),
                    Span::raw(" tool  "),
                    Span::styled("Space", Style::default().fg(theme.hint)),
                    Span::raw(" toggle  "),
                    Span::styled("Enter", Style::default().fg(theme.hint)),
                    Span::raw(" create  "),
                    Span::styled("Esc", Style::default().fg(theme.hint)),
                    Span::raw(" cancel"),
                ])
            } else {
                Line::from(vec![
                    Span::styled("Tab", Style::default().fg(theme.hint)),
                    Span::raw(" next  "),
                    Span::styled("Space", Style::default().fg(theme.hint)),
                    Span::raw(" toggle  "),
                    Span::styled("Enter", Style::default().fg(theme.hint)),
                    Span::raw(" create  "),
                    Span::styled("Esc", Style::default().fg(theme.hint)),
                    Span::raw(" cancel"),
                ])
            };
            frame.render_widget(Paragraph::new(hint), chunks[5]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn shift_key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::SHIFT)
    }

    fn single_tool_dialog() -> NewSessionDialog {
        NewSessionDialog::new_with_tools(vec!["claude"], "/tmp/project".to_string())
    }

    fn multi_tool_dialog() -> NewSessionDialog {
        NewSessionDialog::new_with_tools(vec!["claude", "opencode"], "/tmp/project".to_string())
    }

    #[test]
    fn test_initial_state() {
        let dialog = single_tool_dialog();
        assert_eq!(dialog.title, "");
        assert_eq!(dialog.path, "/tmp/project");
        assert_eq!(dialog.group, "");
        assert_eq!(dialog.focused_field, 0);
        assert_eq!(dialog.tool_index, 0);
    }

    #[test]
    fn test_esc_cancels() {
        let mut dialog = single_tool_dialog();
        let result = dialog.handle_key(key(KeyCode::Esc));
        assert!(matches!(result, DialogResult::Cancel));
    }

    #[test]
    fn test_enter_submits_with_auto_title() {
        let mut dialog = single_tool_dialog();
        let result = dialog.handle_key(key(KeyCode::Enter));
        match result {
            DialogResult::Submit(data) => {
                assert!(
                    civilizations::CIVILIZATIONS.contains(&data.title.as_str()),
                    "Expected a civilization name, got: {}",
                    data.title
                );
                assert_eq!(data.path, "/tmp/project");
                assert_eq!(data.group, "");
                assert_eq!(data.tool, "claude");
            }
            _ => panic!("Expected Submit"),
        }
    }

    #[test]
    fn test_enter_preserves_custom_title() {
        let mut dialog = single_tool_dialog();
        dialog.title = "My Custom Title".to_string();
        let result = dialog.handle_key(key(KeyCode::Enter));
        match result {
            DialogResult::Submit(data) => {
                assert_eq!(data.title, "My Custom Title");
            }
            _ => panic!("Expected Submit"),
        }
    }

    #[test]
    fn test_tab_cycles_fields_single_tool() {
        let mut dialog = single_tool_dialog();
        assert_eq!(dialog.focused_field, 0);

        dialog.handle_key(key(KeyCode::Tab));
        assert_eq!(dialog.focused_field, 1);

        dialog.handle_key(key(KeyCode::Tab));
        assert_eq!(dialog.focused_field, 2);

        dialog.handle_key(key(KeyCode::Tab));
        assert_eq!(dialog.focused_field, 3); // worktree branch

        dialog.handle_key(key(KeyCode::Tab));
        assert_eq!(dialog.focused_field, 0); // wrap to start
    }

    #[test]
    fn test_tab_cycles_fields_multi_tool() {
        let mut dialog = multi_tool_dialog();
        assert_eq!(dialog.focused_field, 0);

        dialog.handle_key(key(KeyCode::Tab));
        assert_eq!(dialog.focused_field, 1);

        dialog.handle_key(key(KeyCode::Tab));
        assert_eq!(dialog.focused_field, 2);

        dialog.handle_key(key(KeyCode::Tab));
        assert_eq!(dialog.focused_field, 3); // tool selection

        dialog.handle_key(key(KeyCode::Tab));
        assert_eq!(dialog.focused_field, 4); // worktree branch

        dialog.handle_key(key(KeyCode::Tab));
        assert_eq!(dialog.focused_field, 0); // wrap to start
    }

    #[test]
    fn test_backtab_cycles_fields_reverse() {
        let mut dialog = single_tool_dialog();
        assert_eq!(dialog.focused_field, 0);

        dialog.handle_key(shift_key(KeyCode::BackTab));
        assert_eq!(dialog.focused_field, 3); // worktree branch (last field)

        dialog.handle_key(shift_key(KeyCode::BackTab));
        assert_eq!(dialog.focused_field, 2); // group

        dialog.handle_key(shift_key(KeyCode::BackTab));
        assert_eq!(dialog.focused_field, 1); // path

        dialog.handle_key(shift_key(KeyCode::BackTab));
        assert_eq!(dialog.focused_field, 0); // title
    }

    #[test]
    fn test_char_input_to_title() {
        let mut dialog = single_tool_dialog();
        dialog.handle_key(key(KeyCode::Char('H')));
        dialog.handle_key(key(KeyCode::Char('i')));
        assert_eq!(dialog.title, "Hi");
    }

    #[test]
    fn test_char_input_to_path() {
        let mut dialog = single_tool_dialog();
        dialog.handle_key(key(KeyCode::Tab));
        dialog.handle_key(key(KeyCode::Char('/')));
        dialog.handle_key(key(KeyCode::Char('a')));
        assert_eq!(dialog.path, "/tmp/project/a");
    }

    #[test]
    fn test_char_input_to_group() {
        let mut dialog = single_tool_dialog();
        dialog.handle_key(key(KeyCode::Tab));
        dialog.handle_key(key(KeyCode::Tab));
        dialog.handle_key(key(KeyCode::Char('w')));
        dialog.handle_key(key(KeyCode::Char('o')));
        dialog.handle_key(key(KeyCode::Char('r')));
        dialog.handle_key(key(KeyCode::Char('k')));
        assert_eq!(dialog.group, "work");
    }

    #[test]
    fn test_backspace_removes_char() {
        let mut dialog = single_tool_dialog();
        dialog.title = "Hello".to_string();
        dialog.handle_key(key(KeyCode::Backspace));
        assert_eq!(dialog.title, "Hell");
    }

    #[test]
    fn test_backspace_on_empty_field() {
        let mut dialog = single_tool_dialog();
        dialog.handle_key(key(KeyCode::Backspace));
        assert_eq!(dialog.title, "");
    }

    #[test]
    fn test_tool_selection_left_right() {
        let mut dialog = multi_tool_dialog();
        dialog.focused_field = 3;
        assert_eq!(dialog.tool_index, 0);

        dialog.handle_key(key(KeyCode::Right));
        assert_eq!(dialog.tool_index, 1);

        dialog.handle_key(key(KeyCode::Right));
        assert_eq!(dialog.tool_index, 0);

        dialog.handle_key(key(KeyCode::Left));
        assert_eq!(dialog.tool_index, 1);
    }

    #[test]
    fn test_tool_selection_space() {
        let mut dialog = multi_tool_dialog();
        dialog.focused_field = 3;
        assert_eq!(dialog.tool_index, 0);

        dialog.handle_key(key(KeyCode::Char(' ')));
        assert_eq!(dialog.tool_index, 1);

        dialog.handle_key(key(KeyCode::Char(' ')));
        assert_eq!(dialog.tool_index, 0);
    }

    #[test]
    fn test_tool_selection_ignored_on_text_field() {
        let mut dialog = multi_tool_dialog();
        dialog.focused_field = 0;
        dialog.handle_key(key(KeyCode::Char(' ')));
        assert_eq!(dialog.title, " ");
        assert_eq!(dialog.tool_index, 0);
    }

    #[test]
    fn test_tool_selection_ignored_single_tool() {
        let mut dialog = single_tool_dialog();
        dialog.focused_field = 3;
        dialog.handle_key(key(KeyCode::Left));
        assert_eq!(dialog.tool_index, 0);
    }

    #[test]
    fn test_submit_with_selected_tool() {
        let mut dialog = multi_tool_dialog();
        dialog.focused_field = 3;
        dialog.handle_key(key(KeyCode::Right));
        dialog.title = "Test".to_string();

        let result = dialog.handle_key(key(KeyCode::Enter));
        match result {
            DialogResult::Submit(data) => {
                assert_eq!(data.tool, "opencode");
            }
            _ => panic!("Expected Submit"),
        }
    }

    #[test]
    fn test_unknown_key_continues() {
        let mut dialog = single_tool_dialog();
        let result = dialog.handle_key(key(KeyCode::F(1)));
        assert!(matches!(result, DialogResult::Continue));
    }

    #[test]
    fn test_error_clears_on_input() {
        let mut dialog = single_tool_dialog();
        dialog.error_message = Some("Some error".to_string());

        dialog.handle_key(key(KeyCode::Char('a')));
        assert_eq!(dialog.error_message, None);
    }

    #[test]
    fn test_esc_clears_error() {
        let mut dialog = single_tool_dialog();
        dialog.error_message = Some("Some error".to_string());

        let result = dialog.handle_key(key(KeyCode::Esc));
        assert!(matches!(result, DialogResult::Cancel));
        assert_eq!(dialog.error_message, None);
    }
}
