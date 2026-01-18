//! Session instance definition and operations

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::docker::{
    self, ContainerConfig, DockerContainer, VolumeMount, CLAUDE_AUTH_VOLUME, OPENCODE_AUTH_VOLUME,
};
use crate::tmux;

fn default_true() -> bool {
    true
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    pub container_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yolo_mode: Option<bool>,
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
        let session = self.terminal_tmux_session()?;

        if !session.exists() {
            session.create(&self.project_path)?;
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
        let session = self.tmux_session()?;

        if session.exists() {
            return Ok(());
        }

        let cmd = if self.is_sandboxed() {
            self.ensure_container_running()?;
            let sandbox = self.sandbox_info.as_ref().unwrap();
            let tool_cmd = if self.is_yolo_mode() && self.tool == "claude" {
                "claude --dangerously-skip-permissions".to_string()
            } else {
                self.get_tool_command().to_string()
            };
            Some(format!(
                "docker exec -it {} {}",
                sandbox.container_name, tool_cmd
            ))
        } else if self.command.is_empty() {
            if self.tool == "claude" {
                Some("claude".to_string())
            } else {
                None
            }
        } else {
            Some(self.command.clone())
        };

        session.create(&self.project_path, cmd.as_deref())?;
        self.status = Status::Starting;
        self.last_start_time = Some(std::time::Instant::now());

        Ok(())
    }

    fn ensure_container_running(&mut self) -> Result<()> {
        let sandbox = self
            .sandbox_info
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Cannot ensure container for non-sandboxed session"))?;

        let image = sandbox
            .image
            .as_deref()
            .unwrap_or(docker::default_sandbox_image());

        let container = DockerContainer::new(&self.id, image);

        if container.is_running()? {
            return Ok(());
        }

        if container.exists()? {
            container.start()?;
            return Ok(());
        }

        docker::ensure_named_volume(CLAUDE_AUTH_VOLUME)?;
        docker::ensure_named_volume(OPENCODE_AUTH_VOLUME)?;

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

        const CONTAINER_HOME: &str = "/home/sandbox";

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

        let named_volumes = vec![
            (
                CLAUDE_AUTH_VOLUME.to_string(),
                format!("{}/.claude", CONTAINER_HOME),
            ),
            (
                OPENCODE_AUTH_VOLUME.to_string(),
                format!("{}/.local/share/opencode", CONTAINER_HOME),
            ),
        ];

        let sandbox_config = super::config::Config::load()
            .ok()
            .map(|c| c.sandbox)
            .unwrap_or_default();

        let mut environment: Vec<(String, String)> = sandbox_config
            .environment
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
        let session = self.tmux_session()?;

        if session.exists() {
            session.kill()?;
        }

        // Small delay to ensure tmux cleanup
        std::thread::sleep(std::time::Duration::from_millis(100));

        self.start()
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

/// Tools that have YOLO mode support configured.
/// When adding a new tool, add it here and implement YOLO support in:
/// - `start()` for command construction (Claude uses CLI flag)
/// - `build_container_config()` for environment variables (OpenCode uses env var)
pub const YOLO_SUPPORTED_TOOLS: &[&str] = &["claude", "opencode"];

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
            image: None,
            container_name: "test".to_string(),
            created_at: None,
            yolo_mode: Some(true),
        });
        assert!(inst.is_yolo_mode());

        inst.sandbox_info.as_mut().unwrap().yolo_mode = Some(false);
        assert!(!inst.is_yolo_mode());

        inst.sandbox_info.as_mut().unwrap().yolo_mode = None;
        assert!(!inst.is_yolo_mode());
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
