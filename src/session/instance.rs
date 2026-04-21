//! Session instance definition and operations

use std::path::Path;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::containers::{self, ContainerRuntimeInterface, DockerContainer};
use crate::tmux;

use super::container_config;
use super::environment::{build_docker_env_args, shell_escape};

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
    Unknown,
    Stopped,
    Error,
    Starting,
    Deleting,
    Creating,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeInfo {
    pub branch: String,
    pub main_repo_path: String,
    pub managed_by_aoe: bool,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceRepo {
    pub name: String,
    pub source_path: String,
    pub branch: String,
    pub worktree_path: String,
    pub main_repo_path: String,
    pub managed_by_aoe: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub branch: String,
    pub workspace_dir: String,
    pub repos: Vec<WorkspaceRepo>,
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
    /// Additional environment entries (session-specific).
    /// `KEY` = pass through from host, `KEY=VALUE` = set explicitly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_env: Option<Vec<String>>,
    /// Custom instruction text to inject into agent launch command
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_instruction: Option<String>,
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
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub extra_args: String,
    #[serde(default)]
    pub tool: String,
    /// Built-in agent name used for status detection, resolved at build time from
    /// config's agent_detect_as map. Avoids loading config during the polling hot path.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detect_as: String,
    #[serde(default)]
    pub yolo_mode: bool,
    #[serde(default)]
    pub status: Status,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_accessed_at: Option<DateTime<Utc>>,

    // Git worktree integration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_info: Option<WorktreeInfo>,

    // Multi-repo workspace integration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_info: Option<WorkspaceInfo>,

    // Docker sandbox integration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_info: Option<SandboxInfo>,

    // Paired terminal session
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_info: Option<TerminalInfo>,

    /// Runtime-only: which profile this instance was loaded from. Not persisted to disk.
    #[serde(default, skip_serializing)]
    pub source_profile: String,

    // Push-notification per-session overrides. None means "inherit the
    // server-wide default for this event type" (WebConfig.notify_on_*).
    // Some(true)/Some(false) is an explicit user toggle and takes
    // precedence over the global. Because the overrides are per-event-
    // type, a session can opt INTO an event that is globally off (e.g.,
    // Running to Idle), not just opt out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notify_on_waiting: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notify_on_idle: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notify_on_error: Option<bool>,

    // Runtime state (not serialized)
    #[serde(skip)]
    pub last_error_check: Option<std::time::Instant>,
    #[serde(skip)]
    pub last_start_time: Option<std::time::Instant>,
    #[serde(skip)]
    pub last_error: Option<String>,
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
            extra_args: String::new(),
            tool: "claude".to_string(),
            detect_as: String::new(),
            yolo_mode: false,
            status: Status::Idle,
            created_at: Utc::now(),
            last_accessed_at: None,
            worktree_info: None,
            workspace_info: None,
            sandbox_info: None,
            terminal_info: None,
            source_profile: String::new(),
            notify_on_waiting: None,
            notify_on_idle: None,
            notify_on_error: None,
            last_error_check: None,
            last_start_time: None,
            last_error: None,
        }
    }

    pub fn is_sub_session(&self) -> bool {
        self.parent_session_id.is_some()
    }

    pub fn is_workspace(&self) -> bool {
        self.workspace_info.is_some()
    }

    pub fn is_sandboxed(&self) -> bool {
        self.sandbox_info.as_ref().is_some_and(|s| s.enabled)
    }

    pub fn is_yolo_mode(&self) -> bool {
        self.yolo_mode
    }

    pub fn has_custom_command(&self) -> bool {
        if !self.extra_args.is_empty() {
            return true;
        }
        self.has_command_override()
    }

    /// True only when the launch command differs from the agent's default
    /// binary (ignores extra_args). Use this for status-detection and
    /// restart guards where only a wrapper script matters.
    pub fn has_command_override(&self) -> bool {
        if self.command.is_empty() {
            return false;
        }
        crate::agents::get_agent(&self.tool)
            .map(|a| self.command != a.binary)
            .unwrap_or(true)
    }

    pub fn expects_shell(&self) -> bool {
        crate::tmux::utils::is_shell_command(self.get_tool_command())
    }

    pub fn get_tool_command(&self) -> &str {
        if self.command.is_empty() {
            crate::agents::get_agent(&self.tool)
                .map(|a| a.binary)
                .unwrap_or("bash")
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
            session.create_with_size(&self.project_path, None, size)?;
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

    pub fn container_terminal_tmux_session(&self) -> Result<tmux::ContainerTerminalSession> {
        tmux::ContainerTerminalSession::new(&self.id, &self.title)
    }

    pub fn has_container_terminal(&self) -> bool {
        self.container_terminal_tmux_session()
            .map(|s| s.exists())
            .unwrap_or(false)
    }

    pub fn start_container_terminal_with_size(&mut self, size: Option<(u16, u16)>) -> Result<()> {
        if !self.is_sandboxed() {
            anyhow::bail!("Cannot create container terminal for non-sandboxed session");
        }

        let container = self.get_container_for_instance()?;
        let sandbox = self.sandbox_info.as_ref().unwrap();

        let env_info = build_docker_env_args(sandbox, std::path::Path::new(&self.project_path));
        let env_part = if env_info.docker_args.is_empty() {
            String::new()
        } else {
            format!("{} ", env_info.docker_args)
        };

        // Get workspace path inside container (handles bare repo worktrees correctly)
        let container_workdir = self.container_workdir();

        let cmd = container.exec_command(
            Some(&format!("-w {} {}", container_workdir, env_part)),
            "/bin/bash",
        );

        // If there are secret env vars, prepend shell exports and use `exec`
        // so the outer shell (whose argv briefly contains the export values)
        // is replaced immediately, keeping secrets out of long-lived process argv.
        let session_cmd = if env_info.exports.is_empty() {
            cmd
        } else {
            let exports = env_info.exports.join("; ");
            format!("{}; exec {}", exports, cmd)
        };

        let session = self.container_terminal_tmux_session()?;
        let is_new = !session.exists();
        if is_new {
            session.create_with_size(&self.project_path, Some(&session_cmd), size)?;
            self.apply_container_terminal_tmux_options();
        }

        Ok(())
    }

    pub fn kill_container_terminal(&self) -> Result<()> {
        let session = self.container_terminal_tmux_session()?;
        if session.exists() {
            session.kill()?;
        }
        Ok(())
    }

    fn sandbox_display(&self) -> Option<crate::tmux::status_bar::SandboxDisplay> {
        self.sandbox_info.as_ref().and_then(|s| {
            if s.enabled {
                Some(crate::tmux::status_bar::SandboxDisplay {
                    container_name: s.container_name.clone(),
                })
            } else {
                None
            }
        })
    }

    /// Apply all configured tmux options to a session with the given name and title.
    fn apply_session_tmux_options(&self, session_name: &str, display_title: &str) {
        let branch = self
            .worktree_info
            .as_ref()
            .map(|w| w.branch.as_str())
            .or_else(|| self.workspace_info.as_ref().map(|w| w.branch.as_str()));
        let sandbox = self.sandbox_display();
        crate::tmux::status_bar::apply_all_tmux_options(
            session_name,
            display_title,
            branch,
            sandbox.as_ref(),
        );
    }

    fn apply_container_terminal_tmux_options(&self) {
        let name = tmux::ContainerTerminalSession::generate_name(&self.id, &self.title);
        self.apply_session_tmux_options(&name, &format!("{} (container)", self.title));
    }

    pub fn start(&mut self) -> Result<()> {
        self.start_with_size(None)
    }

    pub fn start_with_size(&mut self, size: Option<(u16, u16)>) -> Result<()> {
        self.start_with_size_opts(size, false)
    }

    /// Start the session, optionally skipping on_launch hooks (e.g. when they
    /// already ran in the background creation poller).
    pub fn start_with_size_opts(
        &mut self,
        size: Option<(u16, u16)>,
        skip_on_launch: bool,
    ) -> Result<()> {
        let session = self.tmux_session()?;

        if session.exists() {
            return Ok(());
        }

        // Resolve on_launch hooks from the full config chain (global > profile > repo).
        // Repo hooks go through trust verification; global/profile hooks are implicitly trusted.
        let on_launch_hooks = if skip_on_launch {
            None
        } else {
            // Start with global+profile hooks as the base
            let profile = super::config::resolve_default_profile();
            let mut resolved_on_launch = super::profile_config::resolve_config(&profile)
                .map(|c| c.hooks.on_launch)
                .unwrap_or_default();

            // Check if repo has trusted hooks that override
            match super::repo_config::check_hook_trust(Path::new(&self.project_path)) {
                Ok(super::repo_config::HookTrustStatus::Trusted(hooks))
                    if !hooks.on_launch.is_empty() =>
                {
                    resolved_on_launch = hooks.on_launch.clone();
                }
                _ => {}
            }

            if resolved_on_launch.is_empty() {
                None
            } else {
                Some(resolved_on_launch)
            }
        };

        // Install status-detection hooks for agents that support them
        let agent = crate::agents::get_agent(&self.tool);
        let hooks_enabled = crate::session::config::Config::load()
            .map(|c| c.session.agent_status_hooks)
            .unwrap_or(true);
        if hooks_enabled {
            if self.tool == "settl" {
                // settl uses TOML config, not JSON settings
                if let Err(e) = crate::hooks::install_settl_hooks() {
                    tracing::warn!("Failed to install settl hooks: {}", e);
                }
            } else if let Some(hook_cfg) = agent.and_then(|a| a.hook_config.as_ref()) {
                if self.is_sandboxed() {
                    // For sandboxed sessions, hooks are installed via build_container_config
                } else {
                    // Install hooks in the user's home directory settings
                    if let Some(home) = dirs::home_dir() {
                        let settings_path = home.join(hook_cfg.settings_rel_path);
                        if let Err(e) = crate::hooks::install_hooks(&settings_path, hook_cfg.events)
                        {
                            tracing::warn!("Failed to install agent hooks: {}", e);
                        }
                    }
                }
            }
        }

        let cmd = if self.is_sandboxed() {
            let container = self.get_container_for_instance()?;
            // Run on_launch hooks inside the container
            if let Some(ref hook_cmds) = on_launch_hooks {
                if let Some(ref sandbox) = self.sandbox_info {
                    let workdir = self.container_workdir();
                    if let Err(e) = super::repo_config::execute_hooks_in_container(
                        hook_cmds,
                        &sandbox.container_name,
                        &workdir,
                    ) {
                        tracing::warn!("on_launch hook failed in container: {}", e);
                    }
                }
            }

            let sandbox = self.sandbox_info.as_ref().unwrap();
            let base_cmd = if self.extra_args.is_empty() {
                self.get_tool_command().to_string()
            } else {
                format!("{} {}", self.get_tool_command(), self.extra_args)
            };
            let mut tool_cmd = if self.is_yolo_mode() {
                if let Some(ref yolo) = agent.and_then(|a| a.yolo.as_ref()) {
                    match yolo {
                        crate::agents::YoloMode::CliFlag(flag) => {
                            format!("{} {}", base_cmd, flag)
                        }
                        crate::agents::YoloMode::EnvVar(..)
                        | crate::agents::YoloMode::AlwaysYolo => base_cmd,
                    }
                } else {
                    base_cmd
                }
            } else {
                base_cmd
            };
            if let Some(ref instruction) = sandbox.custom_instruction {
                if !instruction.is_empty() {
                    if let Some(flag_template) = agent.and_then(|a| a.instruction_flag) {
                        let escaped = shell_escape(instruction);
                        let flag = flag_template.replace("{}", &escaped);
                        tool_cmd = format!("{} {}", tool_cmd, flag);
                    }
                }
            }

            let env_info = build_docker_env_args(sandbox, std::path::Path::new(&self.project_path));
            // AOE_INSTANCE_ID is not secret, goes directly in docker args
            let docker_args = format!("{} -e AOE_INSTANCE_ID={}", env_info.docker_args, self.id);
            let env_part = format!("{} ", docker_args);
            let wrapped =
                wrap_command_ignore_suspend(&container.exec_command(Some(&env_part), &tool_cmd));
            if env_info.exports.is_empty() {
                Some(wrapped)
            } else {
                // Prepend shell exports for secret env vars. The outer shell
                // runs the exports (builtins), then `exec` replaces it with
                // the wrapped command. This keeps secret values out of all
                // long-lived process argv: the outer shell's argv (which
                // contains the export values) disappears in milliseconds
                // when exec replaces the process image.
                let exports = env_info.exports.join("; ");
                Some(format!("{}; exec {}", exports, wrapped))
            }
        } else {
            // Run on_launch hooks on host for non-sandboxed sessions
            if let Some(ref hook_cmds) = on_launch_hooks {
                if let Err(e) =
                    super::repo_config::execute_hooks(hook_cmds, Path::new(&self.project_path))
                {
                    tracing::warn!("on_launch hook failed: {}", e);
                }
            }

            // Prepend AOE_INSTANCE_ID env var if this agent supports hooks
            // (either JSON-based hook_config or settl's TOML hooks)
            let has_hooks =
                agent.and_then(|a| a.hook_config.as_ref()).is_some() || self.tool == "settl";
            let env_prefix = if has_hooks {
                format!("AOE_INSTANCE_ID={} ", self.id)
            } else {
                String::new()
            };

            if self.command.is_empty() {
                crate::agents::get_agent(&self.tool).map(|a| {
                    let mut cmd = a.binary.to_string();
                    if !self.extra_args.is_empty() {
                        cmd = format!("{} {}", cmd, self.extra_args);
                    }
                    if self.is_yolo_mode() {
                        if let Some(ref yolo) = a.yolo {
                            match yolo {
                                crate::agents::YoloMode::CliFlag(flag) => {
                                    cmd = format!("{} {}", cmd, flag);
                                }
                                crate::agents::YoloMode::EnvVar(key, value) => {
                                    cmd = format_env_var_prefix(key, value, &cmd);
                                }
                                crate::agents::YoloMode::AlwaysYolo => {}
                            }
                        }
                    }
                    wrap_command_ignore_suspend(&format!("{}{}", env_prefix, cmd))
                })
            } else {
                let mut cmd = self.command.clone();
                if !self.extra_args.is_empty() {
                    cmd = format!("{} {}", cmd, self.extra_args);
                }
                if self.is_yolo_mode() {
                    if let Some(ref yolo) = agent.and_then(|a| a.yolo.as_ref()) {
                        match yolo {
                            crate::agents::YoloMode::CliFlag(flag) => {
                                cmd = format!("{} {}", cmd, flag);
                            }
                            crate::agents::YoloMode::EnvVar(key, value) => {
                                cmd = format_env_var_prefix(key, value, &cmd);
                            }
                            crate::agents::YoloMode::AlwaysYolo => {}
                        }
                    }
                }
                Some(wrap_command_ignore_suspend(&format!(
                    "{}{}",
                    env_prefix, cmd
                )))
            }
        };

        tracing::debug!(
            "container cmd: {}",
            cmd.as_ref().map_or("none".to_string(), |v| {
                super::environment::redact_env_values(v)
            })
        );

        session.create_with_size(&self.project_path, cmd.as_deref(), size)?;

        // Apply all configured tmux options (status bar, mouse, etc.)
        self.apply_tmux_options();

        self.status = Status::Starting;
        self.last_start_time = Some(std::time::Instant::now());

        Ok(())
    }

    fn apply_tmux_options(&self) {
        let name = tmux::Session::generate_name(&self.id, &self.title);
        self.apply_session_tmux_options(&name, &self.title);
    }

    fn apply_terminal_tmux_options(&self) {
        let name = tmux::TerminalSession::generate_name(&self.id, &self.title);
        self.apply_session_tmux_options(&name, &format!("{} (terminal)", self.title));
    }

    pub fn get_container_for_instance(&mut self) -> Result<containers::DockerContainer> {
        let sandbox = self
            .sandbox_info
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Cannot ensure container for non-sandboxed session"))?;

        let image = &sandbox.image;
        let container = DockerContainer::new(&self.id, image);

        if container.is_running()? {
            container_config::refresh_agent_configs();
            return Ok(container);
        }

        if container.exists()? {
            container_config::refresh_agent_configs();
            container.start()?;
            return Ok(container);
        }

        // Ensure image is available (always pulls to get latest)
        let runtime = containers::get_container_runtime();
        runtime.ensure_image(image)?;

        let config = self.build_container_config()?;
        let container_id = container.create(&config)?;

        if let Some(ref mut sandbox) = self.sandbox_info {
            sandbox.container_id = Some(container_id);
            sandbox.created_at = Some(Utc::now());
        }

        Ok(container)
    }

    /// Get the container working directory for this instance.
    pub fn container_workdir(&self) -> String {
        container_config::compute_volume_paths(Path::new(&self.project_path), &self.project_path)
            .map(|(_, wd)| wd)
            .unwrap_or_else(|_| "/workspace".to_string())
    }

    fn build_container_config(&self) -> Result<crate::containers::ContainerConfig> {
        container_config::build_container_config(
            &self.project_path,
            self.sandbox_info.as_ref().unwrap(),
            &self.tool,
            self.is_yolo_mode(),
            &self.id,
            self.workspace_info.as_ref(),
        )
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

    /// Stop the session: kill the tmux session and stop the Docker container
    /// (if sandboxed). The container is stopped but not removed, so it can be
    /// restarted on re-attach.
    pub fn stop(&self) -> Result<()> {
        self.kill()?;

        if self.is_sandboxed() {
            let container = containers::DockerContainer::from_session_id(&self.id);
            if container.is_running().unwrap_or(false) {
                container.stop()?;
            }
        }

        crate::hooks::cleanup_hook_status_dir(&self.id);

        Ok(())
    }

    /// Update status using pre-fetched pane metadata to avoid per-instance
    /// subprocess spawns. Falls back to subprocess calls if metadata is missing.
    pub fn update_status_with_metadata(&mut self, metadata: Option<&tmux::PaneMetadata>) {
        if matches!(
            self.status,
            Status::Stopped | Status::Deleting | Status::Creating
        ) {
            return;
        }

        if self.status == Status::Error {
            if let Some(last_check) = self.last_error_check {
                if last_check.elapsed().as_secs() < 30 {
                    return;
                }
            }
        }

        if let Some(start_time) = self.last_start_time {
            if start_time.elapsed().as_secs() < 3 {
                self.status = Status::Starting;
                return;
            }
        }

        let session = match self.tmux_session() {
            Ok(s) => s,
            Err(_) => {
                tracing::trace!(
                    "status '{}': tmux_session() failed, setting Error",
                    self.title
                );
                self.status = Status::Error;
                if self.last_error.is_none() {
                    self.last_error = Some(
                        "Could not reach tmux. Is tmux still running on the host?".to_string(),
                    );
                }
                self.last_error_check = Some(std::time::Instant::now());
                return;
            }
        };

        if !session.exists() {
            tracing::trace!(
                "status '{}': session.exists()=false (tmux name={}), setting Error",
                self.title,
                tmux::Session::generate_name(&self.id, &self.title)
            );
            self.status = Status::Error;
            if self.last_error.is_none() {
                self.last_error = Some(
                    "tmux session is gone. The agent process may have exited or been killed."
                        .to_string(),
                );
            }
            self.last_error_check = Some(std::time::Instant::now());
            return;
        }

        let is_dead = metadata
            .map(|m| m.pane_dead)
            .unwrap_or_else(|| session.is_pane_dead());

        let pane_cmd = metadata
            .and_then(|m| m.pane_current_command.clone())
            .or_else(|| {
                let name = tmux::Session::generate_name(&self.id, &self.title);
                tmux::utils::pane_current_command(&name)
            });

        tracing::trace!(
            "status '{}': exists=true, is_dead={}, pane_cmd={:?}, tool={}, cmd_override={}",
            self.title,
            is_dead,
            pane_cmd,
            self.tool,
            self.has_command_override()
        );

        if let Some(hook_status) = crate::hooks::read_hook_status(&self.id) {
            tracing::trace!(
                "status '{}': hook detected {:?}, is_dead={}",
                self.title,
                hook_status,
                is_dead
            );
            if is_dead {
                self.status = Status::Error;
                if self.last_error.is_none() {
                    let pane_content = session.capture_pane(20).unwrap_or_default();
                    self.last_error = Some(summarize_error_from_pane(&pane_content));
                }
            } else {
                self.status = hook_status;
                self.last_error = None;
            }
            return;
        }

        let pane_content = session.capture_pane(50).unwrap_or_default();
        let detection_tool = if self.detect_as.is_empty() {
            &self.tool
        } else {
            &self.detect_as
        };
        let detected = tmux::detect_status_from_content(&pane_content, detection_tool);
        tracing::trace!(
            "status '{}': detected={:?}, cmd_override={}, custom_cmd={}",
            self.title,
            detected,
            self.has_command_override(),
            self.has_custom_command(),
        );
        let is_shell_stale = || {
            let expects = self.expects_shell();
            if expects {
                return false;
            }
            let shell_check = metadata
                .and_then(|m| m.pane_current_command.as_deref())
                .map(tmux::utils::is_shell_command)
                .unwrap_or_else(|| session.is_pane_running_shell());
            tracing::trace!(
                "status '{}': is_shell_stale check: expects_shell={}, shell_check={}",
                self.title,
                expects,
                shell_check,
            );
            shell_check
        };
        self.status = match detected {
            Status::Idle if self.has_command_override() => {
                // Custom commands run agents through wrapper scripts that appear
                // as shell processes to tmux. Only declare Error when the pane is
                // actually dead; don't use is_shell_stale() since the shell IS
                // the expected wrapper process.
                if is_dead {
                    Status::Error
                } else {
                    Status::Unknown
                }
            }
            Status::Idle if is_dead => Status::Error,
            Status::Idle if is_shell_stale() => {
                // A shell is the foreground process but the pane is alive.
                // Check captured pane content: if it contains the agent's
                // UI the agent is still alive; only declare Error when the
                // content looks like a bare shell prompt.
                if pane_has_agent_content(&pane_content, &self.tool) {
                    tracing::trace!(
                        "status '{}': shell stale but pane has agent content, staying Idle",
                        self.title,
                    );
                    Status::Idle
                } else {
                    tracing::trace!(
                        "status '{}': shell stale, no agent content, setting Error",
                        self.title,
                    );
                    Status::Error
                }
            }
            other => other,
        };

        tracing::trace!("status '{}': final={:?}", self.title, self.status);

        if self.status == Status::Error {
            if self.last_error.is_none() {
                self.last_error = Some(summarize_error_from_pane(&pane_content));
            }
        } else {
            self.last_error = None;
        }
    }

    pub fn update_status(&mut self) {
        self.update_status_with_metadata(None);
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

/// Build a short human-readable hint for why a session transitioned to Error.
///
/// Called when we set Status::Error but don't already have a `last_error`
/// populated (e.g. an agent process exited on its own). We grab the last few
/// non-empty lines of the pane and pick something that looks like an error
/// message; otherwise fall back to a generic "stopped responding" string so
/// the UI never renders an Error state without any explanation.
fn summarize_error_from_pane(pane_content: &str) -> String {
    let cleaned = crate::tmux::utils::strip_ansi(pane_content);
    let tail: Vec<&str> = cleaned
        .lines()
        .rev()
        .map(|l| l.trim_end())
        .filter(|l| !l.is_empty())
        .take(12)
        .collect();

    for line in &tail {
        let lower = line.to_lowercase();
        if lower.contains("error")
            || lower.contains("command not found")
            || lower.contains("permission denied")
            || lower.contains("cannot")
            || lower.contains("failed")
            || lower.contains("no such file")
            || lower.contains("traceback")
            || lower.contains("panic")
        {
            return truncate_error_line(line);
        }
    }

    if let Some(last) = tail.first() {
        return format!(
            "Agent stopped responding. Last line: {}",
            truncate_error_line(last)
        );
    }

    "Agent stopped responding and the pane is empty.".to_string()
}

fn truncate_error_line(line: &str) -> String {
    const MAX: usize = 200;
    let trimmed = line.trim();
    if trimmed.len() <= MAX {
        trimmed.to_string()
    } else {
        let mut out = String::with_capacity(MAX + 1);
        for (i, ch) in trimmed.char_indices() {
            if i >= MAX {
                break;
            }
            out.push(ch);
        }
        out.push('…');
        out
    }
}

/// Format an environment variable assignment as a shell-safe command prefix.
///
/// Uses `shell_escape` (single-quote escaping) so the value is preserved
/// verbatim when parsed by the inner `bash -c '...'` shell created by
/// `wrap_command_ignore_suspend`.
fn format_env_var_prefix(key: &str, value: &str, cmd: &str) -> String {
    let escaped = shell_escape(value);
    format!("{}={} {}", key, escaped, cmd)
}

/// Wrap a command to disable Ctrl-Z (SIGTSTP) suspension.
///
/// When running agents directly as tmux session commands (without a parent shell),
/// pressing Ctrl-Z suspends the process with no way to recover via job control.
/// This wrapper disables the suspend character at the terminal level before exec'ing
/// the actual command.
///
/// Uses POSIX-standard `stty susp undef` which works on both Linux and macOS.
/// Single quotes in `cmd` are escaped with the `'\''` technique to prevent
/// breaking out of the outer single-quoted wrapper.
fn wrap_command_ignore_suspend(cmd: &str) -> String {
    let user = super::environment::user_shell();
    let posix = super::environment::user_posix_shell();
    let escaped = cmd.replace('\'', "'\\''");
    // Use login shell (-l) so version-manager PATHs (NVM, etc.) are available.
    // Skip -l when falling back to bash for a non-POSIX user shell (fish, nu,
    // pwsh): bash's login scripts won't contain the user's PATH setup and -l
    // may reset the inherited PATH that already has the correct entries.
    let flag = if user == posix { "-lc" } else { "-c" };
    format!("{} {} 'stty susp undef; exec env {}'", posix, flag, escaped)
}

/// Check whether captured pane content indicates a living agent rather than
/// a bare shell prompt. Used to prevent `is_shell_stale()` from producing
/// false `Error` status when the agent binary is a shell wrapper or spawns
/// persistent child shell processes.
fn pane_has_agent_content(raw_content: &str, tool: &str) -> bool {
    let clean = crate::tmux::utils::strip_ansi(raw_content);
    let non_empty: Vec<&str> = clean.lines().filter(|l| !l.trim().is_empty()).collect();

    if non_empty.is_empty() {
        return false;
    }

    // If the last visible line looks like a shell prompt, the agent
    // likely exited and the shell took over. This catches servers with
    // verbose MOTD that would otherwise exceed the line-count threshold.
    let last = non_empty.last().unwrap().trim();
    if last.ends_with('$')
        || last.ends_with('#')
        || last.ends_with('%')
        || last.ends_with('\u{276f}')
    {
        return false;
    }

    // Agent TUIs fill the screen with UI elements. A bare shell prompt
    // (after MOTD) rarely exceeds this threshold once the prompt check
    // above filters out typical shell endings.
    if non_empty.len() > 5 {
        return true;
    }

    // Use word-boundary matching so short names like "pi" don't produce
    // false positives inside words like "api" or "pipeline".
    let tool_lower = tool.to_lowercase();
    let lower = clean.to_lowercase();
    if lower
        .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
        .any(|word| word == tool_lower)
    {
        return true;
    }

    false
}

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
    fn test_all_agents_have_yolo_support() {
        for agent in crate::agents::AGENTS {
            assert!(
                agent.yolo.is_some(),
                "Agent '{}' should have YOLO mode configured",
                agent.name
            );
        }
    }

    #[test]
    fn test_yolo_mode_helper() {
        let mut inst = Instance::new("test", "/tmp/test");
        assert!(!inst.is_yolo_mode());

        inst.yolo_mode = true;
        assert!(inst.is_yolo_mode());

        inst.yolo_mode = false;
        assert!(!inst.is_yolo_mode());
    }

    #[test]
    fn test_yolo_mode_without_sandbox() {
        let mut inst = Instance::new("test", "/tmp/test");
        assert!(!inst.is_sandboxed());

        inst.yolo_mode = true;
        assert!(inst.is_yolo_mode());
        assert!(!inst.is_sandboxed());
    }

    #[test]
    fn test_yolo_envvar_command_is_quoted() {
        // EnvVar values containing JSON must be shell-escaped to prevent
        // the inner bash from expanding special characters ({, *, ").
        let result = format_env_var_prefix("OPENCODE_PERMISSION", r#"{"*":"allow"}"#, "opencode");
        assert_eq!(result, r#"OPENCODE_PERMISSION='{"*":"allow"}' opencode"#);
    }

    #[test]
    fn test_yolo_envvar_survives_suspend_wrapper() {
        // The full chain: format_env_var_prefix -> wrap_command_ignore_suspend
        // must preserve the JSON value through both quoting layers.
        // Single quotes from shell_escape are escaped by wrap_command_ignore_suspend
        // via the '\'' technique, which correctly round-trips through the shell.
        let cmd = format_env_var_prefix("OPENCODE_PERMISSION", r#"{"*":"allow"}"#, "opencode");
        let wrapped = wrap_command_ignore_suspend(&cmd);
        // The inner single quotes from shell_escape become '\'' in the outer wrapper
        assert!(
            wrapped.contains(r#"OPENCODE_PERMISSION='\''{"*":"allow"}'\'' opencode"#),
            "wrapped command should contain the escaped env var assignment: {}",
            wrapped,
        );
    }

    #[test]
    #[serial_test::serial(shell_env)]
    fn test_wrap_command_posix_shell_uses_login() {
        let original = std::env::var("SHELL").ok();
        std::env::set_var("SHELL", "/bin/zsh");
        let wrapped = wrap_command_ignore_suspend("claude");
        // POSIX shell: should use -lc for version-manager PATHs
        assert!(
            wrapped.contains("-lc"),
            "POSIX shell should use -lc: {}",
            wrapped,
        );
        match original {
            Some(v) => std::env::set_var("SHELL", v),
            None => std::env::remove_var("SHELL"),
        }
    }

    #[test]
    #[serial_test::serial(shell_env)]
    fn test_wrap_command_fish_skips_login() {
        let original = std::env::var("SHELL").ok();
        std::env::set_var("SHELL", "/usr/bin/fish");
        let wrapped = wrap_command_ignore_suspend("claude");
        // Fish: should use -c (no -l) because bash's login scripts
        // won't have fish's PATH setup.
        assert!(
            wrapped.starts_with("bash -c "),
            "fish shell should fall back to bash -c (no login): {}",
            wrapped,
        );
        assert!(
            !wrapped.contains("-lc"),
            "fish shell should NOT use -lc: {}",
            wrapped,
        );
        match original {
            Some(v) => std::env::set_var("SHELL", v),
            None => std::env::remove_var("SHELL"),
        }
    }

    #[test]
    #[serial_test::serial(shell_env)]
    fn test_wrap_command_nu_skips_login() {
        let original = std::env::var("SHELL").ok();
        std::env::set_var("SHELL", "/usr/bin/nu");
        let wrapped = wrap_command_ignore_suspend("claude");
        assert!(
            wrapped.starts_with("bash -c "),
            "nu shell should fall back to bash -c (no login): {}",
            wrapped,
        );
        match original {
            Some(v) => std::env::set_var("SHELL", v),
            None => std::env::remove_var("SHELL"),
        }
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
            extra_env: None,
            custom_instruction: None,
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
            extra_env: None,
            custom_instruction: None,
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
    fn test_get_tool_command_gemini() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.tool = "gemini".to_string();
        assert_eq!(inst.get_tool_command(), "gemini");
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
            Status::Unknown,
            Status::Stopped,
            Status::Error,
            Status::Starting,
            Status::Deleting,
            Status::Creating,
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
        };

        let json = serde_json::to_string(&info).unwrap();
        let deserialized: WorktreeInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(info.branch, deserialized.branch);
        assert_eq!(info.main_repo_path, deserialized.main_repo_path);
        assert_eq!(info.managed_by_aoe, deserialized.managed_by_aoe);
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
            extra_env: Some(vec!["MY_VAR".to_string(), "OTHER_VAR".to_string()]),
            custom_instruction: None,
        };

        let json = serde_json::to_string(&info).unwrap();
        let deserialized: SandboxInfo = serde_json::from_str(&json).unwrap();

        assert_eq!(info.enabled, deserialized.enabled);
        assert_eq!(info.container_id, deserialized.container_id);
        assert_eq!(info.image, deserialized.image);
        assert_eq!(info.container_name, deserialized.container_name);
        assert_eq!(info.extra_env, deserialized.extra_env);
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
        });

        let json = serde_json::to_string(&inst).unwrap();
        let deserialized: Instance = serde_json::from_str(&json).unwrap();

        assert!(deserialized.worktree_info.is_some());
        let wt = deserialized.worktree_info.unwrap();
        assert_eq!(wt.branch, "feature/abc");
        assert!(wt.managed_by_aoe);
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

    #[test]
    fn test_has_custom_command_empty() {
        let inst = Instance::new("test", "/tmp/test");
        assert!(!inst.has_custom_command());
    }

    #[test]
    fn test_has_custom_command_same_as_agent_binary() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.tool = "claude".to_string();
        inst.command = "claude".to_string();
        assert!(!inst.has_custom_command());
    }

    #[test]
    fn test_has_custom_command_override() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.tool = "claude".to_string();
        inst.command = "my-wrapper".to_string();
        assert!(inst.has_custom_command());
    }

    #[test]
    fn test_has_custom_command_unknown_tool() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.tool = "unknown_agent".to_string();
        inst.command = "some-binary".to_string();
        assert!(inst.has_custom_command());
    }

    #[test]
    fn test_has_command_override_extra_args_only() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.tool = "claude".to_string();
        inst.extra_args = "--model opus".to_string();
        assert!(!inst.has_command_override());
        assert!(inst.has_custom_command());
    }

    #[test]
    fn test_expects_shell() {
        let mut inst = Instance::new("test", "/tmp/test");
        assert!(!inst.expects_shell());

        inst.tool = "unknown-tool".to_string();
        inst.command = String::new();
        assert!(inst.expects_shell());

        inst.tool = "claude".to_string();
        inst.command = "bash".to_string();
        assert!(inst.expects_shell());

        inst.command = "my-agent".to_string();
        assert!(!inst.expects_shell());
    }

    #[test]
    fn test_status_unknown_serialization() {
        let status = Status::Unknown;
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, "\"unknown\"");
        let deserialized: Status = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, Status::Unknown);
    }

    #[test]
    fn test_pane_has_agent_content_bare_shell() {
        assert!(!pane_has_agent_content("$ ", "opencode"));
        assert!(!pane_has_agent_content("user@host:~$ ", "opencode"));
        assert!(!pane_has_agent_content("\n\n$ \n", "opencode"));
    }

    #[test]
    fn test_pane_has_agent_content_agent_ui() {
        let opencode_idle = "ctrl+p commands \u{2022} OpenCode 1.3.13+650d0db";
        assert!(pane_has_agent_content(opencode_idle, "opencode"));
    }

    #[test]
    fn test_pane_has_agent_content_substantial_output() {
        let many_lines = (0..10)
            .map(|i| format!("line {i}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(pane_has_agent_content(&many_lines, "vibe"));
    }

    #[test]
    fn test_pane_has_agent_content_empty() {
        assert!(!pane_has_agent_content("", "opencode"));
        assert!(!pane_has_agent_content("   \n  \n  ", "opencode"));
    }

    #[test]
    fn test_pane_has_agent_content_shell_prompt_at_end() {
        // Verbose MOTD followed by shell prompt should be detected as a
        // bare shell, not agent content, even with >5 lines.
        let motd_then_prompt = "Welcome to Ubuntu 22.04 LTS\n\
            System load:  0.5\n\
            Memory usage: 42%\n\
            Disk usage:   67%\n\
            Swap usage:   0%\n\
            Temperature:  45C\n\
            2 updates available\n\
            user@host:~$ ";
        assert!(!pane_has_agent_content(motd_then_prompt, "opencode"));

        // Same with # prompt (root)
        let root_prompt = "line1\nline2\nline3\nline4\nline5\nline6\n# ";
        assert!(!pane_has_agent_content(root_prompt, "opencode"));

        // Fish/zsh fancy prompt (❯)
        let fancy_prompt = "line1\nline2\nline3\nline4\nline5\nline6\n\u{276f}";
        assert!(!pane_has_agent_content(fancy_prompt, "opencode"));
    }

    #[test]
    fn test_pane_has_agent_content_short_tool_name() {
        // Short tool names like "pi" should NOT match substrings in
        // unrelated content (e.g., "api" contains "pi").
        assert!(!pane_has_agent_content("api endpoint ready", "pi"));
        assert!(!pane_has_agent_content("pipeline started", "pi"));

        // But "pi" as a standalone word should match.
        assert!(pane_has_agent_content("pi file saved", "pi"));
        assert!(pane_has_agent_content("done\npi>", "pi"));

        // Longer names like "opencode" should still match.
        assert!(pane_has_agent_content("OpenCode v1.0", "opencode"));
    }
}
