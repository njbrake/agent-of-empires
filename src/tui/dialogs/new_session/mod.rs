//! New session dialog

mod group_input;
mod path_input;
mod render;

#[cfg(test)]
mod tests;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::Instant;
use tui_input::backend::crossterm::EventHandler;
use tui_input::Input;

use super::DialogResult;
use crate::containers::{self, ContainerRuntimeInterface};
use crate::session::config::{DefaultTerminalMode, SandboxConfig};
use crate::session::repo_config::HookProgress;
#[cfg(test)]
use crate::session::Config;
use crate::session::{civilizations, resolve_config};
use crate::tmux::AvailableTools;
use crate::tui::components::{
    DirPicker, DirPickerResult, GroupGhostCompletion, ListPicker, ListPickerResult,
};
use path_input::PathGhostCompletion;

pub(super) struct FieldHelp {
    pub(super) name: &'static str,
    pub(super) description: &'static str,
}

pub(super) const HELP_DIALOG_WIDTH: u16 = 85;

pub(super) const FIELD_HELP: &[FieldHelp] = &[
    FieldHelp {
        name: "Profile",
        description: "Settings profile for session defaults (Left/Right to cycle)",
    },
    FieldHelp {
        name: "Title",
        description: "Session name (auto-generates if empty)",
    },
    FieldHelp {
        name: "Path",
        description: "Working directory for the session",
    },
    FieldHelp {
        name: "Tool",
        description: "Which AI tool to use (Ctrl+P to configure command and extra args)",
    },
    FieldHelp {
        name: "YOLO Mode",
        description:
            "Skip permission prompts for autonomous operation (--dangerously-skip-permissions)",
    },
    FieldHelp {
        name: "Worktree",
        description:
            "Branch name for git worktree (Ctrl+P to configure branch mode and extra repos)",
    },
    FieldHelp {
        name: "Sandbox",
        description: "Run session in Docker container for isolation (Ctrl+P to configure)",
    },
    FieldHelp {
        name: "Image",
        description: "Docker image. Edit config.toml [sandbox] default_image to change default",
    },
    FieldHelp {
        name: "Environment",
        description: "Env vars: bare KEY passes host value, KEY=VALUE sets explicitly",
    },
    FieldHelp {
        name: "Group",
        description: "Optional grouping for organization (Ctrl+P to browse existing groups)",
    },
];

#[derive(Clone)]
pub struct NewSessionData {
    pub profile: String,
    pub title: String,
    pub path: String,
    pub group: String,
    pub tool: String,
    pub worktree_branch: Option<String>,
    pub create_new_branch: bool,
    pub extra_repo_paths: Vec<String>,
    pub sandbox: bool,
    /// The sandbox image to use (always populated from the input field).
    pub sandbox_image: String,
    pub yolo_mode: bool,
    /// Additional environment entries for the container.
    /// `KEY` = pass through from host, `KEY=VALUE` = set explicitly.
    pub extra_env: Vec<String>,
    /// Extra arguments to append after the agent binary
    pub extra_args: String,
    /// Command override for the agent binary (replaces the default binary)
    pub command_override: String,
}

/// Spinner frames for loading animation
pub(super) const SPINNER_FRAMES: &[&str] = &["◐", "◓", "◑", "◒"];

pub struct NewSessionDialog {
    pub(super) profile: String,
    pub(super) available_profiles: Vec<String>,
    pub(super) profile_index: usize,
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
    pub(super) yolo_mode_default: bool,
    /// Additional repo paths for multi-repo workspace
    pub(super) workspace_repos: Vec<String>,
    /// Whether the workspace repos list is expanded (editing mode)
    pub(super) workspace_repos_expanded: bool,
    /// Currently selected index in the workspace repos list
    pub(super) workspace_repo_selected_index: usize,
    /// Input for editing/adding workspace repo entries
    pub(super) workspace_repo_editing_input: Option<Input>,
    /// Whether we are adding a new repo entry (vs editing existing)
    pub(super) workspace_repo_adding_new: bool,
    /// Ghost completion for workspace repo path editing
    pub(super) workspace_repo_ghost: Option<path_input::PathGhostCompletion>,
    /// Whether the dir picker was opened for a workspace repo (vs the main path)
    pub(super) workspace_repo_dir_picker_active: bool,
    /// Worktree configuration overlay mode (Ctrl+P on worktree field)
    pub(super) worktree_config_mode: bool,
    /// Focused field within the worktree config overlay (0=new_branch, 1=extra_repos)
    pub(super) worktree_config_focused_field: usize,
    /// Extra environment entries (session-specific).
    /// `KEY` = pass through, `KEY=VALUE` = set explicitly.
    pub(super) extra_env: Vec<String>,
    /// Whether the env list is expanded (editing mode)
    pub(super) env_list_expanded: bool,
    /// Currently selected index in the env list
    pub(super) env_selected_index: usize,
    /// Input for editing/adding env entries
    pub(super) env_editing_input: Option<Input>,
    /// Whether we are adding a new entry (vs editing existing)
    pub(super) env_adding_new: bool,
    /// Pre-computed label/value pairs for non-default inherited sandbox settings.
    pub(super) inherited_settings: Vec<(String, String)>,
    pub(super) sandbox_config_mode: bool,
    pub(super) sandbox_focused_field: usize,
    /// Tool configuration mode (Ctrl+P on tool field)
    pub(super) tool_config_mode: bool,
    pub(super) tool_config_focused_field: usize,
    /// Extra args for the selected tool (loaded from config)
    pub(super) extra_args: Input,
    /// Command override for the selected tool (loaded from config)
    pub(super) command_override: Input,
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
    /// Whether hooks are being executed during loading
    pub(super) has_hooks: bool,
    /// The currently running hook command
    pub(super) current_hook: Option<String>,
    /// Accumulated output lines from hook execution
    pub(super) hook_output: Vec<String>,
    /// Temporary highlight state for invalid path input.
    pub(super) path_invalid_flash_until: Option<Instant>,
    /// Ghost text completion for the path field (fish-shell style).
    path_ghost: Option<PathGhostCompletion>,
    /// Ghost text completion for the group field (fish-shell style).
    group_ghost: Option<GroupGhostCompletion>,
    /// Inline confirmation for creating a non-existent directory.
    /// None = inactive, Some(true) = Yes selected, Some(false) = No selected.
    pub(super) confirm_create_dir: Option<bool>,
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

/// Build label/value pairs for non-default inherited sandbox settings.
fn build_inherited_settings(sandbox: &SandboxConfig) -> Vec<(String, String)> {
    let mut settings = Vec::new();
    if sandbox.mount_ssh {
        settings.push(("Mount SSH".to_string(), "yes".to_string()));
    }
    if !sandbox.extra_volumes.is_empty() {
        settings.push((
            "Extra Volumes".to_string(),
            format!("{} items", sandbox.extra_volumes.len()),
        ));
    }
    if !sandbox.volume_ignores.is_empty() {
        settings.push((
            "Volume Ignores".to_string(),
            format!("{} items", sandbox.volume_ignores.len()),
        ));
    }
    if let Some(ref cpu) = sandbox.cpu_limit {
        settings.push(("CPU Limit".to_string(), cpu.clone()));
    }
    if let Some(ref mem) = sandbox.memory_limit {
        settings.push(("Memory Limit".to_string(), mem.clone()));
    }
    if sandbox.default_terminal_mode == DefaultTerminalMode::Container {
        settings.push(("Terminal Mode".to_string(), "container".to_string()));
    }
    settings
}

impl NewSessionDialog {
    pub fn new(
        tools: AvailableTools,
        existing_titles: Vec<String>,
        existing_groups: Vec<String>,
        profile: &str,
        available_profiles: Vec<String>,
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
        let yolo_mode = config.session.yolo_mode_default;

        // Load extra args and command override for the default tool
        let selected_tool = available_tools
            .get(tool_index)
            .or_else(|| available_tools.first())
            .copied()
            .unwrap_or("claude");
        let extra_args_value = config
            .session
            .agent_extra_args
            .get(selected_tool)
            .cloned()
            .unwrap_or_default();
        let command_override_value = config
            .session
            .agent_command_override
            .get(selected_tool)
            .cloned()
            .unwrap_or_default();

        // Initialize env entries and inherited settings from config when sandbox is enabled
        let (extra_env, inherited_settings) = if sandbox_enabled {
            let inherited = build_inherited_settings(&config.sandbox);
            (config.sandbox.environment.clone(), inherited)
        } else {
            (Vec::new(), Vec::new())
        };

        let profile_index = available_profiles
            .iter()
            .position(|p| p == profile)
            .unwrap_or(0);

        Self {
            profile: profile.to_string(),
            available_profiles,
            profile_index,
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
            workspace_repos: Vec::new(),
            workspace_repos_expanded: false,
            workspace_repo_selected_index: 0,
            workspace_repo_editing_input: None,
            workspace_repo_adding_new: false,
            workspace_repo_ghost: None,
            workspace_repo_dir_picker_active: false,
            worktree_config_mode: false,
            worktree_config_focused_field: 0,
            sandbox_enabled,
            sandbox_image: Input::new(
                containers::get_container_runtime().effective_default_image(),
            ),
            docker_available,
            yolo_mode,
            yolo_mode_default: yolo_mode,
            extra_env,
            env_list_expanded: false,
            env_selected_index: 0,
            env_editing_input: None,
            env_adding_new: false,
            inherited_settings,
            sandbox_config_mode: false,
            sandbox_focused_field: 0,
            tool_config_mode: false,
            tool_config_focused_field: 0,
            extra_args: Input::new(extra_args_value),
            command_override: Input::new(command_override_value),
            error_message: None,
            show_help: false,
            loading: false,
            spinner_frame: 0,
            has_hooks: false,
            current_hook: None,
            hook_output: Vec::new(),
            path_invalid_flash_until: None,
            path_ghost: None,
            group_ghost: None,
            confirm_create_dir: None,
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
        }
    }

    /// Check if the dialog is in loading state
    pub fn is_loading(&self) -> bool {
        self.loading
    }

    /// Advance dialog timers (spinner and transient highlights).
    /// Returns true when visual state changed and the UI should redraw.
    pub fn tick(&mut self) -> bool {
        let mut changed = false;

        if self.loading {
            self.spinner_frame = (self.spinner_frame + 1) % SPINNER_FRAMES.len();
            changed = true;
        }

        if let Some(until) = self.path_invalid_flash_until {
            if Instant::now() >= until {
                self.path_invalid_flash_until = None;
                changed = true;
            }
        }

        changed
    }

    pub(super) fn selected_profile(&self) -> &str {
        &self.available_profiles[self.profile_index]
    }

    pub(super) fn has_profile_selection(&self) -> bool {
        self.available_profiles.len() > 1
    }

    /// Whether the currently selected tool is always in YOLO mode (no opt-in needed).
    fn selected_tool_always_yolo(&self) -> bool {
        let tool_name = self.available_tools[self.tool_index];
        crate::agents::get_agent(tool_name)
            .and_then(|a| a.yolo.as_ref())
            .is_some_and(|y| matches!(y, crate::agents::YoloMode::AlwaysYolo))
    }

    /// The field index of the path field (shifts based on whether profile picker is visible)
    fn path_field(&self) -> usize {
        if self.has_profile_selection() {
            2
        } else {
            1
        }
    }

    /// Re-resolve config defaults when the profile changes.
    /// Resets tool, yolo, sandbox, and env settings but preserves user inputs
    /// (title, path, group, worktree).
    fn reload_config_defaults(&mut self) {
        let profile = self.selected_profile().to_string();
        self.profile = profile.clone();
        let config = resolve_config(&profile).unwrap_or_default();

        // Reset tool index
        self.tool_index = if let Some(ref default_tool) = config.session.default_tool {
            self.available_tools
                .iter()
                .position(|&t| t == default_tool.as_str())
                .unwrap_or(0)
        } else {
            0
        };

        // Reset sandbox/yolo defaults
        self.yolo_mode_default = config.session.yolo_mode_default;
        self.yolo_mode = self.yolo_mode_default;
        self.sandbox_enabled = self.docker_available && config.sandbox.enabled_by_default;

        // Reset sandbox image from resolved config (includes profile overrides)
        self.sandbox_image = Input::new(config.sandbox.default_image.clone());

        // Reset env entries and inherited settings
        if self.sandbox_enabled {
            self.extra_env = config.sandbox.environment.clone();
            self.inherited_settings = build_inherited_settings(&config.sandbox);
        } else {
            self.extra_env.clear();
            self.inherited_settings.clear();
        }

        // Reset extra args and command override for new default tool
        let selected_tool = self
            .available_tools
            .get(self.tool_index)
            .or_else(|| self.available_tools.first())
            .copied()
            .unwrap_or("claude");
        self.extra_args = Input::new(
            config
                .session
                .agent_extra_args
                .get(selected_tool)
                .cloned()
                .unwrap_or_default(),
        );
        self.command_override = Input::new(
            config
                .session
                .agent_command_override
                .get(selected_tool)
                .cloned()
                .unwrap_or_default(),
        );
        self.tool_config_mode = false;
        self.tool_config_focused_field = 0;

        // Reset expanded states
        self.env_list_expanded = false;
        self.env_editing_input = None;
        self.sandbox_config_mode = false;
        self.sandbox_focused_field = 0;
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
            available_profiles: vec!["default".to_string()],
            profile_index: 0,
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
            workspace_repos: Vec::new(),
            workspace_repos_expanded: false,
            workspace_repo_selected_index: 0,
            workspace_repo_editing_input: None,
            workspace_repo_adding_new: false,
            workspace_repo_ghost: None,
            workspace_repo_dir_picker_active: false,
            worktree_config_mode: false,
            worktree_config_focused_field: 0,
            sandbox_enabled: false,
            sandbox_image: Input::new(
                containers::get_container_runtime().effective_default_image(),
            ),
            docker_available: false,
            yolo_mode: false,
            yolo_mode_default: false,
            extra_env: Vec::new(),
            env_list_expanded: false,
            env_selected_index: 0,
            env_editing_input: None,
            env_adding_new: false,
            inherited_settings: Vec::new(),
            sandbox_config_mode: false,
            sandbox_focused_field: 0,
            tool_config_mode: false,
            tool_config_focused_field: 0,
            extra_args: Input::default(),
            command_override: Input::default(),
            error_message: None,
            show_help: false,
            loading: false,
            spinner_frame: 0,
            has_hooks: false,
            current_hook: None,
            hook_output: Vec::new(),
            path_invalid_flash_until: None,
            path_ghost: None,
            group_ghost: None,
            confirm_create_dir: None,
        }
    }

    #[cfg(test)]
    pub(super) fn new_with_tools(tools: Vec<&'static str>, path: String) -> Self {
        Self {
            profile: "default".to_string(),
            available_profiles: vec!["default".to_string()],
            profile_index: 0,
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
            workspace_repos: Vec::new(),
            workspace_repos_expanded: false,
            workspace_repo_selected_index: 0,
            workspace_repo_editing_input: None,
            workspace_repo_adding_new: false,
            workspace_repo_ghost: None,
            workspace_repo_dir_picker_active: false,
            worktree_config_mode: false,
            worktree_config_focused_field: 0,
            sandbox_enabled: false,
            sandbox_image: Input::new(
                containers::get_container_runtime().effective_default_image(),
            ),
            docker_available: false,
            yolo_mode: false,
            yolo_mode_default: false,
            extra_env: Vec::new(),
            env_list_expanded: false,
            env_selected_index: 0,
            env_editing_input: None,
            env_adding_new: false,
            inherited_settings: Vec::new(),
            sandbox_config_mode: false,
            sandbox_focused_field: 0,
            tool_config_mode: false,
            tool_config_focused_field: 0,
            extra_args: Input::default(),
            command_override: Input::default(),
            error_message: None,
            show_help: false,
            loading: false,
            spinner_frame: 0,
            has_hooks: false,
            current_hook: None,
            hook_output: Vec::new(),
            path_invalid_flash_until: None,
            path_ghost: None,
            group_ghost: None,
            confirm_create_dir: None,
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

        // Delegate to sandbox config mode handler when active
        if self.sandbox_config_mode {
            return self.handle_sandbox_config_key(key);
        }

        // Delegate to tool config mode handler when active
        if self.tool_config_mode {
            return self.handle_tool_config_key(key);
        }

        // Delegate to worktree config mode handler when active
        if self.worktree_config_mode {
            return self.handle_worktree_config_key(key);
        }

        if self.confirm_create_dir.is_some() {
            return self.handle_confirm_create_dir_key(key);
        }

        if self.group_picker.is_active() {
            if let ListPickerResult::Selected(value) = self.group_picker.handle_key(key) {
                self.group = Input::new(value);
                self.clear_group_ghost();
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
                    if self.workspace_repo_dir_picker_active {
                        self.workspace_repo_editing_input = Some(Input::new(path));
                        self.workspace_repo_ghost = self
                            .workspace_repo_editing_input
                            .as_ref()
                            .and_then(path_input::compute_path_ghost);
                        self.workspace_repo_dir_picker_active = false;
                    } else {
                        self.path = Input::new(path);
                        self.recompute_path_ghost();
                    }
                }
                DirPickerResult::Cancelled => {
                    self.workspace_repo_dir_picker_active = false;
                }
                DirPickerResult::Continue => {}
            }
            return DialogResult::Continue;
        }

        let has_profile_selection = self.available_profiles.len() > 1;
        let has_tool_selection = self.available_tools.len() > 1;
        let has_sandbox = self.docker_available;
        let has_yolo = !self.selected_tool_always_yolo();
        // Field order: [profile], title, path, [tool], [yolo], worktree, [sandbox], group
        // Worktree sub-options (new_branch, extra_repos) are in a Ctrl+P overlay.
        // Tool config (extra_args, command_override) is in a Ctrl+P overlay on tool field.
        // Sandbox sub-options are in a separate sandbox_config_mode overlay.
        let profile_field = if has_profile_selection { 0 } else { usize::MAX };
        let mut fi = if has_profile_selection { 1 } else { 0 }; // next field index
        fi += 2; // title + path
        let tool_field = if has_tool_selection {
            let f = fi;
            fi += 1;
            f
        } else {
            usize::MAX
        };
        let yolo_mode_field = if has_yolo {
            let f = fi;
            fi += 1;
            f
        } else {
            usize::MAX
        };
        let worktree_field = fi;
        fi += 1;
        let sandbox_field = if has_sandbox {
            let f = fi;
            fi += 1;
            f
        } else {
            usize::MAX
        };
        let group_field = fi;
        fi += 1;
        let max_field = fi;

        // Ctrl+P opens a context-sensitive picker/config overlay
        if key.code == KeyCode::Char('p') && key.modifiers.contains(KeyModifiers::CONTROL) {
            if self.focused_field == self.path_field() {
                let path_value = self.path.value().trim().to_string();
                self.dir_picker.activate(&path_value);
                return DialogResult::Continue;
            }
            if self.focused_field == tool_field {
                self.tool_config_mode = true;
                self.tool_config_focused_field = 0;
                return DialogResult::Continue;
            }
            if self.focused_field == group_field && !self.existing_groups.is_empty() {
                self.group_picker.activate(self.existing_groups.clone());
                return DialogResult::Continue;
            }
            if self.focused_field == worktree_field && !self.worktree_branch.value().is_empty() {
                self.worktree_config_mode = true;
                self.worktree_config_focused_field = 0;
                return DialogResult::Continue;
            }
            if self.focused_field == sandbox_field && self.sandbox_enabled {
                self.sandbox_config_mode = true;
                self.sandbox_focused_field = 0;
                return DialogResult::Continue;
            }
        }

        if self.handle_path_shortcuts(key) {
            return DialogResult::Continue;
        }

        if self.handle_group_shortcuts(key, group_field) {
            return DialogResult::Continue;
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
            KeyCode::Enter => {
                self.error_message = None;
                let path_str = self.path.value().trim().to_string();
                let resolved = path_input::expand_tilde(&path_str);
                if !std::path::Path::new(&resolved).exists() {
                    self.confirm_create_dir = Some(false);
                    return DialogResult::Continue;
                }
                self.build_submit_result()
            }
            KeyCode::Tab | KeyCode::Down => {
                if self.focused_field == self.path_field() {
                    self.clear_path_ghost();
                }
                if self.focused_field == group_field {
                    self.clear_group_ghost();
                }
                self.focused_field = (self.focused_field + 1) % max_field;
                if self.focused_field == self.path_field() {
                    self.recompute_path_ghost();
                }
                if self.focused_field == group_field {
                    self.recompute_group_ghost();
                }
                DialogResult::Continue
            }
            KeyCode::BackTab | KeyCode::Up => {
                if self.focused_field == self.path_field() {
                    self.clear_path_ghost();
                }
                if self.focused_field == group_field {
                    self.clear_group_ghost();
                }
                self.focused_field = if self.focused_field == 0 {
                    max_field - 1
                } else {
                    self.focused_field - 1
                };
                if self.focused_field == self.path_field() {
                    self.recompute_path_ghost();
                }
                if self.focused_field == group_field {
                    self.recompute_group_ghost();
                }
                DialogResult::Continue
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
                if self.focused_field == profile_field =>
            {
                if self.available_profiles.len() > 1 {
                    if key.code == KeyCode::Left {
                        self.profile_index = if self.profile_index == 0 {
                            self.available_profiles.len() - 1
                        } else {
                            self.profile_index - 1
                        };
                    } else {
                        self.profile_index =
                            (self.profile_index + 1) % self.available_profiles.len();
                    }
                    self.reload_config_defaults();
                }
                DialogResult::Continue
            }
            KeyCode::Left | KeyCode::Right if self.focused_field == tool_field => {
                self.tool_index = (self.tool_index + 1) % self.available_tools.len();
                if self.selected_tool_always_yolo() {
                    self.yolo_mode = true;
                } else {
                    self.yolo_mode = self.yolo_mode_default;
                }
                self.reload_tool_config();
                DialogResult::Continue
            }
            KeyCode::Char(' ') if self.focused_field == tool_field => {
                self.tool_index = (self.tool_index + 1) % self.available_tools.len();
                if self.selected_tool_always_yolo() {
                    self.yolo_mode = true;
                } else {
                    self.yolo_mode = self.yolo_mode_default;
                }
                self.reload_tool_config();
                DialogResult::Continue
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
                if self.focused_field == sandbox_field =>
            {
                self.sandbox_enabled = !self.sandbox_enabled;
                if self.sandbox_enabled {
                    let config = resolve_config(&self.profile).unwrap_or_default();
                    self.extra_env = config.sandbox.environment.clone();
                    self.inherited_settings = build_inherited_settings(&config.sandbox);
                } else {
                    self.extra_env.clear();
                    self.env_list_expanded = false;
                    self.env_editing_input = None;
                    self.inherited_settings.clear();
                    self.sandbox_config_mode = false;
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
                if self.focused_field != profile_field
                    && self.focused_field != tool_field
                    && self.focused_field != sandbox_field
                    && self.focused_field != yolo_mode_field
                {
                    self.current_input_mut()
                        .handle_event(&crossterm::event::Event::Key(key));
                    self.error_message = None;
                    if self.focused_field == self.path_field() {
                        self.path_invalid_flash_until = None;
                        self.recompute_path_ghost();
                    }
                    if self.focused_field == group_field {
                        self.recompute_group_ghost();
                    }
                }
                DialogResult::Continue
            }
        }
    }

    /// Handle key events when in sandbox configuration mode.
    fn handle_sandbox_config_key(&mut self, key: KeyEvent) -> DialogResult<NewSessionData> {
        // Sandbox config fields: 0=image, 1=env (inherited is always-visible, not focusable)
        const SANDBOX_IMAGE: usize = 0;
        const SANDBOX_ENV: usize = 1;
        const SANDBOX_MAX: usize = 2;

        // Handle env list editing when expanded
        if self.env_list_expanded && self.sandbox_focused_field == SANDBOX_ENV {
            return self.handle_env_list_key(key);
        }

        match key.code {
            KeyCode::Esc => {
                self.sandbox_config_mode = false;
                DialogResult::Continue
            }
            KeyCode::Char('?') => {
                self.show_help = true;
                DialogResult::Continue
            }
            KeyCode::Enter if self.sandbox_focused_field == SANDBOX_ENV => {
                self.env_list_expanded = true;
                self.env_selected_index = 0;
                DialogResult::Continue
            }
            KeyCode::Enter => {
                self.sandbox_config_mode = false;
                DialogResult::Continue
            }
            KeyCode::Tab | KeyCode::Down => {
                self.sandbox_focused_field = (self.sandbox_focused_field + 1) % SANDBOX_MAX;
                DialogResult::Continue
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.sandbox_focused_field = if self.sandbox_focused_field == 0 {
                    SANDBOX_MAX - 1
                } else {
                    self.sandbox_focused_field - 1
                };
                DialogResult::Continue
            }
            _ => {
                // Text input for image field only
                if self.sandbox_focused_field == SANDBOX_IMAGE {
                    self.sandbox_image
                        .handle_event(&crossterm::event::Event::Key(key));
                }
                DialogResult::Continue
            }
        }
    }

    /// Handle key events when in tool configuration mode.
    fn handle_tool_config_key(&mut self, key: KeyEvent) -> DialogResult<NewSessionData> {
        // Tool config fields: 0=command override, 1=extra args
        const TOOL_CMD: usize = 0;
        const TOOL_ARGS: usize = 1;
        const TOOL_MAX: usize = 2;

        match key.code {
            KeyCode::Esc => {
                self.tool_config_mode = false;
                DialogResult::Continue
            }
            KeyCode::Char('?') => {
                self.show_help = true;
                DialogResult::Continue
            }
            KeyCode::Enter => {
                self.tool_config_mode = false;
                DialogResult::Continue
            }
            KeyCode::Tab | KeyCode::Down => {
                self.tool_config_focused_field = (self.tool_config_focused_field + 1) % TOOL_MAX;
                DialogResult::Continue
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.tool_config_focused_field = if self.tool_config_focused_field == 0 {
                    TOOL_MAX - 1
                } else {
                    self.tool_config_focused_field - 1
                };
                DialogResult::Continue
            }
            _ => {
                match self.tool_config_focused_field {
                    TOOL_CMD => {
                        self.command_override
                            .handle_event(&crossterm::event::Event::Key(key));
                    }
                    TOOL_ARGS => {
                        self.extra_args
                            .handle_event(&crossterm::event::Event::Key(key));
                    }
                    _ => {}
                }
                DialogResult::Continue
            }
        }
    }

    /// Handle key events when in worktree configuration mode.
    fn handle_worktree_config_key(&mut self, key: KeyEvent) -> DialogResult<NewSessionData> {
        // Worktree config fields: 0=new_branch checkbox, 1=extra_repos list
        const WT_NEW_BRANCH: usize = 0;
        const WT_EXTRA_REPOS: usize = 1;
        const WT_MAX: usize = 2;

        // Handle workspace repos list editing when expanded
        if self.workspace_repos_expanded && self.worktree_config_focused_field == WT_EXTRA_REPOS {
            return self.handle_workspace_repos_list_key(key);
        }

        match key.code {
            KeyCode::Esc => {
                self.worktree_config_mode = false;
                DialogResult::Continue
            }
            KeyCode::Char('?') => {
                self.show_help = true;
                DialogResult::Continue
            }
            // Ctrl+P on new_branch field opens branch picker
            KeyCode::Char('p')
                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && self.worktree_config_focused_field == WT_NEW_BRANCH =>
            {
                let path = std::path::Path::new(self.path.value().trim());
                if let Ok(branches) = crate::git::diff::list_branches(path) {
                    if !branches.is_empty() {
                        self.branch_picker.activate(branches);
                    }
                }
                DialogResult::Continue
            }
            KeyCode::Enter if self.worktree_config_focused_field == WT_EXTRA_REPOS => {
                self.workspace_repos_expanded = true;
                self.workspace_repo_selected_index = 0;
                DialogResult::Continue
            }
            KeyCode::Enter => {
                self.worktree_config_mode = false;
                DialogResult::Continue
            }
            KeyCode::Tab | KeyCode::Down => {
                self.worktree_config_focused_field =
                    (self.worktree_config_focused_field + 1) % WT_MAX;
                DialogResult::Continue
            }
            KeyCode::BackTab | KeyCode::Up => {
                self.worktree_config_focused_field = if self.worktree_config_focused_field == 0 {
                    WT_MAX - 1
                } else {
                    self.worktree_config_focused_field - 1
                };
                DialogResult::Continue
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Char(' ')
                if self.worktree_config_focused_field == WT_NEW_BRANCH =>
            {
                self.create_new_branch = !self.create_new_branch;
                DialogResult::Continue
            }
            _ => DialogResult::Continue,
        }
    }

    /// Handle key events when the env list is expanded
    fn handle_env_list_key(&mut self, key: KeyEvent) -> DialogResult<NewSessionData> {
        let validate =
            |value: &str, list: &[String]| !value.is_empty() && !list.contains(&value.to_string());
        let snapshot: Vec<String> = self.extra_env.clone();
        let result = handle_editable_list_key(
            key,
            &mut self.extra_env,
            &mut self.env_list_expanded,
            &mut self.env_selected_index,
            &mut self.env_editing_input,
            &mut self.env_adding_new,
            validate,
        );

        // Validate the current entry if the list changed
        if self.extra_env != snapshot {
            self.error_message = self
                .extra_env
                .get(self.env_selected_index)
                .and_then(|entry| crate::session::validate_env_entry(entry));
        }

        result
    }

    /// Handle key events when the workspace repos list is expanded
    fn handle_workspace_repos_list_key(&mut self, key: KeyEvent) -> DialogResult<NewSessionData> {
        // When actively editing a repo path, handle path-specific keys first
        if self.workspace_repo_editing_input.is_some() {
            // Ctrl+P: open dir picker for repo path
            if key.code == KeyCode::Char('p') && key.modifiers.contains(KeyModifiers::CONTROL) {
                let initial = self
                    .workspace_repo_editing_input
                    .as_ref()
                    .map(|i| i.value().trim().to_string())
                    .unwrap_or_default();
                let initial = if initial.is_empty() {
                    std::env::current_dir()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_else(|_| ".".to_string())
                } else {
                    initial
                };
                self.workspace_repo_dir_picker_active = true;
                self.dir_picker.activate(&initial);
                return DialogResult::Continue;
            }

            // Right/End at end of input: accept ghost text
            if matches!(key.code, KeyCode::Right | KeyCode::End)
                && key.modifiers == KeyModifiers::NONE
            {
                if let Some(ref input) = self.workspace_repo_editing_input {
                    let cursor = input.visual_cursor();
                    let char_len = input.value().chars().count();
                    if cursor >= char_len {
                        if let Some(ghost) = self.workspace_repo_ghost.take() {
                            if let Some(ref mut input) = self.workspace_repo_editing_input {
                                let value = input.value().to_string();
                                let cursor_char = input.visual_cursor().min(value.chars().count());
                                if ghost.input_snapshot == value
                                    && ghost.cursor_snapshot == cursor_char
                                {
                                    let mut new_value = value;
                                    new_value.push_str(&ghost.ghost_text);
                                    *input = Input::new(new_value);
                                    self.workspace_repo_ghost =
                                        path_input::compute_path_ghost(input);
                                    return DialogResult::Continue;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Intercept 'a' to pre-populate with the expanded cwd (like the main path field)
        if self.workspace_repo_editing_input.is_none()
            && key.code == KeyCode::Char('a')
            && key.modifiers == KeyModifiers::NONE
        {
            let cwd = std::env::current_dir()
                .map(|p| {
                    let mut s = path_input::collapse_tilde(&p.to_string_lossy());
                    if !s.ends_with('/') {
                        s.push('/');
                    }
                    s
                })
                .unwrap_or_default();
            self.workspace_repo_editing_input = Some(Input::new(cwd));
            self.workspace_repo_adding_new = true;
            self.workspace_repo_ghost = self
                .workspace_repo_editing_input
                .as_ref()
                .and_then(path_input::compute_path_ghost);
            return DialogResult::Continue;
        }

        let validate =
            |value: &str, list: &[String]| !value.is_empty() && !list.contains(&value.to_string());

        // Wrap the generic handler to add tilde expansion and ghost recomputation
        let had_input = self.workspace_repo_editing_input.is_some();
        let was_adding = self.workspace_repo_adding_new;
        let edit_index = self.workspace_repo_selected_index;
        let result = handle_editable_list_key(
            key,
            &mut self.workspace_repos,
            &mut self.workspace_repos_expanded,
            &mut self.workspace_repo_selected_index,
            &mut self.workspace_repo_editing_input,
            &mut self.workspace_repo_adding_new,
            validate,
        );

        // If editing just finished (Enter pressed), expand tilde in the stored value
        if had_input && self.workspace_repo_editing_input.is_none() {
            let idx = if was_adding {
                self.workspace_repos.len().saturating_sub(1)
            } else {
                edit_index
            };
            if let Some(entry) = self.workspace_repos.get_mut(idx) {
                *entry = path_input::expand_tilde(entry);
            }
            self.workspace_repo_ghost = None;
        }

        // If still editing, recompute ghost
        if self.workspace_repo_editing_input.is_some() {
            self.workspace_repo_ghost = self
                .workspace_repo_editing_input
                .as_ref()
                .and_then(path_input::compute_path_ghost);
        } else {
            self.workspace_repo_ghost = None;
        }

        result
    }

    fn reload_tool_config(&mut self) {
        let profile = self.selected_profile().to_string();
        let config = resolve_config(&profile).unwrap_or_default();
        let tool = self
            .available_tools
            .get(self.tool_index)
            .or_else(|| self.available_tools.first())
            .copied()
            .unwrap_or("claude");
        self.extra_args = Input::new(
            config
                .session
                .agent_extra_args
                .get(tool)
                .cloned()
                .unwrap_or_default(),
        );
        self.command_override = Input::new(
            config
                .session
                .agent_command_override
                .get(tool)
                .cloned()
                .unwrap_or_default(),
        );
    }

    fn current_input_mut(&mut self) -> &mut Input {
        let has_tool_selection = self.available_tools.len() > 1;
        let has_yolo = !self.selected_tool_always_yolo();
        let base = if self.has_profile_selection() { 1 } else { 0 };

        // Field layout: [profile], title, path, [tool], [yolo], worktree, [sandbox], group
        let mut fi = base + 2 + if has_tool_selection { 1 } else { 0 };
        if has_yolo {
            fi += 1;
        }
        let worktree_field = fi;
        let mut next = worktree_field + 1;
        if self.docker_available {
            next += 1; // sandbox checkbox
        }
        let group_field = next;

        let path_field = self.path_field();
        let title_field = if self.has_profile_selection() { 1 } else { 0 };
        match self.focused_field {
            n if n == title_field => &mut self.title,
            n if n == path_field => &mut self.path,
            n if n == worktree_field => &mut self.worktree_branch,
            n if n == group_field => &mut self.group,
            _ => &mut self.title,
        }
    }

    fn build_submit_result(&self) -> DialogResult<NewSessionData> {
        let title_value = self.title.value().trim();
        let final_title = if title_value.is_empty() {
            let refs: Vec<&str> = self.existing_titles.iter().map(|s| s.as_str()).collect();
            civilizations::generate_random_title(&refs)
        } else {
            title_value.to_string()
        };
        let worktree_value = self.worktree_branch.value().trim();
        let has_worktree_branch = !worktree_value.is_empty();
        let worktree_branch = if has_worktree_branch {
            Some(worktree_value.to_string())
        } else {
            None
        };
        DialogResult::Submit(NewSessionData {
            profile: self.selected_profile().to_string(),
            title: final_title,
            path: self.path.value().trim().to_string(),
            group: self.group.value().trim().to_string(),
            tool: self.available_tools[self.tool_index].to_string(),
            worktree_branch,
            create_new_branch: self.create_new_branch,
            extra_repo_paths: if has_worktree_branch {
                self.workspace_repos.clone()
            } else {
                Vec::new()
            },
            sandbox: self.sandbox_enabled,
            sandbox_image: self.sandbox_image.value().trim().to_string(),
            yolo_mode: self.yolo_mode || self.selected_tool_always_yolo(),
            extra_env: if self.sandbox_enabled {
                self.extra_env.clone()
            } else {
                Vec::new()
            },
            extra_args: self.extra_args.value().trim().to_string(),
            command_override: self.command_override.value().trim().to_string(),
        })
    }

    fn handle_confirm_create_dir_key(&mut self, key: KeyEvent) -> DialogResult<NewSessionData> {
        let selected = self.confirm_create_dir.as_mut().unwrap();
        match key.code {
            KeyCode::Left | KeyCode::Char('h') => {
                *selected = true;
                DialogResult::Continue
            }
            KeyCode::Right | KeyCode::Char('l') => {
                *selected = false;
                DialogResult::Continue
            }
            KeyCode::Tab => {
                *selected = !*selected;
                DialogResult::Continue
            }
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                self.confirm_create_dir = None;
                self.try_create_dir_and_submit()
            }
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                self.confirm_create_dir = None;
                self.focused_field = self.path_field();
                DialogResult::Continue
            }
            KeyCode::Enter => {
                let yes = *selected;
                self.confirm_create_dir = None;
                if yes {
                    self.try_create_dir_and_submit()
                } else {
                    self.focused_field = self.path_field();
                    DialogResult::Continue
                }
            }
            _ => DialogResult::Continue,
        }
    }

    fn try_create_dir_and_submit(&mut self) -> DialogResult<NewSessionData> {
        let path_str = self.path.value().trim().to_string();
        let resolved = path_input::expand_tilde(&path_str);
        match std::fs::create_dir_all(&resolved) {
            Ok(()) => self.build_submit_result(),
            Err(e) => {
                self.error_message = Some(format!("Failed to create directory: {}", e));
                self.focused_field = self.path_field();
                DialogResult::Continue
            }
        }
    }
}
