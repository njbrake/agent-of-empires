//! Setting field definitions and config mapping

use crate::session::{
    validate_check_interval, Config, ContainerRuntimeName, DefaultTerminalMode, ProfileConfig,
    TmuxMouseMode, TmuxStatusBarMode,
};
use crate::sound::{
    validate_sound_exists, volume_from_option, volume_options, volume_to_index, SoundMode,
};
use crate::tui::styles::available_themes;

use super::SettingsScope;

/// Categories of settings
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsCategory {
    Theme,
    Updates,
    Worktree,
    Sandbox,
    Tmux,
    Session,
    Sound,
    Hooks,
    Web,
}

impl SettingsCategory {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Theme => "Theme",
            Self::Updates => "Updates",
            Self::Worktree => "Worktree",
            Self::Sandbox => "Sandbox",
            Self::Tmux => "Tmux",
            Self::Session => "Session",
            Self::Sound => "Sound",
            Self::Hooks => "Hooks",
            Self::Web => "Web",
        }
    }
}

/// Type-safe field identifiers (prevents typos in string matching)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKey {
    // Theme
    ThemeName,
    ThemeColorMode,
    IdleDecayMinutes,
    // Updates
    CheckEnabled,
    CheckIntervalHours,
    NotifyInCli,
    // Worktree
    PathTemplate,
    BareRepoPathTemplate,
    WorktreeAutoCleanup,
    DeleteBranchOnCleanup,
    WorkspacePathTemplate,
    // Sandbox
    SandboxEnabledByDefault,
    YoloModeDefault,
    DefaultImage,
    Environment,
    SandboxAutoCleanup,
    CpuLimit,
    MemoryLimit,
    DefaultTerminalMode,
    ExtraVolumes,
    PortMappings,
    VolumeIgnores,
    MountSsh,
    CustomInstruction,
    ContainerRuntime,
    // Tmux
    StatusBar,
    Mouse,
    // Session
    DefaultTool,
    StrictHotkeys,
    AgentExtraArgs,
    AgentCommandOverride,
    AgentStatusHooks,
    CustomAgents,
    AgentDetectAs,
    // Sound
    SoundEnabled,
    SoundMode,
    SoundVolume,
    SoundOnStart,
    SoundOnRunning,
    SoundOnWaiting,
    SoundOnIdle,
    SoundOnError,
    // Hooks
    HookOnCreate,
    HookOnLaunch,
    HookOnDestroy,
    // Web
    WebNotificationsEnabled,
    WebNotifyOnWaiting,
    WebNotifyOnIdle,
    WebNotifyOnError,
}

/// Resolve a field value from global config and optional profile override.
/// Returns (value, has_override).
fn resolve_value<T: Clone>(scope: SettingsScope, global: T, profile: Option<T>) -> (T, bool) {
    match scope {
        SettingsScope::Global | SettingsScope::Repo => (global, false),
        SettingsScope::Profile => {
            let has_override = profile.is_some();
            let value = profile.unwrap_or(global);
            (value, has_override)
        }
    }
}

/// Resolve an optional field (Option<T>) where both global and profile values are Option<T>.
/// The `has_explicit_override` flag indicates if the profile explicitly set this field.
fn resolve_optional<T: Clone>(
    scope: SettingsScope,
    global: Option<T>,
    profile: Option<T>,
    has_explicit_override: bool,
) -> (Option<T>, bool) {
    match scope {
        SettingsScope::Global | SettingsScope::Repo => (global, false),
        SettingsScope::Profile => {
            let value = profile.or(global);
            (value, has_explicit_override)
        }
    }
}

/// Convert a FieldValue to a human-readable display string.
fn value_display_string(value: &FieldValue) -> String {
    match value {
        FieldValue::Bool(v) => if *v { "on" } else { "off" }.to_string(),
        FieldValue::Text(v) => {
            if v.is_empty() {
                "(empty)".to_string()
            } else {
                v.clone()
            }
        }
        FieldValue::Number(v) => v.to_string(),
        FieldValue::Select { selected, options } => {
            options.get(*selected).cloned().unwrap_or_default()
        }
        FieldValue::List(items) => format!("[{} items]", items.len()),
        FieldValue::OptionalText(v) => v.clone().unwrap_or_else(|| "(empty)".to_string()),
    }
}

/// Build the inherited display string when a field has an override.
fn inherited_if(has_override: bool, global_value: FieldValue) -> Option<String> {
    if has_override {
        Some(value_display_string(&global_value))
    } else {
        None
    }
}

/// Helper to set a profile override. Always stores the value; use 'r' key to clear overrides.
fn set_profile_override<T, S, F>(new_value: T, section: &mut Option<S>, set_field: F)
where
    T: Clone,
    S: Default,
    F: FnOnce(&mut S, Option<T>),
{
    let s = section.get_or_insert_with(S::default);
    set_field(s, Some(new_value));
}

/// Parse a list of "key=value" strings into a HashMap.
fn parse_key_value_list(items: &[String]) -> std::collections::HashMap<String, String> {
    items
        .iter()
        .filter_map(|item| {
            let (k, v) = item.split_once('=')?;
            Some((k.to_string(), v.to_string()))
        })
        .collect()
}

/// Value types for settings fields
#[derive(Debug, Clone)]
pub enum FieldValue {
    Bool(bool),
    Text(String),
    Number(u64),
    Select {
        selected: usize,
        options: Vec<String>,
    },
    List(Vec<String>),
    OptionalText(Option<String>),
}

/// A setting field with metadata
#[derive(Debug, Clone)]
pub struct SettingField {
    pub key: FieldKey,
    pub label: &'static str,
    pub description: &'static str,
    pub value: FieldValue,
    pub category: SettingsCategory,
    /// Whether this field has a profile/repo override
    pub has_override: bool,
    /// Human-readable display of the inherited (global/base) value, set when has_override is true
    pub inherited_display: Option<String>,
}

impl SettingField {
    pub fn validate(&self) -> Result<(), String> {
        match (&self.key, &self.value) {
            (FieldKey::CheckIntervalHours, FieldValue::Number(n)) => {
                validate_check_interval(*n)?;
                Ok(())
            }
            (FieldKey::MemoryLimit, FieldValue::OptionalText(Some(v))) => {
                crate::session::validate_memory_limit(v)?;
                Ok(())
            }
            // Sound field validation - check if sound file exists
            (
                FieldKey::SoundOnStart
                | FieldKey::SoundOnRunning
                | FieldKey::SoundOnWaiting
                | FieldKey::SoundOnIdle
                | FieldKey::SoundOnError,
                FieldValue::OptionalText(Some(name)),
            ) => {
                if !name.is_empty() {
                    validate_sound_exists(name)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// Build fields for a category based on scope and current config values.
///
/// For Repo scope, `global` should be the resolved (global+profile merged) config,
/// and `profile` should be the repo config converted to ProfileConfig via `repo_config_to_profile`.
pub fn build_fields_for_category(
    category: SettingsCategory,
    scope: SettingsScope,
    global: &Config,
    profile: &ProfileConfig,
) -> Vec<SettingField> {
    match category {
        SettingsCategory::Theme => build_theme_fields(scope, global, profile),
        SettingsCategory::Updates => build_updates_fields(scope, global, profile),
        SettingsCategory::Worktree => build_worktree_fields(scope, global, profile),
        SettingsCategory::Sandbox => build_sandbox_fields(scope, global, profile),
        SettingsCategory::Tmux => build_tmux_fields(scope, global, profile),
        SettingsCategory::Session => build_session_fields(scope, global, profile),
        SettingsCategory::Sound => build_sound_fields(scope, global, profile),
        SettingsCategory::Hooks => build_hooks_fields(scope, global, profile),
        SettingsCategory::Web => build_web_fields(scope, global, profile),
    }
}

fn build_web_fields(
    scope: SettingsScope,
    global: &Config,
    _profile: &ProfileConfig,
) -> Vec<SettingField> {
    // Web settings are server-global, not profile-scoped. In Profile mode
    // we still surface the field (read-only) so users discover it; writes
    // always apply to the global config.
    let _ = scope;

    vec![
        SettingField {
            key: FieldKey::WebNotificationsEnabled,
            label: "Push notifications",
            description: "Allow the web dashboard to deliver browser push notifications (server-wide kill switch).",
            value: FieldValue::Bool(global.web.notifications_enabled),
            category: SettingsCategory::Web,
            has_override: false,
            inherited_display: None,
        },
        SettingField {
            key: FieldKey::WebNotifyOnWaiting,
            label: "Notify on waiting",
            description: "Default: send a push when a session transitions Running to Waiting (agent is asking for input). Sessions can override individually.",
            value: FieldValue::Bool(global.web.notify_on_waiting),
            category: SettingsCategory::Web,
            has_override: false,
            inherited_display: None,
        },
        SettingField {
            key: FieldKey::WebNotifyOnIdle,
            label: "Notify on idle",
            description: "Default: send a push when a session finishes (Running to Idle). Off by default because short sessions make this noisy; sessions can opt in individually.",
            value: FieldValue::Bool(global.web.notify_on_idle),
            category: SettingsCategory::Web,
            has_override: false,
            inherited_display: None,
        },
        SettingField {
            key: FieldKey::WebNotifyOnError,
            label: "Notify on error",
            description: "Default: send a push when a session errors (Running to Error).",
            value: FieldValue::Bool(global.web.notify_on_error),
            category: SettingsCategory::Web,
            has_override: false,
            inherited_display: None,
        },
    ]
}

fn build_theme_fields(
    scope: SettingsScope,
    global: &Config,
    profile: &ProfileConfig,
) -> Vec<SettingField> {
    let theme = profile.theme.as_ref();

    let (name, has_override) = resolve_value(
        scope,
        global.theme.name.clone(),
        theme.and_then(|t| t.name.clone()),
    );

    let options: Vec<String> = available_themes();
    let selected = options.iter().position(|s| s == &name).unwrap_or(0);

    let global_selected = options
        .iter()
        .position(|s| s == &global.theme.name)
        .unwrap_or(0);
    let inherited = inherited_if(
        has_override,
        FieldValue::Select {
            selected: global_selected,
            options: options.clone(),
        },
    );

    let color_mode_options: Vec<String> = vec!["truecolor".to_string(), "palette".to_string()];
    let (color_mode_val, cm_has_override) = resolve_value(
        scope,
        global.theme.color_mode.clone(),
        theme.and_then(|t| t.color_mode.clone()),
    );
    let cm_selected = match color_mode_val {
        crate::session::config::ColorMode::Truecolor => 0,
        crate::session::config::ColorMode::Palette => 1,
    };
    let global_cm_selected = match global.theme.color_mode {
        crate::session::config::ColorMode::Truecolor => 0,
        crate::session::config::ColorMode::Palette => 1,
    };
    let cm_inherited = inherited_if(
        cm_has_override,
        FieldValue::Select {
            selected: global_cm_selected,
            options: color_mode_options.clone(),
        },
    );

    let (idle_decay_minutes, idle_decay_override) = resolve_value(
        scope,
        global.theme.idle_decay_minutes,
        theme.and_then(|t| t.idle_decay_minutes),
    );

    vec![
        SettingField {
            key: FieldKey::ThemeName,
            label: "Theme",
            description: "Color theme for the TUI",
            value: FieldValue::Select { selected, options },
            category: SettingsCategory::Theme,
            has_override,
            inherited_display: inherited,
        },
        SettingField {
            key: FieldKey::ThemeColorMode,
            label: "Color Mode",
            description: "Truecolor (24-bit RGB) or palette (xterm-256). Use palette if your terminal mangles RGB escapes.",
            value: FieldValue::Select {
                selected: cm_selected,
                options: color_mode_options,
            },
            category: SettingsCategory::Theme,
            has_override: cm_has_override,
            inherited_display: cm_inherited,
        },
        SettingField {
            key: FieldKey::IdleDecayMinutes,
            label: "Idle Decay (minutes)",
            description: "Minutes a freshly-stopped Idle session keeps the fresh-idle tint and animated icon before fading to neutral. Also gates the `w` keybind. 0 disables.",
            value: FieldValue::Number(idle_decay_minutes),
            category: SettingsCategory::Theme,
            has_override: idle_decay_override,
            inherited_display: inherited_if(
                idle_decay_override,
                FieldValue::Number(global.theme.idle_decay_minutes),
            ),
        },
    ]
}

fn build_updates_fields(
    scope: SettingsScope,
    global: &Config,
    profile: &ProfileConfig,
) -> Vec<SettingField> {
    let updates = profile.updates.as_ref();

    let (check_enabled, o1) = resolve_value(
        scope,
        global.updates.check_enabled,
        updates.and_then(|u| u.check_enabled),
    );
    let (check_interval, o2) = resolve_value(
        scope,
        global.updates.check_interval_hours,
        updates.and_then(|u| u.check_interval_hours),
    );
    let (notify_in_cli, o3) = resolve_value(
        scope,
        global.updates.notify_in_cli,
        updates.and_then(|u| u.notify_in_cli),
    );

    vec![
        SettingField {
            key: FieldKey::CheckEnabled,
            label: "Check for Updates",
            description: "Automatically check for updates on startup",
            value: FieldValue::Bool(check_enabled),
            category: SettingsCategory::Updates,
            has_override: o1,
            inherited_display: inherited_if(o1, FieldValue::Bool(global.updates.check_enabled)),
        },
        SettingField {
            key: FieldKey::CheckIntervalHours,
            label: "Check Interval (hours)",
            description: "How often to check for updates",
            value: FieldValue::Number(check_interval),
            category: SettingsCategory::Updates,
            has_override: o2,
            inherited_display: inherited_if(
                o2,
                FieldValue::Number(global.updates.check_interval_hours),
            ),
        },
        SettingField {
            key: FieldKey::NotifyInCli,
            label: "Notify in CLI",
            description: "Show update notifications in CLI output",
            value: FieldValue::Bool(notify_in_cli),
            category: SettingsCategory::Updates,
            has_override: o3,
            inherited_display: inherited_if(o3, FieldValue::Bool(global.updates.notify_in_cli)),
        },
    ]
}

fn build_worktree_fields(
    scope: SettingsScope,
    global: &Config,
    profile: &ProfileConfig,
) -> Vec<SettingField> {
    let wt = profile.worktree.as_ref();

    let (path_template, o1) = resolve_value(
        scope,
        global.worktree.path_template.clone(),
        wt.and_then(|w| w.path_template.clone()),
    );
    let (bare_repo_template, o2) = resolve_value(
        scope,
        global.worktree.bare_repo_path_template.clone(),
        wt.and_then(|w| w.bare_repo_path_template.clone()),
    );
    let (auto_cleanup, o3) = resolve_value(
        scope,
        global.worktree.auto_cleanup,
        wt.and_then(|w| w.auto_cleanup),
    );
    let (delete_branch_on_cleanup, o4) = resolve_value(
        scope,
        global.worktree.delete_branch_on_cleanup,
        wt.and_then(|w| w.delete_branch_on_cleanup),
    );
    let (workspace_path_template, o5) = resolve_value(
        scope,
        global.worktree.workspace_path_template.clone(),
        wt.and_then(|w| w.workspace_path_template.clone()),
    );

    vec![
        SettingField {
            key: FieldKey::PathTemplate,
            label: "Path Template",
            description: "Template for worktree paths ({repo-name}, {branch})",
            value: FieldValue::Text(path_template),
            category: SettingsCategory::Worktree,
            has_override: o1,
            inherited_display: inherited_if(
                o1,
                FieldValue::Text(global.worktree.path_template.clone()),
            ),
        },
        SettingField {
            key: FieldKey::BareRepoPathTemplate,
            label: "Bare Repo Template",
            description: "Template for bare repo worktree paths",
            value: FieldValue::Text(bare_repo_template),
            category: SettingsCategory::Worktree,
            has_override: o2,
            inherited_display: inherited_if(
                o2,
                FieldValue::Text(global.worktree.bare_repo_path_template.clone()),
            ),
        },
        SettingField {
            key: FieldKey::WorktreeAutoCleanup,
            label: "Auto Cleanup",
            description: "Automatically clean up worktrees on session delete",
            value: FieldValue::Bool(auto_cleanup),
            category: SettingsCategory::Worktree,
            has_override: o3,
            inherited_display: inherited_if(o3, FieldValue::Bool(global.worktree.auto_cleanup)),
        },
        SettingField {
            key: FieldKey::DeleteBranchOnCleanup,
            label: "Delete Branch on Cleanup",
            description: "Also delete the git branch when deleting a worktree",
            value: FieldValue::Bool(delete_branch_on_cleanup),
            category: SettingsCategory::Worktree,
            has_override: o4,
            inherited_display: inherited_if(
                o4,
                FieldValue::Bool(global.worktree.delete_branch_on_cleanup),
            ),
        },
        SettingField {
            key: FieldKey::WorkspacePathTemplate,
            label: "Workspace Path Template",
            description: "Template for multi-repo workspace directories ({branch}, {session-id})",
            value: FieldValue::Text(workspace_path_template),
            category: SettingsCategory::Worktree,
            has_override: o5,
            inherited_display: inherited_if(
                o5,
                FieldValue::Text(global.worktree.workspace_path_template.clone()),
            ),
        },
    ]
}

fn build_sandbox_fields(
    scope: SettingsScope,
    global: &Config,
    profile: &ProfileConfig,
) -> Vec<SettingField> {
    let sb = profile.sandbox.as_ref();

    let (enabled_by_default, o1) = resolve_value(
        scope,
        global.sandbox.enabled_by_default,
        sb.and_then(|s| s.enabled_by_default),
    );
    let (default_image, o3) = resolve_value(
        scope,
        global.sandbox.default_image.clone(),
        sb.and_then(|s| s.default_image.clone()),
    );
    let (environment, o4) = resolve_value(
        scope,
        global.sandbox.environment.clone(),
        sb.and_then(|s| s.environment.clone()),
    );
    let (auto_cleanup, o5) = resolve_value(
        scope,
        global.sandbox.auto_cleanup,
        sb.and_then(|s| s.auto_cleanup),
    );
    let (cpu_limit, o_cpu) = resolve_optional(
        scope,
        global.sandbox.cpu_limit.clone(),
        sb.and_then(|s| s.cpu_limit.clone()),
        sb.map(|s| s.cpu_limit.is_some()).unwrap_or(false),
    );
    let (memory_limit, o_mem) = resolve_optional(
        scope,
        global.sandbox.memory_limit.clone(),
        sb.and_then(|s| s.memory_limit.clone()),
        sb.map(|s| s.memory_limit.is_some()).unwrap_or(false),
    );
    let (default_terminal_mode, o6) = resolve_value(
        scope,
        global.sandbox.default_terminal_mode,
        sb.and_then(|s| s.default_terminal_mode),
    );
    let (extra_volumes, o_ev) = resolve_value(
        scope,
        global.sandbox.extra_volumes.clone(),
        sb.and_then(|s| s.extra_volumes.clone()),
    );
    let (port_mappings, o_pm) = resolve_value(
        scope,
        global.sandbox.port_mappings.clone(),
        sb.and_then(|s| s.port_mappings.clone()),
    );
    let (volume_ignores, o7) = resolve_value(
        scope,
        global.sandbox.volume_ignores.clone(),
        sb.and_then(|s| s.volume_ignores.clone()),
    );
    let (mount_ssh, o8) = resolve_value(
        scope,
        global.sandbox.mount_ssh,
        sb.and_then(|s| s.mount_ssh),
    );
    let (custom_instruction, o_ci) = resolve_optional(
        scope,
        global.sandbox.custom_instruction.clone(),
        sb.and_then(|s| s.custom_instruction.clone()),
        sb.map(|s| s.custom_instruction.is_some()).unwrap_or(false),
    );
    let (container_runtime, o_cr) = resolve_value(
        scope,
        global.sandbox.container_runtime,
        sb.and_then(|s| s.container_runtime),
    );

    let terminal_mode_selected = match default_terminal_mode {
        DefaultTerminalMode::Host => 0,
        DefaultTerminalMode::Container => 1,
    };

    let container_runtime_selected = match container_runtime {
        ContainerRuntimeName::Docker => 0,
        ContainerRuntimeName::AppleContainer => 1,
    };

    let global_terminal_mode_selected = match global.sandbox.default_terminal_mode {
        DefaultTerminalMode::Host => 0,
        DefaultTerminalMode::Container => 1,
    };
    let terminal_mode_options = vec!["Host".into(), "Container".into()];

    let global_container_runtime_selected = match global.sandbox.container_runtime {
        ContainerRuntimeName::Docker => 0,
        ContainerRuntimeName::AppleContainer => 1,
    };
    let container_runtime_options = vec!["Docker".into(), "Apple Container".into()];

    vec![
        SettingField {
            key: FieldKey::SandboxEnabledByDefault,
            label: "Enabled by Default",
            description: "Enable sandbox mode by default for new sessions",
            value: FieldValue::Bool(enabled_by_default),
            category: SettingsCategory::Sandbox,
            has_override: o1,
            inherited_display: inherited_if(
                o1,
                FieldValue::Bool(global.sandbox.enabled_by_default),
            ),
        },
        SettingField {
            key: FieldKey::DefaultImage,
            label: "Default Image",
            description: "Docker image to use for sandboxes",
            value: FieldValue::Text(default_image),
            category: SettingsCategory::Sandbox,
            has_override: o3,
            inherited_display: inherited_if(
                o3,
                FieldValue::Text(global.sandbox.default_image.clone()),
            ),
        },
        SettingField {
            key: FieldKey::Environment,
            label: "Environment",
            description: "Env vars: bare KEY passes host value, KEY=VALUE sets explicitly",
            value: FieldValue::List(environment),
            category: SettingsCategory::Sandbox,
            has_override: o4,
            inherited_display: inherited_if(
                o4,
                FieldValue::List(global.sandbox.environment.clone()),
            ),
        },
        SettingField {
            key: FieldKey::SandboxAutoCleanup,
            label: "Auto Cleanup",
            description: "Remove containers when sessions are deleted",
            value: FieldValue::Bool(auto_cleanup),
            category: SettingsCategory::Sandbox,
            has_override: o5,
            inherited_display: inherited_if(o5, FieldValue::Bool(global.sandbox.auto_cleanup)),
        },
        SettingField {
            key: FieldKey::CpuLimit,
            label: "CPU Limit",
            description: "CPU limit for containers (e.g. \"4\")",
            value: FieldValue::OptionalText(cpu_limit),
            category: SettingsCategory::Sandbox,
            has_override: o_cpu,
            inherited_display: inherited_if(
                o_cpu,
                FieldValue::OptionalText(global.sandbox.cpu_limit.clone()),
            ),
        },
        SettingField {
            key: FieldKey::MemoryLimit,
            label: "Memory Limit",
            description: "Memory limit for containers (e.g. \"8g\", \"512m\")",
            value: FieldValue::OptionalText(memory_limit),
            category: SettingsCategory::Sandbox,
            has_override: o_mem,
            inherited_display: inherited_if(
                o_mem,
                FieldValue::OptionalText(global.sandbox.memory_limit.clone()),
            ),
        },
        SettingField {
            key: FieldKey::DefaultTerminalMode,
            label: "Default Terminal Mode",
            description: "Default terminal for sandboxed sessions (toggle with 'c' key)",
            value: FieldValue::Select {
                selected: terminal_mode_selected,
                options: terminal_mode_options.clone(),
            },
            category: SettingsCategory::Sandbox,
            has_override: o6,
            inherited_display: inherited_if(
                o6,
                FieldValue::Select {
                    selected: global_terminal_mode_selected,
                    options: terminal_mode_options,
                },
            ),
        },
        SettingField {
            key: FieldKey::ExtraVolumes,
            label: "Extra Volumes",
            description: "Additional volume mounts (host:container or host:container:ro)",
            value: FieldValue::List(extra_volumes),
            category: SettingsCategory::Sandbox,
            has_override: o_ev,
            inherited_display: inherited_if(
                o_ev,
                FieldValue::List(global.sandbox.extra_volumes.clone()),
            ),
        },
        SettingField {
            key: FieldKey::PortMappings,
            label: "Port Mappings",
            description: "Expose container ports to host (e.g. 3000:3000)",
            value: FieldValue::List(port_mappings),
            category: SettingsCategory::Sandbox,
            has_override: o_pm,
            inherited_display: inherited_if(
                o_pm,
                FieldValue::List(global.sandbox.port_mappings.clone()),
            ),
        },
        SettingField {
            key: FieldKey::VolumeIgnores,
            label: "Volume Ignores",
            description: "Directories to exclude from host mount (e.g. target, node_modules)",
            value: FieldValue::List(volume_ignores),
            category: SettingsCategory::Sandbox,
            has_override: o7,
            inherited_display: inherited_if(
                o7,
                FieldValue::List(global.sandbox.volume_ignores.clone()),
            ),
        },
        SettingField {
            key: FieldKey::MountSsh,
            label: "Mount SSH",
            description: "Mount ~/.ssh into sandbox containers (for git SSH access)",
            value: FieldValue::Bool(mount_ssh),
            category: SettingsCategory::Sandbox,
            has_override: o8,
            inherited_display: inherited_if(o8, FieldValue::Bool(global.sandbox.mount_ssh)),
        },
        SettingField {
            key: FieldKey::CustomInstruction,
            label: "Custom Instruction",
            description: "Custom instruction text appended to the agent's system prompt in sandboxed sessions (Claude, Codex only)",
            value: FieldValue::OptionalText(custom_instruction),
            category: SettingsCategory::Sandbox,
            has_override: o_ci,
            inherited_display: inherited_if(
                o_ci,
                FieldValue::OptionalText(global.sandbox.custom_instruction.clone()),
            ),
        },
        SettingField {
            key: FieldKey::ContainerRuntime,
            label: "Container Runtime",
            description: "Container runtime for sandboxing (Docker or Apple Container on macOS)",
            value: FieldValue::Select {
                selected: container_runtime_selected,
                options: container_runtime_options.clone(),
            },
            category: SettingsCategory::Sandbox,
            has_override: o_cr,
            inherited_display: inherited_if(
                o_cr,
                FieldValue::Select {
                    selected: global_container_runtime_selected,
                    options: container_runtime_options,
                },
            ),
        },
    ]
}

fn build_tmux_fields(
    scope: SettingsScope,
    global: &Config,
    profile: &ProfileConfig,
) -> Vec<SettingField> {
    let tmux = profile.tmux.as_ref();

    let (status_bar, status_bar_override) = resolve_value(
        scope,
        global.tmux.status_bar,
        tmux.and_then(|t| t.status_bar),
    );

    let (mouse, mouse_override) =
        resolve_value(scope, global.tmux.mouse, tmux.and_then(|t| t.mouse));

    let status_bar_selected = match status_bar {
        TmuxStatusBarMode::Auto => 0,
        TmuxStatusBarMode::Enabled => 1,
        TmuxStatusBarMode::Disabled => 2,
    };

    let mouse_selected = match mouse {
        TmuxMouseMode::Auto => 0,
        TmuxMouseMode::Enabled => 1,
        TmuxMouseMode::Disabled => 2,
    };

    let global_status_bar_selected = match global.tmux.status_bar {
        TmuxStatusBarMode::Auto => 0,
        TmuxStatusBarMode::Enabled => 1,
        TmuxStatusBarMode::Disabled => 2,
    };
    let global_mouse_selected = match global.tmux.mouse {
        TmuxMouseMode::Auto => 0,
        TmuxMouseMode::Enabled => 1,
        TmuxMouseMode::Disabled => 2,
    };
    let tmux_options = vec!["Auto".into(), "Enabled".into(), "Disabled".into()];

    vec![
        SettingField {
            key: FieldKey::StatusBar,
            label: "Status Bar",
            description: "Control tmux status bar styling (Auto respects your tmux config)",
            value: FieldValue::Select {
                selected: status_bar_selected,
                options: tmux_options.clone(),
            },
            category: SettingsCategory::Tmux,
            has_override: status_bar_override,
            inherited_display: inherited_if(
                status_bar_override,
                FieldValue::Select {
                    selected: global_status_bar_selected,
                    options: tmux_options.clone(),
                },
            ),
        },
        SettingField {
            key: FieldKey::Mouse,
            label: "Mouse Support",
            description: "Control mouse scrolling (Auto respects your tmux config)",
            value: FieldValue::Select {
                selected: mouse_selected,
                options: tmux_options.clone(),
            },
            category: SettingsCategory::Tmux,
            has_override: mouse_override,
            inherited_display: inherited_if(
                mouse_override,
                FieldValue::Select {
                    selected: global_mouse_selected,
                    options: tmux_options,
                },
            ),
        },
    ]
}

fn build_session_fields(
    scope: SettingsScope,
    global: &Config,
    profile: &ProfileConfig,
) -> Vec<SettingField> {
    let session = profile.session.as_ref();

    let (default_tool, has_override) = resolve_optional(
        scope,
        global.session.default_tool.clone(),
        session.and_then(|s| s.default_tool.clone()),
        session.map(|s| s.default_tool.is_some()).unwrap_or(false),
    );

    let selected = crate::agents::settings_index_from_name(default_tool.as_deref());

    let mut options = vec!["Auto (first available)".to_string()];
    options.extend(crate::agents::agent_names().iter().map(|n| n.to_string()));

    let (yolo_mode_default, yolo_override) = resolve_value(
        scope,
        global.session.yolo_mode_default,
        session.and_then(|s| s.yolo_mode_default),
    );

    let (strict_hotkeys, strict_hotkeys_override) = resolve_value(
        scope,
        global.session.strict_hotkeys,
        session.and_then(|s| s.strict_hotkeys),
    );

    let (agent_status_hooks, status_hooks_override) = resolve_value(
        scope,
        global.session.agent_status_hooks,
        session.and_then(|s| s.agent_status_hooks),
    );

    // Agent extra args: HashMap -> Vec<String> of "key=value" items for List field
    let (extra_args_map, extra_args_override) = resolve_value(
        scope,
        global.session.agent_extra_args.clone(),
        session.and_then(|s| s.agent_extra_args.clone()),
    );
    let extra_args_list: Vec<String> = {
        let mut items: Vec<_> = extra_args_map
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        items.sort();
        items
    };

    // Agent command override: HashMap -> Vec<String> of "key=value" items
    let (cmd_override_map, cmd_override_override) = resolve_value(
        scope,
        global.session.agent_command_override.clone(),
        session.and_then(|s| s.agent_command_override.clone()),
    );
    let cmd_override_list: Vec<String> = {
        let mut items: Vec<_> = cmd_override_map
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        items.sort();
        items
    };

    let global_tool_selected =
        crate::agents::settings_index_from_name(global.session.default_tool.as_deref());

    let global_extra_args_list: Vec<String> = {
        let mut items: Vec<_> = global
            .session
            .agent_extra_args
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        items.sort();
        items
    };
    let global_cmd_override_list: Vec<String> = {
        let mut items: Vec<_> = global
            .session
            .agent_command_override
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        items.sort();
        items
    };

    // Custom agents: HashMap -> Vec<String> of "name=command" items
    let (custom_agents_map, custom_agents_override) = resolve_value(
        scope,
        global.session.custom_agents.clone(),
        session.and_then(|s| s.custom_agents.clone()),
    );
    let custom_agents_list: Vec<String> = {
        let mut items: Vec<_> = custom_agents_map
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        items.sort();
        items
    };
    let global_custom_agents_list: Vec<String> = {
        let mut items: Vec<_> = global
            .session
            .custom_agents
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        items.sort();
        items
    };

    // Agent detect_as: HashMap -> Vec<String> of "name=builtin" items
    let (detect_as_map, detect_as_override) = resolve_value(
        scope,
        global.session.agent_detect_as.clone(),
        session.and_then(|s| s.agent_detect_as.clone()),
    );
    let detect_as_list: Vec<String> = {
        let mut items: Vec<_> = detect_as_map
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        items.sort();
        items
    };
    let global_detect_as_list: Vec<String> = {
        let mut items: Vec<_> = global
            .session
            .agent_detect_as
            .iter()
            .map(|(k, v)| format!("{}={}", k, v))
            .collect();
        items.sort();
        items
    };

    vec![
        SettingField {
            key: FieldKey::DefaultTool,
            label: "Default Tool",
            description: "Default coding tool for new sessions",
            value: FieldValue::Select {
                selected,
                options: options.clone(),
            },
            category: SettingsCategory::Session,
            has_override,
            inherited_display: inherited_if(
                has_override,
                FieldValue::Select {
                    selected: global_tool_selected,
                    options,
                },
            ),
        },
        SettingField {
            key: FieldKey::YoloModeDefault,
            label: "YOLO Mode Default",
            description: "Enable YOLO mode by default for new sessions",
            value: FieldValue::Bool(yolo_mode_default),
            category: SettingsCategory::Session,
            has_override: yolo_override,
            inherited_display: inherited_if(
                yolo_override,
                FieldValue::Bool(global.session.yolo_mode_default),
            ),
        },
        SettingField {
            key: FieldKey::StrictHotkeys,
            label: "Strict Hotkeys",
            description:
                "Require Shift/Ctrl for action hotkeys (guards against dictation/stray input)",
            value: FieldValue::Bool(strict_hotkeys),
            category: SettingsCategory::Session,
            has_override: strict_hotkeys_override,
            inherited_display: inherited_if(
                strict_hotkeys_override,
                FieldValue::Bool(global.session.strict_hotkeys),
            ),
        },
        SettingField {
            key: FieldKey::AgentExtraArgs,
            label: "Agent Extra Args",
            description:
                "Per-agent extra arguments appended after the binary (e.g. opencode=--port 8080)",
            value: FieldValue::List(extra_args_list),
            category: SettingsCategory::Session,
            has_override: extra_args_override,
            inherited_display: inherited_if(
                extra_args_override,
                FieldValue::List(global_extra_args_list),
            ),
        },
        SettingField {
            key: FieldKey::AgentCommandOverride,
            label: "Agent Command Override",
            description: "Per-agent command override replacing the binary (e.g. claude=my-wrapper)",
            value: FieldValue::List(cmd_override_list),
            category: SettingsCategory::Session,
            has_override: cmd_override_override,
            inherited_display: inherited_if(
                cmd_override_override,
                FieldValue::List(global_cmd_override_list),
            ),
        },
        SettingField {
            key: FieldKey::CustomAgents,
            label: "Custom Agents",
            description:
                "User-defined agents: name=command (e.g. lenovo-claude=ssh -t lenovo claude)",
            value: FieldValue::List(custom_agents_list),
            category: SettingsCategory::Session,
            has_override: custom_agents_override,
            inherited_display: inherited_if(
                custom_agents_override,
                FieldValue::List(global_custom_agents_list),
            ),
        },
        SettingField {
            key: FieldKey::AgentDetectAs,
            label: "Agent Detect As",
            description: "Status detection mapping: agent=builtin (e.g. lenovo-claude=claude)",
            value: FieldValue::List(detect_as_list),
            category: SettingsCategory::Session,
            has_override: detect_as_override,
            inherited_display: inherited_if(
                detect_as_override,
                FieldValue::List(global_detect_as_list),
            ),
        },
        SettingField {
            key: FieldKey::AgentStatusHooks,
            label: "Agent Status Hooks",
            description: "Install status-detection hooks into the agent's settings file",
            value: FieldValue::Bool(agent_status_hooks),
            category: SettingsCategory::Session,
            has_override: status_hooks_override,
            inherited_display: inherited_if(
                status_hooks_override,
                FieldValue::Bool(global.session.agent_status_hooks),
            ),
        },
    ]
}

fn build_sound_fields(
    scope: SettingsScope,
    global: &Config,
    profile: &ProfileConfig,
) -> Vec<SettingField> {
    let snd = profile.sound.as_ref();

    let (enabled, o1) = resolve_value(scope, global.sound.enabled, snd.and_then(|s| s.enabled));

    let (mode, o2) = resolve_value(
        scope,
        global.sound.mode.clone(),
        snd.and_then(|s| s.mode.clone()),
    );

    let mode_selected = match &mode {
        SoundMode::Random => 0,
        SoundMode::Specific(_) => 1,
    };

    let (on_start, o3) = resolve_optional(
        scope,
        global.sound.on_start.clone(),
        snd.and_then(|s| s.on_start.clone()),
        snd.map(|s| s.on_start.is_some()).unwrap_or(false),
    );
    let (on_running, o4) = resolve_optional(
        scope,
        global.sound.on_running.clone(),
        snd.and_then(|s| s.on_running.clone()),
        snd.map(|s| s.on_running.is_some()).unwrap_or(false),
    );
    let (on_waiting, o5) = resolve_optional(
        scope,
        global.sound.on_waiting.clone(),
        snd.and_then(|s| s.on_waiting.clone()),
        snd.map(|s| s.on_waiting.is_some()).unwrap_or(false),
    );
    let (on_idle, o6) = resolve_optional(
        scope,
        global.sound.on_idle.clone(),
        snd.and_then(|s| s.on_idle.clone()),
        snd.map(|s| s.on_idle.is_some()).unwrap_or(false),
    );
    let (on_error, o7) = resolve_optional(
        scope,
        global.sound.on_error.clone(),
        snd.and_then(|s| s.on_error.clone()),
        snd.map(|s| s.on_error.is_some()).unwrap_or(false),
    );

    let global_mode_selected = match &global.sound.mode {
        SoundMode::Random => 0,
        SoundMode::Specific(_) => 1,
    };
    let sound_mode_options = vec!["Random".into(), "Specific".into()];

    let (volume, o_vol) = resolve_value(scope, global.sound.volume, snd.and_then(|s| s.volume));
    let vol_opts = volume_options();
    let vol_idx = volume_to_index(volume);

    vec![
        SettingField {
            key: FieldKey::SoundEnabled,
            label: "Enabled",
            description: "Play sounds on agent state transitions",
            value: FieldValue::Bool(enabled),
            category: SettingsCategory::Sound,
            has_override: o1,
            inherited_display: inherited_if(o1, FieldValue::Bool(global.sound.enabled)),
        },
        SettingField {
            key: FieldKey::SoundMode,
            label: "Mode",
            description: "How to select sounds (Random or Specific file name)",
            value: FieldValue::Select {
                selected: mode_selected,
                options: sound_mode_options.clone(),
            },
            category: SettingsCategory::Sound,
            has_override: o2,
            inherited_display: inherited_if(
                o2,
                FieldValue::Select {
                    selected: global_mode_selected,
                    options: sound_mode_options,
                },
            ),
        },
        SettingField {
            key: FieldKey::SoundVolume,
            label: "Volume",
            description: "Playback volume (0.1 = min, 1.0 = normal, 1.5 = max), step 0.1. Ignored when aplay is the Linux backend.",
            value: FieldValue::Select {
                selected: vol_idx,
                options: vol_opts.clone(),
            },
            category: SettingsCategory::Sound,
            has_override: o_vol,
            inherited_display: inherited_if(
                o_vol,
                FieldValue::Select {
                    selected: volume_to_index(global.sound.volume),
                    options: vol_opts,
                },
            ),
        },
        SettingField {
            key: FieldKey::SoundOnStart,
            label: "On Start",
            description: "Specify file name with extension",
            value: FieldValue::OptionalText(on_start),
            category: SettingsCategory::Sound,
            has_override: o3,
            inherited_display: inherited_if(
                o3,
                FieldValue::OptionalText(global.sound.on_start.clone()),
            ),
        },
        SettingField {
            key: FieldKey::SoundOnRunning,
            label: "On Running",
            description: "Specify file name with extension",
            value: FieldValue::OptionalText(on_running),
            category: SettingsCategory::Sound,
            has_override: o4,
            inherited_display: inherited_if(
                o4,
                FieldValue::OptionalText(global.sound.on_running.clone()),
            ),
        },
        SettingField {
            key: FieldKey::SoundOnWaiting,
            label: "On Waiting",
            description: "Specify file name with extension",
            value: FieldValue::OptionalText(on_waiting),
            category: SettingsCategory::Sound,
            has_override: o5,
            inherited_display: inherited_if(
                o5,
                FieldValue::OptionalText(global.sound.on_waiting.clone()),
            ),
        },
        SettingField {
            key: FieldKey::SoundOnIdle,
            label: "On Idle",
            description: "Specify file name with extension",
            value: FieldValue::OptionalText(on_idle),
            category: SettingsCategory::Sound,
            has_override: o6,
            inherited_display: inherited_if(
                o6,
                FieldValue::OptionalText(global.sound.on_idle.clone()),
            ),
        },
        SettingField {
            key: FieldKey::SoundOnError,
            label: "On Error",
            description: "Specify file name with extension",
            value: FieldValue::OptionalText(on_error),
            category: SettingsCategory::Sound,
            has_override: o7,
            inherited_display: inherited_if(
                o7,
                FieldValue::OptionalText(global.sound.on_error.clone()),
            ),
        },
    ]
}

fn build_hooks_fields(
    scope: SettingsScope,
    global: &Config,
    profile: &ProfileConfig,
) -> Vec<SettingField> {
    let hooks = profile.hooks.as_ref();

    let (on_create, o1) = resolve_value(
        scope,
        global.hooks.on_create.clone(),
        hooks.and_then(|h| h.on_create.clone()),
    );
    let (on_launch, o2) = resolve_value(
        scope,
        global.hooks.on_launch.clone(),
        hooks.and_then(|h| h.on_launch.clone()),
    );
    let (on_destroy, o3) = resolve_value(
        scope,
        global.hooks.on_destroy.clone(),
        hooks.and_then(|h| h.on_destroy.clone()),
    );

    vec![
        SettingField {
            key: FieldKey::HookOnCreate,
            label: "On Create",
            description: "Commands run once when a session is first created. Runs inside sandbox when enabled.",
            value: FieldValue::List(on_create),
            category: SettingsCategory::Hooks,
            has_override: o1,
            inherited_display: inherited_if(
                o1,
                FieldValue::List(global.hooks.on_create.clone()),
            ),
        },
        SettingField {
            key: FieldKey::HookOnLaunch,
            label: "On Launch",
            description: "Commands run every time a session starts. Runs inside sandbox when enabled.",
            value: FieldValue::List(on_launch),
            category: SettingsCategory::Hooks,
            has_override: o2,
            inherited_display: inherited_if(
                o2,
                FieldValue::List(global.hooks.on_launch.clone()),
            ),
        },
        SettingField {
            key: FieldKey::HookOnDestroy,
            label: "On Destroy",
            description: "Commands run when a session is deleted, before cleanup. Use for teardown (e.g. docker-compose down).",
            value: FieldValue::List(on_destroy),
            category: SettingsCategory::Hooks,
            has_override: o3,
            inherited_display: inherited_if(
                o3,
                FieldValue::List(global.hooks.on_destroy.clone()),
            ),
        },
    ]
}

/// Apply a field's value back to the appropriate config.
/// For profile scope, the value is always stored as an override.
pub fn apply_field_to_config(
    field: &SettingField,
    scope: SettingsScope,
    global: &mut Config,
    profile: &mut ProfileConfig,
) {
    match scope {
        SettingsScope::Global => apply_field_to_global(field, global),
        SettingsScope::Profile | SettingsScope::Repo => {
            apply_field_to_profile(field, global, profile)
        }
    }
}

fn apply_field_to_global(field: &SettingField, config: &mut Config) {
    match (&field.key, &field.value) {
        // Theme
        (FieldKey::ThemeName, FieldValue::Select { selected, options }) => {
            config.theme.name = options.get(*selected).cloned().unwrap_or_default();
        }
        (FieldKey::ThemeColorMode, FieldValue::Select { selected, .. }) => {
            config.theme.color_mode = match selected {
                1 => crate::session::config::ColorMode::Palette,
                _ => crate::session::config::ColorMode::Truecolor,
            };
        }
        (FieldKey::IdleDecayMinutes, FieldValue::Number(v)) => {
            config.theme.idle_decay_minutes = *v;
        }
        // Updates
        (FieldKey::CheckEnabled, FieldValue::Bool(v)) => config.updates.check_enabled = *v,
        (FieldKey::CheckIntervalHours, FieldValue::Number(v)) => {
            config.updates.check_interval_hours = *v
        }
        (FieldKey::NotifyInCli, FieldValue::Bool(v)) => config.updates.notify_in_cli = *v,
        // Worktree
        (FieldKey::PathTemplate, FieldValue::Text(v)) => config.worktree.path_template = v.clone(),
        (FieldKey::BareRepoPathTemplate, FieldValue::Text(v)) => {
            config.worktree.bare_repo_path_template = v.clone()
        }
        (FieldKey::WorktreeAutoCleanup, FieldValue::Bool(v)) => config.worktree.auto_cleanup = *v,
        (FieldKey::DeleteBranchOnCleanup, FieldValue::Bool(v)) => {
            config.worktree.delete_branch_on_cleanup = *v
        }
        (FieldKey::WorkspacePathTemplate, FieldValue::Text(v)) => {
            config.worktree.workspace_path_template = v.clone()
        }
        // Sandbox
        (FieldKey::SandboxEnabledByDefault, FieldValue::Bool(v)) => {
            config.sandbox.enabled_by_default = *v
        }
        (FieldKey::YoloModeDefault, FieldValue::Bool(v)) => config.session.yolo_mode_default = *v,
        (FieldKey::StrictHotkeys, FieldValue::Bool(v)) => config.session.strict_hotkeys = *v,
        (FieldKey::AgentStatusHooks, FieldValue::Bool(v)) => {
            config.session.agent_status_hooks = *v;
        }
        (FieldKey::DefaultImage, FieldValue::Text(v)) => config.sandbox.default_image = v.clone(),
        (FieldKey::Environment, FieldValue::List(v)) => config.sandbox.environment = v.clone(),
        (FieldKey::ExtraVolumes, FieldValue::List(v)) => config.sandbox.extra_volumes = v.clone(),
        (FieldKey::PortMappings, FieldValue::List(v)) => config.sandbox.port_mappings = v.clone(),
        (FieldKey::VolumeIgnores, FieldValue::List(v)) => config.sandbox.volume_ignores = v.clone(),
        (FieldKey::MountSsh, FieldValue::Bool(v)) => config.sandbox.mount_ssh = *v,
        (FieldKey::SandboxAutoCleanup, FieldValue::Bool(v)) => config.sandbox.auto_cleanup = *v,
        (FieldKey::CpuLimit, FieldValue::OptionalText(v)) => {
            config.sandbox.cpu_limit = v.clone();
        }
        (FieldKey::MemoryLimit, FieldValue::OptionalText(v)) => {
            config.sandbox.memory_limit = v.clone();
        }
        (FieldKey::CustomInstruction, FieldValue::OptionalText(v)) => {
            config.sandbox.custom_instruction = v.clone();
        }
        (FieldKey::DefaultTerminalMode, FieldValue::Select { selected, .. }) => {
            config.sandbox.default_terminal_mode = match selected {
                0 => DefaultTerminalMode::Host,
                _ => DefaultTerminalMode::Container,
            };
        }
        (FieldKey::ContainerRuntime, FieldValue::Select { selected, .. }) => {
            config.sandbox.container_runtime = match selected {
                0 => ContainerRuntimeName::Docker,
                _ => ContainerRuntimeName::AppleContainer,
            };
        }
        // Tmux
        (FieldKey::StatusBar, FieldValue::Select { selected, .. }) => {
            config.tmux.status_bar = match selected {
                0 => TmuxStatusBarMode::Auto,
                1 => TmuxStatusBarMode::Enabled,
                _ => TmuxStatusBarMode::Disabled,
            };
        }
        (FieldKey::Mouse, FieldValue::Select { selected, .. }) => {
            config.tmux.mouse = match selected {
                0 => TmuxMouseMode::Auto,
                1 => TmuxMouseMode::Enabled,
                _ => TmuxMouseMode::Disabled,
            };
        }
        // Session
        (FieldKey::DefaultTool, FieldValue::Select { selected, .. }) => {
            config.session.default_tool =
                crate::agents::name_from_settings_index(*selected).map(|s| s.to_string());
        }
        (FieldKey::AgentExtraArgs, FieldValue::List(v)) => {
            config.session.agent_extra_args = parse_key_value_list(v);
        }
        (FieldKey::AgentCommandOverride, FieldValue::List(v)) => {
            config.session.agent_command_override = parse_key_value_list(v);
        }
        (FieldKey::CustomAgents, FieldValue::List(v)) => {
            config.session.custom_agents = parse_key_value_list(v);
        }
        (FieldKey::AgentDetectAs, FieldValue::List(v)) => {
            config.session.agent_detect_as = parse_key_value_list(v);
        }
        // Sound
        (FieldKey::SoundEnabled, FieldValue::Bool(v)) => config.sound.enabled = *v,
        (FieldKey::SoundMode, FieldValue::Select { selected, .. }) => {
            config.sound.mode = match selected {
                1 => SoundMode::Specific(String::new()),
                _ => SoundMode::Random,
            };
        }
        (FieldKey::SoundVolume, FieldValue::Select { selected, options }) => {
            if let Some(s) = options.get(*selected) {
                config.sound.volume = volume_from_option(s);
            }
        }
        (FieldKey::SoundOnStart, FieldValue::OptionalText(v)) => {
            config.sound.on_start = v.clone();
        }
        (FieldKey::SoundOnRunning, FieldValue::OptionalText(v)) => {
            config.sound.on_running = v.clone();
        }
        (FieldKey::SoundOnWaiting, FieldValue::OptionalText(v)) => {
            config.sound.on_waiting = v.clone();
        }
        (FieldKey::SoundOnIdle, FieldValue::OptionalText(v)) => {
            config.sound.on_idle = v.clone();
        }
        (FieldKey::SoundOnError, FieldValue::OptionalText(v)) => {
            config.sound.on_error = v.clone();
        }
        // Hooks
        (FieldKey::HookOnCreate, FieldValue::List(v)) => config.hooks.on_create = v.clone(),
        (FieldKey::HookOnLaunch, FieldValue::List(v)) => config.hooks.on_launch = v.clone(),
        (FieldKey::HookOnDestroy, FieldValue::List(v)) => config.hooks.on_destroy = v.clone(),
        // Web
        (FieldKey::WebNotificationsEnabled, FieldValue::Bool(v)) => {
            config.web.notifications_enabled = *v;
        }
        (FieldKey::WebNotifyOnWaiting, FieldValue::Bool(v)) => {
            config.web.notify_on_waiting = *v;
        }
        (FieldKey::WebNotifyOnIdle, FieldValue::Bool(v)) => {
            config.web.notify_on_idle = *v;
        }
        (FieldKey::WebNotifyOnError, FieldValue::Bool(v)) => {
            config.web.notify_on_error = *v;
        }
        _ => {}
    }
}

/// Apply a field to the profile config.
/// Always stores the value as an override; use 'r' key to clear overrides.
fn apply_field_to_profile(field: &SettingField, _global: &Config, config: &mut ProfileConfig) {
    match (&field.key, &field.value) {
        // Theme
        (FieldKey::ThemeName, FieldValue::Select { selected, options }) => {
            let name = options.get(*selected).cloned().unwrap_or_default();
            use crate::session::ThemeConfigOverride;
            let t = config
                .theme
                .get_or_insert_with(ThemeConfigOverride::default);
            t.name = Some(name);
        }
        (FieldKey::ThemeColorMode, FieldValue::Select { selected, .. }) => {
            use crate::session::ThemeConfigOverride;
            let t = config
                .theme
                .get_or_insert_with(ThemeConfigOverride::default);
            t.color_mode = Some(match selected {
                1 => crate::session::config::ColorMode::Palette,
                _ => crate::session::config::ColorMode::Truecolor,
            });
        }
        (FieldKey::IdleDecayMinutes, FieldValue::Number(v)) => {
            use crate::session::ThemeConfigOverride;
            let t = config
                .theme
                .get_or_insert_with(ThemeConfigOverride::default);
            t.idle_decay_minutes = Some(*v);
        }
        // Updates
        (FieldKey::CheckEnabled, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.updates, |s, val| s.check_enabled = val);
        }
        (FieldKey::CheckIntervalHours, FieldValue::Number(v)) => {
            set_profile_override(*v, &mut config.updates, |s, val| {
                s.check_interval_hours = val
            });
        }
        (FieldKey::NotifyInCli, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.updates, |s, val| s.notify_in_cli = val);
        }
        // Worktree
        (FieldKey::PathTemplate, FieldValue::Text(v)) => {
            set_profile_override(v.clone(), &mut config.worktree, |s, val| {
                s.path_template = val
            });
        }
        (FieldKey::BareRepoPathTemplate, FieldValue::Text(v)) => {
            set_profile_override(v.clone(), &mut config.worktree, |s, val| {
                s.bare_repo_path_template = val
            });
        }
        (FieldKey::WorktreeAutoCleanup, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.worktree, |s, val| s.auto_cleanup = val);
        }
        (FieldKey::DeleteBranchOnCleanup, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.worktree, |s, val| {
                s.delete_branch_on_cleanup = val
            });
        }
        (FieldKey::WorkspacePathTemplate, FieldValue::Text(v)) => {
            set_profile_override(v.clone(), &mut config.worktree, |s, val| {
                s.workspace_path_template = val
            });
        }
        // Sandbox
        (FieldKey::SandboxEnabledByDefault, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.sandbox, |s, val| s.enabled_by_default = val);
        }
        (FieldKey::DefaultImage, FieldValue::Text(v)) => {
            set_profile_override(v.clone(), &mut config.sandbox, |s, val| {
                s.default_image = val
            });
        }
        (FieldKey::Environment, FieldValue::List(v)) => {
            set_profile_override(v.clone(), &mut config.sandbox, |s, val| s.environment = val);
        }
        (FieldKey::ExtraVolumes, FieldValue::List(v)) => {
            set_profile_override(v.clone(), &mut config.sandbox, |s, val| {
                s.extra_volumes = val
            });
        }
        (FieldKey::PortMappings, FieldValue::List(v)) => {
            set_profile_override(v.clone(), &mut config.sandbox, |s, val| {
                s.port_mappings = val
            });
        }
        (FieldKey::VolumeIgnores, FieldValue::List(v)) => {
            set_profile_override(v.clone(), &mut config.sandbox, |s, val| {
                s.volume_ignores = val
            });
        }
        (FieldKey::MountSsh, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.sandbox, |s, val| s.mount_ssh = val);
        }
        (FieldKey::SandboxAutoCleanup, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.sandbox, |s, val| s.auto_cleanup = val);
        }
        (FieldKey::CpuLimit, FieldValue::OptionalText(v)) => {
            use crate::session::SandboxConfigOverride;
            let s = config
                .sandbox
                .get_or_insert_with(SandboxConfigOverride::default);
            s.cpu_limit = v.clone();
        }
        (FieldKey::MemoryLimit, FieldValue::OptionalText(v)) => {
            use crate::session::SandboxConfigOverride;
            let s = config
                .sandbox
                .get_or_insert_with(SandboxConfigOverride::default);
            s.memory_limit = v.clone();
        }
        (FieldKey::CustomInstruction, FieldValue::OptionalText(v)) => {
            use crate::session::SandboxConfigOverride;
            let s = config
                .sandbox
                .get_or_insert_with(SandboxConfigOverride::default);
            s.custom_instruction = v.clone();
        }
        (FieldKey::DefaultTerminalMode, FieldValue::Select { selected, .. }) => {
            let mode = match selected {
                0 => DefaultTerminalMode::Host,
                _ => DefaultTerminalMode::Container,
            };
            set_profile_override(mode, &mut config.sandbox, |s, val| {
                s.default_terminal_mode = val
            });
        }
        (FieldKey::ContainerRuntime, FieldValue::Select { selected, .. }) => {
            let runtime = match selected {
                0 => ContainerRuntimeName::Docker,
                _ => ContainerRuntimeName::AppleContainer,
            };
            set_profile_override(runtime, &mut config.sandbox, |s, val| {
                s.container_runtime = val
            });
        }
        // Tmux
        (FieldKey::StatusBar, FieldValue::Select { selected, .. }) => {
            let mode = match selected {
                0 => TmuxStatusBarMode::Auto,
                1 => TmuxStatusBarMode::Enabled,
                _ => TmuxStatusBarMode::Disabled,
            };
            set_profile_override(mode, &mut config.tmux, |s, val| s.status_bar = val);
        }
        (FieldKey::Mouse, FieldValue::Select { selected, .. }) => {
            let mode = match selected {
                0 => TmuxMouseMode::Auto,
                1 => TmuxMouseMode::Enabled,
                _ => TmuxMouseMode::Disabled,
            };
            set_profile_override(mode, &mut config.tmux, |s, val| s.mouse = val);
        }
        // Session
        (FieldKey::DefaultTool, FieldValue::Select { selected, .. }) => {
            let tool = crate::agents::name_from_settings_index(*selected).map(|s| s.to_string());
            use crate::session::SessionConfigOverride;
            let session = config
                .session
                .get_or_insert_with(SessionConfigOverride::default);
            session.default_tool = tool;
        }
        (FieldKey::YoloModeDefault, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.session, |s, val| s.yolo_mode_default = val);
        }
        (FieldKey::StrictHotkeys, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.session, |s, val| s.strict_hotkeys = val);
        }
        (FieldKey::AgentStatusHooks, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.session, |s, val| {
                s.agent_status_hooks = val;
            });
        }
        (FieldKey::AgentExtraArgs, FieldValue::List(v)) => {
            let map = parse_key_value_list(v);
            use crate::session::SessionConfigOverride;
            let s = config
                .session
                .get_or_insert_with(SessionConfigOverride::default);
            s.agent_extra_args = Some(map);
        }
        (FieldKey::AgentCommandOverride, FieldValue::List(v)) => {
            let map = parse_key_value_list(v);
            use crate::session::SessionConfigOverride;
            let s = config
                .session
                .get_or_insert_with(SessionConfigOverride::default);
            s.agent_command_override = Some(map);
        }
        (FieldKey::CustomAgents, FieldValue::List(v)) => {
            let map = parse_key_value_list(v);
            use crate::session::SessionConfigOverride;
            let s = config
                .session
                .get_or_insert_with(SessionConfigOverride::default);
            s.custom_agents = Some(map);
        }
        (FieldKey::AgentDetectAs, FieldValue::List(v)) => {
            let map = parse_key_value_list(v);
            use crate::session::SessionConfigOverride;
            let s = config
                .session
                .get_or_insert_with(SessionConfigOverride::default);
            s.agent_detect_as = Some(map);
        }
        // Sound
        (FieldKey::SoundEnabled, FieldValue::Bool(v)) => {
            set_profile_override(*v, &mut config.sound, |s, val| s.enabled = val);
        }
        (FieldKey::SoundMode, FieldValue::Select { selected, .. }) => {
            let mode = match selected {
                1 => SoundMode::Specific(String::new()),
                _ => SoundMode::Random,
            };
            set_profile_override(mode, &mut config.sound, |s, val| s.mode = val);
        }
        (FieldKey::SoundVolume, FieldValue::Select { selected, options }) => {
            if let Some(s) = options.get(*selected) {
                let vol = volume_from_option(s);
                set_profile_override(vol, &mut config.sound, |s, val| s.volume = val);
            }
        }
        (FieldKey::SoundOnStart, FieldValue::OptionalText(v)) => {
            let s = config
                .sound
                .get_or_insert_with(crate::sound::SoundConfigOverride::default);
            s.on_start = v.clone();
        }
        (FieldKey::SoundOnRunning, FieldValue::OptionalText(v)) => {
            let s = config
                .sound
                .get_or_insert_with(crate::sound::SoundConfigOverride::default);
            s.on_running = v.clone();
        }
        (FieldKey::SoundOnWaiting, FieldValue::OptionalText(v)) => {
            let s = config
                .sound
                .get_or_insert_with(crate::sound::SoundConfigOverride::default);
            s.on_waiting = v.clone();
        }
        (FieldKey::SoundOnIdle, FieldValue::OptionalText(v)) => {
            let s = config
                .sound
                .get_or_insert_with(crate::sound::SoundConfigOverride::default);
            s.on_idle = v.clone();
        }
        (FieldKey::SoundOnError, FieldValue::OptionalText(v)) => {
            let s = config
                .sound
                .get_or_insert_with(crate::sound::SoundConfigOverride::default);
            s.on_error = v.clone();
        }
        // Hooks
        (FieldKey::HookOnCreate, FieldValue::List(v)) => {
            set_profile_override(v.clone(), &mut config.hooks, |s, val| s.on_create = val);
        }
        (FieldKey::HookOnLaunch, FieldValue::List(v)) => {
            set_profile_override(v.clone(), &mut config.hooks, |s, val| s.on_launch = val);
        }
        (FieldKey::HookOnDestroy, FieldValue::List(v)) => {
            set_profile_override(v.clone(), &mut config.hooks, |s, val| s.on_destroy = val);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::{Config, ProfileConfig};

    #[test]
    fn test_profile_field_has_no_override_after_global_change() {
        // Start with default configs
        let mut global = Config::default();
        let profile = ProfileConfig::default();

        // Verify initial state - profile shows no override
        let fields = build_fields_for_category(
            SettingsCategory::Updates,
            SettingsScope::Profile,
            &global,
            &profile,
        );

        let check_enabled_field = fields
            .iter()
            .find(|f| f.key == FieldKey::CheckEnabled)
            .unwrap();
        assert!(
            !check_enabled_field.has_override,
            "Profile should not show override initially"
        );

        // Change global setting
        global.updates.check_enabled = !global.updates.check_enabled;

        // Rebuild profile fields - should still show no override
        let fields = build_fields_for_category(
            SettingsCategory::Updates,
            SettingsScope::Profile,
            &global,
            &profile,
        );

        let check_enabled_field = fields
            .iter()
            .find(|f| f.key == FieldKey::CheckEnabled)
            .unwrap();
        assert!(
            !check_enabled_field.has_override,
            "Profile should NOT show override after global change - it should inherit"
        );
    }

    #[test]
    fn test_profile_field_shows_override_after_profile_change() {
        let global = Config::default();
        let mut profile = ProfileConfig::default();

        // Initially no override
        let fields = build_fields_for_category(
            SettingsCategory::Updates,
            SettingsScope::Profile,
            &global,
            &profile,
        );
        let check_enabled_field = fields
            .iter()
            .find(|f| f.key == FieldKey::CheckEnabled)
            .unwrap();
        assert!(!check_enabled_field.has_override);

        // Set a profile override
        profile.updates = Some(crate::session::UpdatesConfigOverride {
            check_enabled: Some(false),
            ..Default::default()
        });

        // Rebuild - should now show override
        let fields = build_fields_for_category(
            SettingsCategory::Updates,
            SettingsScope::Profile,
            &global,
            &profile,
        );
        let check_enabled_field = fields
            .iter()
            .find(|f| f.key == FieldKey::CheckEnabled)
            .unwrap();
        assert!(
            check_enabled_field.has_override,
            "Profile SHOULD show override after explicit profile change"
        );
    }

    #[test]
    fn test_default_tool_options_include_all_registered_agents() {
        let global = Config::default();
        let profile = ProfileConfig::default();

        let fields = build_fields_for_category(
            SettingsCategory::Session,
            SettingsScope::Global,
            &global,
            &profile,
        );

        let tool_field = fields
            .iter()
            .find(|f| f.key == FieldKey::DefaultTool)
            .expect("DefaultTool field should exist");

        let options = match &tool_field.value {
            FieldValue::Select { options, .. } => options,
            _ => panic!("DefaultTool should be a Select field"),
        };

        let tool_options: Vec<&str> = options.iter().skip(1).map(|s| s.as_str()).collect();
        let agent_names = crate::agents::agent_names();

        for name in &agent_names {
            assert!(
                tool_options.contains(name),
                "Settings UI missing agent '{}'. UI options: {:?}",
                name,
                tool_options
            );
        }

        for option in &tool_options {
            assert!(
                agent_names.contains(option),
                "Settings UI has unknown agent '{}' not in registry.",
                option
            );
        }
    }

    #[test]
    fn test_profile_override_preserved_when_matching_global() {
        let global = Config::default();
        let mut profile = ProfileConfig::default();

        // Set a profile override that matches the global value
        let global_check_enabled = global.updates.check_enabled;
        profile.updates = Some(crate::session::UpdatesConfigOverride {
            check_enabled: Some(global_check_enabled),
            ..Default::default()
        });

        // Apply the same value through the field system
        let fields = build_fields_for_category(
            SettingsCategory::Updates,
            SettingsScope::Profile,
            &global,
            &profile,
        );
        let field = fields
            .iter()
            .find(|f| f.key == FieldKey::CheckEnabled)
            .unwrap();

        // Re-apply the field (simulates user saving without changing the value)
        apply_field_to_profile(field, &global, &mut profile);

        // The override should still be present
        assert!(
            profile
                .updates
                .as_ref()
                .and_then(|u| u.check_enabled)
                .is_some(),
            "Profile override should be preserved even when value matches global"
        );
    }

    #[test]
    fn test_bool_toggle_back_to_global_preserves_override() {
        let global = Config::default();
        let mut profile = ProfileConfig::default();
        let original = global.updates.check_enabled;

        // Toggle to non-global value
        profile.updates = Some(crate::session::UpdatesConfigOverride {
            check_enabled: Some(!original),
            ..Default::default()
        });

        // Now toggle back to match global
        let field = SettingField {
            key: FieldKey::CheckEnabled,
            label: "Check Enabled",
            description: "",
            value: FieldValue::Bool(original),
            category: SettingsCategory::Updates,
            has_override: true,
            inherited_display: None,
        };

        apply_field_to_profile(&field, &global, &mut profile);

        // Override should still be present (not silently cleared)
        assert!(
            profile
                .updates
                .as_ref()
                .and_then(|u| u.check_enabled)
                .is_some(),
            "Toggling back to match global should preserve the override, not silently clear it"
        );
        assert_eq!(
            profile.updates.as_ref().unwrap().check_enabled,
            Some(original),
            "Override value should match what was set"
        );
    }
}
