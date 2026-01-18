//! New session dialog

mod render;

#[cfg(test)]
mod tests;

use crossterm::event::{KeyCode, KeyEvent};
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

use super::DialogResult;
use crate::docker;
use crate::session::civilizations;
use crate::tmux::AvailableTools;

pub(super) struct FieldHelp {
    pub(super) name: &'static str,
    pub(super) description: &'static str,
}

pub(super) const HELP_DIALOG_WIDTH: u16 = 85;

pub(super) const FIELD_HELP: &[FieldHelp] = &[
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
        description: "Branch name for git worktree",
    },
    FieldHelp {
        name: "New Branch",
        description:
            "Checked: create new branch. Unchecked: use existing (creates worktree if needed)",
    },
    FieldHelp {
        name: "Sandbox",
        description: "Run session in Docker container for isolation",
    },
    FieldHelp {
        name: "Image",
        description: "Docker image. Edit config.toml [sandbox] default_image to change default",
    },
    FieldHelp {
        name: "YOLO Mode",
        description:
            "Skip permission prompts for autonomous operation (--dangerously-skip-permissions)",
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
    pub yolo_mode: bool,
}

pub struct NewSessionDialog {
    pub(super) title: Input,
    pub(super) path: Input,
    pub(super) group: Input,
    pub(super) tool_index: usize,
    pub(super) focused_field: usize,
    pub(super) available_tools: Vec<&'static str>,
    pub(super) existing_titles: Vec<String>,
    pub(super) worktree_branch: Input,
    pub(super) create_new_branch: bool,
    pub(super) sandbox_enabled: bool,
    pub(super) sandbox_image: Input,
    pub(super) default_sandbox_image: String,
    pub(super) docker_available: bool,
    pub(super) yolo_mode: bool,
    pub(super) error_message: Option<String>,
    pub(super) show_help: bool,
}

impl NewSessionDialog {
    pub fn new(tools: AvailableTools, existing_titles: Vec<String>) -> Self {
        let current_dir = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let available_tools = tools.available_list();
        let docker_available = docker::is_docker_available();

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
            create_new_branch: true,
            sandbox_enabled: false,
            sandbox_image: Input::new(default_sandbox_image.clone()),
            default_sandbox_image,
            docker_available,
            yolo_mode: false,
            error_message: None,
            show_help: false,
        }
    }

    #[cfg(test)]
    pub(super) fn new_with_tools(tools: Vec<&'static str>, path: String) -> Self {
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
            create_new_branch: true,
            sandbox_enabled: false,
            sandbox_image: Input::new(default_image.clone()),
            default_sandbox_image: default_image,
            docker_available: false,
            yolo_mode: false,
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
        let has_worktree = !self.worktree_branch.value().is_empty();
        let sandbox_options_visible = has_sandbox && self.sandbox_enabled;
        // Fields: title(0), path(1), group(2), [tool(3)], worktree(3/4), [new_branch(4/5)], [sandbox(5/6)], [image(6/7)], [yolo(7/8)]
        let tool_field = if has_tool_selection { 3 } else { usize::MAX };
        let worktree_field = if has_tool_selection { 4 } else { 3 };
        let new_branch_field = if has_worktree {
            worktree_field + 1
        } else {
            usize::MAX
        };
        let sandbox_field = if has_sandbox {
            if has_worktree {
                new_branch_field + 1
            } else {
                worktree_field + 1
            }
        } else {
            usize::MAX
        };
        let sandbox_image_field = if sandbox_options_visible {
            sandbox_field + 1
        } else {
            usize::MAX
        };
        let yolo_mode_field = if sandbox_options_visible {
            sandbox_image_field + 1
        } else {
            usize::MAX
        };
        let max_field = if sandbox_options_visible {
            yolo_mode_field + 1
        } else if has_sandbox {
            sandbox_field + 1
        } else if has_worktree {
            new_branch_field + 1
        } else {
            worktree_field + 1
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
                let sandbox_image = if self.sandbox_enabled {
                    let image_val = self.sandbox_image.value().trim().to_string();
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
                    create_new_branch: self.create_new_branch,
                    sandbox: self.sandbox_enabled,
                    sandbox_image,
                    yolo_mode: self.sandbox_enabled && self.yolo_mode,
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
                if self.focused_field == new_branch_field =>
            {
                self.create_new_branch = !self.create_new_branch;
                DialogResult::Continue
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
                if self.focused_field == sandbox_field =>
            {
                self.sandbox_enabled = !self.sandbox_enabled;
                if !self.sandbox_enabled {
                    self.yolo_mode = false;
                    if self.focused_field > sandbox_field {
                        self.focused_field = sandbox_field;
                    }
                }
                DialogResult::Continue
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
                if self.focused_field == yolo_mode_field =>
            {
                self.yolo_mode = !self.yolo_mode;
                DialogResult::Continue
            }
            _ => {
                if self.focused_field != tool_field
                    && self.focused_field != new_branch_field
                    && self.focused_field != sandbox_field
                    && self.focused_field != yolo_mode_field
                {
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
}
