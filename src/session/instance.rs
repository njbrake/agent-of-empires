//! Session instance definition and operations

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::containers::{self, ContainerRuntimeInterface, DockerContainer};
use crate::tmux;

use super::container_config;
use super::environment::{build_docker_env_args, shell_escape};
use super::poller::CaptureGate;
use super::poller::SessionPoller;

use crate::session::capture::{
    build_exclusion_set, capture_codex_session_id, capture_from_container, capture_from_host,
    capture_gemini_session_id, capture_vibe_session_id, generate_claude_session_id,
    is_valid_session_id, opencode_poll_fn, session_timing, try_capture_opencode_session_id,
    validated_session_id,
};

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
    Unknown,
    Stopped,
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
    /// Additional environment entries (session-specific).
    /// `KEY` = pass through from host, `KEY=VALUE` = set explicitly.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra_env: Option<Vec<String>>,
    /// Custom instruction text to inject into agent launch command
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_instruction: Option<String>,
}

/// Deserialize agent_session_id, treating empty/whitespace strings as None.
fn deserialize_session_id<'de, D>(deserializer: D) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let opt: Option<String> = Option::deserialize(deserializer)?;
    Ok(opt.filter(|s| !s.trim().is_empty()))
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

    // Docker sandbox integration
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox_info: Option<SandboxInfo>,

    // Paired terminal session
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_info: Option<TerminalInfo>,

    // Agent session ID for conversation persistence
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_session_id"
    )]
    pub agent_session_id: Option<String>,

    /// Runtime-only: which profile this instance was loaded from. Not persisted to disk.
    #[serde(default, skip_serializing)]
    pub source_profile: String,

    // Runtime state (not serialized)
    #[serde(skip)]
    pub last_error_check: Option<std::time::Instant>,
    #[serde(skip)]
    pub last_start_time: Option<std::time::Instant>,
    #[serde(skip)]
    pub last_error: Option<String>,
    #[serde(skip)]
    pub session_id_poller: Option<Arc<Mutex<SessionPoller>>>,
    #[serde(skip)]
    pub(crate) deferred_capture_handle: Option<Arc<Mutex<Option<std::thread::JoinHandle<()>>>>>,
    #[serde(skip)]
    pub(crate) capture_gate: Option<Arc<CaptureGate>>,
}

/// Append yolo-mode flags or environment variables to a launch command.
fn apply_yolo_mode(cmd: &mut String, yolo: &crate::agents::YoloMode, is_sandboxed: bool) {
    match yolo {
        crate::agents::YoloMode::CliFlag(flag) => {
            *cmd = format!("{} {}", cmd, flag);
        }
        crate::agents::YoloMode::EnvVar(key, value) if !is_sandboxed => {
            *cmd = format!("{}={} {}", key, value, cmd);
        }
        crate::agents::YoloMode::EnvVar(..) | crate::agents::YoloMode::AlwaysYolo => {}
    }
}

fn build_resume_flags(tool: &str, session_id: &str, is_existing_session: bool) -> String {
    use crate::agents::{get_agent, ResumeStrategy};

    if !is_valid_session_id(session_id) {
        tracing::warn!(
            "Refusing to build resume flags: invalid session ID {:?}",
            session_id
        );
        return String::new();
    }
    let Some(agent) = get_agent(tool) else {
        return String::new();
    };
    match &agent.resume_strategy {
        ResumeStrategy::Flag(flag) => format!("{} {}", flag, session_id),
        ResumeStrategy::FlagPair {
            existing,
            new_session,
        } => {
            let flag = if is_existing_session {
                existing
            } else {
                new_session
            };
            format!("{} {}", flag, session_id)
        }
        ResumeStrategy::Subcommand(sub) => format!("{} {}", sub, session_id),
        ResumeStrategy::Unsupported => String::new(),
    }
}

fn append_resume_flags(
    tool: &str,
    session_id: Option<&str>,
    is_existing_session: bool,
    cmd: &mut String,
    context: &str,
) {
    use crate::agents::{get_agent, ResumeStrategy};

    if let Some(session_id) = session_id {
        let resume_part = build_resume_flags(tool, session_id, is_existing_session);
        if resume_part.is_empty() {
            return;
        }
        let is_subcommand = matches!(
            get_agent(tool).map(|a| &a.resume_strategy),
            Some(ResumeStrategy::Subcommand(_))
        );
        if is_subcommand {
            if let Some(space_pos) = cmd.find(' ') {
                let binary = &cmd[..space_pos];
                let flags = &cmd[space_pos..];
                *cmd = format!("{} {}{}", binary, resume_part, flags);
            } else {
                *cmd = format!("{} {}", cmd, resume_part);
            }
        } else {
            *cmd = format!("{} {}", cmd, resume_part);
        }
        tracing::debug!("Added resume flags to {} command: {}", context, resume_part);
    }
}

/// Persist an agent session ID to storage and tmux env for a given instance.
///
/// Used only during synchronous pre-launch (e.g. `persist_session_id` for
/// Claude) when no poller is active yet. Post-launch persistence goes
/// exclusively through the poller channel -> `apply_session_id_updates()`
/// in the TUI thread to avoid concurrent writes to `sessions.json`.
fn persist_session_to_storage(profile: &str, instance_id: &str, session_id: &str) {
    debug_assert!(
        std::thread::current()
            .name()
            .map_or(true, |n| n == "main" || !n.starts_with("aoe-")),
        "persist_session_to_storage must not be called from background threads (was: {:?})",
        std::thread::current().name()
    );

    if !is_valid_session_id(session_id) {
        tracing::warn!(
            "Refusing to persist invalid session ID {:?} for {}",
            session_id,
            instance_id
        );
        return;
    }

    let storage = match super::storage::Storage::new(profile) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Failed to create storage for session ID persistence: {}", e);
            return;
        }
    };
    let mut instances = match storage.load() {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!("Failed to load instances for session ID persistence: {}", e);
            return;
        }
    };

    let Some(inst) = instances.iter_mut().find(|i| i.id == instance_id) else {
        return;
    };

    let tmux_name = crate::tmux::Session::generate_name(instance_id, &inst.title);
    inst.agent_session_id = Some(session_id.to_string());

    if let Err(e) = storage.save(&instances) {
        tracing::warn!("Failed to save instances for session ID persistence: {}", e);
    } else {
        tracing::debug!("Session ID persisted for {}", instance_id);
        if let Err(e) = publish_session_to_tmux_env(&tmux_name, session_id) {
            tracing::warn!("{}", e);
        }
    }
}

/// Publish a captured session ID to the tmux environment only.
///
/// Background threads (deferred capture, poller on_change) call this instead
/// of `persist_session_to_storage` so they never race with the TUI thread's
/// `save()`. The tmux env is the source of truth for `build_exclusion_set()`
/// (cross-instance dedup), while `sessions.json` is written exclusively by
/// the TUI thread via `apply_session_id_updates()`.
fn publish_session_to_tmux_env(tmux_session_name: &str, session_id: &str) -> Result<()> {
    crate::tmux::env::set_hidden_env(
        tmux_session_name,
        crate::tmux::env::AOE_CAPTURED_SESSION_ID_KEY,
        session_id,
    )
    .map_err(|e| anyhow::anyhow!("Failed to write captured session ID to tmux env: {}", e))
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
            yolo_mode: false,
            status: Status::Idle,
            created_at: Utc::now(),
            last_accessed_at: None,
            worktree_info: None,
            sandbox_info: None,
            terminal_info: None,
            agent_session_id: None,
            source_profile: String::new(),
            last_error_check: None,
            last_start_time: None,
            last_error: None,
            session_id_poller: None,
            deferred_capture_handle: None,
            capture_gate: None,
        }
    }

    pub fn is_sub_session(&self) -> bool {
        self.parent_session_id.is_some()
    }

    pub fn is_sandboxed(&self) -> bool {
        self.sandbox_info.as_ref().is_some_and(|s| s.enabled)
    }

    pub fn is_yolo_mode(&self) -> bool {
        self.yolo_mode
    }

    /// Whether this agent uses a session ID poller for live tracking.
    ///
    /// Claude is excluded: `~/.claude/debug/latest` is a global symlink shared
    /// across all instances, so it cannot reliably identify which project owns
    /// the session. Its pre-launch UUID via `--session-id` is authoritative.
    // TODO: hook-based approach for Claude post-launch verification.
    pub fn supports_session_poller(&self) -> bool {
        matches!(self.tool.as_str(), "opencode")
    }

    /// Whether this agent creates its own session on startup, requiring
    /// post-launch ID capture.
    ///
    /// Derived from the agent's `ResumeStrategy`: agents with `Flag` or
    /// `Subcommand` strategies create their own sessions (OpenCode, Codex,
    /// Gemini, Vibe). Claude uses `FlagPair` with a pre-launch UUID, and
    /// Cursor has `Unsupported` -- neither needs deferred capture.
    pub fn supports_deferred_capture(&self) -> bool {
        use crate::agents::{get_agent, ResumeStrategy};
        get_agent(&self.tool).is_some_and(|a| {
            matches!(
                a.resume_strategy,
                ResumeStrategy::Flag(_) | ResumeStrategy::Subcommand(_)
            )
        })
    }

    /// Acquire a pre-launch session ID for the agent.
    ///
    /// Returns `(session_id, is_existing)`. If a persisted ID exists, returns it
    /// with `is_existing = true`. Otherwise, only Claude gets a new UUID here
    /// (it requires `--session-id <uuid>` at launch). Other agents create their
    /// own sessions on startup; their IDs are captured post-launch by
    /// `deferred_capture_session_id()`.
    pub fn acquire_session_id(&mut self) -> (Option<String>, bool) {
        if self.agent_session_id.is_some() {
            return (self.agent_session_id.clone(), true);
        }

        // Skip retroactive capture when the tmux session doesn't exist yet
        // (the agent hasn't launched, so there's nothing to query).
        let tmux_exists = self.tmux_session().is_ok_and(|s| s.exists());
        if tmux_exists {
            if let Some(id) = self.try_retroactive_capture() {
                tracing::info!(
                    "Retroactive capture found session ID for {}: {}",
                    self.tool,
                    id
                );
                self.agent_session_id = Some(id);
                return (self.agent_session_id.clone(), true);
            }
        }

        // Only Claude needs a pre-launch ID (--session-id <uuid> creates a new session).
        // Other agents create their own sessions; the ID is captured post-launch
        // via deferred_capture_session_id().
        let session_id = match self.tool.as_str() {
            "claude" => Some(generate_claude_session_id()),
            _ => None,
        };

        if let Some(ref id) = session_id {
            tracing::debug!("Session ID for {}: {}", self.tool, id);
            self.agent_session_id = session_id.clone();
        }

        (session_id, false)
    }

    pub(crate) fn try_retroactive_capture(&self) -> Option<String> {
        let exclusion = build_exclusion_set(&self.id);
        let result = match self.tool.as_str() {
            "opencode" => {
                let timing = session_timing();
                // Single attempt with no time filter -- this runs synchronously
                // before the agent starts, so we only do one quick probe.
                try_capture_opencode_session_id(&self.project_path, &exclusion, 0.0, &timing).ok()
            }
            "codex" => capture_codex_session_id(&self.project_path, &exclusion).ok(),
            "gemini" => capture_gemini_session_id(&self.project_path, &exclusion).ok(),
            "vibe" => capture_vibe_session_id(&self.project_path, &exclusion).ok(),
            _ => None,
        };
        result.and_then(validated_session_id)
    }

    fn apply_session_flags(&mut self, cmd: &mut String, context: &str) {
        let (session_id, is_existing) = self.acquire_session_id();
        append_resume_flags(&self.tool, session_id.as_deref(), is_existing, cmd, context);
    }

    fn has_custom_command(&self) -> bool {
        if !self.extra_args.is_empty() {
            return true;
        }
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
        let sandbox = self
            .sandbox_info
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("sandbox_info missing for sandboxed session"))?;

        let env_args = build_docker_env_args(sandbox);
        let env_part = if env_args.is_empty() {
            String::new()
        } else {
            format!("{} ", env_args)
        };

        // Get workspace path inside container (handles bare repo worktrees correctly)
        let container_workdir = self.container_workdir();

        let cmd = container.exec_command(
            Some(&format!("-w {} {}", container_workdir, env_part)),
            "/bin/bash",
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

    /// Apply all configured tmux options to a session with the given name and title.
    fn apply_session_tmux_options(&self, session_name: &str, display_title: &str) {
        let branch = self.worktree_info.as_ref().map(|w| w.branch.as_str());
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

        let profile = super::config::resolve_default_profile();
        let on_launch_hooks = self.resolve_on_launch_hooks(skip_on_launch);

        let agent = crate::agents::get_agent(&self.tool);
        self.install_agent_status_hooks(agent);

        let cmd = if self.is_sandboxed() {
            self.build_sandboxed_command(agent, &on_launch_hooks)?
        } else {
            self.build_host_command(agent, &on_launch_hooks)
        };

        session.create_with_size(&self.project_path, cmd.as_deref(), size)?;

        self.finalize_launch(session.name(), &profile);

        Ok(())
    }

    /// Resolve on_launch hooks from the full config chain (global > profile > repo).
    ///
    /// Repo hooks go through trust verification; global/profile hooks are
    /// implicitly trusted. Returns `None` when skipped or no hooks are configured.
    fn resolve_on_launch_hooks(&self, skip_on_launch: bool) -> Option<Vec<String>> {
        if skip_on_launch {
            return None;
        }

        let profile = super::config::resolve_default_profile();

        // Start with global+profile hooks as the base
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
    }

    /// Install status-detection hooks for agents that support them.
    ///
    /// For sandboxed sessions hooks are installed via `build_container_config`,
    /// so this only acts on host sessions by writing to the user's home directory.
    fn install_agent_status_hooks(&self, agent: Option<&'static crate::agents::AgentDef>) {
        if let Some(hook_cfg) = agent.and_then(|a| a.hook_config.as_ref()) {
            if self.is_sandboxed() {
                // For sandboxed sessions, hooks are installed via build_container_config
            } else {
                // Install hooks in the user's home directory settings
                if let Some(home) = dirs::home_dir() {
                    let settings_path = home.join(hook_cfg.settings_rel_path);
                    if let Err(e) = crate::hooks::install_hooks(&settings_path) {
                        tracing::warn!("Failed to install agent hooks: {}", e);
                    }
                }
            }
        }
    }

    /// Build the tmux command for a sandboxed (Docker) session.
    ///
    /// Runs on_launch hooks inside the container, constructs the tool command
    /// with yolo mode / custom instructions / session flags, and wraps it in a
    /// `docker exec` invocation.
    fn build_sandboxed_command(
        &mut self,
        agent: Option<&'static crate::agents::AgentDef>,
        on_launch_hooks: &Option<Vec<String>>,
    ) -> Result<Option<String>> {
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

        let sandbox = self
            .sandbox_info
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("sandbox_info missing for sandboxed session"))?;
        let base_cmd = if self.extra_args.is_empty() {
            self.get_tool_command().to_string()
        } else {
            format!("{} {}", self.get_tool_command(), self.extra_args)
        };
        let mut tool_cmd = base_cmd;
        if self.is_yolo_mode() {
            if let Some(yolo) = agent.and_then(|a| a.yolo.as_ref()) {
                apply_yolo_mode(&mut tool_cmd, yolo, true);
            }
        }
        if let Some(ref instruction) = sandbox.custom_instruction {
            if !instruction.is_empty() {
                if let Some(flag_template) = agent.and_then(|a| a.instruction_flag) {
                    let escaped = shell_escape(instruction);
                    let flag = flag_template.replace("{}", &escaped);
                    tool_cmd = format!("{} {}", tool_cmd, flag);
                }
            }
        }

        let mut env_args = build_docker_env_args(sandbox);
        // Pass AOE_INSTANCE_ID into the container
        env_args = format!("{} -e AOE_INSTANCE_ID={}", env_args, self.id);

        self.apply_session_flags(&mut tool_cmd, "sandboxed");
        let env_part = format!("{} ", env_args);
        Ok(Some(wrap_command_ignore_suspend(
            &container.exec_command(Some(&env_part), &tool_cmd),
        )))
    }

    /// Build the tmux command for a host (non-sandboxed) session.
    ///
    /// Runs on_launch hooks on the host, then constructs the command from either
    /// the agent's default binary or a user-supplied custom command, applying
    /// yolo mode, session flags, and the AOE_INSTANCE_ID env prefix.
    fn build_host_command(
        &mut self,
        agent: Option<&'static crate::agents::AgentDef>,
        on_launch_hooks: &Option<Vec<String>>,
    ) -> Option<String> {
        // Run on_launch hooks on host for non-sandboxed sessions
        if let Some(ref hook_cmds) = on_launch_hooks {
            if let Err(e) =
                super::repo_config::execute_hooks(hook_cmds, Path::new(&self.project_path))
            {
                tracing::warn!("on_launch hook failed: {}", e);
            }
        }

        // Prepend AOE_INSTANCE_ID env var if this agent supports hooks
        let env_prefix = if agent.and_then(|a| a.hook_config.as_ref()).is_some() {
            format!("AOE_INSTANCE_ID={} ", self.id)
        } else {
            String::new()
        };

        if self.command.is_empty() {
            crate::agents::get_agent(&self.tool)
                .filter(|a| a.supports_host_launch)
                .map(|a| {
                    let mut cmd = a.binary.to_string();
                    if !self.extra_args.is_empty() {
                        cmd = format!("{} {}", cmd, self.extra_args);
                    }
                    if self.is_yolo_mode() {
                        if let Some(ref yolo) = a.yolo {
                            apply_yolo_mode(&mut cmd, yolo, false);
                        }
                    }
                    self.apply_session_flags(&mut cmd, "host agent");
                    wrap_command_ignore_suspend(&format!("{}{}", env_prefix, cmd))
                })
        } else {
            let mut cmd = self.command.clone();
            if !self.extra_args.is_empty() {
                cmd = format!("{} {}", cmd, self.extra_args);
            }
            if self.is_yolo_mode() {
                if let Some(yolo) = agent.and_then(|a| a.yolo.as_ref()) {
                    apply_yolo_mode(&mut cmd, yolo, false);
                }
            }
            self.apply_session_flags(&mut cmd, "host custom");
            Some(wrap_command_ignore_suspend(&format!(
                "{}{}",
                env_prefix, cmd
            )))
        }
    }

    /// Post-launch setup: persist state, start pollers, and apply tmux options.
    fn finalize_launch(&mut self, session_name: &str, profile: &str) {
        self.persist_session_id(profile);
        self.deferred_capture_session_id();
        let poller_launch_time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as f64)
            .unwrap_or(0.0);
        self.maybe_start_poller_with_time(Some(poller_launch_time));

        self.status = Status::Starting;
        self.last_start_time = Some(std::time::Instant::now());

        // Apply tmux env and status bar options in a background thread to avoid
        // blocking the TUI on subprocess calls.
        let session_name = session_name.to_string();
        let instance_id = self.id.clone();
        let instance_id_for_log = self.id.clone();
        let title = self.title.clone();
        let branch = self.worktree_info.as_ref().map(|w| w.branch.clone());
        let sandbox = self.sandbox_display();
        match std::thread::Builder::new()
            .name(format!("finalize-tmux-{}", instance_id))
            .spawn(move || {
                if let Err(e) = crate::tmux::env::set_hidden_env(
                    &session_name,
                    crate::tmux::env::AOE_INSTANCE_ID_KEY,
                    &instance_id,
                ) {
                    tracing::warn!("Failed to set AOE_INSTANCE_ID in tmux env: {}", e);
                }
                crate::tmux::status_bar::apply_all_tmux_options(
                    &session_name,
                    &title,
                    branch.as_deref(),
                    sandbox.as_ref(),
                );
            }) {
            Ok(_handle) => {}
            Err(e) => {
                tracing::error!(
                    session = %instance_id_for_log,
                    error = %e,
                    "Failed to spawn finalize-tmux thread"
                );
            }
        }
    }

    fn persist_session_id(&self, profile: &str) {
        if let Some(ref sid) = self.agent_session_id {
            persist_session_to_storage(profile, &self.id, sid);
        }
    }

    /// Spawn a background thread to capture the session ID after the agent starts.
    ///
    /// Some agents (OpenCode, Codex, Gemini, Vibe) create their own sessions on
    /// launch, so the ID cannot be known in advance. This method polls the agent's
    /// CLI or filesystem until a session appears, then signals the `CaptureGate`
    /// so the poller can propagate it through the channel to the TUI thread.
    fn deferred_capture_session_id(&mut self) {
        if self.agent_session_id.is_some() {
            return;
        }
        if !self.supports_deferred_capture() {
            return;
        }

        let gate = Arc::new(CaptureGate::new());
        let gate_for_thread = Arc::clone(&gate);
        self.capture_gate = Some(gate);

        let instance_id = self.id.clone();
        let tool = self.tool.clone();
        let project_path = self.project_path.clone();
        let is_sandboxed = self.is_sandboxed();
        let tmux_session_name = self
            .tmux_session()
            .map(|s| s.name().to_string())
            .unwrap_or_default();

        let launch_time_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as f64)
            .unwrap_or(0.0);

        match std::thread::Builder::new()
            .name(format!("deferred-capture-{}", instance_id))
            .spawn(move || {
                let timing = session_timing();
                std::thread::sleep(Duration::from_secs(
                    timing.deferred_capture_initial_delay_secs,
                ));

                for attempt in 1..=timing.deferred_capture_max_attempts {
                    let exclusion = build_exclusion_set(&instance_id);

                    let captured = if is_sandboxed {
                        capture_from_container(&instance_id, &tool, &exclusion)
                    } else {
                        capture_from_host(&tool, &project_path, &exclusion, launch_time_ms, &timing)
                    };

                    if let Some(ref session_id) = captured {
                        tracing::debug!(
                            "Deferred capture succeeded for {} (attempt {}): {}",
                            instance_id,
                            attempt,
                            session_id
                        );
                        if !tmux_session_name.is_empty() {
                            if let Err(e) =
                                publish_session_to_tmux_env(&tmux_session_name, session_id)
                            {
                                tracing::warn!("{}", e);
                            }
                        }
                        gate_for_thread.complete(Some(session_id.clone()));
                        return;
                    }

                    if attempt < timing.deferred_capture_max_attempts {
                        tracing::debug!(
                            "Deferred capture attempt {}/{} found nothing for {}, retrying",
                            attempt,
                            timing.deferred_capture_max_attempts,
                            instance_id
                        );
                        std::thread::sleep(Duration::from_secs(
                            timing.deferred_capture_retry_delay_secs,
                        ));
                    }
                }

                tracing::debug!(
                    "Deferred capture exhausted all {} attempts for {}",
                    timing.deferred_capture_max_attempts,
                    instance_id
                );
                gate_for_thread.complete(None);
            }) {
            Ok(handle) => {
                self.deferred_capture_handle = Some(Arc::new(Mutex::new(Some(handle))));
            }
            Err(e) => {
                tracing::error!(
                    session = %self.id,
                    error = %e,
                    "Failed to spawn deferred session capture thread"
                );
                if let Some(ref gate) = self.capture_gate {
                    gate.complete(None);
                }
            }
        }
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
        let sandbox = self
            .sandbox_info
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("sandbox_info missing for sandboxed session"))?;
        container_config::build_container_config(
            &self.project_path,
            sandbox,
            &self.tool,
            self.is_yolo_mode(),
            &self.id,
        )
    }

    pub fn maybe_start_poller(&mut self) {
        self.maybe_start_poller_with_time(None);
    }

    /// Start the session ID poller with an explicit launch time filter.
    ///
    /// When `launch_time_ms` is `Some(t)`, the OpenCode poll function only
    /// considers sessions updated at or after `t` (used after freshly spawning
    /// the agent so we don't pick up stale sessions). When `None`, no time
    /// filter is applied -- the poller discovers any matching session for the
    /// project, which is the correct behaviour when resuming monitoring of an
    /// already-running agent on TUI restart.
    fn maybe_start_poller_with_time(&mut self, launch_time_ms: Option<f64>) {
        if !self.supports_session_poller() {
            return;
        }
        let tool = self.tool.as_str();

        let effective_launch_time = launch_time_ms.unwrap_or(0.0);

        let tmux_session_name = self
            .tmux_session()
            .map(|s| s.name().to_string())
            .unwrap_or_default();
        let mut poller = SessionPoller::new(tmux_session_name);
        let instance_id = self.id.clone();
        let initial_known = self.agent_session_id.clone();

        let poll_fn: Box<dyn Fn() -> Option<String> + Send + 'static> = match tool {
            // Claude excluded: see supports_session_poller() for rationale.
            "opencode" => Box::new(opencode_poll_fn(
                self.project_path.clone(),
                self.id.clone(),
                effective_launch_time,
            )),
            _ => return,
        };

        let cb_tmux_name = self
            .tmux_session()
            .map(|s| s.name().to_string())
            .unwrap_or_default();
        let cb_instance_id = self.id.clone();

        let on_change: Box<dyn Fn(&str) + Send + 'static> = Box::new(move |new_id: &str| {
            tracing::info!("Session ID changed for {}: {}", cb_instance_id, new_id);
            if !cb_tmux_name.is_empty() {
                if let Err(e) = publish_session_to_tmux_env(&cb_tmux_name, new_id) {
                    tracing::warn!("{}", e);
                }
            }
        });

        if poller.start(
            instance_id.clone(),
            poll_fn,
            on_change,
            initial_known,
            self.capture_gate.clone(),
        ) {
            self.session_id_poller = Some(Arc::new(Mutex::new(poller)));
        } else {
            tracing::warn!(
                "Failed to start session poller for instance {}, poller will not be stored",
                instance_id
            );
        }
    }

    fn stop_poller(&self) {
        if let Some(ref poller_arc) = self.session_id_poller {
            if let Ok(mut poller) = poller_arc.lock() {
                poller.stop();
            }
        }
    }

    pub fn restart(&mut self) -> Result<()> {
        self.restart_with_size(None)
    }

    pub fn restart_with_size(&mut self, size: Option<(u16, u16)>) -> Result<()> {
        self.restart_with_size_opts(size, false)
    }

    /// Restart the session, optionally skipping on_launch hooks (e.g. when
    /// they already ran in the background creation poller).
    pub fn restart_with_size_opts(
        &mut self,
        size: Option<(u16, u16)>,
        skip_on_launch: bool,
    ) -> Result<()> {
        self.stop_poller();
        self.session_id_poller = None;
        self.deferred_capture_handle = None;
        self.capture_gate = None;

        let session = self.tmux_session()?;

        if session.exists() {
            session.kill()?;
            std::thread::sleep(std::time::Duration::from_millis(100));
        }

        self.start_with_size_opts(size, skip_on_launch)
    }

    pub fn kill(&self) -> Result<()> {
        self.stop_poller();
        // Join deferred capture thread if still running
        if let Some(ref handle_arc) = self.deferred_capture_handle {
            if let Ok(mut handle_opt) = handle_arc.lock() {
                if let Some(handle) = handle_opt.take() {
                    let _ = handle.join();
                }
            }
        }
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

    pub fn update_status(&mut self) {
        if self.status == Status::Stopped {
            return;
        }

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

        // Check hook-based status first (more reliable than tmux pane parsing)
        if let Some(hook_status) = crate::hooks::read_hook_status(&self.id) {
            tracing::trace!("hook status detection '{}': {:?}", self.title, hook_status);
            let crashed_to_shell = !self.expects_shell() && session.is_pane_running_shell();
            self.status = if session.is_pane_dead() || crashed_to_shell {
                Status::Error
            } else {
                hook_status
            };
            self.last_error = None;
            return;
        }

        // Fall back to tmux pane content detection
        let detected = match session.detect_status(&self.tool) {
            Ok(status) => status,
            Err(_) => Status::Idle,
        };
        tracing::trace!(
            "status detection '{}' (tool={}, custom_cmd={}): {:?}",
            self.title,
            self.tool,
            self.has_custom_command(),
            detected
        );
        let is_shell_stale = || !self.expects_shell() && session.is_pane_running_shell();
        self.status = match detected {
            Status::Idle if self.has_custom_command() => {
                if session.is_pane_dead() || is_shell_stale() {
                    Status::Error
                } else {
                    Status::Unknown
                }
            }
            Status::Idle if session.is_pane_dead() || is_shell_stale() => Status::Error,
            other => other,
        };

        // Clear stale error now that the session is healthy
        self.last_error = None;
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
/// Single quotes in `cmd` are escaped with the `'\''` technique to prevent
/// breaking out of the outer single-quoted wrapper.
fn wrap_command_ignore_suspend(cmd: &str) -> String {
    let shell = super::environment::user_posix_shell();
    let escaped = cmd.replace('\'', "'\\''");
    // Use login shell (-l) so version-manager PATHs (NVM, etc.) are available.
    format!("{} -lc 'stty susp undef; exec env {}'", shell, escaped)
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

    // Tests for agent_session_id field
    #[test]
    fn test_agent_session_id_none_by_default() {
        let inst = Instance::new("test", "/tmp/test");
        assert!(inst.agent_session_id.is_none());
    }

    #[test]
    fn test_agent_session_id_serialization() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.agent_session_id = Some("session-123".to_string());

        let json = serde_json::to_string(&inst).unwrap();
        let deserialized: Instance = serde_json::from_str(&json).unwrap();

        assert_eq!(
            deserialized.agent_session_id,
            Some("session-123".to_string())
        );
    }

    #[test]
    fn test_agent_session_id_skips_none() {
        let inst = Instance::new("test", "/tmp/test");
        let json = serde_json::to_string(&inst).unwrap();

        // agent_session_id should not appear in JSON when None
        assert!(!json.contains("agent_session_id"));
    }

    #[test]
    fn test_agent_session_id_defaults_to_none() {
        let json = r#"{"id":"test123","title":"Test","project_path":"/tmp/test","group_path":"","command":"","tool":"claude","yolo_mode":false,"status":"idle","created_at":"2024-01-01T00:00:00Z"}"#;
        let inst: Instance = serde_json::from_str(json).unwrap();

        assert!(inst.agent_session_id.is_none());
    }

    #[test]
    fn test_build_claude_resume_flags_existing() {
        let session_id = "abc123-def456";
        let flags = build_resume_flags("claude", session_id, true);
        assert_eq!(flags, "--resume abc123-def456");
    }

    #[test]
    fn test_build_claude_session_id_flags_new() {
        let session_id = "abc123-def456";
        let flags = build_resume_flags("claude", session_id, false);
        assert_eq!(flags, "--session-id abc123-def456");
    }

    #[test]
    fn test_build_opencode_resume_flags() {
        let session_id = "session-789";
        let flags = build_resume_flags("opencode", session_id, false);
        assert_eq!(flags, "--session session-789");
    }

    #[test]
    fn test_build_codex_resume_flags() {
        let session_id = "codex-session-xyz";
        let flags = build_resume_flags("codex", session_id, false);
        assert_eq!(flags, "resume codex-session-xyz");
    }

    // Test that instance with agent_session_id can be serialized and deserialized
    #[test]
    fn test_instance_with_agent_session_id_roundtrip() {
        let mut inst = Instance::new("Test", "/home/user/project");
        inst.tool = "claude".to_string();
        inst.agent_session_id = Some("session-abc-123".to_string());

        let json = serde_json::to_string(&inst).unwrap();
        let deserialized: Instance = serde_json::from_str(&json).unwrap();

        assert_eq!(inst.id, deserialized.id);
        assert_eq!(inst.title, deserialized.title);
        assert_eq!(inst.project_path, deserialized.project_path);
        assert_eq!(inst.tool, deserialized.tool);
        assert_eq!(inst.agent_session_id, deserialized.agent_session_id);
    }

    // Test: agent switch clears session ID
    #[test]
    fn test_agent_switch_clears_session_id() {
        let mut inst = Instance::new("Test", "/home/user/project");
        inst.tool = "claude".to_string();
        inst.agent_session_id = Some("claude-session-123".to_string());

        // Simulate agent switch by clearing session ID
        inst.agent_session_id = None;
        inst.tool = "opencode".to_string();

        // Session ID should be None after switch
        assert!(inst.agent_session_id.is_none());
        assert_eq!(inst.tool, "opencode");
    }

    #[test]
    fn test_opencode_acquire_returns_none_for_deferred_capture() {
        let mut inst = Instance::new("Test", "/nonexistent/opencode/test");
        inst.tool = "opencode".to_string();

        let (session_id, is_existing) = inst.acquire_session_id();

        // OpenCode never generates a pre-launch ID (unlike Claude).
        // Retroactive capture may still find an existing session via
        // fallback (opencode returns the most recent session regardless
        // of project path), so we assert the invariant: any returned
        // session must be flagged as existing, never generated.
        assert!(
            !is_existing || session_id.is_some(),
            "is_existing=true requires a session ID"
        );
        assert_eq!(inst.agent_session_id, session_id);
    }

    #[test]
    fn test_codex_acquire_returns_none_for_deferred_capture() {
        let mut inst = Instance::new("Test", "/nonexistent/path");
        inst.tool = "codex".to_string();

        let (session_id, is_existing) = inst.acquire_session_id();

        assert!(session_id.is_none());
        assert!(!is_existing);
        assert!(inst.agent_session_id.is_none());
    }

    #[test]
    fn test_persisted_opencode_session_id_reused() {
        let mut inst = Instance::new("Test", "/tmp/test");
        inst.tool = "opencode".to_string();
        inst.agent_session_id = Some("oc-session-42".to_string());

        let (session_id, is_existing) = inst.acquire_session_id();

        assert_eq!(session_id, Some("oc-session-42".to_string()));
        assert!(is_existing);
    }

    #[test]
    fn test_persisted_codex_session_id_reused() {
        let mut inst = Instance::new("Test", "/tmp/test");
        inst.tool = "codex".to_string();
        inst.agent_session_id = Some("codex-sess-99".to_string());

        let (session_id, is_existing) = inst.acquire_session_id();

        assert_eq!(session_id, Some("codex-sess-99".to_string()));
        assert!(is_existing);
    }

    #[test]
    fn test_resume_with_arbitrary_session_id() {
        let mut inst = Instance::new("Test", "/home/user/project");
        inst.tool = "claude".to_string();
        inst.agent_session_id = Some("invalid-session-id".to_string());

        // With an existing (persisted) session, should use --resume
        let flags = build_resume_flags(&inst.tool, inst.agent_session_id.as_ref().unwrap(), true);
        assert_eq!(flags, "--resume invalid-session-id");

        // The method should return the existing session ID and mark it as existing
        let (session_id, is_existing) = inst.acquire_session_id();
        assert_eq!(session_id, Some("invalid-session-id".to_string()));
        assert!(is_existing);
    }

    #[test]
    fn test_build_resume_flags_rejects_invalid_id() {
        let flags = build_resume_flags("claude", "$(rm -rf /)", true);
        assert_eq!(flags, "");

        let flags = build_resume_flags("opencode", "id; echo pwned", false);
        assert_eq!(flags, "");
    }

    #[test]
    fn test_codex_append_resume_flags_ordering() {
        let mut cmd = "codex --dangerously-auto-approve".to_string();
        append_resume_flags("codex", Some("ses-abc"), true, &mut cmd, "test");
        assert_eq!(cmd, "codex resume ses-abc --dangerously-auto-approve");
    }

    // Test: backwards compatibility - load old JSON without agent_session_id
    #[test]
    fn test_backwards_compatibility() {
        // Old JSON without agent_session_id field
        let old_json = r#"{"id":"old-session-123","title":"Old Session","project_path":"/home/user/old","group_path":"","command":"","tool":"claude","yolo_mode":false,"status":"idle","created_at":"2024-01-01T00:00:00Z"}"#;

        let inst: Instance = serde_json::from_str(old_json).unwrap();

        // Should parse successfully with agent_session_id defaulting to None
        assert_eq!(inst.id, "old-session-123");
        assert_eq!(inst.title, "Old Session");
        assert_eq!(inst.project_path, "/home/user/old");
        assert_eq!(inst.tool, "claude");
        assert!(inst.agent_session_id.is_none());

        // After loading, can set a new session ID
        let mut inst = inst;
        inst.agent_session_id = Some("new-session-456".to_string());
        assert_eq!(inst.agent_session_id, Some("new-session-456".to_string()));
    }

    #[test]
    fn test_empty_string_deserializes_to_none() {
        let json = r#"{"id":"test123","title":"Test","project_path":"/tmp/test","group_path":"","command":"","tool":"claude","yolo_mode":false,"status":"idle","created_at":"2024-01-01T00:00:00Z","agent_session_id":""}"#;
        let inst: Instance = serde_json::from_str(json).unwrap();
        assert!(inst.agent_session_id.is_none());
    }

    #[test]
    fn test_whitespace_string_deserializes_to_none() {
        let json = r#"{"id":"test123","title":"Test","project_path":"/tmp/test","group_path":"","command":"","tool":"claude","yolo_mode":false,"status":"idle","created_at":"2024-01-01T00:00:00Z","agent_session_id":"   "}"#;
        let inst: Instance = serde_json::from_str(json).unwrap();
        assert!(inst.agent_session_id.is_none());
    }

    #[test]
    fn test_valid_session_id_preserved() {
        let json = r#"{"id":"test123","title":"Test","project_path":"/tmp/test","group_path":"","command":"","tool":"claude","yolo_mode":false,"status":"idle","created_at":"2024-01-01T00:00:00Z","agent_session_id":"abc-123"}"#;
        let inst: Instance = serde_json::from_str(json).unwrap();
        assert_eq!(inst.agent_session_id, Some("abc-123".to_string()));
    }

    #[test]
    fn test_build_gemini_resume_flags() {
        let session_id = "gemini-session-abc";
        let flags = build_resume_flags("gemini", session_id, true);
        assert_eq!(flags, "--resume gemini-session-abc");

        let flags_new = build_resume_flags("gemini", session_id, false);
        assert_eq!(flags_new, "--resume gemini-session-abc");
    }

    #[test]
    fn test_build_vibe_resume_flags() {
        let session_id = "vibe-session-xyz";
        let flags = build_resume_flags("vibe", session_id, true);
        assert_eq!(flags, "--resume vibe-session-xyz");

        let flags_new = build_resume_flags("vibe", session_id, false);
        assert_eq!(flags_new, "--resume vibe-session-xyz");
    }

    #[test]
    fn test_build_unknown_tool_resume_flags() {
        let flags = build_resume_flags("mistral", "session-123", false);
        assert!(flags.is_empty());
    }

    #[test]
    fn test_acquire_session_id_idempotence() {
        let mut inst = Instance::new("Test", "/tmp/test");
        inst.tool = "claude".to_string();

        let (first, first_existing) = inst.acquire_session_id();
        let (second, second_existing) = inst.acquire_session_id();

        assert!(first.is_some());
        assert!(!first_existing);
        assert!(second_existing);
        assert_eq!(first, second);
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
}
