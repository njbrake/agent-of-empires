//! User configuration management

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
    pub tmux: TmuxConfig,

    #[serde(default)]
    pub session: SessionConfig,

    #[serde(default)]
    pub diff: DiffConfig,

    #[serde(default)]
    pub sound: crate::sound::SoundConfig,

    #[serde(default)]
    pub app_state: AppStateConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppStateConfig {
    #[serde(default)]
    pub has_seen_welcome: bool,

    #[serde(default)]
    pub last_seen_version: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub home_list_width: Option<u16>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff_file_list_width: Option<u16>,
}

/// Session-related configuration defaults
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionConfig {
    /// Default coding tool for new sessions (claude, opencode, vibe, codex)
    /// If not set or tool is unavailable, falls back to first available tool
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_tool: Option<String>,
}

/// Diff view configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffConfig {
    /// Default branch to compare against (e.g., "main", "master")
    /// If not set, will try to auto-detect from the repository
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_branch: Option<String>,

    /// Number of context lines to show around changes
    #[serde(default = "default_context_lines")]
    pub context_lines: usize,
}

impl Default for DiffConfig {
    fn default() -> Self {
        Self {
            default_branch: None,
            context_lines: 3,
        }
    }
}

fn default_context_lines() -> usize {
    3
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

    /// Path template for bare repo setups (linked worktree pattern).
    /// Defaults to "./{branch}" to keep worktrees as siblings within the repo directory.
    #[serde(default = "default_bare_repo_template")]
    pub bare_repo_path_template: String,

    #[serde(default = "default_true")]
    pub auto_cleanup: bool,

    #[serde(default = "default_true")]
    pub show_branch_in_tui: bool,

    /// When deleting a worktree, also delete the associated git branch.
    /// Default: false (unchecked in delete dialog)
    #[serde(default)]
    pub delete_branch_on_cleanup: bool,
}

impl Default for WorktreeConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            path_template: default_worktree_template(),
            bare_repo_path_template: default_bare_repo_template(),
            auto_cleanup: true,
            show_branch_in_tui: true,
            delete_branch_on_cleanup: false,
        }
    }
}

fn default_worktree_template() -> String {
    "../{repo-name}-worktrees/{branch}".to_string()
}

fn default_bare_repo_template() -> String {
    "./{branch}".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    #[serde(default)]
    pub enabled_by_default: bool,

    /// When sandbox is enabled, default YOLO mode to true (skip permission prompts)
    #[serde(default)]
    pub yolo_mode_default: bool,

    #[serde(default = "default_sandbox_image")]
    pub default_image: String,

    #[serde(default)]
    pub extra_volumes: Vec<String>,

    #[serde(default = "default_sandbox_environment")]
    pub environment: Vec<String>,

    /// Environment variables with explicit values to inject into sandbox containers.
    /// Unlike `environment` (which passes through host values), these are stored in config
    /// and injected directly. Useful for sandbox-specific credentials like GH_TOKEN.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub environment_values: HashMap<String, String>,

    #[serde(default = "default_true")]
    pub auto_cleanup: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_limit: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_limit: Option<String>,

    /// Default terminal mode for sandboxed sessions (host or container)
    #[serde(default)]
    pub default_terminal_mode: DefaultTerminalMode,

    /// Relative directory paths to exclude from the host bind mount via anonymous volumes
    #[serde(default)]
    pub volume_ignores: Vec<String>,

    /// Mount ~/.ssh into sandbox containers (default: false)
    #[serde(default)]
    pub mount_ssh: bool,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            enabled_by_default: false,
            yolo_mode_default: false,
            default_image: default_sandbox_image(),
            extra_volumes: Vec::new(),
            environment: default_sandbox_environment(),
            environment_values: HashMap::new(),
            auto_cleanup: true,
            cpu_limit: None,
            memory_limit: None,
            default_terminal_mode: DefaultTerminalMode::default(),
            volume_ignores: Vec::new(),
            mount_ssh: false,
        }
    }
}

fn default_sandbox_image() -> String {
    crate::docker::default_sandbox_image().to_string()
}

fn default_sandbox_environment() -> Vec<String> {
    vec![
        "TERM".to_string(),
        "COLORTERM".to_string(),
        "FORCE_COLOR".to_string(),
        "NO_COLOR".to_string(),
    ]
}

/// Default terminal mode for sandboxed sessions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum DefaultTerminalMode {
    /// Default to host terminal (shell on the host machine)
    #[default]
    Host,
    /// Default to container terminal (shell inside the Docker container)
    Container,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TmuxStatusBarMode {
    #[default]
    Auto,
    Enabled,
    Disabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum TmuxMouseMode {
    /// Only enable mouse if user doesn't have their own tmux config
    #[default]
    Auto,
    /// Always enable mouse for aoe sessions
    Enabled,
    /// Never enable mouse for aoe sessions (explicitly disable)
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmuxConfig {
    #[serde(default)]
    pub status_bar: TmuxStatusBarMode,

    /// Mouse support mode (auto, enabled, disabled)
    #[serde(default)]
    pub mouse: TmuxMouseMode,
}

impl Default for TmuxConfig {
    fn default() -> Self {
        Self {
            status_bar: TmuxStatusBarMode::Auto,
            mouse: TmuxMouseMode::Auto,
        }
    }
}

/// Check if user has a tmux configuration file.
/// Returns true if ~/.tmux.conf or ~/.config/tmux/tmux.conf exists.
pub fn user_has_tmux_config() -> bool {
    if let Some(home) = dirs::home_dir() {
        let traditional = home.join(".tmux.conf");
        let xdg = home.join(".config").join("tmux").join("tmux.conf");
        return traditional.exists() || xdg.exists();
    }
    false
}

/// Determine if status bar styling should be applied based on config and environment.
pub fn should_apply_tmux_status_bar() -> bool {
    let config = Config::load().unwrap_or_default();
    match config.tmux.status_bar {
        TmuxStatusBarMode::Enabled => true,
        TmuxStatusBarMode::Disabled => false,
        TmuxStatusBarMode::Auto => !user_has_tmux_config(),
    }
}

/// Determine if mouse support should be enabled based on config and environment.
/// Returns Some(true) to enable, Some(false) to disable, None to not touch the setting.
pub fn should_apply_tmux_mouse() -> Option<bool> {
    let config = Config::load().unwrap_or_default();
    match config.tmux.mouse {
        TmuxMouseMode::Enabled => Some(true),
        TmuxMouseMode::Disabled => Some(false),
        TmuxMouseMode::Auto => {
            // In auto mode, only enable mouse if user doesn't have their own tmux config
            if user_has_tmux_config() {
                None // Don't touch - let user's config apply
            } else {
                Some(true) // Enable mouse for users without custom config
            }
        }
    }
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
    Ok(Some(Config::load()?))
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
        assert!(sb.environment.contains(&"TERM".to_string()));
        assert!(sb.environment.contains(&"COLORTERM".to_string()));
        assert!(sb.cpu_limit.is_none());
        assert!(sb.memory_limit.is_none());
        assert!(sb.volume_ignores.is_empty());
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

    #[test]
    fn test_sandbox_config_volume_ignores_deserialize() {
        let toml = r#"
            volume_ignores = ["target", ".venv", "node_modules"]
        "#;
        let sb: SandboxConfig = toml::from_str(toml).unwrap();
        assert_eq!(sb.volume_ignores, vec!["target", ".venv", "node_modules"]);
    }

    #[test]
    fn test_sandbox_config_volume_ignores_defaults_empty() {
        let toml = r#"enabled_by_default = false"#;
        let sb: SandboxConfig = toml::from_str(toml).unwrap();
        assert!(sb.volume_ignores.is_empty());
    }

    #[test]
    fn test_sandbox_config_volume_ignores_roundtrip() {
        let mut config = Config::default();
        config.sandbox.volume_ignores = vec!["target".to_string(), "node_modules".to_string()];

        let serialized = toml::to_string(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();

        assert_eq!(
            deserialized.sandbox.volume_ignores,
            vec!["target", "node_modules"]
        );
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
        let config = Config {
            default_profile: "test".to_string(),
            worktree: WorktreeConfig {
                enabled: true,
                ..Default::default()
            },
            sandbox: SandboxConfig {
                enabled_by_default: true,
                ..Default::default()
            },
            updates: UpdatesConfig {
                check_interval_hours: 48,
                ..Default::default()
            },
            ..Default::default()
        };

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

    // Tests for TmuxConfig
    #[test]
    fn test_tmux_config_default() {
        let tmux = TmuxConfig::default();
        assert_eq!(tmux.status_bar, TmuxStatusBarMode::Auto);
        assert_eq!(tmux.mouse, TmuxMouseMode::Auto);
    }

    #[test]
    fn test_tmux_status_bar_mode_default() {
        let mode = TmuxStatusBarMode::default();
        assert_eq!(mode, TmuxStatusBarMode::Auto);
    }

    #[test]
    fn test_tmux_config_deserialize() {
        let toml = r#"status_bar = "enabled""#;
        let tmux: TmuxConfig = toml::from_str(toml).unwrap();
        assert_eq!(tmux.status_bar, TmuxStatusBarMode::Enabled);
    }

    #[test]
    fn test_tmux_config_deserialize_disabled() {
        let toml = r#"status_bar = "disabled""#;
        let tmux: TmuxConfig = toml::from_str(toml).unwrap();
        assert_eq!(tmux.status_bar, TmuxStatusBarMode::Disabled);
    }

    #[test]
    fn test_tmux_config_deserialize_auto() {
        let toml = r#"status_bar = "auto""#;
        let tmux: TmuxConfig = toml::from_str(toml).unwrap();
        assert_eq!(tmux.status_bar, TmuxStatusBarMode::Auto);
    }

    #[test]
    fn test_tmux_config_in_full_config() {
        let toml = r#"
            [tmux]
            status_bar = "enabled"
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.tmux.status_bar, TmuxStatusBarMode::Enabled);
    }

    #[test]
    fn test_tmux_config_serialization_roundtrip() {
        let mut config = Config::default();
        config.tmux.status_bar = TmuxStatusBarMode::Disabled;

        let serialized = toml::to_string(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();

        assert_eq!(config.tmux.status_bar, deserialized.tmux.status_bar);
    }

    #[test]
    fn test_tmux_config_mouse_deserialize() {
        let toml = r#"mouse = "enabled""#;
        let tmux: TmuxConfig = toml::from_str(toml).unwrap();
        assert_eq!(tmux.mouse, TmuxMouseMode::Enabled);
        assert_eq!(tmux.status_bar, TmuxStatusBarMode::Auto);
    }

    #[test]
    fn test_tmux_config_mouse_default_auto() {
        let toml = r#""#;
        let tmux: TmuxConfig = toml::from_str(toml).unwrap();
        assert_eq!(tmux.mouse, TmuxMouseMode::Auto);
    }

    #[test]
    fn test_tmux_config_mouse_disabled() {
        let toml = r#"mouse = "disabled""#;
        let tmux: TmuxConfig = toml::from_str(toml).unwrap();
        assert_eq!(tmux.mouse, TmuxMouseMode::Disabled);
    }

    #[test]
    fn test_tmux_mouse_mode_default() {
        let mode = TmuxMouseMode::default();
        assert_eq!(mode, TmuxMouseMode::Auto);
    }

    #[test]
    fn test_tmux_config_with_both_settings() {
        let toml = r#"
            status_bar = "enabled"
            mouse = "enabled"
        "#;
        let tmux: TmuxConfig = toml::from_str(toml).unwrap();
        assert_eq!(tmux.status_bar, TmuxStatusBarMode::Enabled);
        assert_eq!(tmux.mouse, TmuxMouseMode::Enabled);
    }

    #[test]
    fn test_tmux_config_in_full_config_with_mouse() {
        let toml = r#"
            [tmux]
            status_bar = "enabled"
            mouse = "enabled"
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.tmux.status_bar, TmuxStatusBarMode::Enabled);
        assert_eq!(config.tmux.mouse, TmuxMouseMode::Enabled);
    }

    // Tests for DiffConfig
    #[test]
    fn test_diff_config_default() {
        let diff = DiffConfig::default();
        assert!(diff.default_branch.is_none());
        assert_eq!(diff.context_lines, 3);
    }

    #[test]
    fn test_diff_config_deserialize() {
        let toml = r#"
            default_branch = "main"
            context_lines = 5
        "#;
        let diff: DiffConfig = toml::from_str(toml).unwrap();
        assert_eq!(diff.default_branch, Some("main".to_string()));
        assert_eq!(diff.context_lines, 5);
    }

    #[test]
    fn test_diff_config_partial_deserialize() {
        let toml = r#"default_branch = "develop""#;
        let diff: DiffConfig = toml::from_str(toml).unwrap();
        assert_eq!(diff.default_branch, Some("develop".to_string()));
        assert_eq!(diff.context_lines, 3);
    }

    #[test]
    fn test_diff_config_in_full_config() {
        let toml = r#"
            [diff]
            default_branch = "main"
            context_lines = 10
        "#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(config.diff.default_branch, Some("main".to_string()));
        assert_eq!(config.diff.context_lines, 10);
    }
}
