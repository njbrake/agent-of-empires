//! Session instance definition and operations

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::docker::{
    self, ContainerConfig, DockerContainer, VolumeMount, CLAUDE_AUTH_VOLUME, CODEX_AUTH_VOLUME,
    OPENCODE_AUTH_VOLUME, VIBE_AUTH_VOLUME,
};
use crate::tmux;

fn default_true() -> bool {
    true
}

/// Terminal environment variables that are always passed through for proper UI/theming
const DEFAULT_TERMINAL_ENV_VARS: &[&str] = &["TERM", "COLORTERM", "FORCE_COLOR", "NO_COLOR"];

/// Build docker exec environment flags from config and optional per-session extra keys
fn build_docker_env_args(extra_env_keys: Option<&Vec<String>>) -> String {
    let config = super::config::Config::load().unwrap_or_default();

    // Start with default terminal variables (always included for proper UI)
    let mut env_keys: Vec<String> = DEFAULT_TERMINAL_ENV_VARS
        .iter()
        .map(|s| s.to_string())
        .collect();

    // Add user-configured variables from global config
    for key in &config.sandbox.environment {
        if !env_keys.contains(key) {
            env_keys.push(key.clone());
        }
    }

    // Add per-session extra env keys
    if let Some(extra_keys) = extra_env_keys {
        for key in extra_keys {
            if !env_keys.contains(key) {
                env_keys.push(key.clone());
            }
        }
    }

    env_keys
        .iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .map(|val| format!("-e {}={}", key, val))
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalInfo {
    #[serde(default)]
    pub created: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Running,
    Waiting,
    #[default]
    Idle,
    Error,
    Starting,
    Deleting,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeInfo {
    pub branch: String,
    pub main_repo_path: String,
    pub managed_by_aoe: bool,
    pub created_at: DateTime<Utc>,
    #[serde(default = "default_true")]
    pub cleanup_on_delete: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxInfo {
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container_id: Option<String>,
    pub image: String,
    pub container_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yolo_mode: Option<bool>,
    /// Additional environment variable keys to pass from host (session-specific)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_env_keys: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instance {
    pub id: String,
    pub title: String,
    pub project_path: String,
    #[serde(default)]
    pub group_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_session_id: Option<String>,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub tool: String,
    #[serde(default)]
    pub status: Status,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_accessed_at: Option<DateTime<Utc>>,

    // Claude Code integration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claude_detected_at: Option<DateTime<Utc>>,

    // Git worktree integration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_info: Option<WorktreeInfo>,

    // Docker sandbox integration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_info: Option<SandboxInfo>,

    // Paired terminal session
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_info: Option<TerminalInfo>,

    // Runtime state (not serialized)
    #[serde(skip)]
    pub last_error_check: Option<std::time::Instant>,
    #[serde(skip)]
    pub last_start_time: Option<std::time::Instant>,
    #[serde(skip)]
    pub last_error: Option<String>,

    // Search optimization: pre-computed lowercase strings (not serialized)
    #[serde(skip)]
    pub title_lower: String,
    #[serde(skip)]
    pub project_path_lower: String,
}

impl Instance {
    pub fn new(title: &str, project_path: &str) -> Self {
        Self {
            id: generate_id(),
            title: title.to_string(),
            project_path: project_path.to_string(),
            group_path: String::new(),
            parent_session_id: None,
            command: String::new(),
            tool: "claude".to_string(),
            status: Status::Idle,
            created_at: Utc::now(),
            last_accessed_at: None,
            claude_session_id: None,
            claude_detected_at: None,
            worktree_info: None,
            sandbox_info: None,
            terminal_info: None,
            last_error_check: None,
            last_start_time: None,
            last_error: None,
            title_lower: title.to_lowercase(),
            project_path_lower: project_path.to_lowercase(),
        }
    }

    /// Update the pre-computed lowercase fields for search optimization.
    /// Call this after loading instances from disk or modifying title/path.
    pub fn update_search_cache(&mut self) {
        self.title_lower = self.title.to_lowercase();
        self.project_path_lower = self.project_path.to_lowercase();
    }

    pub fn is_sub_session(&self) -> bool {
        self.parent_session_id.is_some()
    }

    pub fn is_sandboxed(&self) -> bool {
        self.sandbox_info.as_ref().is_some_and(|s| s.enabled)
    }

    pub fn is_yolo_mode(&self) -> bool {
        self.sandbox_info
            .as_ref()
            .is_some_and(|s| s.yolo_mode.unwrap_or(false))
    }

    pub fn get_tool_command(&self) -> &str {
        if self.command.is_empty() {
            match self.tool.as_str() {
                "claude" => "claude",
                "opencode" => "opencode",
                "vibe" => "vibe",
                "codex" => "codex",
                _ => "bash",
            }
        } else {
            &self.command
        }
    }

    pub fn tmux_session(&self) -> Result<tmux::Session> {
        tmux::Session::new(&self.id, &self.title)
    }

    pub fn terminal_tmux_session(&self) -> Result<tmux::TerminalSession> {
        tmux::TerminalSession::new(&self.id, &self.title)
    }

    pub fn has_terminal(&self) -> bool {
        self.terminal_info
            .as_ref()
            .map(|t| t.created)
            .unwrap_or(false)
    }

    pub fn start_terminal(&mut self) -> Result<()> {
        self.start_terminal_with_size(None)
    }

    pub fn start_terminal_with_size(&mut self, size: Option<(u16, u16)>) -> Result<()> {
        let session = self.terminal_tmux_session()?;

        let is_new = !session.exists();
        if is_new {
            session.create_with_size(&self.project_path, size)?;
        }

        // Apply all configured tmux options to terminal sessions too
        if is_new {
            self.apply_terminal_tmux_options();
        }

        self.terminal_info = Some(TerminalInfo {
            created: true,
            created_at: Some(Utc::now()),
        });

        Ok(())
    }

    pub fn kill_terminal(&self) -> Result<()> {
        let session = self.terminal_tmux_session()?;
        if session.exists() {
            session.kill()?;
        }
        Ok(())
    }

    pub fn start(&mut self) -> Result<()> {
        self.start_with_size(None)
    }

    pub fn start_with_size(&mut self, size: Option<(u16, u16)>) -> Result<()> {
        let session = self.tmux_session()?;

        if session.exists() {
            return Ok(());
        }

        let cmd = if self.is_sandboxed() {
            self.ensure_container_running()?;
            let sandbox = self.sandbox_info.as_ref().unwrap();
            let tool_cmd = if self.is_yolo_mode() {
                match self.tool.as_str() {
                    "claude" => "claude --dangerously-skip-permissions".to_string(),
                    "vibe" => "vibe --agent auto-approve".to_string(),
                    "codex" => "codex --dangerously-bypass-approvals-and-sandbox".to_string(),
                    _ => self.get_tool_command().to_string(),
                }
            } else {
                self.get_tool_command().to_string()
            };
            let env_args = build_docker_env_args(sandbox.extra_env_keys.as_ref());
            let env_part = if env_args.is_empty() {
                String::new()
            } else {
                format!("{} ", env_args)
            };
            Some(wrap_command_ignore_suspend(&format!(
                "docker exec -it {}{} {}",
                env_part, sandbox.container_name, tool_cmd
            )))
        } else if self.command.is_empty() {
            match self.tool.as_str() {
                "claude" => Some(wrap_command_ignore_suspend("claude")),
                "vibe" => Some(wrap_command_ignore_suspend("vibe")),
                "codex" => Some(wrap_command_ignore_suspend("codex")),
                _ => None,
            }
        } else {
            Some(wrap_command_ignore_suspend(&self.command))
        };

        session.create_with_size(&self.project_path, cmd.as_deref(), size)?;

        // Apply all configured tmux options (status bar, mouse, etc.)
        self.apply_tmux_options();

        self.status = Status::Starting;
        self.last_start_time = Some(std::time::Instant::now());

        Ok(())
    }

    /// Apply all configured tmux options (status bar, mouse, etc.) to the agent session.
    fn apply_tmux_options(&self) {
        use crate::tmux::status_bar::{apply_all_tmux_options, SandboxDisplay};

        let session_name = tmux::Session::generate_name(&self.id, &self.title);
        let branch = self.worktree_info.as_ref().map(|w| w.branch.as_str());
        let sandbox = self.sandbox_info.as_ref().and_then(|s| {
            if s.enabled {
                Some(SandboxDisplay {
                    container_name: s.container_name.clone(),
                })
            } else {
                None
            }
        });

        apply_all_tmux_options(&session_name, &self.title, branch, sandbox.as_ref());
    }

    /// Apply all configured tmux options to the terminal session.
    fn apply_terminal_tmux_options(&self) {
        use crate::tmux::status_bar::{apply_all_tmux_options, SandboxDisplay};

        let session_name = tmux::TerminalSession::generate_name(&self.id, &self.title);
        let terminal_title = format!("{} (terminal)", self.title);
        let branch = self.worktree_info.as_ref().map(|w| w.branch.as_str());
        let sandbox = self.sandbox_info.as_ref().and_then(|s| {
            if s.enabled {
                Some(SandboxDisplay {
                    container_name: s.container_name.clone(),
                })
            } else {
                None
            }
        });

        apply_all_tmux_options(&session_name, &terminal_title, branch, sandbox.as_ref());
    }

    fn ensure_container_running(&mut self) -> Result<()> {
        let sandbox = self
            .sandbox_info
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Cannot ensure container for non-sandboxed session"))?;

        let image = &sandbox.image;
        let container = DockerContainer::new(&self.id, image);

        if container.is_running()? {
            return Ok(());
        }

        if container.exists()? {
            container.start()?;
            return Ok(());
        }

        // Ensure image is available (always pulls to get latest)
        docker::ensure_image(image)?;

        docker::ensure_named_volume(CLAUDE_AUTH_VOLUME)?;
        docker::ensure_named_volume(OPENCODE_AUTH_VOLUME)?;
        docker::ensure_named_volume(VIBE_AUTH_VOLUME)?;
        docker::ensure_named_volume(CODEX_AUTH_VOLUME)?;

        crate::migrations::run_lazy_docker_migrations();

        let config = self.build_container_config()?;
        let container_id = container.create(&config)?;

        if let Some(ref mut sandbox) = self.sandbox_info {
            sandbox.container_id = Some(container_id);
            sandbox.created_at = Some(Utc::now());
        }

        Ok(())
    }

    fn build_container_config(&self) -> Result<ContainerConfig> {
        let home =
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;

        // Extract dir name from project path to preserve it in the container mount
        let dir_name = std::path::Path::new(&self.project_path)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "workspace".to_string());
        let workspace_path = format!("/workspace/{}", dir_name);

        let mut volumes = vec![VolumeMount {
            host_path: self.project_path.clone(),
            container_path: workspace_path.clone(),
            read_only: false,
        }];

        const CONTAINER_HOME: &str = "/root";

        let gitconfig = home.join(".gitconfig");
        if gitconfig.exists() {
            volumes.push(VolumeMount {
                host_path: gitconfig.to_string_lossy().to_string(),
                container_path: format!("{}/.gitconfig", CONTAINER_HOME),
                read_only: true,
            });
        }

        let ssh_dir = home.join(".ssh");
        if ssh_dir.exists() {
            volumes.push(VolumeMount {
                host_path: ssh_dir.to_string_lossy().to_string(),
                container_path: format!("{}/.ssh", CONTAINER_HOME),
                read_only: true,
            });
        }

        let opencode_config = home.join(".config").join("opencode");
        if opencode_config.exists() {
            volumes.push(VolumeMount {
                host_path: opencode_config.to_string_lossy().to_string(),
                container_path: format!("{}/.config/opencode", CONTAINER_HOME),
                read_only: true,
            });
        }

        let vibe_config = home.join(".vibe");
        let has_vibe_host_mount = vibe_config.exists();
        if has_vibe_host_mount {
            volumes.push(VolumeMount {
                host_path: vibe_config.to_string_lossy().to_string(),
                container_path: format!("{}/.vibe", CONTAINER_HOME),
                read_only: false,
            });
        }

        let mut named_volumes = vec![
            (
                CLAUDE_AUTH_VOLUME.to_string(),
                format!("{}/.claude", CONTAINER_HOME),
            ),
            (
                OPENCODE_AUTH_VOLUME.to_string(),
                format!("{}/.local/share/opencode", CONTAINER_HOME),
            ),
            (
                CODEX_AUTH_VOLUME.to_string(),
                format!("{}/.codex", CONTAINER_HOME),
            ),
        ];

        // Only add vibe auth volume if we didn't already mount the host config
        // (can't have duplicate mount points)
        if !has_vibe_host_mount {
            named_volumes.push((
                VIBE_AUTH_VOLUME.to_string(),
                format!("{}/.vibe", CONTAINER_HOME),
            ));
        }

        let sandbox_config = super::config::Config::load()
            .ok()
            .map(|c| c.sandbox)
            .unwrap_or_default();

        // Start with default terminal variables (always included for proper UI)
        let mut env_keys: Vec<String> = DEFAULT_TERMINAL_ENV_VARS
            .iter()
            .map(|s| s.to_string())
            .collect();

        // Add user-configured variables from global config
        for key in &sandbox_config.environment {
            if !env_keys.contains(key) {
                env_keys.push(key.clone());
            }
        }

        // Add per-session extra env keys
        if let Some(extra_keys) = self
            .sandbox_info
            .as_ref()
            .and_then(|s| s.extra_env_keys.as_ref())
        {
            for key in extra_keys {
                if !env_keys.contains(key) {
                    env_keys.push(key.clone());
                }
            }
        }

        let mut environment: Vec<(String, String)> = env_keys
            .iter()
            .filter_map(|key| std::env::var(key).ok().map(|val| (key.clone(), val)))
            .collect();

        environment.push((
            "CLAUDE_CONFIG_DIR".to_string(),
            format!("{}/.claude", CONTAINER_HOME),
        ));

        if self.is_yolo_mode() && self.tool == "opencode" {
            environment.push((
                "OPENCODE_PERMISSION".to_string(),
                r#"{"*":"allow"}"#.to_string(),
            ));
        }

        Ok(ContainerConfig {
            working_dir: workspace_path,
            volumes,
            named_volumes,
            environment,
            cpu_limit: sandbox_config.cpu_limit,
            memory_limit: sandbox_config.memory_limit,
        })
    }

    pub fn restart(&mut self) -> Result<()> {
        self.restart_with_size(None)
    }

    pub fn restart_with_size(&mut self, size: Option<(u16, u16)>) -> Result<()> {
        let session = self.tmux_session()?;

        if session.exists() {
            session.kill()?;
        }

        // Small delay to ensure tmux cleanup
        std::thread::sleep(std::time::Duration::from_millis(100));

        self.start_with_size(size)
    }

    pub fn kill(&self) -> Result<()> {
        let session = self.tmux_session()?;
        if session.exists() {
            session.kill()?;
        }
        Ok(())
    }

    pub fn update_status(&mut self) {
        // Skip expensive checks for recently errored sessions
        if self.status == Status::Error {
            if let Some(last_check) = self.last_error_check {
                if last_check.elapsed().as_secs() < 30 {
                    return;
                }
            }
        }

        // Grace period for starting sessions
        if let Some(start_time) = self.last_start_time {
            if start_time.elapsed().as_secs() < 3 {
                self.status = Status::Starting;
                return;
            }
        }

        let session = match self.tmux_session() {
            Ok(s) => s,
            Err(_) => {
                self.status = Status::Error;
                self.last_error_check = Some(std::time::Instant::now());
                return;
            }
        };

        if !session.exists() {
            self.status = Status::Error;
            self.last_error_check = Some(std::time::Instant::now());
            return;
        }

        // Detect status from pane content
        self.status = match session.detect_status(&self.tool) {
            Ok(status) => status,
            Err(_) => Status::Idle,
        };

        // Detect Claude session ID if applicable
        if self.tool == "claude" && self.claude_session_id.is_none() {
            if let Ok(Some(id)) = super::claude::detect_session_id(&self.project_path) {
                self.claude_session_id = Some(id);
                self.claude_detected_at = Some(Utc::now());
            }
        }
    }

    pub fn fork(&self, new_title: &str, new_group: &str) -> Result<Instance> {
        if self.tool != "claude" {
            anyhow::bail!("Fork is only supported for Claude sessions");
        }

        let claude_id = self
            .claude_session_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No Claude session ID to fork"))?;

        let mut forked = Self::new(new_title, &self.project_path);
        forked.group_path = new_group.to_string();
        forked.command = format!("claude --resume {}", claude_id);
        forked.tool = "claude".to_string();
        forked.parent_session_id = Some(self.id.clone());

        Ok(forked)
    }

    pub fn capture_output_with_size(
        &self,
        lines: usize,
        width: u16,
        height: u16,
    ) -> Result<String> {
        let session = self.tmux_session()?;
        session.capture_pane_with_size(lines, Some(width), Some(height))
    }
}

fn generate_id() -> String {
    Uuid::new_v4().to_string().replace("-", "")[..16].to_string()
}

/// Wrap a command to disable Ctrl-Z (SIGTSTP) suspension.
///
/// When running agents directly as tmux session commands (without a parent shell),
/// pressing Ctrl-Z suspends the process with no way to recover via job control.
/// This wrapper disables the suspend character at the terminal level before exec'ing
/// the actual command.
///
/// Uses POSIX-standard `stty susp undef` which works on both Linux and macOS.
fn wrap_command_ignore_suspend(cmd: &str) -> String {
    format!("bash -c 'stty susp undef; exec {}'", cmd)
}

/// All supported coding tools.
/// When adding a new tool, update:
/// - This constant
/// - `detect_tool()` in cli/add.rs
/// - `detect_status_from_content()` in tmux/status_detection.rs
/// - `default_tool_fields()` in tui/settings/fields.rs (options list and match statements)
/// - `apply_field_to_global()` and `apply_field_to_profile()` in tui/settings/fields.rs
pub const SUPPORTED_TOOLS: &[&str] = &["claude", "opencode", "vibe", "codex"];

/// Tools that have YOLO mode support configured.
/// When adding a new tool, add it here and implement YOLO support in:
/// - `start()` for command construction (Claude uses CLI flag, Vibe uses --auto-approve, Codex uses CLI flag)
/// - `build_container_config()` for environment variables (OpenCode uses env var)
pub const YOLO_SUPPORTED_TOOLS: &[&str] = &["claude", "opencode", "vibe", "codex"];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_instance() {
        let inst = Instance::new("test", "/tmp/test");
        assert_eq!(inst.title, "test");
        assert_eq!(inst.project_path, "/tmp/test");
        assert_eq!(inst.status, Status::Idle);
        assert_eq!(inst.id.len(), 16);
    }

    #[test]
    fn test_is_sub_session() {
        let mut inst = Instance::new("test", "/tmp/test");
        assert!(!inst.is_sub_session());

        inst.parent_session_id = Some("parent123".to_string());
        assert!(inst.is_sub_session());
    }

    #[test]
    fn test_all_available_tools_have_yolo_support() {
        // This test ensures that when a new tool is added to AvailableTools,
        // YOLO mode support is also configured for it.
        // If this test fails, add the new tool to YOLO_SUPPORTED_TOOLS and
        // implement YOLO support in start() and/or build_container_config().
        let available_tools = crate::tmux::AvailableTools {
            claude: true,
            opencode: true,
            vibe: true,
            codex: true,
        };
        for tool in available_tools.available_list() {
            assert!(
                YOLO_SUPPORTED_TOOLS.contains(&tool),
                "Tool '{}' is available but not in YOLO_SUPPORTED_TOOLS. \
                 Add YOLO mode support for this tool in start() and/or build_container_config(), \
                 then add it to YOLO_SUPPORTED_TOOLS.",
                tool
            );
        }
    }

    #[test]
    fn test_yolo_mode_helper() {
        let mut inst = Instance::new("test", "/tmp/test");
        assert!(!inst.is_yolo_mode());

        inst.sandbox_info = Some(SandboxInfo {
            enabled: true,
            container_id: None,
            image: "test-image".to_string(),
            container_name: "test".to_string(),
            created_at: None,
            yolo_mode: Some(true),
            extra_env_keys: None,
        });
        assert!(inst.is_yolo_mode());

        inst.sandbox_info.as_mut().unwrap().yolo_mode = Some(false);
        assert!(!inst.is_yolo_mode());

        inst.sandbox_info.as_mut().unwrap().yolo_mode = None;
        assert!(!inst.is_yolo_mode());
    }

    // Additional tests for is_sandboxed
    #[test]
    fn test_is_sandboxed_without_sandbox_info() {
        let inst = Instance::new("test", "/tmp/test");
        assert!(!inst.is_sandboxed());
    }

    #[test]
    fn test_is_sandboxed_with_disabled_sandbox() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.sandbox_info = Some(SandboxInfo {
            enabled: false,
            container_id: None,
            image: "test-image".to_string(),
            container_name: "test".to_string(),
            created_at: None,
            yolo_mode: None,
            extra_env_keys: None,
        });
        assert!(!inst.is_sandboxed());
    }

    #[test]
    fn test_is_sandboxed_with_enabled_sandbox() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.sandbox_info = Some(SandboxInfo {
            enabled: true,
            container_id: None,
            image: "test-image".to_string(),
            container_name: "test".to_string(),
            created_at: None,
            yolo_mode: None,
            extra_env_keys: None,
        });
        assert!(inst.is_sandboxed());
    }

    // Tests for get_tool_command
    #[test]
    fn test_get_tool_command_default_claude() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.tool = "claude".to_string();
        assert_eq!(inst.get_tool_command(), "claude");
    }

    #[test]
    fn test_get_tool_command_opencode() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.tool = "opencode".to_string();
        assert_eq!(inst.get_tool_command(), "opencode");
    }

    #[test]
    fn test_get_tool_command_codex() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.tool = "codex".to_string();
        assert_eq!(inst.get_tool_command(), "codex");
    }

    #[test]
    fn test_get_tool_command_unknown_tool() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.tool = "unknown".to_string();
        assert_eq!(inst.get_tool_command(), "bash");
    }

    #[test]
    fn test_get_tool_command_custom_command() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.tool = "claude".to_string();
        inst.command = "claude --resume abc123".to_string();
        assert_eq!(inst.get_tool_command(), "claude --resume abc123");
    }

    // Tests for update_search_cache
    #[test]
    fn test_update_search_cache() {
        let mut inst = Instance::new("Test Title", "/Path/To/Project");
        // Manually modify title
        inst.title = "New Title".to_string();
        inst.project_path = "/New/Path".to_string();

        // Cache is stale
        assert_ne!(inst.title_lower, "new title");
        assert_ne!(inst.project_path_lower, "/new/path");

        // Update cache
        inst.update_search_cache();

        assert_eq!(inst.title_lower, "new title");
        assert_eq!(inst.project_path_lower, "/new/path");
    }

    // Tests for Status enum
    #[test]
    fn test_status_default() {
        let status = Status::default();
        assert_eq!(status, Status::Idle);
    }

    #[test]
    fn test_status_serialization() {
        let statuses = vec![
            Status::Running,
            Status::Waiting,
            Status::Idle,
            Status::Error,
            Status::Starting,
            Status::Deleting,
        ];

        for status in statuses {
            let json = serde_json::to_string(&status).unwrap();
            let deserialized: Status = serde_json::from_str(&json).unwrap();
            assert_eq!(status, deserialized);
        }
    }

    // Tests for WorktreeInfo
    #[test]
    fn test_worktree_info_serialization() {
        let info = WorktreeInfo {
            branch: "feature/test".to_string(),
            main_repo_path: "/home/user/repo".to_string(),
            managed_by_aoe: true,
            created_at: Utc::now(),
            cleanup_on_delete: true,
        };

        let json = serde_json::to_string(&info).unwrap();
        let deserialized: WorktreeInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(info.branch, deserialized.branch);
        assert_eq!(info.main_repo_path, deserialized.main_repo_path);
        assert_eq!(info.managed_by_aoe, deserialized.managed_by_aoe);
        assert_eq!(info.cleanup_on_delete, deserialized.cleanup_on_delete);
    }

    #[test]
    fn test_worktree_info_default_cleanup_on_delete() {
        // Deserialize without cleanup_on_delete field - should default to true
        let json = r#"{"branch":"test","main_repo_path":"/path","managed_by_aoe":true,"created_at":"2024-01-01T00:00:00Z"}"#;
        let info: WorktreeInfo = serde_json::from_str(json).unwrap();
        assert!(info.cleanup_on_delete);
    }

    // Tests for SandboxInfo
    #[test]
    fn test_sandbox_info_serialization() {
        let info = SandboxInfo {
            enabled: true,
            container_id: Some("abc123".to_string()),
            image: "myimage:latest".to_string(),
            container_name: "test_container".to_string(),
            created_at: Some(Utc::now()),
            yolo_mode: Some(true),
            extra_env_keys: Some(vec!["MY_VAR".to_string(), "OTHER_VAR".to_string()]),
        };

        let json = serde_json::to_string(&info).unwrap();
        let deserialized: SandboxInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(info.enabled, deserialized.enabled);
        assert_eq!(info.container_id, deserialized.container_id);
        assert_eq!(info.image, deserialized.image);
        assert_eq!(info.container_name, deserialized.container_name);
        assert_eq!(info.yolo_mode, deserialized.yolo_mode);
        assert_eq!(info.extra_env_keys, deserialized.extra_env_keys);
    }

    #[test]
    fn test_sandbox_info_minimal_serialization() {
        // Required fields: enabled, image, container_name
        let json = r#"{"enabled":false,"image":"test-image","container_name":"test"}"#;
        let info: SandboxInfo = serde_json::from_str(json).unwrap();

        assert!(!info.enabled);
        assert_eq!(info.image, "test-image");
        assert_eq!(info.container_name, "test");
        assert!(info.container_id.is_none());
        assert!(info.created_at.is_none());
        assert!(info.yolo_mode.is_none());
    }

    // Tests for Instance serialization
    #[test]
    fn test_instance_serialization_roundtrip() {
        let mut inst = Instance::new("Test Project", "/home/user/project");
        inst.tool = "claude".to_string();
        inst.group_path = "work/clients".to_string();
        inst.command = "claude --resume xyz".to_string();

        let json = serde_json::to_string(&inst).unwrap();
        let deserialized: Instance = serde_json::from_str(&json).unwrap();

        assert_eq!(inst.id, deserialized.id);
        assert_eq!(inst.title, deserialized.title);
        assert_eq!(inst.project_path, deserialized.project_path);
        assert_eq!(inst.group_path, deserialized.group_path);
        assert_eq!(inst.tool, deserialized.tool);
        assert_eq!(inst.command, deserialized.command);
    }

    #[test]
    fn test_instance_serialization_skips_runtime_fields() {
        let mut inst = Instance::new("Test", "/tmp/test");
        inst.last_error_check = Some(std::time::Instant::now());
        inst.last_start_time = Some(std::time::Instant::now());
        inst.last_error = Some("test error".to_string());

        let json = serde_json::to_string(&inst).unwrap();

        // Runtime fields should not appear in JSON
        assert!(!json.contains("last_error_check"));
        assert!(!json.contains("last_start_time"));
        assert!(!json.contains("last_error"));
    }

    #[test]
    fn test_instance_with_worktree_info() {
        let mut inst = Instance::new("Test", "/tmp/worktree");
        inst.worktree_info = Some(WorktreeInfo {
            branch: "feature/abc".to_string(),
            main_repo_path: "/tmp/main".to_string(),
            managed_by_aoe: true,
            created_at: Utc::now(),
            cleanup_on_delete: true,
        });

        let json = serde_json::to_string(&inst).unwrap();
        let deserialized: Instance = serde_json::from_str(&json).unwrap();

        assert!(deserialized.worktree_info.is_some());
        let wt = deserialized.worktree_info.unwrap();
        assert_eq!(wt.branch, "feature/abc");
        assert!(wt.managed_by_aoe);
    }

    // Tests for fork
    #[test]
    fn test_fork_non_claude_tool() {
        let mut inst = Instance::new("Test", "/tmp/test");
        inst.tool = "opencode".to_string();

        let result = inst.fork("Forked", "group");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("only supported for Claude"));
    }

    #[test]
    fn test_fork_without_claude_session_id() {
        let mut inst = Instance::new("Test", "/tmp/test");
        inst.tool = "claude".to_string();
        inst.claude_session_id = None;

        let result = inst.fork("Forked", "group");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("No Claude session ID"));
    }

    #[test]
    fn test_fork_success() {
        let mut inst = Instance::new("Original", "/tmp/test");
        inst.tool = "claude".to_string();
        inst.claude_session_id = Some("session123".to_string());

        let forked = inst.fork("Forked Session", "new/group").unwrap();

        assert_eq!(forked.title, "Forked Session");
        assert_eq!(forked.group_path, "new/group");
        assert_eq!(forked.project_path, inst.project_path);
        assert_eq!(forked.tool, "claude");
        assert!(forked.command.contains("--resume"));
        assert!(forked.command.contains("session123"));
        assert_eq!(forked.parent_session_id, Some(inst.id.clone()));
    }

    // Test generate_id function properties
    #[test]
    fn test_generate_id_uniqueness() {
        let ids: Vec<String> = (0..100).map(|_| Instance::new("t", "/t").id).collect();
        let unique_ids: std::collections::HashSet<_> = ids.iter().collect();
        assert_eq!(ids.len(), unique_ids.len());
    }

    #[test]
    fn test_generate_id_format() {
        let inst = Instance::new("test", "/tmp/test");
        // ID should be 16 hex characters
        assert_eq!(inst.id.len(), 16);
        assert!(inst.id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_has_terminal_false_by_default() {
        let inst = Instance::new("test", "/tmp/test");
        assert!(!inst.has_terminal());
    }

    #[test]
    fn test_has_terminal_true_when_created() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.terminal_info = Some(TerminalInfo {
            created: true,
            created_at: Some(Utc::now()),
        });
        assert!(inst.has_terminal());
    }

    #[test]
    fn test_terminal_info_none_means_no_terminal() {
        let inst = Instance::new("test", "/tmp/test");
        assert!(inst.terminal_info.is_none());
        assert!(!inst.has_terminal());
    }

    #[test]
    fn test_terminal_info_created_false_means_no_terminal() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.terminal_info = Some(TerminalInfo {
            created: false,
            created_at: None,
        });
        assert!(!inst.has_terminal());
    }
}
