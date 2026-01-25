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
    pub key: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub value: FieldValue,
    pub category: SettingsCategory,
    /// Whether this field has a profile override (only relevant in profile scope)
    pub has_override: bool,
}

impl SettingField {
    pub fn validate(&self) -> Result<(), String> {
        match &self.value {
            FieldValue::OptionalText(Some(s)) => {
                if self.key == "memory_limit" {
                    validate_memory_limit(s)?;
                }
                Ok(())
            }
            FieldValue::Number(n) => {
                if self.key == "check_interval_hours" {
                    validate_check_interval(*n)?;
                }
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
    let (check_enabled, check_enabled_override) = match scope {
        SettingsScope::Global => (global.updates.check_enabled, false),
        SettingsScope::Profile => {
            let has_override = profile
                .updates
                .as_ref()
                .and_then(|u| u.check_enabled)
                .is_some();
            let value = profile
                .updates
                .as_ref()
                .and_then(|u| u.check_enabled)
                .unwrap_or(global.updates.check_enabled);
            (value, has_override)
        }
    };

    let (check_interval, check_interval_override) = match scope {
        SettingsScope::Global => (global.updates.check_interval_hours, false),
        SettingsScope::Profile => {
            let has_override = profile
                .updates
                .as_ref()
                .and_then(|u| u.check_interval_hours)
                .is_some();
            let value = profile
                .updates
                .as_ref()
                .and_then(|u| u.check_interval_hours)
                .unwrap_or(global.updates.check_interval_hours);
            (value, has_override)
        }
    };

    let (notify_in_cli, notify_in_cli_override) = match scope {
        SettingsScope::Global => (global.updates.notify_in_cli, false),
        SettingsScope::Profile => {
            let has_override = profile
                .updates
                .as_ref()
                .and_then(|u| u.notify_in_cli)
                .is_some();
            let value = profile
                .updates
                .as_ref()
                .and_then(|u| u.notify_in_cli)
                .unwrap_or(global.updates.notify_in_cli);
            (value, has_override)
        }
    };

    vec![
        SettingField {
            key: "check_enabled",
            label: "Check for Updates",
            description: "Automatically check for updates on startup",
            value: FieldValue::Bool(check_enabled),
            category: SettingsCategory::Updates,
            has_override: check_enabled_override,
        },
        SettingField {
            key: "check_interval_hours",
            label: "Check Interval (hours)",
            description: "How often to check for updates",
            value: FieldValue::Number(check_interval),
            category: SettingsCategory::Updates,
            has_override: check_interval_override,
        },
        SettingField {
            key: "notify_in_cli",
            label: "Notify in CLI",
            description: "Show update notifications in CLI output",
            value: FieldValue::Bool(notify_in_cli),
            category: SettingsCategory::Updates,
            has_override: notify_in_cli_override,
        },
    ]
}

fn build_worktree_fields(
    scope: SettingsScope,
    global: &Config,
    profile: &ProfileConfig,
) -> Vec<SettingField> {
    let (path_template, path_template_override) = match scope {
        SettingsScope::Global => (global.worktree.path_template.clone(), false),
        SettingsScope::Profile => {
            let has_override = profile
                .worktree
                .as_ref()
                .and_then(|w| w.path_template.as_ref())
                .is_some();
            let value = profile
                .worktree
                .as_ref()
                .and_then(|w| w.path_template.clone())
                .unwrap_or_else(|| global.worktree.path_template.clone());
            (value, has_override)
        }
    };

    let (bare_repo_template, bare_repo_template_override) = match scope {
        SettingsScope::Global => (global.worktree.bare_repo_path_template.clone(), false),
        SettingsScope::Profile => {
            let has_override = profile
                .worktree
                .as_ref()
                .and_then(|w| w.bare_repo_path_template.as_ref())
                .is_some();
            let value = profile
                .worktree
                .as_ref()
                .and_then(|w| w.bare_repo_path_template.clone())
                .unwrap_or_else(|| global.worktree.bare_repo_path_template.clone());
            (value, has_override)
        }
    };

    let (auto_cleanup, auto_cleanup_override) = match scope {
        SettingsScope::Global => (global.worktree.auto_cleanup, false),
        SettingsScope::Profile => {
            let has_override = profile
                .worktree
                .as_ref()
                .and_then(|w| w.auto_cleanup)
                .is_some();
            let value = profile
                .worktree
                .as_ref()
                .and_then(|w| w.auto_cleanup)
                .unwrap_or(global.worktree.auto_cleanup);
            (value, has_override)
        }
    };

    vec![
        SettingField {
            key: "path_template",
            label: "Path Template",
            description: "Template for worktree paths ({repo-name}, {branch})",
            value: FieldValue::Text(path_template),
            category: SettingsCategory::Worktree,
            has_override: path_template_override,
        },
        SettingField {
            key: "bare_repo_path_template",
            label: "Bare Repo Template",
            description: "Template for bare repo worktree paths",
            value: FieldValue::Text(bare_repo_template),
            category: SettingsCategory::Worktree,
            has_override: bare_repo_template_override,
        },
        SettingField {
            key: "auto_cleanup",
            label: "Auto Cleanup",
            description: "Automatically clean up worktrees on session delete",
            value: FieldValue::Bool(auto_cleanup),
            category: SettingsCategory::Worktree,
            has_override: auto_cleanup_override,
        },
    ]
}

fn build_sandbox_fields(
    scope: SettingsScope,
    global: &Config,
    profile: &ProfileConfig,
) -> Vec<SettingField> {
    let (default_image, default_image_override) = match scope {
        SettingsScope::Global => (global.sandbox.default_image.clone(), false),
        SettingsScope::Profile => {
            let has_override = profile
                .sandbox
                .as_ref()
                .and_then(|s| s.default_image.as_ref())
                .is_some();
            let value = profile
                .sandbox
                .as_ref()
                .and_then(|s| s.default_image.clone())
                .unwrap_or_else(|| global.sandbox.default_image.clone());
            (value, has_override)
        }
    };

    let (environment, environment_override) = match scope {
        SettingsScope::Global => (global.sandbox.environment.clone(), false),
        SettingsScope::Profile => {
            let has_override = profile
                .sandbox
                .as_ref()
                .and_then(|s| s.environment.as_ref())
                .is_some();
            let value = profile
                .sandbox
                .as_ref()
                .and_then(|s| s.environment.clone())
                .unwrap_or_else(|| global.sandbox.environment.clone());
            (value, has_override)
        }
    };

    let (auto_cleanup, auto_cleanup_override) = match scope {
        SettingsScope::Global => (global.sandbox.auto_cleanup, false),
        SettingsScope::Profile => {
            let has_override = profile
                .sandbox
                .as_ref()
                .and_then(|s| s.auto_cleanup)
                .is_some();
            let value = profile
                .sandbox
                .as_ref()
                .and_then(|s| s.auto_cleanup)
                .unwrap_or(global.sandbox.auto_cleanup);
            (value, has_override)
        }
    };

    let (cpu_limit, cpu_limit_override) = match scope {
        SettingsScope::Global => (global.sandbox.cpu_limit.clone(), false),
        SettingsScope::Profile => {
            let has_override = profile
                .sandbox
                .as_ref()
                .and_then(|s| s.cpu_limit.as_ref())
                .is_some();
            let value = profile
                .sandbox
                .as_ref()
                .and_then(|s| s.cpu_limit.clone())
                .or_else(|| global.sandbox.cpu_limit.clone());
            (value, has_override)
        }
    };

    let (memory_limit, memory_limit_override) = match scope {
        SettingsScope::Global => (global.sandbox.memory_limit.clone(), false),
        SettingsScope::Profile => {
            let has_override = profile
                .sandbox
                .as_ref()
                .and_then(|s| s.memory_limit.as_ref())
                .is_some();
            let value = profile
                .sandbox
                .as_ref()
                .and_then(|s| s.memory_limit.clone())
                .or_else(|| global.sandbox.memory_limit.clone());
            (value, has_override)
        }
    };

    vec![
        SettingField {
            key: "default_image",
            label: "Default Image",
            description: "Docker image to use for sandboxes",
            value: FieldValue::Text(default_image),
            category: SettingsCategory::Sandbox,
            has_override: default_image_override,
        },
        SettingField {
            key: "environment",
            label: "Environment Variables",
            description: "Environment variables to pass to container",
            value: FieldValue::List(environment),
            category: SettingsCategory::Sandbox,
            has_override: environment_override,
        },
        SettingField {
            key: "auto_cleanup",
            label: "Auto Cleanup",
            description: "Remove containers when sessions are deleted",
            value: FieldValue::Bool(auto_cleanup),
            category: SettingsCategory::Sandbox,
            has_override: auto_cleanup_override,
        },
        SettingField {
            key: "cpu_limit",
            label: "CPU Limit",
            description: "CPU limit for containers (e.g., '2' for 2 cores)",
            value: FieldValue::OptionalText(cpu_limit),
            category: SettingsCategory::Sandbox,
            has_override: cpu_limit_override,
        },
        SettingField {
            key: "memory_limit",
            label: "Memory Limit",
            description: "Memory limit for containers (e.g., '2g', '512m')",
            value: FieldValue::OptionalText(memory_limit),
            category: SettingsCategory::Sandbox,
            has_override: memory_limit_override,
        },
    ]
}

fn build_tmux_fields(
    scope: SettingsScope,
    global: &Config,
    profile: &ProfileConfig,
) -> Vec<SettingField> {
    let (status_bar, status_bar_override) = match scope {
        SettingsScope::Global => (global.tmux.status_bar, false),
        SettingsScope::Profile => {
            let has_override = profile.tmux.as_ref().and_then(|t| t.status_bar).is_some();
            let value = profile
                .tmux
                .as_ref()
                .and_then(|t| t.status_bar)
                .unwrap_or(global.tmux.status_bar);
            (value, has_override)
        }
    };

    let (selected, options) = match status_bar {
        TmuxStatusBarMode::Auto => (0, vec!["Auto", "Enabled", "Disabled"]),
        TmuxStatusBarMode::Enabled => (1, vec!["Auto", "Enabled", "Disabled"]),
        TmuxStatusBarMode::Disabled => (2, vec!["Auto", "Enabled", "Disabled"]),
    };

    vec![SettingField {
        key: "status_bar",
        label: "Status Bar",
        description: "Control tmux status bar styling (Auto respects your tmux config)",
        value: FieldValue::Select {
            selected,
            options: options.into_iter().map(String::from).collect(),
        },
        category: SettingsCategory::Tmux,
        has_override: status_bar_override,
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
    match field.category {
        SettingsCategory::Updates => match field.key {
            "check_enabled" => {
                if let FieldValue::Bool(v) = field.value {
                    config.updates.check_enabled = v;
                }
            }
            "check_interval_hours" => {
                if let FieldValue::Number(v) = field.value {
                    config.updates.check_interval_hours = v;
                }
            }
            "notify_in_cli" => {
                if let FieldValue::Bool(v) = field.value {
                    config.updates.notify_in_cli = v;
                }
            }
            _ => {}
        },
        SettingsCategory::Worktree => match field.key {
            "path_template" => {
                if let FieldValue::Text(ref v) = field.value {
                    config.worktree.path_template = v.clone();
                }
            }
            "bare_repo_path_template" => {
                if let FieldValue::Text(ref v) = field.value {
                    config.worktree.bare_repo_path_template = v.clone();
                }
            }
            "auto_cleanup" => {
                if let FieldValue::Bool(v) = field.value {
                    config.worktree.auto_cleanup = v;
                }
            }
            _ => {}
        },
        SettingsCategory::Sandbox => match field.key {
            "default_image" => {
                if let FieldValue::Text(ref v) = field.value {
                    config.sandbox.default_image = v.clone();
                }
            }
            "environment" => {
                if let FieldValue::List(ref v) = field.value {
                    config.sandbox.environment = v.clone();
                }
            }
            "auto_cleanup" => {
                if let FieldValue::Bool(v) = field.value {
                    config.sandbox.auto_cleanup = v;
                }
            }
            "cpu_limit" => {
                if let FieldValue::OptionalText(ref v) = field.value {
                    config.sandbox.cpu_limit = v.clone();
                }
            }
            "memory_limit" => {
                if let FieldValue::OptionalText(ref v) = field.value {
                    config.sandbox.memory_limit = v.clone();
                }
            }
            _ => {}
        },
        SettingsCategory::Tmux => {
            if field.key == "status_bar" {
                if let FieldValue::Select { selected, .. } = field.value {
                    config.tmux.status_bar = match selected {
                        0 => TmuxStatusBarMode::Auto,
                        1 => TmuxStatusBarMode::Enabled,
                        _ => TmuxStatusBarMode::Disabled,
                    };
                }
            }
        }
    }
}

/// Apply a field to the profile config.
/// If the value matches the global config, the override is cleared instead of set.
fn apply_field_to_profile(field: &SettingField, global: &Config, config: &mut ProfileConfig) {
    use crate::session::{
        SandboxConfigOverride, TmuxConfigOverride, UpdatesConfigOverride, WorktreeConfigOverride,
    };

    match field.category {
        SettingsCategory::Updates => match field.key {
            "check_enabled" => {
                if let FieldValue::Bool(v) = field.value {
                    if v == global.updates.check_enabled {
                        if let Some(ref mut updates) = config.updates {
                            updates.check_enabled = None;
                        }
                    } else {
                        let updates = config
                            .updates
                            .get_or_insert_with(UpdatesConfigOverride::default);
                        updates.check_enabled = Some(v);
                    }
                }
            }
            "check_interval_hours" => {
                if let FieldValue::Number(v) = field.value {
                    if v == global.updates.check_interval_hours {
                        if let Some(ref mut updates) = config.updates {
                            updates.check_interval_hours = None;
                        }
                    } else {
                        let updates = config
                            .updates
                            .get_or_insert_with(UpdatesConfigOverride::default);
                        updates.check_interval_hours = Some(v);
                    }
                }
            }
            "notify_in_cli" => {
                if let FieldValue::Bool(v) = field.value {
                    if v == global.updates.notify_in_cli {
                        if let Some(ref mut updates) = config.updates {
                            updates.notify_in_cli = None;
                        }
                    } else {
                        let updates = config
                            .updates
                            .get_or_insert_with(UpdatesConfigOverride::default);
                        updates.notify_in_cli = Some(v);
                    }
                }
            }
            _ => {}
        },
        SettingsCategory::Worktree => match field.key {
            "path_template" => {
                if let FieldValue::Text(ref v) = field.value {
                    if v == &global.worktree.path_template {
                        if let Some(ref mut wt) = config.worktree {
                            wt.path_template = None;
                        }
                    } else {
                        let wt = config
                            .worktree
                            .get_or_insert_with(WorktreeConfigOverride::default);
                        wt.path_template = Some(v.clone());
                    }
                }
            }
            "bare_repo_path_template" => {
                if let FieldValue::Text(ref v) = field.value {
                    if v == &global.worktree.bare_repo_path_template {
                        if let Some(ref mut wt) = config.worktree {
                            wt.bare_repo_path_template = None;
                        }
                    } else {
                        let wt = config
                            .worktree
                            .get_or_insert_with(WorktreeConfigOverride::default);
                        wt.bare_repo_path_template = Some(v.clone());
                    }
                }
            }
            "auto_cleanup" => {
                if let FieldValue::Bool(v) = field.value {
                    if v == global.worktree.auto_cleanup {
                        if let Some(ref mut wt) = config.worktree {
                            wt.auto_cleanup = None;
                        }
                    } else {
                        let wt = config
                            .worktree
                            .get_or_insert_with(WorktreeConfigOverride::default);
                        wt.auto_cleanup = Some(v);
                    }
                }
            }
            _ => {}
        },
        SettingsCategory::Sandbox => match field.key {
            "default_image" => {
                if let FieldValue::Text(ref v) = field.value {
                    if v == &global.sandbox.default_image {
                        if let Some(ref mut sb) = config.sandbox {
                            sb.default_image = None;
                        }
                    } else {
                        let sb = config
                            .sandbox
                            .get_or_insert_with(SandboxConfigOverride::default);
                        sb.default_image = Some(v.clone());
                    }
                }
            }
            "environment" => {
                if let FieldValue::List(ref v) = field.value {
                    if v == &global.sandbox.environment {
                        if let Some(ref mut sb) = config.sandbox {
                            sb.environment = None;
                        }
                    } else {
                        let sb = config
                            .sandbox
                            .get_or_insert_with(SandboxConfigOverride::default);
                        sb.environment = Some(v.clone());
                    }
                }
            }
            "auto_cleanup" => {
                if let FieldValue::Bool(v) = field.value {
                    if v == global.sandbox.auto_cleanup {
                        if let Some(ref mut sb) = config.sandbox {
                            sb.auto_cleanup = None;
                        }
                    } else {
                        let sb = config
                            .sandbox
                            .get_or_insert_with(SandboxConfigOverride::default);
                        sb.auto_cleanup = Some(v);
                    }
                }
            }
            "cpu_limit" => {
                if let FieldValue::OptionalText(ref v) = field.value {
                    if v == &global.sandbox.cpu_limit {
                        if let Some(ref mut sb) = config.sandbox {
                            sb.cpu_limit = None;
                        }
                    } else if let Some(ref val) = v {
                        let sb = config
                            .sandbox
                            .get_or_insert_with(SandboxConfigOverride::default);
                        sb.cpu_limit = Some(val.clone());
                    }
                }
            }
            "memory_limit" => {
                if let FieldValue::OptionalText(ref v) = field.value {
                    if v == &global.sandbox.memory_limit {
                        if let Some(ref mut sb) = config.sandbox {
                            sb.memory_limit = None;
                        }
                    } else if let Some(ref val) = v {
                        let sb = config
                            .sandbox
                            .get_or_insert_with(SandboxConfigOverride::default);
                        sb.memory_limit = Some(val.clone());
                    }
                }
            }
            _ => {}
        },
        SettingsCategory::Tmux => {
            if field.key == "status_bar" {
                if let FieldValue::Select { selected, .. } = field.value {
                    let mode = match selected {
                        0 => TmuxStatusBarMode::Auto,
                        1 => TmuxStatusBarMode::Enabled,
                        _ => TmuxStatusBarMode::Disabled,
                    };
                    if mode == global.tmux.status_bar {
                        if let Some(ref mut tmux) = config.tmux {
                            tmux.status_bar = None;
                        }
                    } else {
                        let tmux = config.tmux.get_or_insert_with(TmuxConfigOverride::default);
                        tmux.status_bar = Some(mode);
                    }
                }
            }
        }
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

        let check_enabled_field = fields.iter().find(|f| f.key == "check_enabled").unwrap();
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

        let check_enabled_field = fields.iter().find(|f| f.key == "check_enabled").unwrap();
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
        let check_enabled_field = fields.iter().find(|f| f.key == "check_enabled").unwrap();
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
        let check_enabled_field = fields.iter().find(|f| f.key == "check_enabled").unwrap();
        assert!(
            check_enabled_field.has_override,
            "Profile SHOULD show override after explicit profile change"
        );
    }
}
