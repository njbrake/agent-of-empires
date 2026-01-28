//! New session dialog

mod render;

#[cfg(test)]
mod tests;

use crossterm::event::{KeyCode, KeyEvent};
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

use super::DialogResult;
use crate::docker;
use crate::session::{civilizations, Config};
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
    FieldHelp {
        name: "Environment",
        description: "Env var names to pass from host to container (extends global config)",
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
    /// The sandbox image to use (always populated from the input field).
    pub sandbox_image: String,
    pub yolo_mode: bool,
    /// Additional environment variable keys to pass from host to container.
    pub extra_env_keys: Vec<String>,
}

/// Spinner frames for loading animation
pub(super) const SPINNER_FRAMES: &[&str] = &["◐", "◓", "◑", "◒"];

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
    pub(super) docker_available: bool,
    pub(super) yolo_mode: bool,
    /// Extra environment variable keys (session-specific)
    pub(super) extra_env_keys: Vec<String>,
    /// Whether the env list is expanded (editing mode)
    pub(super) env_list_expanded: bool,
    /// Currently selected index in the env list
    pub(super) env_selected_index: usize,
    /// Input for editing/adding env var keys
    pub(super) env_editing_input: Option<Input>,
    /// Whether we are adding a new entry (vs editing existing)
    pub(super) env_adding_new: bool,
    pub(super) error_message: Option<String>,
    pub(super) show_help: bool,
    /// Whether the dialog is in loading state (creating session in background)
    pub(super) loading: bool,
    /// Spinner animation frame counter
    pub(super) spinner_frame: usize,
    /// Whether a Docker image pull will be needed (image not present locally)
    pub(super) needs_image_pull: bool,
}

impl NewSessionDialog {
    pub fn new(tools: AvailableTools, existing_titles: Vec<String>) -> Self {
        let current_dir = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let available_tools = tools.available_list();
        let docker_available = docker::is_docker_available();

        // Load config to get defaults
        let config = Config::load().unwrap_or_default();

        // Determine default tool index based on config
        let tool_index = if let Some(ref default_tool) = config.session.default_tool {
            available_tools
                .iter()
                .position(|&t| t == default_tool.as_str())
                .unwrap_or(0)
        } else {
            0
        };

        // Apply sandbox defaults from config
        let sandbox_enabled = docker_available && config.sandbox.enabled_by_default;
        let yolo_mode = sandbox_enabled && config.sandbox.yolo_mode_default;

        // Initialize env keys from config when sandbox is enabled
        let extra_env_keys = if sandbox_enabled {
            config.sandbox.environment.clone()
        } else {
            Vec::new()
        };

        Self {
            title: Input::default(),
            path: Input::new(current_dir),
            group: Input::default(),
            tool_index,
            focused_field: 0,
            available_tools,
            existing_titles,
            worktree_branch: Input::default(),
            create_new_branch: true,
            sandbox_enabled,
            sandbox_image: Input::new(docker::effective_default_image()),
            docker_available,
            yolo_mode,
            extra_env_keys,
            env_list_expanded: false,
            env_selected_index: 0,
            env_editing_input: None,
            env_adding_new: false,
            error_message: None,
            show_help: false,
            loading: false,
            spinner_frame: 0,
            needs_image_pull: false,
        }
    }

    /// Set the dialog to loading state
    pub fn set_loading(&mut self, loading: bool) {
        self.loading = loading;
        if loading {
            self.error_message = None;
            // Check if image pull will be needed (only relevant for sandbox sessions)
            if self.sandbox_enabled {
                let image = self.sandbox_image.value().trim();
                self.needs_image_pull = !docker::image_exists_locally(image);
            }
        }
    }

    /// Check if the dialog is in loading state
    pub fn is_loading(&self) -> bool {
        self.loading
    }

    /// Advance the spinner animation frame. Call this periodically when loading.
    pub fn tick(&mut self) {
        self.spinner_frame = (self.spinner_frame + 1) % SPINNER_FRAMES.len();
    }

    #[cfg(test)]
    pub(super) fn new_with_tools(tools: Vec<&'static str>, path: String) -> Self {
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
            sandbox_image: Input::new(docker::effective_default_image()),
            docker_available: false,
            yolo_mode: false,
            extra_env_keys: Vec::new(),
            env_list_expanded: false,
            env_selected_index: 0,
            env_editing_input: None,
            env_adding_new: false,
            error_message: None,
            show_help: false,
            loading: false,
            spinner_frame: 0,
            needs_image_pull: false,
        }
    }

    pub fn set_error(&mut self, error: String) {
        self.error_message = Some(error);
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> DialogResult<NewSessionData> {
        // When loading, only allow Esc to cancel
        if self.loading {
            if matches!(key.code, KeyCode::Esc) {
                self.loading = false;
                return DialogResult::Cancel;
            }
            return DialogResult::Continue;
        }

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
        // Fields: title(0), path(1), group(2), [tool(3)], worktree(3/4), [new_branch(4/5)], [sandbox(5/6)], [image(6/7)], [yolo(7/8)], [env(8/9)]
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
        let env_field = if sandbox_options_visible {
            yolo_mode_field + 1
        } else {
            usize::MAX
        };
        let max_field = if sandbox_options_visible {
            env_field + 1
        } else if has_sandbox {
            sandbox_field + 1
        } else if has_worktree {
            new_branch_field + 1
        } else {
            worktree_field + 1
        };

        // Handle env list editing mode
        if self.env_list_expanded && self.focused_field == env_field {
            return self.handle_env_list_key(key);
        }

        match key.code {
            KeyCode::Char('?') => {
                self.show_help = true;
                DialogResult::Continue
            }
            KeyCode::Esc => {
                self.error_message = None;
                DialogResult::Cancel
            }
            KeyCode::Enter if self.focused_field == env_field => {
                self.env_list_expanded = true;
                self.env_selected_index = 0;
                DialogResult::Continue
            }
            KeyCode::Enter => {
                self.error_message = None;
                let title_value = self.title.value().trim();
                let final_title = if title_value.is_empty() {
                    let refs: Vec<&str> = self.existing_titles.iter().map(|s| s.as_str()).collect();
                    civilizations::generate_random_title(&refs)
                } else {
                    title_value.to_string()
                };
                let worktree_value = self.worktree_branch.value().trim();
                let worktree_branch = if worktree_value.is_empty() {
                    None
                } else {
                    Some(worktree_value.to_string())
                };
                DialogResult::Submit(NewSessionData {
                    title: final_title,
                    path: self.path.value().trim().to_string(),
                    group: self.group.value().trim().to_string(),
                    tool: self.available_tools[self.tool_index].to_string(),
                    worktree_branch,
                    create_new_branch: self.create_new_branch,
                    sandbox: self.sandbox_enabled,
                    sandbox_image: self.sandbox_image.value().trim().to_string(),
                    yolo_mode: self.sandbox_enabled && self.yolo_mode,
                    extra_env_keys: if self.sandbox_enabled {
                        self.extra_env_keys.clone()
                    } else {
                        Vec::new()
                    },
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
                if self.sandbox_enabled {
                    // Apply yolo_mode_default and reload env keys from config
                    let config = Config::load().unwrap_or_default();
                    self.yolo_mode = config.sandbox.yolo_mode_default;
                    self.extra_env_keys = config.sandbox.environment.clone();
                } else {
                    self.yolo_mode = false;
                    self.extra_env_keys.clear();
                    self.env_list_expanded = false;
                    self.env_editing_input = None;
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
                    && self.focused_field != env_field
                {
                    self.current_input_mut()
                        .handle_event(&crossterm::event::Event::Key(key));
                    self.error_message = None;
                }
                DialogResult::Continue
            }
        }
    }

    /// Handle key events when the env list is expanded
    fn handle_env_list_key(&mut self, key: KeyEvent) -> DialogResult<NewSessionData> {
        // Handle text input mode (editing or adding)
        if let Some(ref mut input) = self.env_editing_input {
            match key.code {
                KeyCode::Enter => {
                    let value = input.value().trim().to_string();
                    if !value.is_empty() && !self.extra_env_keys.contains(&value) {
                        if self.env_adding_new {
                            self.extra_env_keys.push(value);
                            self.env_selected_index = self.extra_env_keys.len().saturating_sub(1);
                        } else if self.env_selected_index < self.extra_env_keys.len() {
                            self.extra_env_keys[self.env_selected_index] = value;
                        }
                    }
                    self.env_editing_input = None;
                    self.env_adding_new = false;
                    return DialogResult::Continue;
                }
                KeyCode::Esc => {
                    self.env_editing_input = None;
                    self.env_adding_new = false;
                    return DialogResult::Continue;
                }
                _ => {
                    input.handle_event(&crossterm::event::Event::Key(key));
                    return DialogResult::Continue;
                }
            }
        }

        // Normal list navigation mode
        match key.code {
            KeyCode::Esc => {
                self.env_list_expanded = false;
                DialogResult::Continue
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.env_selected_index > 0 {
                    self.env_selected_index -= 1;
                }
                DialogResult::Continue
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.env_selected_index < self.extra_env_keys.len().saturating_sub(1) {
                    self.env_selected_index += 1;
                }
                DialogResult::Continue
            }
            KeyCode::Char('a') => {
                self.env_editing_input = Some(Input::default());
                self.env_adding_new = true;
                DialogResult::Continue
            }
            KeyCode::Char('d') => {
                if !self.extra_env_keys.is_empty()
                    && self.env_selected_index < self.extra_env_keys.len()
                {
                    self.extra_env_keys.remove(self.env_selected_index);
                    if self.env_selected_index > 0
                        && self.env_selected_index >= self.extra_env_keys.len()
                    {
                        self.env_selected_index = self.extra_env_keys.len().saturating_sub(1);
                    }
                }
                DialogResult::Continue
            }
            KeyCode::Enter => {
                if !self.extra_env_keys.is_empty()
                    && self.env_selected_index < self.extra_env_keys.len()
                {
                    let current = self.extra_env_keys[self.env_selected_index].clone();
                    self.env_editing_input = Some(Input::new(current));
                    self.env_adding_new = false;
                }
                DialogResult::Continue
            }
            _ => DialogResult::Continue,
        }
    }

    fn current_input_mut(&mut self) -> &mut Input {
        let has_tool_selection = self.available_tools.len() > 1;
        let has_worktree = !self.worktree_branch.value().is_empty();

        let worktree_field = if has_tool_selection { 4 } else { 3 };
        let new_branch_field = if has_worktree {
            worktree_field + 1
        } else {
            usize::MAX
        };
        let sandbox_field = if self.docker_available {
            if has_worktree {
                new_branch_field + 1
            } else {
                worktree_field + 1
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
