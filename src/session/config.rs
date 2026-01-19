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
