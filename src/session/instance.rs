//! Session instance definition and operations

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::docker::{
    self, ContainerConfig, DockerContainer, VolumeMount, CLAUDE_AUTH_VOLUME, CODEX_AUTH_VOLUME,
    GEMINI_AUTH_VOLUME, OPENCODE_AUTH_VOLUME, VIBE_AUTH_VOLUME,
};
use crate::git::GitWorktree;
use crate::tmux;

fn default_true() -> bool {
    true
}

/// Terminal environment variables that are always passed through for proper UI/theming
const DEFAULT_TERMINAL_ENV_VARS: &[&str] = &["TERM", "COLORTERM", "FORCE_COLOR", "NO_COLOR"];

/// Shell-escape a value for safe interpolation into a shell command string.
/// Uses double-quote escaping so values can be nested inside `bash -c '...'`
/// (single quotes in the outer wrapper are literal, double quotes work inside).
fn shell_escape(val: &str) -> String {
    let escaped = val
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('$', "\\$")
        .replace('`', "\\`")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    format!("\"{}\"", escaped)
}

/// Resolve an environment_values entry. If the value starts with `$`, read the
/// named variable from the host environment (use `$$` to escape a literal `$`).
/// Otherwise return the literal value.
fn resolve_env_value(val: &str) -> Option<String> {
    if let Some(rest) = val.strip_prefix("$$") {
        Some(format!("${}", rest))
    } else if let Some(var_name) = val.strip_prefix('$') {
        std::env::var(var_name).ok()
    } else {
        Some(val.to_string())
    }
}

/// Collect all environment variable keys from defaults, global config, and per-session extras.
fn collect_env_keys(
    sandbox_config: &super::config::SandboxConfig,
    sandbox_info: &SandboxInfo,
) -> Vec<String> {
    let mut env_keys: Vec<String> = DEFAULT_TERMINAL_ENV_VARS
        .iter()
        .map(|s| s.to_string())
        .collect();

    for key in &sandbox_config.environment {
        if !env_keys.contains(key) {
            env_keys.push(key.clone());
        }
    }

    if let Some(extra_keys) = &sandbox_info.extra_env_keys {
        for key in extra_keys {
            if !env_keys.contains(key) {
                env_keys.push(key.clone());
            }
        }
    }

    env_keys
}

/// Collect all key=value environment pairs from global config and per-session extras.
fn collect_env_values(
    sandbox_config: &super::config::SandboxConfig,
    sandbox_info: &SandboxInfo,
) -> Vec<(String, String)> {
    let mut values = Vec::new();

    for (key, val) in &sandbox_config.environment_values {
        if let Some(resolved) = resolve_env_value(val) {
            values.push((key.clone(), resolved));
        }
    }

    if let Some(extra_vals) = &sandbox_info.extra_env_values {
        for (key, val) in extra_vals {
            if let Some(resolved) = resolve_env_value(val) {
                values.push((key.clone(), resolved));
            }
        }
    }

    values
}

/// Build docker exec environment flags from config and optional per-session extra keys.
/// Used for `docker exec` commands (shell string interpolation, hence shell-escaping).
/// Container creation uses `ContainerConfig.environment` (separate args, no escaping needed).
fn build_docker_env_args(sandbox: &SandboxInfo) -> String {
    let config = super::config::Config::load().unwrap_or_default();

    let env_keys = collect_env_keys(&config.sandbox, sandbox);

    let mut args: Vec<String> = env_keys
        .iter()
        .filter_map(|key| {
            std::env::var(key)
                .ok()
                .map(|val| format!("-e {}={}", key, shell_escape(&val)))
        })
        .collect();

    for (key, resolved) in collect_env_values(&config.sandbox, sandbox) {
        args.push(format!("-e {}={}", key, shell_escape(&resolved)));
    }

    args.join(" ")
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
    /// Additional KEY=VALUE environment variables (session-specific overrides)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_env_values: Option<std::collections::HashMap<String, String>>,
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
    #[serde(default)]
    pub tool: String,
    #[serde(default)]
    pub status: Status,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_accessed_at: Option<DateTime<Utc>>,

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
                "gemini" => "gemini",
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

        self.ensure_container_running()?;
        let sandbox = self.sandbox_info.as_ref().unwrap();

        let env_args = build_docker_env_args(sandbox);
        let env_part = if env_args.is_empty() {
            String::new()
        } else {
            format!("{} ", env_args)
        };

        // Get workspace path inside container (handles bare repo worktrees correctly)
        let project_path = std::path::Path::new(&self.project_path);
        let (_, _, container_workdir) = self.compute_volume_paths(project_path)?;

        let cmd = format!(
            "docker exec -it -w {} {}{} /bin/bash",
            container_workdir, env_part, sandbox.container_name
        );

        let session = self.container_terminal_tmux_session()?;
        let is_new = !session.exists();
        if is_new {
            session.create_with_size(&self.project_path, Some(&cmd), size)?;
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

    /// Apply all configured tmux options to the container terminal session.
    fn apply_container_terminal_tmux_options(&self) {
        let session_name = tmux::ContainerTerminalSession::generate_name(&self.id, &self.title);
        let terminal_title = format!("{} (container)", self.title);
        let branch = self.worktree_info.as_ref().map(|w| w.branch.as_str());
        let sandbox = self.sandbox_display();

        crate::tmux::status_bar::apply_all_tmux_options(
            &session_name,
            &terminal_title,
            branch,
            sandbox.as_ref(),
        );
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
            let profile = super::config::Config::load()
                .map(|c| c.default_profile)
                .unwrap_or_else(|_| "default".to_string());
            let mut resolved_on_launch = super::profile_config::resolve_config(&profile)
                .map(|c| c.hooks.on_launch)
                .unwrap_or_default();

            // Check if repo has trusted hooks that override
            match super::repo_config::check_hook_trust(std::path::Path::new(&self.project_path)) {
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

        let cmd = if self.is_sandboxed() {
            self.ensure_container_running()?;

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
            let mut tool_cmd = if self.is_yolo_mode() {
                match self.tool.as_str() {
                    "claude" => "claude --dangerously-skip-permissions".to_string(),
                    "vibe" => "vibe --agent auto-approve".to_string(),
                    "codex" => "codex --dangerously-bypass-approvals-and-sandbox".to_string(),
                    "gemini" => "gemini --approval-mode yolo".to_string(),
                    _ => self.get_tool_command().to_string(),
                }
            } else {
                self.get_tool_command().to_string()
            };
            // Inject custom instruction CLI flags for supported agents
            if let Some(ref instruction) = sandbox.custom_instruction {
                if !instruction.is_empty() {
                    let escaped = shell_escape(instruction);
                    tool_cmd = match self.tool.as_str() {
                        "claude" => format!("{} --append-system-prompt {}", tool_cmd, escaped),
                        "codex" => {
                            format!("{} --config developer_instructions={}", tool_cmd, escaped)
                        }
                        _ => tool_cmd,
                    };
                }
            }

            let env_args = build_docker_env_args(sandbox);
            let env_part = if env_args.is_empty() {
                String::new()
            } else {
                format!("{} ", env_args)
            };
            Some(wrap_command_ignore_suspend(&format!(
                "docker exec -it {}{} {}",
                env_part, sandbox.container_name, tool_cmd
            )))
        } else {
            // Run on_launch hooks on host for non-sandboxed sessions
            if let Some(ref hook_cmds) = on_launch_hooks {
                if let Err(e) = super::repo_config::execute_hooks(
                    hook_cmds,
                    std::path::Path::new(&self.project_path),
                ) {
                    tracing::warn!("on_launch hook failed: {}", e);
                }
            }

            if self.command.is_empty() {
                match self.tool.as_str() {
                    "claude" => Some(wrap_command_ignore_suspend("claude")),
                    "vibe" => Some(wrap_command_ignore_suspend("vibe")),
                    "codex" => Some(wrap_command_ignore_suspend("codex")),
                    "gemini" => Some(wrap_command_ignore_suspend("gemini")),
                    _ => None,
                }
            } else {
                Some(wrap_command_ignore_suspend(&self.command))
            }
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
        let session_name = tmux::Session::generate_name(&self.id, &self.title);
        let branch = self.worktree_info.as_ref().map(|w| w.branch.as_str());
        let sandbox = self.sandbox_display();

        crate::tmux::status_bar::apply_all_tmux_options(
            &session_name,
            &self.title,
            branch,
            sandbox.as_ref(),
        );
    }

    /// Apply all configured tmux options to the terminal session.
    fn apply_terminal_tmux_options(&self) {
        let session_name = tmux::TerminalSession::generate_name(&self.id, &self.title);
        let terminal_title = format!("{} (terminal)", self.title);
        let branch = self.worktree_info.as_ref().map(|w| w.branch.as_str());
        let sandbox = self.sandbox_display();

        crate::tmux::status_bar::apply_all_tmux_options(
            &session_name,
            &terminal_title,
            branch,
            sandbox.as_ref(),
        );
    }

    pub fn ensure_container_running(&mut self) -> Result<()> {
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
        docker::ensure_named_volume(GEMINI_AUTH_VOLUME)?;

        let config = self.build_container_config()?;
        let container_id = container.create(&config)?;

        if let Some(ref mut sandbox) = self.sandbox_info {
            sandbox.container_id = Some(container_id);
            sandbox.created_at = Some(Utc::now());
        }

        Ok(())
    }

    /// Compute volume mount paths for Docker container.
    ///
    /// For bare repo worktrees, mounts the entire bare repo and sets working_dir to the worktree.
    /// This allows git commands inside the container to access the full repository structure.
    ///
    /// Returns (host_mount_path, container_mount_path, working_dir)
    fn compute_volume_paths(
        &self,
        project_path: &std::path::Path,
    ) -> Result<(String, String, String)> {
        // Try to find the main repo if this is a git repository
        if let Ok(main_repo) = GitWorktree::find_main_repo(project_path) {
            // Canonicalize paths for reliable comparison (handles symlinks like /tmp -> /private/tmp)
            let main_repo_canonical = main_repo
                .canonicalize()
                .unwrap_or_else(|_| main_repo.clone());
            let project_canonical = project_path
                .canonicalize()
                .unwrap_or_else(|_| project_path.to_path_buf());

            // Check if main repo is a bare repo and project_path is a worktree within it
            if GitWorktree::is_bare_repo(&main_repo) && main_repo_canonical != project_canonical {
                // Bare repo worktree: mount the entire repo, set working_dir to the worktree
                let repo_name = main_repo_canonical
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "workspace".to_string());

                // Calculate relative path from main_repo to project_path (using canonical paths)
                let relative_worktree = project_canonical
                    .strip_prefix(&main_repo_canonical)
                    .map(|p| p.to_path_buf())
                    .unwrap_or_default();

                let container_base = format!("/workspace/{}", repo_name);
                let working_dir = if relative_worktree.as_os_str().is_empty() {
                    container_base.clone()
                } else {
                    format!("{}/{}", container_base, relative_worktree.display())
                };

                return Ok((
                    main_repo_canonical.to_string_lossy().to_string(),
                    container_base,
                    working_dir,
                ));
            }
        }

        // Default behavior: mount project_path directly
        let dir_name = project_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "workspace".to_string());
        let workspace_path = format!("/workspace/{}", dir_name);

        Ok((
            self.project_path.clone(),
            workspace_path.clone(),
            workspace_path,
        ))
    }

    /// Get the container working directory for this instance.
    pub fn container_workdir(&self) -> String {
        self.compute_volume_paths(std::path::Path::new(&self.project_path))
            .map(|(_, _, wd)| wd)
            .unwrap_or_else(|_| "/workspace".to_string())
    }

    fn build_container_config(&self) -> Result<ContainerConfig> {
        let home =
            dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;

        let project_path = std::path::Path::new(&self.project_path);

        // Determine mount path and working directory.
        // For bare repo worktrees, mount the entire bare repo and set working_dir to the worktree.
        // This allows git commands to access the full repository structure.
        let (mount_host_path, container_base_path, workspace_path) =
            self.compute_volume_paths(project_path)?;

        let mut volumes = vec![VolumeMount {
            host_path: mount_host_path,
            container_path: container_base_path,
            read_only: false,
        }];

        let sandbox_config = match super::config::Config::load() {
            Ok(c) => {
                tracing::debug!(
                    "Loaded sandbox config: extra_volumes={:?}, mount_ssh={}, volume_ignores={:?}",
                    c.sandbox.extra_volumes,
                    c.sandbox.mount_ssh,
                    c.sandbox.volume_ignores
                );
                c.sandbox
            }
            Err(e) => {
                tracing::warn!("Failed to load config, using defaults: {}", e);
                Default::default()
            }
        };

        const CONTAINER_HOME: &str = "/root";

        let gitconfig = home.join(".gitconfig");
        if gitconfig.exists() {
            volumes.push(VolumeMount {
                host_path: gitconfig.to_string_lossy().to_string(),
                container_path: format!("{}/.gitconfig", CONTAINER_HOME),
                read_only: true,
            });
        }

        if sandbox_config.mount_ssh {
            let ssh_dir = home.join(".ssh");
            if ssh_dir.exists() {
                volumes.push(VolumeMount {
                    host_path: ssh_dir.to_string_lossy().to_string(),
                    container_path: format!("{}/.ssh", CONTAINER_HOME),
                    read_only: true,
                });
            }
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
            (
                GEMINI_AUTH_VOLUME.to_string(),
                format!("{}/.gemini", CONTAINER_HOME),
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

        let sandbox_info = self.sandbox_info.as_ref().unwrap();
        let env_keys = collect_env_keys(&sandbox_config, sandbox_info);

        let mut environment: Vec<(String, String)> = env_keys
            .iter()
            .filter_map(|key| std::env::var(key).ok().map(|val| (key.clone(), val)))
            .collect();

        environment.push((
            "CLAUDE_CONFIG_DIR".to_string(),
            format!("{}/.claude", CONTAINER_HOME),
        ));

        environment.extend(collect_env_values(&sandbox_config, sandbox_info));

        if self.is_yolo_mode() && self.tool == "opencode" {
            environment.push((
                "OPENCODE_PERMISSION".to_string(),
                r#"{"*":"allow"}"#.to_string(),
            ));
        }

        // Add extra_volumes from config (host:container format)
        // Also collect container paths to filter conflicting volume_ignores later
        tracing::debug!(
            "extra_volumes from config: {:?}",
            sandbox_config.extra_volumes
        );
        let mut extra_volume_container_paths: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for entry in &sandbox_config.extra_volumes {
            let parts: Vec<&str> = entry.splitn(3, ':').collect();
            if parts.len() >= 2 {
                tracing::info!(
                    "Mounting extra volume: {} -> {} (ro: {})",
                    parts[0],
                    parts[1],
                    parts.get(2) == Some(&"ro")
                );
                extra_volume_container_paths.insert(parts[1].to_string());
                volumes.push(VolumeMount {
                    host_path: parts[0].to_string(),
                    container_path: parts[1].to_string(),
                    read_only: parts.get(2) == Some(&"ro"),
                });
            } else {
                tracing::warn!("Ignoring malformed extra_volume entry: {}", entry);
            }
        }

        // Filter anonymous_volumes to exclude paths that conflict with extra_volumes
        // (extra_volumes should take precedence over volume_ignores)
        // Conflicts include:
        //   - Exact match: both point to same path
        //   - Anonymous volume is parent of extra_volume (would shadow the mount)
        //   - Anonymous volume is inside extra_volume (redundant/conflicting)
        let anonymous_volumes: Vec<String> = sandbox_config
            .volume_ignores
            .iter()
            .map(|ignore| format!("{}/{}", workspace_path, ignore))
            .filter(|anon_path| {
                !extra_volume_container_paths.iter().any(|extra_path| {
                    anon_path == extra_path
                        || extra_path.starts_with(&format!("{}/", anon_path))
                        || anon_path.starts_with(&format!("{}/", extra_path))
                })
            })
            .collect();

        Ok(ContainerConfig {
            working_dir: workspace_path,
            volumes,
            named_volumes,
            anonymous_volumes,
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
pub const SUPPORTED_TOOLS: &[&str] = &["claude", "opencode", "vibe", "codex", "gemini"];

/// Tools that have YOLO mode support configured.
/// When adding a new tool, add it here and implement YOLO support in:
/// - `start()` for command construction (Claude uses CLI flag, Vibe uses --auto-approve, Codex uses CLI flag)
/// - `build_container_config()` for environment variables (OpenCode uses env var)
pub const YOLO_SUPPORTED_TOOLS: &[&str] = &["claude", "opencode", "vibe", "codex", "gemini"];

/// Tools that support custom instruction injection via CLI flags.
pub const INSTRUCTION_SUPPORTED_TOOLS: &[&str] = &["claude", "codex"];

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
    fn test_shell_escape_simple() {
        assert_eq!(shell_escape("hello"), "\"hello\"");
    }

    #[test]
    fn test_shell_escape_quotes() {
        assert_eq!(shell_escape("say \"hello\""), "\"say \\\"hello\\\"\"");
    }

    #[test]
    fn test_shell_escape_backslash() {
        assert_eq!(shell_escape("path\\to\\file"), "\"path\\\\to\\\\file\"");
    }

    #[test]
    fn test_shell_escape_dollar() {
        assert_eq!(shell_escape("$HOME/path"), "\"\\$HOME/path\"");
    }

    #[test]
    fn test_shell_escape_backtick() {
        assert_eq!(shell_escape("run `cmd`"), "\"run \\`cmd\\`\"");
    }

    #[test]
    fn test_shell_escape_newline() {
        assert_eq!(shell_escape("line1\nline2"), "\"line1\\nline2\"");
    }

    #[test]
    fn test_shell_escape_carriage_return() {
        assert_eq!(shell_escape("line1\rline2"), "\"line1\\rline2\"");
    }

    #[test]
    fn test_shell_escape_multiline_instruction() {
        let instruction = "First instruction.\nSecond instruction.\nThird instruction.";
        let escaped = shell_escape(instruction);
        assert_eq!(
            escaped,
            "\"First instruction.\\nSecond instruction.\\nThird instruction.\""
        );
        // Verify no actual newlines in the escaped string
        assert!(!escaped.contains('\n'));
    }

    #[test]
    fn test_shell_escape_crlf() {
        assert_eq!(shell_escape("line1\r\nline2"), "\"line1\\r\\nline2\"");
    }

    #[test]
    fn test_shell_escape_combined() {
        // Test a complex string with multiple special characters
        let input = "Say \"hello\"\nRun `echo $HOME`";
        let escaped = shell_escape(input);
        assert_eq!(escaped, "\"Say \\\"hello\\\"\\nRun \\`echo \\$HOME\\`\"");
        assert!(!escaped.contains('\n'));
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
            gemini: true,
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
            extra_env_values: None,
            custom_instruction: None,
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
            extra_env_values: None,
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
            yolo_mode: None,
            extra_env_keys: None,
            extra_env_values: None,
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
            extra_env_values: None,
            custom_instruction: None,
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

    mod compute_volume_paths_tests {
        use super::*;
        use std::path::Path;
        use tempfile::TempDir;

        fn setup_regular_repo() -> (TempDir, std::path::PathBuf) {
            let dir = TempDir::new().unwrap();
            let repo = git2::Repository::init(dir.path()).unwrap();

            // Create initial commit so HEAD is valid
            let sig = git2::Signature::now("Test", "test@example.com").unwrap();
            let tree_id = repo.index().unwrap().write_tree().unwrap();
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "Initial", &tree, &[])
                .unwrap();

            let repo_path = dir.path().to_path_buf();
            (dir, repo_path)
        }

        fn setup_bare_repo_with_worktree() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
            let dir = TempDir::new().unwrap();
            let bare_path = dir.path().join(".bare");

            // Create bare repository
            let repo = git2::Repository::init_bare(&bare_path).unwrap();

            // Create initial commit
            let sig = git2::Signature::now("Test", "test@example.com").unwrap();
            let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
            let tree = repo.find_tree(tree_id).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "Initial", &tree, &[])
                .unwrap();

            // Create .git file pointing to bare repo
            std::fs::write(dir.path().join(".git"), "gitdir: ./.bare\n").unwrap();

            // Create worktree
            let worktree_path = dir.path().join("main");
            let _ = std::process::Command::new("git")
                .args(["worktree", "add", worktree_path.to_str().unwrap(), "HEAD"])
                .current_dir(&bare_path)
                .output();

            let main_repo_path = dir.path().to_path_buf();
            (dir, main_repo_path, worktree_path)
        }

        #[test]
        fn test_compute_volume_paths_regular_repo() {
            let (_dir, repo_path) = setup_regular_repo();
            let inst = Instance::new("test", repo_path.to_str().unwrap());

            let (mount_path, container_path, working_dir) =
                inst.compute_volume_paths(&repo_path).unwrap();

            // Regular repo: mount path should be the project path
            assert_eq!(mount_path, repo_path.to_string_lossy().to_string());
            // Container path and working dir should be the same
            assert_eq!(container_path, working_dir);
            // Should be /workspace/{dir_name}
            let dir_name = repo_path.file_name().unwrap().to_string_lossy();
            assert_eq!(container_path, format!("/workspace/{}", dir_name));
        }

        #[test]
        fn test_compute_volume_paths_non_git_directory() {
            let dir = TempDir::new().unwrap();
            let inst = Instance::new("test", dir.path().to_str().unwrap());

            let (mount_path, container_path, working_dir) =
                inst.compute_volume_paths(dir.path()).unwrap();

            // Non-git: mount path should be the project path
            assert_eq!(mount_path, dir.path().to_string_lossy().to_string());
            // Container path and working dir should be the same
            assert_eq!(container_path, working_dir);
        }

        #[test]
        fn test_compute_volume_paths_bare_repo_worktree() {
            let (_dir, main_repo_path, worktree_path) = setup_bare_repo_with_worktree();

            // Skip if worktree wasn't created (git might not be available)
            if !worktree_path.exists() {
                return;
            }

            let inst = Instance::new("test", worktree_path.to_str().unwrap());

            let (mount_path, container_path, working_dir) =
                inst.compute_volume_paths(&worktree_path).unwrap();

            // Canonicalize paths for comparison (handles /var -> /private/var on macOS)
            let mount_path_canon = Path::new(&mount_path).canonicalize().unwrap();
            let main_repo_canon = main_repo_path.canonicalize().unwrap();

            // For bare repo worktree: mount the entire repo root
            assert_eq!(
                mount_path_canon, main_repo_canon,
                "Should mount the bare repo root, not just the worktree"
            );

            // Container path should be /workspace/{repo_name}
            let repo_name = main_repo_path.file_name().unwrap().to_string_lossy();
            assert_eq!(
                container_path,
                format!("/workspace/{}", repo_name),
                "Container mount path should be /workspace/{{repo_name}}"
            );

            // Working dir should point to the worktree within the mount
            assert!(
                working_dir.starts_with(&format!("/workspace/{}", repo_name)),
                "Working dir should be under /workspace/{{repo_name}}"
            );
            assert!(
                working_dir.ends_with("/main"),
                "Working dir should end with worktree name 'main', got: {}",
                working_dir
            );
        }

        #[test]
        fn test_compute_volume_paths_bare_repo_root() {
            let (_dir, main_repo_path, _worktree_path) = setup_bare_repo_with_worktree();

            // When project_path is the bare repo root itself
            let inst = Instance::new("test", main_repo_path.to_str().unwrap());

            let (mount_path, _container_path, working_dir) =
                inst.compute_volume_paths(&main_repo_path).unwrap();

            // When at repo root, mount path equals project path
            let mount_canon = Path::new(&mount_path).canonicalize().unwrap();
            let main_canon = main_repo_path.canonicalize().unwrap();
            assert_eq!(mount_canon, main_canon);

            // Working dir should be set
            assert!(!working_dir.is_empty());
        }
    }
}
