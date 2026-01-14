//! New session dialog

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::*;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

use super::DialogResult;
use crate::docker;
use crate::session::civilizations;
use crate::tmux::AvailableTools;
use crate::tui::styles::Theme;

struct FieldHelp {
    name: &'static str,
    description: &'static str,
}

const FIELD_HELP: &[FieldHelp] = &[
    FieldHelp {
        name: "Title",
        description: "Session name (auto-generates if empty)",
    },
    FieldHelp {
        name: "Path",
        description: "Working directory for the session",
    },
    FieldHelp {
        name: "Group",
        description: "Optional grouping for organization",
    },
    FieldHelp {
        name: "Tool",
        description: "Which AI tool to use",
    },
    FieldHelp {
        name: "Worktree Branch",
        description: "Creates a git worktree if specified",
    },
    FieldHelp {
        name: "Sandbox",
        description: "Run session in Docker container for isolation",
    },
    FieldHelp {
        name: "Image",
        description: "Docker image. Edit config.toml [sandbox] default_image to change default",
    },
];

#[derive(Clone)]
pub struct NewSessionData {
    pub title: String,
    pub path: String,
    pub group: String,
    pub tool: String,
    pub worktree_branch: Option<String>,
    pub create_new_branch: bool,
    pub sandbox: bool,
    pub sandbox_image: Option<String>,
}

pub struct NewSessionDialog {
    title: Input,
    path: Input,
    group: Input,
    tool_index: usize,
    focused_field: usize,
    available_tools: Vec<&'static str>,
    existing_titles: Vec<String>,
    worktree_branch: Input,
    sandbox_enabled: bool,
    sandbox_image: Input,
    default_sandbox_image: String,
    docker_available: bool,
    error_message: Option<String>,
    show_help: bool,
}

impl NewSessionDialog {
    pub fn new(tools: AvailableTools, existing_titles: Vec<String>) -> Self {
        let current_dir = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let available_tools = tools.available_list();
        let docker_available = docker::is_docker_available();

        // Load default image from config or use hardcoded fallback
        let default_sandbox_image = crate::session::Config::load()
            .ok()
            .map(|c| c.sandbox.default_image)
            .unwrap_or_else(|| docker::default_sandbox_image().to_string());

        Self {
            title: Input::default(),
            path: Input::new(current_dir),
            group: Input::default(),
            tool_index: 0,
            focused_field: 0,
            available_tools,
            existing_titles,
            worktree_branch: Input::default(),
            sandbox_enabled: false,
            sandbox_image: Input::new(default_sandbox_image.clone()),
            default_sandbox_image,
            docker_available,
            error_message: None,
            show_help: false,
        }
    }

    #[cfg(test)]
    fn new_with_tools(tools: Vec<&'static str>, path: String) -> Self {
        let default_image = docker::default_sandbox_image().to_string();
        Self {
            title: Input::default(),
            path: Input::new(path),
            group: Input::default(),
            tool_index: 0,
            focused_field: 0,
            available_tools: tools,
            existing_titles: Vec::new(),
            worktree_branch: Input::default(),
            sandbox_enabled: false,
            sandbox_image: Input::new(default_image.clone()),
            default_sandbox_image: default_image,
            docker_available: false,
            error_message: None,
            show_help: false,
        }
    }

    pub fn set_error(&mut self, error: String) {
        self.error_message = Some(error);
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult<NewSessionData> {
        if self.show_help {
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('?')) {
                self.show_help = false;
            }
            return DialogResult::Continue;
        }

        let has_tool_selection = self.available_tools.len() > 1;
        let has_sandbox = self.docker_available;
        let base_max_field = match (has_tool_selection, has_sandbox) {
            (true, true) => 6,
            (true, false) => 5,
            (false, true) => 5,
            (false, false) => 4,
        };
        // Add 1 more field if sandbox is enabled (for image input)
        let max_field = if has_sandbox && self.sandbox_enabled {
            base_max_field + 1
        } else {
            base_max_field
        };
        let tool_field = if has_tool_selection { 3 } else { usize::MAX };
        let sandbox_field = if has_sandbox {
            if has_tool_selection {
                5
            } else {
                4
            }
        } else {
            usize::MAX
        };

        match key.code {
            KeyCode::Char('?') => {
                self.show_help = true;
                DialogResult::Continue
            }
            KeyCode::Esc => {
                self.error_message = None;
                DialogResult::Cancel
            }
            KeyCode::Enter => {
                self.error_message = None;
                let title_value = self.title.value();
                let final_title = if title_value.is_empty() {
                    let refs: Vec<&str> = self.existing_titles.iter().map(|s| s.as_str()).collect();
                    civilizations::generate_random_title(&refs)
                } else {
                    title_value.to_string()
                };
                let worktree_value = self.worktree_branch.value();
                let worktree_branch = if worktree_value.is_empty() {
                    None
                } else {
                    Some(worktree_value.to_string())
                };
                // Determine sandbox image override
                let sandbox_image = if self.sandbox_enabled {
                    let image_val = self.sandbox_image.value().trim().to_string();
                    // Only set if different from default and non-empty
                    if !image_val.is_empty() && image_val != self.default_sandbox_image {
                        Some(image_val)
                    } else {
                        None
                    }
                } else {
                    None
                };
                DialogResult::Submit(NewSessionData {
                    title: final_title,
                    path: self.path.value().to_string(),
                    group: self.group.value().to_string(),
                    tool: self.available_tools[self.tool_index].to_string(),
                    worktree_branch,
                    create_new_branch: true,
                    sandbox: self.sandbox_enabled,
                    sandbox_image,
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
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
                if self.focused_field == sandbox_field =>
            {
                self.sandbox_enabled = !self.sandbox_enabled;
                // If sandbox was disabled and cursor was on image field, move back to sandbox field
                if !self.sandbox_enabled && self.focused_field > sandbox_field {
                    self.focused_field = sandbox_field;
                }
                DialogResult::Continue
            }
            _ => {
                if self.focused_field != tool_field && self.focused_field != sandbox_field {
                    self.current_input_mut()
                        .handle_event(&crossterm::event::Event::Key(key));
                    self.error_message = None;
                }
                DialogResult::Continue
            }
        }
    }

    fn current_input_mut(&mut self) -> &mut Input {
        let has_tool_selection = self.available_tools.len() > 1;
        let worktree_field = if has_tool_selection { 4 } else { 3 };
        let sandbox_field = if self.docker_available {
            if has_tool_selection {
                5
            } else {
                4
            }
        } else {
            usize::MAX
        };
        let sandbox_image_field = if self.docker_available && self.sandbox_enabled {
            sandbox_field + 1
        } else {
            usize::MAX
        };

        match self.focused_field {
            0 => &mut self.title,
            1 => &mut self.path,
            2 => &mut self.group,
            n if n == worktree_field => &mut self.worktree_branch,
            n if n == sandbox_image_field => &mut self.sandbox_image,
            _ => &mut self.title,
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let has_tool_selection = self.available_tools.len() > 1;
        let has_sandbox = self.docker_available;
        let sandbox_image_visible = has_sandbox && self.sandbox_enabled;
        let dialog_width = 80;
        let dialog_height = if sandbox_image_visible {
            20 // Extra 2 lines for image field
        } else if has_sandbox {
            18
        } else {
            16
        };
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

        let mut constraints = vec![
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
            Constraint::Length(2),
        ];
        if sandbox_image_visible {
            constraints.push(Constraint::Length(2)); // Image field
        }
        constraints.push(Constraint::Min(1)); // Hints/errors

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints(constraints)
            .split(inner);

        let text_fields: [(&str, &Input); 3] = [
            ("Title:", &self.title),
            ("Path:", &self.path),
            ("Group:", &self.group),
        ];

        for (idx, (label, input)) in text_fields.iter().enumerate() {
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

            let value = input.value();
            let cursor_pos = input.visual_cursor();

            let display_value = if value.is_empty() && idx == 0 {
                "(random civ)".to_string()
            } else if is_focused {
                let (before, after) = value.split_at(cursor_pos.min(value.len()));
                format!("{}█{}", before, after)
            } else {
                value.to_string()
            };

            let line = Line::from(vec![
                Span::styled(*label, label_style),
                Span::styled(format!(" {}", display_value), value_style),
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

        let wt_value = self.worktree_branch.value();
        let wt_cursor_pos = self.worktree_branch.visual_cursor();
        let wt_display = if wt_value.is_empty() && !is_wt_focused {
            "(leave empty to skip worktree creation)".to_string()
        } else if is_wt_focused {
            let (before, after) = wt_value.split_at(wt_cursor_pos.min(wt_value.len()));
            format!("{}█{}", before, after)
        } else {
            wt_value.to_string()
        };
        let wt_line = Line::from(vec![
            Span::styled("Worktree Name:", wt_label_style),
            Span::styled(format!(" {}", wt_display), wt_value_style),
        ]);
        frame.render_widget(Paragraph::new(wt_line), chunks[4]);

        let hint_chunk = if has_sandbox {
            let sandbox_field = if has_tool_selection { 5 } else { 4 };
            let is_sandbox_focused = self.focused_field == sandbox_field;
            let sandbox_label_style = if is_sandbox_focused {
                Style::default().fg(theme.accent).underlined()
            } else {
                Style::default().fg(theme.text)
            };

            let checkbox = if self.sandbox_enabled { "[x]" } else { "[ ]" };
            let checkbox_style = if self.sandbox_enabled {
                Style::default().fg(theme.accent).bold()
            } else {
                Style::default().fg(theme.dimmed)
            };

            let sandbox_line = Line::from(vec![
                Span::styled("Sandbox:", sandbox_label_style),
                Span::raw(" "),
                Span::styled(checkbox, checkbox_style),
                Span::styled(
                    " Run in Docker container",
                    if self.sandbox_enabled {
                        Style::default().fg(theme.accent)
                    } else {
                        Style::default().fg(theme.dimmed)
                    },
                ),
            ]);
            frame.render_widget(Paragraph::new(sandbox_line), chunks[5]);

            // Render sandbox image field if sandbox is enabled
            if sandbox_image_visible {
                let sandbox_image_field = sandbox_field + 1;
                let is_image_focused = self.focused_field == sandbox_image_field;
                let image_label_style = if is_image_focused {
                    Style::default().fg(theme.accent).underlined()
                } else {
                    Style::default().fg(theme.text)
                };
                let image_value_style = if is_image_focused {
                    Style::default().fg(theme.accent)
                } else {
                    Style::default().fg(theme.text)
                };

                let image_value = self.sandbox_image.value();
                let image_cursor_pos = self.sandbox_image.visual_cursor();

                let image_display = if is_image_focused {
                    let (before, after) =
                        image_value.split_at(image_cursor_pos.min(image_value.len()));
                    format!("{}█{}", before, after)
                } else {
                    image_value.to_string()
                };

                let image_line = Line::from(vec![
                    Span::styled("  Image:", image_label_style), // Indented to show it's related to sandbox
                    Span::styled(format!(" {}", image_display), image_value_style),
                ]);
                frame.render_widget(Paragraph::new(image_line), chunks[6]);
                7
            } else {
                6
            }
        } else {
            5
        };

        if let Some(error) = &self.error_message {
            let error_line = Line::from(vec![
                Span::styled("✗ Error: ", Style::default().fg(Color::Red).bold()),
                Span::styled(error, Style::default().fg(Color::Red)),
            ]);
            frame.render_widget(Paragraph::new(error_line), chunks[hint_chunk]);
        } else {
            let hint = if has_tool_selection {
                Line::from(vec![
                    Span::styled("Tab", Style::default().fg(theme.hint)),
                    Span::raw(" next  "),
                    Span::styled("←/→", Style::default().fg(theme.hint)),
                    Span::raw(" tool  "),
                    Span::styled("Enter", Style::default().fg(theme.hint)),
                    Span::raw(" create  "),
                    Span::styled("?", Style::default().fg(theme.hint)),
                    Span::raw(" help  "),
                    Span::styled("Esc", Style::default().fg(theme.hint)),
                    Span::raw(" cancel"),
                ])
            } else {
                Line::from(vec![
                    Span::styled("Tab", Style::default().fg(theme.hint)),
                    Span::raw(" next  "),
                    Span::styled("Enter", Style::default().fg(theme.hint)),
                    Span::raw(" create  "),
                    Span::styled("?", Style::default().fg(theme.hint)),
                    Span::raw(" help  "),
                    Span::styled("Esc", Style::default().fg(theme.hint)),
                    Span::raw(" cancel"),
                ])
            };
            frame.render_widget(Paragraph::new(hint), chunks[hint_chunk]);
        }

        if self.show_help {
            self.render_help_overlay(frame, area, theme);
        }
    }

    fn render_help_overlay(&self, frame: &mut Frame, area: Rect, theme: &Theme) {
        let has_tool_selection = self.available_tools.len() > 1;
        let has_sandbox = self.docker_available;
        let show_image_help = has_sandbox && self.sandbox_enabled;

        // Adjust dialog height for conditional help entries
        let dialog_width: u16 = 70; // Wider to fit image description
        let base_height: u16 = 14;
        let dialog_height: u16 = base_height
            + if has_tool_selection { 3 } else { 0 }
            + if has_sandbox { 3 } else { 0 }
            + if show_image_help { 3 } else { 0 };

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
            .border_style(Style::default().fg(theme.border))
            .title(" New Session Help ")
            .title_style(Style::default().fg(theme.title).bold());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let mut lines: Vec<Line> = Vec::new();

        for (idx, help) in FIELD_HELP.iter().enumerate() {
            // Skip tool help if only one tool
            if idx == 3 && !has_tool_selection {
                continue;
            }
            // Skip sandbox help if Docker not available
            if idx == 5 && !has_sandbox {
                continue;
            }
            // Skip image help if sandbox not enabled
            if idx == 6 && !show_image_help {
                continue;
            }

            lines.push(Line::from(Span::styled(
                help.name,
                Style::default().fg(theme.accent).bold(),
            )));
            lines.push(Line::from(Span::styled(
                format!("  {}", help.description),
                Style::default().fg(theme.text),
            )));
            lines.push(Line::from(""));
        }

        lines.push(Line::from(vec![
            Span::styled("Press ", Style::default().fg(theme.dimmed)),
            Span::styled("?", Style::default().fg(theme.hint)),
            Span::styled(" or ", Style::default().fg(theme.dimmed)),
            Span::styled("Esc", Style::default().fg(theme.hint)),
            Span::styled(" to close", Style::default().fg(theme.dimmed)),
        ]));

        frame.render_widget(Paragraph::new(lines), inner);
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
        assert_eq!(dialog.title.value(), "");
        assert_eq!(dialog.path.value(), "/tmp/project");
        assert_eq!(dialog.group.value(), "");
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
        dialog.title = Input::new("My Custom Title".to_string());
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
        assert_eq!(dialog.title.value(), "Hi");
    }

    #[test]
    fn test_char_input_to_path() {
        let mut dialog = single_tool_dialog();
        dialog.focused_field = 1;
        dialog.handle_key(key(KeyCode::Char('/')));
        dialog.handle_key(key(KeyCode::Char('a')));
        assert_eq!(dialog.path.value(), "/tmp/project/a");
    }

    #[test]
    fn test_char_input_to_group() {
        let mut dialog = single_tool_dialog();
        dialog.focused_field = 2;
        dialog.handle_key(key(KeyCode::Char('w')));
        dialog.handle_key(key(KeyCode::Char('o')));
        dialog.handle_key(key(KeyCode::Char('r')));
        dialog.handle_key(key(KeyCode::Char('k')));
        assert_eq!(dialog.group.value(), "work");
    }

    #[test]
    fn test_backspace_removes_char() {
        let mut dialog = single_tool_dialog();
        dialog.title = Input::new("Hello".to_string());
        dialog.handle_key(key(KeyCode::Backspace));
        assert_eq!(dialog.title.value(), "Hell");
    }

    #[test]
    fn test_backspace_on_empty_field() {
        let mut dialog = single_tool_dialog();
        dialog.handle_key(key(KeyCode::Backspace));
        assert_eq!(dialog.title.value(), "");
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
        assert_eq!(dialog.title.value(), " ");
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
        dialog.title = Input::new("Test".to_string());

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

    // Sandbox image tests

    #[test]
    fn test_sandbox_disabled_by_default() {
        let dialog = multi_tool_dialog();
        assert!(!dialog.sandbox_enabled);
    }

    #[test]
    fn test_sandbox_image_initialized_with_default() {
        let dialog = multi_tool_dialog();
        assert_eq!(dialog.sandbox_image.value(), dialog.default_sandbox_image);
    }

    #[test]
    fn test_tab_includes_sandbox_image_when_sandbox_enabled() {
        let mut dialog = multi_tool_dialog();
        dialog.docker_available = true;
        dialog.sandbox_enabled = true;

        // Tab through all fields including sandbox image
        // 0: title, 1: path, 2: group, 3: tool, 4: worktree, 5: sandbox, 6: image
        for _ in 0..6 {
            dialog.handle_key(key(KeyCode::Tab));
        }
        assert_eq!(dialog.focused_field, 6); // sandbox image field

        dialog.handle_key(key(KeyCode::Tab));
        assert_eq!(dialog.focused_field, 0); // wrap to start
    }

    #[test]
    fn test_tab_skips_sandbox_image_when_sandbox_disabled() {
        let mut dialog = multi_tool_dialog();
        dialog.docker_available = true;
        dialog.sandbox_enabled = false;

        // Tab through all fields - should not include sandbox image
        // 0: title, 1: path, 2: group, 3: tool, 4: worktree, 5: sandbox (no image)
        for _ in 0..5 {
            dialog.handle_key(key(KeyCode::Tab));
        }
        assert_eq!(dialog.focused_field, 5); // sandbox field (last)

        dialog.handle_key(key(KeyCode::Tab));
        assert_eq!(dialog.focused_field, 0); // wrap to start
    }

    #[test]
    fn test_submit_with_custom_sandbox_image() {
        let mut dialog = multi_tool_dialog();
        dialog.docker_available = true;
        dialog.sandbox_enabled = true;
        dialog.sandbox_image = Input::new("custom/image:tag".to_string());
        dialog.title = Input::new("Test".to_string());

        let result = dialog.handle_key(key(KeyCode::Enter));
        match result {
            DialogResult::Submit(data) => {
                assert!(data.sandbox);
                assert_eq!(data.sandbox_image, Some("custom/image:tag".to_string()));
            }
            _ => panic!("Expected Submit"),
        }
    }

    #[test]
    fn test_submit_with_default_image_returns_none() {
        let mut dialog = multi_tool_dialog();
        dialog.docker_available = true;
        dialog.sandbox_enabled = true;
        // sandbox_image is already initialized to default
        dialog.title = Input::new("Test".to_string());

        let result = dialog.handle_key(key(KeyCode::Enter));
        match result {
            DialogResult::Submit(data) => {
                assert!(data.sandbox);
                assert_eq!(data.sandbox_image, None); // Should be None when same as default
            }
            _ => panic!("Expected Submit"),
        }
    }

    #[test]
    fn test_submit_with_sandbox_disabled_no_image() {
        let mut dialog = multi_tool_dialog();
        dialog.docker_available = true;
        dialog.sandbox_enabled = false;
        dialog.sandbox_image = Input::new("custom/image:tag".to_string());
        dialog.title = Input::new("Test".to_string());

        let result = dialog.handle_key(key(KeyCode::Enter));
        match result {
            DialogResult::Submit(data) => {
                assert!(!data.sandbox);
                assert_eq!(data.sandbox_image, None); // Should be None when sandbox disabled
            }
            _ => panic!("Expected Submit"),
        }
    }

    #[test]
    fn test_sandbox_image_input_works() {
        let mut dialog = multi_tool_dialog();
        dialog.docker_available = true;
        dialog.sandbox_enabled = true;
        dialog.focused_field = 6; // sandbox image field

        dialog.handle_key(key(KeyCode::Char('a')));
        dialog.handle_key(key(KeyCode::Char('b')));
        dialog.handle_key(key(KeyCode::Char('c')));

        // The default image should have "abc" appended
        let expected = format!("{}abc", dialog.default_sandbox_image);
        assert_eq!(dialog.sandbox_image.value(), expected);
    }
}
