//! Setting field definitions and config mapping

use std::collections::HashMap;

use crate::session::{
    validate_check_interval, Config, DefaultTerminalMode, ProfileConfig, TmuxMouseMode,
    TmuxStatusBarMode,
};
use crate::sound::{validate_sound_exists, SoundMode};

use super::SettingsScope;

/// Categories of settings
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsCategory {
    Updates,
    Worktree,
    Sandbox,
    Tmux,
    Session,
    Sound,
    Hooks,
}

impl SettingsCategory {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Updates => "Updates",
            Self::Worktree => "Worktree",
            Self::Sandbox => "Sandbox",
            Self::Tmux => "Tmux",
            Self::Session => "Session",
            Self::Sound => "Sound",
            Self::Hooks => "Hooks",
        }
    }
}

/// Type-safe field identifiers (prevents typos in string matching)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldKey {
    // Updates
    CheckEnabled,
    CheckIntervalHours,
    NotifyInCli,
    // Worktree
    PathTemplate,
    BareRepoPathTemplate,
    WorktreeAutoCleanup,
    DeleteBranchOnCleanup,
    // Sandbox
    SandboxEnabledByDefault,
    YoloModeDefault,
    DefaultImage,
    Environment,
    EnvironmentValues,
    SandboxAutoCleanup,
    CpuLimit,
    MemoryLimit,
    DefaultTerminalMode,
    ExtraVolumes,
    VolumeIgnores,
    MountSsh,
    // Tmux
    StatusBar,
    Mouse,
    // Session
    DefaultTool,
    // Sound
    SoundEnabled,
    SoundMode,
    SoundOnStart,
    SoundOnRunning,
    SoundOnWaiting,
    SoundOnIdle,
    SoundOnError,
    // Hooks
    HookOnCreate,
    HookOnLaunch,
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

/// Helper to set or clear a profile override based on whether value matches global.
fn set_or_clear_override<T, S, F>(
    new_value: T,
    global_value: &T,
    section: &mut Option<S>,
    set_field: F,
) where
    T: Clone + PartialEq,
    S: Default,
    F: FnOnce(&mut S, Option<T>),
{
    if new_value == *global_value {
        if let Some(ref mut s) = section {
            set_field(s, None);
        }
    } else {
        let s = section.get_or_insert_with(S::default);
        set_field(s, Some(new_value));
    }
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
    /// Whether this field has a profile override (only relevant in profile scope)
    pub has_override: bool,
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
        SettingsCategory::Updates => build_updates_fields(scope, global, profile),
        SettingsCategory::Worktree => build_worktree_fields(scope, global, profile),
        SettingsCategory::Sandbox => build_sandbox_fields(scope, global, profile),
        SettingsCategory::Tmux => build_tmux_fields(scope, global, profile),
        SettingsCategory::Session => build_session_fields(scope, global, profile),
        SettingsCategory::Sound => build_sound_fields(scope, global, profile),
        SettingsCategory::Hooks => build_hooks_fields(scope, global, profile),
    }
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
        },
        SettingField {
            key: FieldKey::CheckIntervalHours,
            label: "Check Interval (hours)",
            description: "How often to check for updates",
            value: FieldValue::Number(check_interval),
            category: SettingsCategory::Updates,
            has_override: o2,
        },
        SettingField {
            key: FieldKey::NotifyInCli,
            label: "Notify in CLI",
            description: "Show update notifications in CLI output",
            value: FieldValue::Bool(notify_in_cli),
            category: SettingsCategory::Updates,
            has_override: o3,
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

    vec![
        SettingField {
            key: FieldKey::PathTemplate,
            label: "Path Template",
            description: "Template for worktree paths ({repo-name}, {branch})",
            value: FieldValue::Text(path_template),
            category: SettingsCategory::Worktree,
            has_override: o1,
        },
        SettingField {
            key: FieldKey::BareRepoPathTemplate,
            label: "Bare Repo Template",
            description: "Template for bare repo worktree paths",
            value: FieldValue::Text(bare_repo_template),
            category: SettingsCategory::Worktree,
            has_override: o2,
        },
        SettingField {
            key: FieldKey::WorktreeAutoCleanup,
            label: "Auto Cleanup",
            description: "Automatically clean up worktrees on session delete",
            value: FieldValue::Bool(auto_cleanup),
            category: SettingsCategory::Worktree,
            has_override: o3,
        },
        SettingField {
            key: FieldKey::DeleteBranchOnCleanup,
            label: "Delete Branch on Cleanup",
            description: "Also delete the git branch when deleting a worktree",
            value: FieldValue::Bool(delete_branch_on_cleanup),
            category: SettingsCategory::Worktree,
            has_override: o4,
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
    let (yolo_mode_default, o2) = resolve_value(
        scope,
        global.sandbox.yolo_mode_default,
        sb.and_then(|s| s.yolo_mode_default),
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
    let (environment_values, o_env_vals) = resolve_value(
        scope,
        global.sandbox.environment_values.clone(),
        sb.and_then(|s| s.environment_values.clone()),
    );
    let env_values_list = {
        let mut entries: Vec<String> = environment_values
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect();
        entries.sort();
        entries
    };
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

    let terminal_mode_selected = match default_terminal_mode {
        DefaultTerminalMode::Host => 0,
        DefaultTerminalMode::Container => 1,
    };

    vec![
        SettingField {
            key: FieldKey::SandboxEnabledByDefault,
            label: "Enabled by Default",
            description: "Enable sandbox mode by default for new sessions",
            value: FieldValue::Bool(enabled_by_default),
            category: SettingsCategory::Sandbox,
            has_override: o1,
        },
        SettingField {
            key: FieldKey::YoloModeDefault,
            label: "YOLO Mode Default",
            description: "Enable YOLO mode by default when sandbox is enabled",
            value: FieldValue::Bool(yolo_mode_default),
            category: SettingsCategory::Sandbox,
            has_override: o2,
        },
        SettingField {
            key: FieldKey::DefaultImage,
            label: "Default Image",
            description: "Docker image to use for sandboxes",
            value: FieldValue::Text(default_image),
            category: SettingsCategory::Sandbox,
            has_override: o3,
        },
        SettingField {
            key: FieldKey::Environment,
            label: "Environment Variables",
            description: "Var names to pass through from host (e.g. GITHUB_TOKEN)",
            value: FieldValue::List(environment),
            category: SettingsCategory::Sandbox,
            has_override: o4,
        },
        SettingField {
            key: FieldKey::EnvironmentValues,
            label: "Environment Values",
            description: "Custom KEY=VALUE env vars for sandbox. Use $VAR to reference host vars",
            value: FieldValue::List(env_values_list),
            category: SettingsCategory::Sandbox,
            has_override: o_env_vals,
        },
        SettingField {
            key: FieldKey::SandboxAutoCleanup,
            label: "Auto Cleanup",
            description: "Remove containers when sessions are deleted",
            value: FieldValue::Bool(auto_cleanup),
            category: SettingsCategory::Sandbox,
            has_override: o5,
        },
        SettingField {
            key: FieldKey::CpuLimit,
            label: "CPU Limit",
            description: "CPU limit for containers (e.g. \"4\")",
            value: FieldValue::OptionalText(cpu_limit),
            category: SettingsCategory::Sandbox,
            has_override: o_cpu,
        },
        SettingField {
            key: FieldKey::MemoryLimit,
            label: "Memory Limit",
            description: "Memory limit for containers (e.g. \"8g\", \"512m\")",
            value: FieldValue::OptionalText(memory_limit),
            category: SettingsCategory::Sandbox,
            has_override: o_mem,
        },
        SettingField {
            key: FieldKey::DefaultTerminalMode,
            label: "Default Terminal Mode",
            description: "Default terminal for sandboxed sessions (toggle with 'c' key)",
            value: FieldValue::Select {
                selected: terminal_mode_selected,
                options: vec!["Host".into(), "Container".into()],
            },
            category: SettingsCategory::Sandbox,
            has_override: o6,
        },
        SettingField {
            key: FieldKey::ExtraVolumes,
            label: "Extra Volumes",
            description: "Additional volume mounts (host:container or host:container:ro)",
            value: FieldValue::List(extra_volumes),
            category: SettingsCategory::Sandbox,
            has_override: o_ev,
        },
        SettingField {
            key: FieldKey::VolumeIgnores,
            label: "Volume Ignores",
            description: "Directories to exclude from host mount (e.g. target, node_modules)",
            value: FieldValue::List(volume_ignores),
            category: SettingsCategory::Sandbox,
            has_override: o7,
        },
        SettingField {
            key: FieldKey::MountSsh,
            label: "Mount SSH",
            description: "Mount ~/.ssh into sandbox containers (for git SSH access)",
            value: FieldValue::Bool(mount_ssh),
            category: SettingsCategory::Sandbox,
            has_override: o8,
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

    vec![
        SettingField {
            key: FieldKey::StatusBar,
            label: "Status Bar",
            description: "Control tmux status bar styling (Auto respects your tmux config)",
            value: FieldValue::Select {
                selected: status_bar_selected,
                options: vec!["Auto".into(), "Enabled".into(), "Disabled".into()],
            },
            category: SettingsCategory::Tmux,
            has_override: status_bar_override,
        },
        SettingField {
            key: FieldKey::Mouse,
            label: "Mouse Support",
            description: "Control mouse scrolling (Auto respects your tmux config)",
            value: FieldValue::Select {
                selected: mouse_selected,
                options: vec!["Auto".into(), "Enabled".into(), "Disabled".into()],
            },
            category: SettingsCategory::Tmux,
            has_override: mouse_override,
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

    // Map tool name to selected index: 0=Auto, 1=claude, 2=opencode, 3=vibe, 4=codex, 5=gemini
    let selected = match default_tool.as_deref() {
        Some("claude") => 1,
        Some("opencode") => 2,
        Some("vibe") => 3,
        Some("codex") => 4,
        Some("gemini") => 5,
        _ => 0, // Auto (use first available)
    };

    vec![SettingField {
        key: FieldKey::DefaultTool,
        label: "Default Tool",
        description: "Default coding tool for new sessions",
        value: FieldValue::Select {
            selected,
            options: vec![
                "Auto (first available)".into(),
                "claude".into(),
                "opencode".into(),
                "vibe".into(),
                "codex".into(),
                "gemini".into(),
            ],
        },
        category: SettingsCategory::Session,
        has_override,
    }]
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

    vec![
        SettingField {
            key: FieldKey::SoundEnabled,
            label: "Enabled",
            description: "Play sounds on agent state transitions",
            value: FieldValue::Bool(enabled),
            category: SettingsCategory::Sound,
            has_override: o1,
        },
        SettingField {
            key: FieldKey::SoundMode,
            label: "Mode",
            description: "How to select sounds (Random or Specific file name)",
            value: FieldValue::Select {
                selected: mode_selected,
                options: vec!["Random".into(), "Specific".into()],
            },
            category: SettingsCategory::Sound,
            has_override: o2,
        },
        SettingField {
            key: FieldKey::SoundOnStart,
            label: "On Start",
            description: "Specify file name with extension",
            value: FieldValue::OptionalText(on_start),
            category: SettingsCategory::Sound,
            has_override: o3,
        },
        SettingField {
            key: FieldKey::SoundOnRunning,
            label: "On Running",
            description: "Specify file name with extension",
            value: FieldValue::OptionalText(on_running),
            category: SettingsCategory::Sound,
            has_override: o4,
        },
        SettingField {
            key: FieldKey::SoundOnWaiting,
            label: "On Waiting",
            description: "Specify file name with extension",
            value: FieldValue::OptionalText(on_waiting),
            category: SettingsCategory::Sound,
            has_override: o5,
        },
        SettingField {
            key: FieldKey::SoundOnIdle,
            label: "On Idle",
            description: "Specify file name with extension",
            value: FieldValue::OptionalText(on_idle),
            category: SettingsCategory::Sound,
            has_override: o6,
        },
        SettingField {
            key: FieldKey::SoundOnError,
            label: "On Error",
            description: "Specify file name with extension",
            value: FieldValue::OptionalText(on_error),
            category: SettingsCategory::Sound,
            has_override: o7,
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

    vec![
        SettingField {
            key: FieldKey::HookOnCreate,
            label: "On Create",
            description: "Commands run once when a session is first created. Runs inside sandbox when enabled.",
            value: FieldValue::List(on_create),
            category: SettingsCategory::Hooks,
            has_override: o1,
        },
        SettingField {
            key: FieldKey::HookOnLaunch,
            label: "On Launch",
            description: "Commands run every time a session starts. Runs inside sandbox when enabled.",
            value: FieldValue::List(on_launch),
            category: SettingsCategory::Hooks,
            has_override: o2,
        },
    ]
}

/// Apply a field's value back to the appropriate config.
/// For profile scope, if the value matches global, the override is removed.
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
        // Sandbox
        (FieldKey::SandboxEnabledByDefault, FieldValue::Bool(v)) => {
            config.sandbox.enabled_by_default = *v
        }
        (FieldKey::YoloModeDefault, FieldValue::Bool(v)) => config.sandbox.yolo_mode_default = *v,
        (FieldKey::DefaultImage, FieldValue::Text(v)) => config.sandbox.default_image = v.clone(),
        (FieldKey::Environment, FieldValue::List(v)) => config.sandbox.environment = v.clone(),
        (FieldKey::EnvironmentValues, FieldValue::List(v)) => {
            config.sandbox.environment_values = parse_env_values_list(v);
        }
        (FieldKey::ExtraVolumes, FieldValue::List(v)) => config.sandbox.extra_volumes = v.clone(),
        (FieldKey::VolumeIgnores, FieldValue::List(v)) => config.sandbox.volume_ignores = v.clone(),
        (FieldKey::MountSsh, FieldValue::Bool(v)) => config.sandbox.mount_ssh = *v,
        (FieldKey::SandboxAutoCleanup, FieldValue::Bool(v)) => config.sandbox.auto_cleanup = *v,
        (FieldKey::CpuLimit, FieldValue::OptionalText(v)) => {
            config.sandbox.cpu_limit = v.clone();
        }
        (FieldKey::MemoryLimit, FieldValue::OptionalText(v)) => {
            config.sandbox.memory_limit = v.clone();
        }
        (FieldKey::DefaultTerminalMode, FieldValue::Select { selected, .. }) => {
            config.sandbox.default_terminal_mode = match selected {
                0 => DefaultTerminalMode::Host,
                _ => DefaultTerminalMode::Container,
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
            config.session.default_tool = match selected {
                1 => Some("claude".to_string()),
                2 => Some("opencode".to_string()),
                3 => Some("vibe".to_string()),
                4 => Some("codex".to_string()),
                5 => Some("gemini".to_string()),
                _ => None, // Auto
            };
        }
        // Sound
        (FieldKey::SoundEnabled, FieldValue::Bool(v)) => config.sound.enabled = *v,
        (FieldKey::SoundMode, FieldValue::Select { selected, .. }) => {
            config.sound.mode = match selected {
                1 => SoundMode::Specific(String::new()),
                _ => SoundMode::Random,
            };
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
        _ => {}
    }
}

/// Apply a field to the profile config.
/// If the value matches the global config, the override is cleared instead of set.
fn apply_field_to_profile(field: &SettingField, global: &Config, config: &mut ProfileConfig) {
    match (&field.key, &field.value) {
        // Updates
        (FieldKey::CheckEnabled, FieldValue::Bool(v)) => {
            set_or_clear_override(
                *v,
                &global.updates.check_enabled,
                &mut config.updates,
                |s, val| s.check_enabled = val,
            );
        }
        (FieldKey::CheckIntervalHours, FieldValue::Number(v)) => {
            set_or_clear_override(
                *v,
                &global.updates.check_interval_hours,
                &mut config.updates,
                |s, val| s.check_interval_hours = val,
            );
        }
        (FieldKey::NotifyInCli, FieldValue::Bool(v)) => {
            set_or_clear_override(
                *v,
                &global.updates.notify_in_cli,
                &mut config.updates,
                |s, val| s.notify_in_cli = val,
            );
        }
        // Worktree
        (FieldKey::PathTemplate, FieldValue::Text(v)) => {
            set_or_clear_override(
                v.clone(),
                &global.worktree.path_template,
                &mut config.worktree,
                |s, val| s.path_template = val,
            );
        }
        (FieldKey::BareRepoPathTemplate, FieldValue::Text(v)) => {
            set_or_clear_override(
                v.clone(),
                &global.worktree.bare_repo_path_template,
                &mut config.worktree,
                |s, val| s.bare_repo_path_template = val,
            );
        }
        (FieldKey::WorktreeAutoCleanup, FieldValue::Bool(v)) => {
            set_or_clear_override(
                *v,
                &global.worktree.auto_cleanup,
                &mut config.worktree,
                |s, val| s.auto_cleanup = val,
            );
        }
        (FieldKey::DeleteBranchOnCleanup, FieldValue::Bool(v)) => {
            set_or_clear_override(
                *v,
                &global.worktree.delete_branch_on_cleanup,
                &mut config.worktree,
                |s, val| s.delete_branch_on_cleanup = val,
            );
        }
        // Sandbox
        (FieldKey::SandboxEnabledByDefault, FieldValue::Bool(v)) => {
            set_or_clear_override(
                *v,
                &global.sandbox.enabled_by_default,
                &mut config.sandbox,
                |s, val| s.enabled_by_default = val,
            );
        }
        (FieldKey::YoloModeDefault, FieldValue::Bool(v)) => {
            set_or_clear_override(
                *v,
                &global.sandbox.yolo_mode_default,
                &mut config.sandbox,
                |s, val| s.yolo_mode_default = val,
            );
        }
        (FieldKey::DefaultImage, FieldValue::Text(v)) => {
            set_or_clear_override(
                v.clone(),
                &global.sandbox.default_image,
                &mut config.sandbox,
                |s, val| s.default_image = val,
            );
        }
        (FieldKey::Environment, FieldValue::List(v)) => {
            set_or_clear_override(
                v.clone(),
                &global.sandbox.environment,
                &mut config.sandbox,
                |s, val| s.environment = val,
            );
        }
        (FieldKey::EnvironmentValues, FieldValue::List(v)) => {
            let map = parse_env_values_list(v);
            set_or_clear_override(
                map,
                &global.sandbox.environment_values,
                &mut config.sandbox,
                |s, val| s.environment_values = val,
            );
        }
        (FieldKey::ExtraVolumes, FieldValue::List(v)) => {
            set_or_clear_override(
                v.clone(),
                &global.sandbox.extra_volumes,
                &mut config.sandbox,
                |s, val| s.extra_volumes = val,
            );
        }
        (FieldKey::VolumeIgnores, FieldValue::List(v)) => {
            set_or_clear_override(
                v.clone(),
                &global.sandbox.volume_ignores,
                &mut config.sandbox,
                |s, val| s.volume_ignores = val,
            );
        }
        (FieldKey::MountSsh, FieldValue::Bool(v)) => {
            set_or_clear_override(
                *v,
                &global.sandbox.mount_ssh,
                &mut config.sandbox,
                |s, val| s.mount_ssh = val,
            );
        }
        (FieldKey::SandboxAutoCleanup, FieldValue::Bool(v)) => {
            set_or_clear_override(
                *v,
                &global.sandbox.auto_cleanup,
                &mut config.sandbox,
                |s, val| s.auto_cleanup = val,
            );
        }
        (FieldKey::CpuLimit, FieldValue::OptionalText(v)) => {
            if *v == global.sandbox.cpu_limit {
                if let Some(ref mut s) = config.sandbox {
                    s.cpu_limit = None;
                }
            } else {
                use crate::session::SandboxConfigOverride;
                let s = config
                    .sandbox
                    .get_or_insert_with(SandboxConfigOverride::default);
                s.cpu_limit = v.clone();
            }
        }
        (FieldKey::MemoryLimit, FieldValue::OptionalText(v)) => {
            if *v == global.sandbox.memory_limit {
                if let Some(ref mut s) = config.sandbox {
                    s.memory_limit = None;
                }
            } else {
                use crate::session::SandboxConfigOverride;
                let s = config
                    .sandbox
                    .get_or_insert_with(SandboxConfigOverride::default);
                s.memory_limit = v.clone();
            }
        }
        (FieldKey::DefaultTerminalMode, FieldValue::Select { selected, .. }) => {
            let mode = match selected {
                0 => DefaultTerminalMode::Host,
                _ => DefaultTerminalMode::Container,
            };
            set_or_clear_override(
                mode,
                &global.sandbox.default_terminal_mode,
                &mut config.sandbox,
                |s, val| s.default_terminal_mode = val,
            );
        }
        // Tmux
        (FieldKey::StatusBar, FieldValue::Select { selected, .. }) => {
            let mode = match selected {
                0 => TmuxStatusBarMode::Auto,
                1 => TmuxStatusBarMode::Enabled,
                _ => TmuxStatusBarMode::Disabled,
            };
            set_or_clear_override(mode, &global.tmux.status_bar, &mut config.tmux, |s, val| {
                s.status_bar = val
            });
        }
        (FieldKey::Mouse, FieldValue::Select { selected, .. }) => {
            let mode = match selected {
                0 => TmuxMouseMode::Auto,
                1 => TmuxMouseMode::Enabled,
                _ => TmuxMouseMode::Disabled,
            };
            set_or_clear_override(mode, &global.tmux.mouse, &mut config.tmux, |s, val| {
                s.mouse = val
            });
        }
        // Session
        (FieldKey::DefaultTool, FieldValue::Select { selected, .. }) => {
            let tool = match selected {
                1 => Some("claude".to_string()),
                2 => Some("opencode".to_string()),
                3 => Some("vibe".to_string()),
                4 => Some("codex".to_string()),
                5 => Some("gemini".to_string()),
                _ => None, // Auto
            };
            // Compare with global and set/clear override accordingly
            if tool == global.session.default_tool {
                if let Some(ref mut session) = config.session {
                    session.default_tool = None;
                }
            } else {
                use crate::session::SessionConfigOverride;
                let session = config
                    .session
                    .get_or_insert_with(SessionConfigOverride::default);
                session.default_tool = tool;
            }
        }
        // Sound
        (FieldKey::SoundEnabled, FieldValue::Bool(v)) => {
            set_or_clear_override(*v, &global.sound.enabled, &mut config.sound, |s, val| {
                s.enabled = val
            });
        }
        (FieldKey::SoundMode, FieldValue::Select { selected, .. }) => {
            let mode = match selected {
                1 => SoundMode::Specific(String::new()),
                _ => SoundMode::Random,
            };
            set_or_clear_override(mode, &global.sound.mode, &mut config.sound, |s, val| {
                s.mode = val
            });
        }
        (FieldKey::SoundOnStart, FieldValue::OptionalText(v)) => {
            if *v == global.sound.on_start {
                if let Some(ref mut s) = config.sound {
                    s.on_start = None;
                }
            } else {
                let s = config
                    .sound
                    .get_or_insert_with(crate::sound::SoundConfigOverride::default);
                s.on_start = v.clone();
            }
        }
        (FieldKey::SoundOnRunning, FieldValue::OptionalText(v)) => {
            if *v == global.sound.on_running {
                if let Some(ref mut s) = config.sound {
                    s.on_running = None;
                }
            } else {
                let s = config
                    .sound
                    .get_or_insert_with(crate::sound::SoundConfigOverride::default);
                s.on_running = v.clone();
            }
        }
        (FieldKey::SoundOnWaiting, FieldValue::OptionalText(v)) => {
            if *v == global.sound.on_waiting {
                if let Some(ref mut s) = config.sound {
                    s.on_waiting = None;
                }
            } else {
                let s = config
                    .sound
                    .get_or_insert_with(crate::sound::SoundConfigOverride::default);
                s.on_waiting = v.clone();
            }
        }
        (FieldKey::SoundOnIdle, FieldValue::OptionalText(v)) => {
            if *v == global.sound.on_idle {
                if let Some(ref mut s) = config.sound {
                    s.on_idle = None;
                }
            } else {
                let s = config
                    .sound
                    .get_or_insert_with(crate::sound::SoundConfigOverride::default);
                s.on_idle = v.clone();
            }
        }
        (FieldKey::SoundOnError, FieldValue::OptionalText(v)) => {
            if *v == global.sound.on_error {
                if let Some(ref mut s) = config.sound {
                    s.on_error = None;
                }
            } else {
                let s = config
                    .sound
                    .get_or_insert_with(crate::sound::SoundConfigOverride::default);
                s.on_error = v.clone();
            }
        }
        // Hooks
        (FieldKey::HookOnCreate, FieldValue::List(v)) => {
            set_or_clear_override(
                v.clone(),
                &global.hooks.on_create,
                &mut config.hooks,
                |s, val| s.on_create = val,
            );
        }
        (FieldKey::HookOnLaunch, FieldValue::List(v)) => {
            set_or_clear_override(
                v.clone(),
                &global.hooks.on_launch,
                &mut config.hooks,
                |s, val| s.on_launch = val,
            );
        }
        _ => {}
    }
}

fn parse_env_values_list(entries: &[String]) -> HashMap<String, String> {
    entries
        .iter()
        .filter_map(|entry| {
            if let Some((key, value)) = entry.split_once('=') {
                Some((key.to_string(), value.to_string()))
            } else {
                tracing::warn!(
                    "Ignoring malformed environment value (missing '='): {}",
                    entry
                );
                None
            }
        })
        .collect()
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
    fn test_default_tool_options_include_all_supported_tools() {
        use crate::session::SUPPORTED_TOOLS;

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

        // First option is "Auto (first available)", rest should be tool names
        let tool_options: Vec<&str> = options.iter().skip(1).map(|s| s.as_str()).collect();

        for tool in SUPPORTED_TOOLS {
            assert!(
                tool_options.contains(tool),
                "Settings UI missing tool '{}'. Update default_tool_fields() in fields.rs \
                 when adding new tools. Supported tools: {:?}, UI options: {:?}",
                tool,
                SUPPORTED_TOOLS,
                tool_options
            );
        }

        // Also verify we don't have extra unknown tools in the UI
        for option in &tool_options {
            assert!(
                SUPPORTED_TOOLS.contains(option),
                "Settings UI has unknown tool '{}' not in SUPPORTED_TOOLS. \
                 Either add to SUPPORTED_TOOLS or remove from UI.",
                option
            );
        }
    }
}
