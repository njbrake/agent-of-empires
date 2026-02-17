//! New session dialog

mod render;

#[cfg(test)]
mod tests;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

use super::DialogResult;
use crate::containers::{self, ContainerRuntimeInterface};
use crate::session::repo_config::HookProgress;
#[cfg(test)]
use crate::session::Config;
use crate::session::{civilizations, resolve_config};
use crate::tmux::AvailableTools;
use crate::tui::components::{DirPicker, DirPickerResult, ListPicker, ListPickerResult};

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
        description: "Optional grouping for organization (Ctrl+P to browse existing groups)",
    },
    FieldHelp {
        name: "Tool",
        description: "Which AI tool to use",
    },
    FieldHelp {
        name: "Worktree Branch",
        description: "Branch name for git worktree (Ctrl+P to browse existing branches)",
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
    FieldHelp {
        name: "Environment Values",
        description: "Custom KEY=VALUE env vars injected into the sandbox container",
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
    /// Custom KEY=VALUE environment variables to inject into the container.
    pub extra_env_values: Vec<String>,
}

/// Spinner frames for loading animation
pub(super) const SPINNER_FRAMES: &[&str] = &["◐", "◓", "◑", "◒"];

pub struct NewSessionDialog {
    pub(super) profile: String,
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
    /// Custom KEY=VALUE environment variables (session-specific)
    pub(super) extra_env_values: Vec<String>,
    pub(super) env_values_list_expanded: bool,
    pub(super) env_values_selected_index: usize,
    pub(super) env_values_editing_input: Option<Input>,
    pub(super) env_values_adding_new: bool,
    pub(super) existing_groups: Vec<String>,
    pub(super) group_picker: ListPicker,
    pub(super) branch_picker: ListPicker,
    pub(super) dir_picker: DirPicker,
    pub(super) error_message: Option<String>,
    pub(super) show_help: bool,
    /// Whether the dialog is in loading state (creating session in background)
    pub(super) loading: bool,
    /// Spinner animation frame counter
    pub(super) spinner_frame: usize,
    /// Whether a Docker image pull will be needed (image not present locally)
    pub(super) needs_image_pull: bool,
    /// Whether hooks are being executed during loading
    pub(super) has_hooks: bool,
    /// The currently running hook command
    pub(super) current_hook: Option<String>,
    /// Accumulated output lines from hook execution
    pub(super) hook_output: Vec<String>,
}

/// Shared logic for handling key events in an editable list (env keys or env values).
fn handle_editable_list_key(
    key: KeyEvent,
    items: &mut Vec<String>,
    expanded: &mut bool,
    selected_index: &mut usize,
    editing_input: &mut Option<Input>,
    adding_new: &mut bool,
    validate: impl Fn(&str, &[String]) -> bool,
) -> DialogResult<NewSessionData> {
    // Handle text input mode (editing or adding)
    if let Some(ref mut input) = editing_input {
        match key.code {
            KeyCode::Enter => {
                let value = input.value().trim().to_string();
                if validate(&value, items) {
                    if *adding_new {
                        items.push(value);
                        *selected_index = items.len().saturating_sub(1);
                    } else if *selected_index < items.len() {
                        items[*selected_index] = value;
                    }
                }
                *editing_input = None;
                *adding_new = false;
                return DialogResult::Continue;
            }
            KeyCode::Esc => {
                *editing_input = None;
                *adding_new = false;
                return DialogResult::Continue;
            }
            _ => {
                input.handle_event(&crossterm::event::Event::Key(key));
                return DialogResult::Continue;
            }
        }
    }

    match key.code {
        KeyCode::Esc => {
            *expanded = false;
            DialogResult::Continue
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if *selected_index > 0 {
                *selected_index -= 1;
            }
            DialogResult::Continue
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if *selected_index < items.len().saturating_sub(1) {
                *selected_index += 1;
            }
            DialogResult::Continue
        }
        KeyCode::Char('a') => {
            *editing_input = Some(Input::default());
            *adding_new = true;
            DialogResult::Continue
        }
        KeyCode::Char('d') => {
            if !items.is_empty() && *selected_index < items.len() {
                items.remove(*selected_index);
                if *selected_index > 0 && *selected_index >= items.len() {
                    *selected_index = items.len().saturating_sub(1);
                }
            }
            DialogResult::Continue
        }
        KeyCode::Enter => {
            if !items.is_empty() && *selected_index < items.len() {
                let current = items[*selected_index].clone();
                *editing_input = Some(Input::new(current));
                *adding_new = false;
            }
            DialogResult::Continue
        }
        _ => DialogResult::Continue,
    }
}

impl NewSessionDialog {
    pub fn new(
        tools: AvailableTools,
        existing_titles: Vec<String>,
        existing_groups: Vec<String>,
        profile: &str,
    ) -> Self {
        let current_dir = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let available_tools = tools.available_list();
        let docker_available = containers::get_container_runtime().is_available();

        // Load resolved config (global merged with profile overrides)
        let config = resolve_config(profile).unwrap_or_default();

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

        // Initialize env keys and values from config when sandbox is enabled
        let (extra_env_keys, extra_env_values) = if sandbox_enabled {
            let env_values: Vec<String> = config
                .sandbox
                .environment_values
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();
            (config.sandbox.environment.clone(), env_values)
        } else {
            (Vec::new(), Vec::new())
        };

        Self {
            profile: profile.to_string(),
            title: Input::default(),
            path: Input::new(current_dir),
            group: Input::default(),
            tool_index,
            focused_field: 0,
            available_tools,
            existing_titles,
            existing_groups,
            group_picker: ListPicker::new("Select Group"),
            branch_picker: ListPicker::new("Select Branch"),
            dir_picker: DirPicker::new(),
            worktree_branch: Input::default(),
            create_new_branch: true,
            sandbox_enabled,
            sandbox_image: Input::new(
                containers::get_container_runtime().effective_default_image(),
            ),
            docker_available,
            yolo_mode,
            extra_env_keys,
            env_list_expanded: false,
            env_selected_index: 0,
            env_editing_input: None,
            env_adding_new: false,
            extra_env_values,
            env_values_list_expanded: false,
            env_values_selected_index: 0,
            env_values_editing_input: None,
            env_values_adding_new: false,
            error_message: None,
            show_help: false,
            loading: false,
            spinner_frame: 0,
            needs_image_pull: false,
            has_hooks: false,
            current_hook: None,
            hook_output: Vec::new(),
        }
    }

    /// Set whether hooks will be executed during session creation
    pub fn set_has_hooks(&mut self, has_hooks: bool) {
        self.has_hooks = has_hooks;
    }

    /// Push a hook progress message into the dialog state
    pub fn push_hook_progress(&mut self, progress: HookProgress) {
        match progress {
            HookProgress::Started(cmd) => {
                self.current_hook = Some(cmd);
            }
            HookProgress::Output(line) => {
                self.hook_output.push(line);
            }
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
                self.needs_image_pull =
                    !containers::get_container_runtime().image_exists_locally(image);
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
    pub(super) fn new_with_config(tools: Vec<&'static str>, path: String, config: Config) -> Self {
        let tool_index = if let Some(ref default_tool) = config.session.default_tool {
            tools
                .iter()
                .position(|&t| t == default_tool.as_str())
                .unwrap_or(0)
        } else {
            0
        };

        Self {
            profile: "default".to_string(),
            title: Input::default(),
            path: Input::new(path),
            group: Input::default(),
            tool_index,
            focused_field: 0,
            available_tools: tools,
            existing_titles: Vec::new(),
            existing_groups: Vec::new(),
            group_picker: ListPicker::new("Select Group"),
            branch_picker: ListPicker::new("Select Branch"),
            dir_picker: DirPicker::new(),
            worktree_branch: Input::default(),
            create_new_branch: true,
            sandbox_enabled: false,
            sandbox_image: Input::new(
                containers::get_container_runtime().effective_default_image(),
            ),
            docker_available: false,
            yolo_mode: false,
            extra_env_keys: Vec::new(),
            env_list_expanded: false,
            env_selected_index: 0,
            env_editing_input: None,
            env_adding_new: false,
            extra_env_values: Vec::new(),
            env_values_list_expanded: false,
            env_values_selected_index: 0,
            env_values_editing_input: None,
            env_values_adding_new: false,
            error_message: None,
            show_help: false,
            loading: false,
            spinner_frame: 0,
            needs_image_pull: false,
            has_hooks: false,
            current_hook: None,
            hook_output: Vec::new(),
        }
    }

    #[cfg(test)]
    pub(super) fn new_with_tools(tools: Vec<&'static str>, path: String) -> Self {
        Self {
            profile: "default".to_string(),
            title: Input::default(),
            path: Input::new(path),
            group: Input::default(),
            tool_index: 0,
            focused_field: 0,
            available_tools: tools,
            existing_titles: Vec::new(),
            existing_groups: Vec::new(),
            group_picker: ListPicker::new("Select Group"),
            branch_picker: ListPicker::new("Select Branch"),
            dir_picker: DirPicker::new(),
            worktree_branch: Input::default(),
            create_new_branch: true,
            sandbox_enabled: false,
            sandbox_image: Input::new(
                containers::get_container_runtime().effective_default_image(),
            ),
            docker_available: false,
            yolo_mode: false,
            extra_env_keys: Vec::new(),
            env_list_expanded: false,
            env_selected_index: 0,
            env_editing_input: None,
            env_adding_new: false,
            extra_env_values: Vec::new(),
            env_values_list_expanded: false,
            env_values_selected_index: 0,
            env_values_editing_input: None,
            env_values_adding_new: false,
            error_message: None,
            show_help: false,
            loading: false,
            spinner_frame: 0,
            needs_image_pull: false,
            has_hooks: false,
            current_hook: None,
            hook_output: Vec::new(),
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

        if self.group_picker.is_active() {
            if let ListPickerResult::Selected(value) = self.group_picker.handle_key(key) {
                self.group = Input::new(value);
            }
            return DialogResult::Continue;
        }

        if self.branch_picker.is_active() {
            if let ListPickerResult::Selected(value) = self.branch_picker.handle_key(key) {
                self.worktree_branch = Input::new(value);
            }
            return DialogResult::Continue;
        }

        if self.dir_picker.is_active() {
            match self.dir_picker.handle_key(key) {
                DirPickerResult::Selected(path) => {
                    self.path = Input::new(path);
                }
                DirPickerResult::Cancelled | DirPickerResult::Continue => {}
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
        let env_values_field = if sandbox_options_visible {
            env_field + 1
        } else {
            usize::MAX
        };
        let max_field = if sandbox_options_visible {
            env_values_field + 1
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
        if self.env_values_list_expanded && self.focused_field == env_values_field {
            return self.handle_env_values_list_key(key);
        }

        // Ctrl+P opens a context-sensitive picker
        if key.code == KeyCode::Char('p') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if self.focused_field == 1 {
                let path_value = self.path.value().trim().to_string();
                self.dir_picker.activate(&path_value);
                return DialogResult::Continue;
            }
            if self.focused_field == 2 && !self.existing_groups.is_empty() {
                self.group_picker.activate(self.existing_groups.clone());
                return DialogResult::Continue;
            }
            if self.focused_field == worktree_field {
                let path = std::path::Path::new(self.path.value().trim());
                if let Ok(branches) = crate::git::diff::list_branches(path) {
                    if !branches.is_empty() {
                        self.branch_picker.activate(branches);
                    }
                }
                return DialogResult::Continue;
            }
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
            KeyCode::Enter if self.focused_field == env_values_field => {
                self.env_values_list_expanded = true;
                self.env_values_selected_index = 0;
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
                    extra_env_values: if self.sandbox_enabled {
                        self.extra_env_values.clone()
                    } else {
                        Vec::new()
                    },
                })
            }
            KeyCode::Tab | KeyCode::Down => {
                self.focused_field = (self.focused_field + 1) % max_field;
                DialogResult::Continue
            }
            KeyCode::BackTab | KeyCode::Up => {
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
                    // Apply yolo_mode_default and reload env keys/values from config
                    let config = resolve_config(&self.profile).unwrap_or_default();
                    self.yolo_mode = config.sandbox.yolo_mode_default;
                    self.extra_env_keys = config.sandbox.environment.clone();
                    self.extra_env_values = config
                        .sandbox
                        .environment_values
                        .iter()
                        .map(|(k, v)| format!("{}={}", k, v))
                        .collect();
                } else {
                    self.yolo_mode = false;
                    self.extra_env_keys.clear();
                    self.env_list_expanded = false;
                    self.env_editing_input = None;
                    self.extra_env_values.clear();
                    self.env_values_list_expanded = false;
                    self.env_values_editing_input = None;
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
                    && self.focused_field != env_values_field
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
        let validate =
            |value: &str, list: &[String]| !value.is_empty() && !list.contains(&value.to_string());
        handle_editable_list_key(
            key,
            &mut self.extra_env_keys,
            &mut self.env_list_expanded,
            &mut self.env_selected_index,
            &mut self.env_editing_input,
            &mut self.env_adding_new,
            validate,
        )
    }

    /// Handle key events when the env values list is expanded
    fn handle_env_values_list_key(&mut self, key: KeyEvent) -> DialogResult<NewSessionData> {
        let validate = |value: &str, _list: &[String]| !value.is_empty() && value.contains('=');
        handle_editable_list_key(
            key,
            &mut self.extra_env_values,
            &mut self.env_values_list_expanded,
            &mut self.env_values_selected_index,
            &mut self.env_values_editing_input,
            &mut self.env_values_adding_new,
            validate,
        )
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
