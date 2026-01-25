//! Setting field definitions and config mapping

use crate::session::{
    validate_check_interval, validate_memory_limit, Config, ProfileConfig, TmuxStatusBarMode,
};

use super::SettingsScope;

/// Categories of settings
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsCategory {
    Updates,
    Worktree,
    Sandbox,
    Tmux,
}

impl SettingsCategory {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Updates => "Updates",
            Self::Worktree => "Worktree",
            Self::Sandbox => "Sandbox",
            Self::Tmux => "Tmux",
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
    // Sandbox
    DefaultImage,
    Environment,
    SandboxAutoCleanup,
    CpuLimit,
    MemoryLimit,
    // Tmux
    StatusBar,
}

/// Resolve a field value from global config and optional profile override.
/// Returns (value, has_override).
fn resolve_value<T: Clone>(scope: SettingsScope, global: T, profile: Option<T>) -> (T, bool) {
    match scope {
        SettingsScope::Global => (global, false),
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
        SettingsScope::Global => (global, false),
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
            (FieldKey::MemoryLimit, FieldValue::OptionalText(Some(s))) => {
                validate_memory_limit(s)?;
                Ok(())
            }
            (FieldKey::CheckIntervalHours, FieldValue::Number(n)) => {
                validate_check_interval(*n)?;
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// Build fields for a category based on scope and current config values
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
    ]
}

fn build_sandbox_fields(
    scope: SettingsScope,
    global: &Config,
    profile: &ProfileConfig,
) -> Vec<SettingField> {
    let sb = profile.sandbox.as_ref();

    let (default_image, o1) = resolve_value(
        scope,
        global.sandbox.default_image.clone(),
        sb.and_then(|s| s.default_image.clone()),
    );
    let (environment, o2) = resolve_value(
        scope,
        global.sandbox.environment.clone(),
        sb.and_then(|s| s.environment.clone()),
    );
    let (auto_cleanup, o3) = resolve_value(
        scope,
        global.sandbox.auto_cleanup,
        sb.and_then(|s| s.auto_cleanup),
    );
    // For optional fields, we need special handling: profile override OR global (both Option<T>)
    let (cpu_limit, o4) = resolve_optional(
        scope,
        global.sandbox.cpu_limit.clone(),
        sb.and_then(|s| s.cpu_limit.clone()),
        sb.map(|s| s.cpu_limit.is_some()).unwrap_or(false),
    );
    let (memory_limit, o5) = resolve_optional(
        scope,
        global.sandbox.memory_limit.clone(),
        sb.and_then(|s| s.memory_limit.clone()),
        sb.map(|s| s.memory_limit.is_some()).unwrap_or(false),
    );

    vec![
        SettingField {
            key: FieldKey::DefaultImage,
            label: "Default Image",
            description: "Docker image to use for sandboxes",
            value: FieldValue::Text(default_image),
            category: SettingsCategory::Sandbox,
            has_override: o1,
        },
        SettingField {
            key: FieldKey::Environment,
            label: "Environment Variables",
            description: "Environment variables to pass to container",
            value: FieldValue::List(environment),
            category: SettingsCategory::Sandbox,
            has_override: o2,
        },
        SettingField {
            key: FieldKey::SandboxAutoCleanup,
            label: "Auto Cleanup",
            description: "Remove containers when sessions are deleted",
            value: FieldValue::Bool(auto_cleanup),
            category: SettingsCategory::Sandbox,
            has_override: o3,
        },
        SettingField {
            key: FieldKey::CpuLimit,
            label: "CPU Limit",
            description: "CPU limit for containers (e.g., '2' for 2 cores)",
            value: FieldValue::OptionalText(cpu_limit),
            category: SettingsCategory::Sandbox,
            has_override: o4,
        },
        SettingField {
            key: FieldKey::MemoryLimit,
            label: "Memory Limit",
            description: "Memory limit for containers (e.g., '2g', '512m')",
            value: FieldValue::OptionalText(memory_limit),
            category: SettingsCategory::Sandbox,
            has_override: o5,
        },
    ]
}

fn build_tmux_fields(
    scope: SettingsScope,
    global: &Config,
    profile: &ProfileConfig,
) -> Vec<SettingField> {
    let tmux = profile.tmux.as_ref();

    let (status_bar, has_override) = resolve_value(
        scope,
        global.tmux.status_bar,
        tmux.and_then(|t| t.status_bar),
    );

    let selected = match status_bar {
        TmuxStatusBarMode::Auto => 0,
        TmuxStatusBarMode::Enabled => 1,
        TmuxStatusBarMode::Disabled => 2,
    };

    vec![SettingField {
        key: FieldKey::StatusBar,
        label: "Status Bar",
        description: "Control tmux status bar styling (Auto respects your tmux config)",
        value: FieldValue::Select {
            selected,
            options: vec!["Auto".into(), "Enabled".into(), "Disabled".into()],
        },
        category: SettingsCategory::Tmux,
        has_override,
    }]
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
        SettingsScope::Profile => apply_field_to_profile(field, global, profile),
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
        // Sandbox
        (FieldKey::DefaultImage, FieldValue::Text(v)) => config.sandbox.default_image = v.clone(),
        (FieldKey::Environment, FieldValue::List(v)) => config.sandbox.environment = v.clone(),
        (FieldKey::SandboxAutoCleanup, FieldValue::Bool(v)) => config.sandbox.auto_cleanup = *v,
        (FieldKey::CpuLimit, FieldValue::OptionalText(v)) => config.sandbox.cpu_limit = v.clone(),
        (FieldKey::MemoryLimit, FieldValue::OptionalText(v)) => {
            config.sandbox.memory_limit = v.clone()
        }
        // Tmux
        (FieldKey::StatusBar, FieldValue::Select { selected, .. }) => {
            config.tmux.status_bar = match selected {
                0 => TmuxStatusBarMode::Auto,
                1 => TmuxStatusBarMode::Enabled,
                _ => TmuxStatusBarMode::Disabled,
            };
        }
        _ => {}
    }
}

/// Apply a field to the profile config.
/// If the value matches the global config, the override is cleared instead of set.
fn apply_field_to_profile(field: &SettingField, global: &Config, config: &mut ProfileConfig) {
    use crate::session::SandboxConfigOverride;

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
        // Sandbox
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
        (FieldKey::SandboxAutoCleanup, FieldValue::Bool(v)) => {
            set_or_clear_override(
                *v,
                &global.sandbox.auto_cleanup,
                &mut config.sandbox,
                |s, val| s.auto_cleanup = val,
            );
        }
        (FieldKey::CpuLimit, FieldValue::OptionalText(v)) => {
            // For optional fields, flatten Option<Option<T>> to Option<T>
            let flat_value = v.clone().unwrap_or_default();
            let flat_global = global.sandbox.cpu_limit.clone().unwrap_or_default();
            if flat_value == flat_global {
                if let Some(ref mut sb) = config.sandbox {
                    sb.cpu_limit = None;
                }
            } else if let Some(val) = v {
                let sb = config
                    .sandbox
                    .get_or_insert_with(SandboxConfigOverride::default);
                sb.cpu_limit = Some(val.clone());
            }
        }
        (FieldKey::MemoryLimit, FieldValue::OptionalText(v)) => {
            let flat_value = v.clone().unwrap_or_default();
            let flat_global = global.sandbox.memory_limit.clone().unwrap_or_default();
            if flat_value == flat_global {
                if let Some(ref mut sb) = config.sandbox {
                    sb.memory_limit = None;
                }
            } else if let Some(val) = v {
                let sb = config
                    .sandbox
                    .get_or_insert_with(SandboxConfigOverride::default);
                sb.memory_limit = Some(val.clone());
            }
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
}
