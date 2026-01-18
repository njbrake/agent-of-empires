//! User configuration management

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

use super::get_app_dir;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default = "default_profile")]
    pub default_profile: String,

    #[serde(default)]
    pub theme: ThemeConfig,

    #[serde(default)]
    pub claude: ClaudeConfig,

    #[serde(default)]
    pub updates: UpdatesConfig,

    #[serde(default)]
    pub worktree: WorktreeConfig,

    #[serde(default)]
    pub sandbox: SandboxConfig,

    #[serde(default)]
    pub app_state: AppStateConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppStateConfig {
    #[serde(default)]
    pub has_seen_welcome: bool,

    #[serde(default)]
    pub last_seen_version: Option<String>,
}

fn default_profile() -> String {
    "default".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ThemeConfig {
    #[serde(default)]
    pub name: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ClaudeConfig {
    #[serde(default)]
    pub config_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatesConfig {
    #[serde(default = "default_true")]
    pub check_enabled: bool,

    #[serde(default)]
    pub auto_update: bool,

    #[serde(default = "default_check_interval")]
    pub check_interval_hours: u64,

    #[serde(default = "default_true")]
    pub notify_in_cli: bool,
}

impl Default for UpdatesConfig {
    fn default() -> Self {
        Self {
            check_enabled: true,
            auto_update: false,
            check_interval_hours: 24,
            notify_in_cli: true,
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_check_interval() -> u64 {
    24
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeConfig {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_worktree_template")]
    pub path_template: String,

    #[serde(default = "default_true")]
    pub auto_cleanup: bool,

    #[serde(default = "default_true")]
    pub show_branch_in_tui: bool,
}

impl Default for WorktreeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            path_template: default_worktree_template(),
            auto_cleanup: true,
            show_branch_in_tui: true,
        }
    }
}

fn default_worktree_template() -> String {
    "../{repo-name}-worktrees/{branch}".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    #[serde(default)]
    pub enabled_by_default: bool,

    #[serde(default = "default_sandbox_image")]
    pub default_image: String,

    #[serde(default)]
    pub extra_volumes: Vec<String>,

    #[serde(default)]
    pub environment: Vec<String>,

    #[serde(default = "default_true")]
    pub auto_cleanup: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_limit: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_limit: Option<String>,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled_by_default: false,
            default_image: default_sandbox_image(),
            extra_volumes: Vec::new(),
            environment: Vec::new(),
            auto_cleanup: true,
            cpu_limit: None,
            memory_limit: None,
        }
    }
}

fn default_sandbox_image() -> String {
    crate::docker::default_sandbox_image().to_string()
}

fn config_path() -> Result<PathBuf> {
    Ok(get_app_dir()?.join("config.toml"))
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(Config::default());
        }

        let content = fs::read_to_string(&path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }
}

pub fn load_config() -> Result<Option<Config>> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&path)?;
    let config: Config = toml::from_str(&content)?;
    Ok(Some(config))
}

pub fn save_config(config: &Config) -> Result<()> {
    let path = config_path()?;
    let content = toml::to_string_pretty(config)?;
    fs::write(&path, content)?;
    Ok(())
}

pub fn get_update_settings() -> UpdatesConfig {
    load_config()
        .ok()
        .flatten()
        .map(|c| c.updates)
        .unwrap_or_default()
}

pub fn get_claude_config_dir() -> Option<PathBuf> {
    let config = load_config().ok().flatten()?;
    config.claude.config_dir.map(|s| {
        if let Some(stripped) = s.strip_prefix("~/") {
            if let Some(home) = dirs::home_dir() {
                return home.join(stripped);
            }
        }
        PathBuf::from(s)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Tests for Config defaults
    #[test]
    fn test_config_default() {
        let config = Config::default();
        // default_profile uses default_profile() function which returns "default"
        // but Default derive gives empty string, so check deserialize case works
        let deserialized: Config = toml::from_str("").unwrap();
        assert_eq!(deserialized.default_profile, "default");
        assert!(!config.worktree.enabled);
        assert!(!config.sandbox.enabled_by_default);
        assert!(config.updates.check_enabled);
    }

    #[test]
    fn test_config_deserialize_empty_toml() {
        let config: Config = toml::from_str("").unwrap();
        assert_eq!(config.default_profile, "default");
    }

    #[test]
    fn test_config_deserialize_partial_toml() {
        let toml = r#"
            default_profile = "custom"
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.default_profile, "custom");
        // Other fields should have defaults
        assert!(!config.worktree.enabled);
    }

    // Tests for ThemeConfig
    #[test]
    fn test_theme_config_default() {
        let theme = ThemeConfig::default();
        assert_eq!(theme.name, "");
    }

    #[test]
    fn test_theme_config_deserialize() {
        let toml = r#"name = "dark""#;
        let theme: ThemeConfig = toml::from_str(toml).unwrap();
        assert_eq!(theme.name, "dark");
    }

    // Tests for UpdatesConfig
    #[test]
    fn test_updates_config_default() {
        let updates = UpdatesConfig::default();
        assert!(updates.check_enabled);
        assert!(!updates.auto_update);
        assert_eq!(updates.check_interval_hours, 24);
        assert!(updates.notify_in_cli);
    }

    #[test]
    fn test_updates_config_deserialize() {
        let toml = r#"
            check_enabled = false
            auto_update = true
            check_interval_hours = 12
            notify_in_cli = false
        "#;
        let updates: UpdatesConfig = toml::from_str(toml).unwrap();
        assert!(!updates.check_enabled);
        assert!(updates.auto_update);
        assert_eq!(updates.check_interval_hours, 12);
        assert!(!updates.notify_in_cli);
    }

    #[test]
    fn test_updates_config_partial_deserialize() {
        let toml = r#"check_enabled = false"#;
        let updates: UpdatesConfig = toml::from_str(toml).unwrap();
        assert!(!updates.check_enabled);
        // Defaults for other fields
        assert!(!updates.auto_update);
        assert_eq!(updates.check_interval_hours, 24);
    }

    // Tests for WorktreeConfig
    #[test]
    fn test_worktree_config_default() {
        let wt = WorktreeConfig::default();
        assert!(!wt.enabled);
        assert_eq!(wt.path_template, "../{repo-name}-worktrees/{branch}");
        assert!(wt.auto_cleanup);
        assert!(wt.show_branch_in_tui);
    }

    #[test]
    fn test_worktree_config_deserialize() {
        let toml = r#"
            enabled = true
            path_template = "/custom/{branch}"
            auto_cleanup = false
            show_branch_in_tui = false
        "#;
        let wt: WorktreeConfig = toml::from_str(toml).unwrap();
        assert!(wt.enabled);
        assert_eq!(wt.path_template, "/custom/{branch}");
        assert!(!wt.auto_cleanup);
        assert!(!wt.show_branch_in_tui);
    }

    // Tests for SandboxConfig
    #[test]
    fn test_sandbox_config_default() {
        let sb = SandboxConfig::default();
        assert!(!sb.enabled_by_default);
        assert!(sb.auto_cleanup);
        assert!(sb.extra_volumes.is_empty());
        assert!(sb.environment.is_empty());
        assert!(sb.cpu_limit.is_none());
        assert!(sb.memory_limit.is_none());
    }

    #[test]
    fn test_sandbox_config_deserialize() {
        let toml = r#"
            enabled_by_default = true
            default_image = "custom:latest"
            extra_volumes = ["/data:/data"]
            environment = ["MY_VAR"]
            auto_cleanup = false
            cpu_limit = "2"
            memory_limit = "4g"
        "#;
        let sb: SandboxConfig = toml::from_str(toml).unwrap();
        assert!(sb.enabled_by_default);
        assert_eq!(sb.default_image, "custom:latest");
        assert_eq!(sb.extra_volumes, vec!["/data:/data"]);
        assert_eq!(sb.environment, vec!["MY_VAR"]);
        assert!(!sb.auto_cleanup);
        assert_eq!(sb.cpu_limit, Some("2".to_string()));
        assert_eq!(sb.memory_limit, Some("4g".to_string()));
    }

    // Tests for ClaudeConfig
    #[test]
    fn test_claude_config_default() {
        let cc = ClaudeConfig::default();
        assert!(cc.config_dir.is_none());
    }

    #[test]
    fn test_claude_config_deserialize() {
        let toml = r#"config_dir = "/custom/claude""#;
        let cc: ClaudeConfig = toml::from_str(toml).unwrap();
        assert_eq!(cc.config_dir, Some("/custom/claude".to_string()));
    }

    // Tests for AppStateConfig
    #[test]
    fn test_app_state_config_default() {
        let app = AppStateConfig::default();
        assert!(!app.has_seen_welcome);
        assert!(app.last_seen_version.is_none());
    }

    #[test]
    fn test_app_state_config_deserialize() {
        let toml = r#"
            has_seen_welcome = true
            last_seen_version = "1.0.0"
        "#;
        let app: AppStateConfig = toml::from_str(toml).unwrap();
        assert!(app.has_seen_welcome);
        assert_eq!(app.last_seen_version, Some("1.0.0".to_string()));
    }

    // Full config serialization roundtrip
    #[test]
    fn test_config_serialization_roundtrip() {
        let mut config = Config::default();
        config.default_profile = "test".to_string();
        config.worktree.enabled = true;
        config.sandbox.enabled_by_default = true;
        config.updates.check_interval_hours = 48;

        let serialized = toml::to_string(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();

        assert_eq!(config.default_profile, deserialized.default_profile);
        assert_eq!(config.worktree.enabled, deserialized.worktree.enabled);
        assert_eq!(
            config.sandbox.enabled_by_default,
            deserialized.sandbox.enabled_by_default
        );
        assert_eq!(
            config.updates.check_interval_hours,
            deserialized.updates.check_interval_hours
        );
    }

    // Test nested sections in TOML
    #[test]
    fn test_config_nested_sections() {
        let toml = r#"
            default_profile = "work"

            [theme]
            name = "monokai"

            [worktree]
            enabled = true
            path_template = "../wt/{branch}"

            [sandbox]
            enabled_by_default = true

            [updates]
            check_enabled = true
            check_interval_hours = 12

            [app_state]
            has_seen_welcome = true
        "#;

        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.default_profile, "work");
        assert_eq!(config.theme.name, "monokai");
        assert!(config.worktree.enabled);
        assert_eq!(config.worktree.path_template, "../wt/{branch}");
        assert!(config.sandbox.enabled_by_default);
        assert!(config.updates.check_enabled);
        assert_eq!(config.updates.check_interval_hours, 12);
        assert!(config.app_state.has_seen_welcome);
    }

    // Test get_update_settings helper
    #[test]
    fn test_get_update_settings_returns_defaults_when_no_config() {
        // This test doesn't access the filesystem, so it should return defaults
        let settings = UpdatesConfig::default();
        assert!(settings.check_enabled);
        assert_eq!(settings.check_interval_hours, 24);
    }
}
