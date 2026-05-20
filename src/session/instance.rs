//! Session instance definition and operations

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::containers::{self, ContainerRuntimeInterface, DockerContainer};
use crate::tmux;

use super::container_config;
use super::environment::{build_docker_env_args, shell_escape};
use super::poller::SessionPoller;

use crate::session::capture::{
    capture_codex_session_id, capture_gemini_session_id, capture_hermes_session_id,
    capture_pi_session_id, capture_vibe_session_id, claude_poll_fn, claude_poll_fn_sandboxed,
    codex_poll_fn, codex_poll_fn_sandboxed, gemini_poll_fn, gemini_poll_fn_sandboxed,
    generate_claude_session_id, hermes_poll_fn, hermes_poll_fn_sandboxed, is_valid_session_id,
    opencode_poll_fn, opencode_poll_fn_sandboxed, pi_poll_fn, pi_poll_fn_sandboxed,
    try_capture_codex_session_id_in_container, try_capture_gemini_session_id_in_container,
    try_capture_hermes_session_id_in_container, try_capture_opencode_session_id,
    try_capture_opencode_session_id_in_container, try_capture_pi_session_id_in_container,
    try_capture_vibe_session_id_in_container, validated_session_id, vibe_poll_fn,
    vibe_poll_fn_sandboxed,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalInfo {
    #[serde(default)]
    pub created: bool,
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

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Running => "running",
            Status::Waiting => "waiting",
            Status::Idle => "idle",
            Status::Unknown => "unknown",
            Status::Stopped => "stopped",
            Status::Error => "error",
            Status::Starting => "starting",
            Status::Deleting => "deleting",
            Status::Creating => "creating",
        }
    }
}

/// Outcome of a `start_with_resume_fallback` cascade.
///
/// Failures (both tiers) propagate as `Err` so callers keep the existing
/// `Status::Error` + `last_error` path. Only successful outcomes are
/// enumerated; mirrors the `EnsureReadyOutcome` shape.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartOutcome {
    /// Session ID was set and resume succeeded; pane is alive.
    Resumed,
    /// Resume attempt crashed the pane fast; the bad sid was cleared and a
    /// fresh start succeeded. Caller should surface this: the user's prior
    /// conversation is gone. `stale_sid` is the sid that was cleared.
    Restarted { stale_sid: String },
    /// No resume cascade ran. Either no prior sid, the agent doesn't support
    /// resume, the sid was invalid, the session is cockpit-mode (no tmux
    /// pane), or the tmux session was already alive when entered (so
    /// `start_with_size_opts` was a no-op and the probe had nothing to
    /// detect). The pane is alive on return; whether a fresh launch
    /// actually occurred this call depends on the caller having killed
    /// any pre-existing pane first.
    Fresh,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbeResult {
    Alive,
    Dead,
}

const RESUME_PROBE_MAX: std::time::Duration = std::time::Duration::from_millis(3000);
const RESUME_PROBE_POLL: std::time::Duration = std::time::Duration::from_millis(50);
/// Grace window we keep observing after the pane stops running its boot
/// shell, before declaring `Alive`. Sized to cover the longest in-pane
/// boot a real agent takes before it would have crashed on a bad sid:
/// opencode (bun-compiled native binary that loads JS, parses argv, and
/// hits the session-not-found path) reaches `pane_dead = true` between
/// ~900ms and ~1100ms after spawn on a warm cache, longer on cold or
/// heavy projects. Healthy resumes pay this entire window once on Tier-1
/// (and again on Tier-2 if it fires); the pane is fully attachable for
/// the duration so the cost is purely in the synchronous restart path's
/// latency, not in agent responsiveness afterward.
const RESUME_PROBE_POST_SHELL_GRACE: std::time::Duration = std::time::Duration::from_millis(2000);

/// Pure decision: should the resume-fallback cascade probe and potentially
/// retry without resume after the initial start? Extracted for unit-testability:
/// the cascade itself needs a real tmux session to test end-to-end.
pub(crate) fn should_attempt_resume(agent_session_id: Option<&str>, tool: &str) -> bool {
    let valid = agent_session_id.map(is_valid_session_id).unwrap_or(false);
    if !valid {
        return false;
    }
    !matches!(
        crate::agents::get_agent(tool).map(|a| &a.resume_strategy),
        Some(crate::agents::ResumeStrategy::Unsupported) | None,
    )
}

/// Outcome of `Instance::ensure_pane_ready`. Callers surface this so the user
/// knows what (if anything) happened on their behalf before a send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnsureReadyOutcome {
    /// Pane was already alive; no action taken.
    AlreadyAlive,
    /// Pane was dead (`#{pane_dead}=1`) and was respawned via the restart path.
    /// `stale_sid` is `Some` when the resume-fallback cascade fired during the
    /// respawn, meaning the agent's prior conversation is gone; callers should
    /// surface this so the user understands why history disappeared.
    Respawned { stale_sid: Option<String> },
    /// Tmux session did not exist and was started via the resume-fallback
    /// cascade. `stale_sid` is `Some` when the cascade fired (sid was on
    /// disk from a prior run, the agent crashed on it, and we cleared it
    /// before retrying), `None` for a healthy resume or fresh launch
    /// without resume. Same surface-to-user contract as `Respawned`.
    Started { stale_sid: Option<String> },
}

/// Errors `ensure_pane_ready` can return. Separating transient lifecycle
/// states from real tmux failures lets HTTP callers map them to 409 (retry)
/// vs 500 (real failure) instead of lumping everything as a tmux error.
#[derive(Debug)]
pub enum EnsureReadyError {
    /// Instance is mid-lifecycle (Creating/Deleting). Caller should retry.
    Transient(Status),
    /// Instance is cockpit-mode (no backing tmux pane); send is not supported.
    CockpitMode,
    /// Underlying tmux operation failed.
    Tmux(anyhow::Error),
}

impl std::fmt::Display for EnsureReadyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EnsureReadyError::Transient(status) => {
                write!(
                    f,
                    "Session is mid-lifecycle ({status:?}); cannot send right now"
                )
            }
            EnsureReadyError::CockpitMode => write!(
                f,
                "Cockpit-mode sessions have no tmux pane; send is not supported"
            ),
            EnsureReadyError::Tmux(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for EnsureReadyError {}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeInfo {
    pub branch: String,
    pub main_repo_path: String,
    pub managed_by_aoe: bool,
    pub created_at: DateTime<Utc>,
    /// Branch the worktree was created from when `managed_by_aoe` is
    /// true. None means "the repo's default branch was used" (the
    /// historical behavior before #948) or the worktree was attached
    /// to a pre-existing branch (`create_branch = false`). Surfaced
    /// in `aoe list --json`, the TUI preview, and the web sessions
    /// API; not used by core logic, so old `sessions.json` files
    /// deserialize without the field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_branch: Option<String>,
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

fn status_hook_env_prefix(
    instance_id: &str,
    tool: &str,
    agent: Option<&crate::agents::AgentDef>,
) -> String {
    let has_hooks = agent.and_then(|a| a.hook_config.as_ref()).is_some()
        || tool == "settl"
        || tool == "hermes"
        || tool == "kiro";

    if has_hooks {
        format!("AOE_INSTANCE_ID={} ", instance_id)
    } else {
        String::new()
    }
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
    /// Wall-clock time of the most recent transition into `Idle`. Used by
    /// the TUI and web dashboard to highlight a freshly-stopped session
    /// for the duration of the configured idle-decay window
    /// (`Config.theme.idle_decay_minutes`); past the window the row drops
    /// back to the regular static idle look. Distinct from
    /// `last_accessed_at`, which is also bumped on user interaction (a
    /// viewed session stays "fresh" by design). `None` for non-Idle
    /// sessions or those that transitioned before this field existed.
    ///
    /// Named `idle_entered_at` rather than `idle_since` to avoid collision
    /// with `DwellState::idle_since` in `src/server/push.rs`, which is an
    /// in-process `Instant` for push-notification dwell timing — a
    /// different concept with a different type and lifetime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_entered_at: Option<DateTime<Utc>>,

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

    /// Per-session override for the diff base ref. Takes precedence
    /// over `DiffConfig.default_branch` and the auto-detected default
    /// branch. Set when the eventual PR target differs from the project
    /// default (e.g. stacked PRs, hotfix off `release/*`). See #970.
    ///
    /// Accepts either a short branch name (`"main"`, `"release-1.2"`)
    /// or a remote-qualified ref (`"upstream/main"`); the diff resolver
    /// hands it straight to `compute_changed_files`, whose
    /// `get_commit_from_ref` resolves both forms.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_branch_override: Option<String>,

    /// Whether this session uses the ACP cockpit instead of a tmux pane.
    /// When true, aoe spawns an ACP agent subprocess and renders structured
    /// events natively; tmux integration is bypassed for this session.
    #[cfg(feature = "serve")]
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cockpit_mode: bool,
    /// Optional cockpit agent name (e.g., "claude-code", "aoe-agent",
    /// "gemini"). When None, the cockpit picks the default for the
    /// session's tool.
    #[cfg(feature = "serve")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cockpit_agent: Option<String>,
    /// Optional model id forwarded to aoe-agent (e.g., "claude-opus-4-7",
    /// "gpt-5", "llama3.3:ollama").
    #[cfg(feature = "serve")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cockpit_model: Option<String>,
    /// Agent-assigned ACP session id captured from `session/new`. When
    /// the agent advertises `agent_capabilities.load_session = true`
    /// (claude-agent-acp does), the next spawn calls `session/load`
    /// with this id so the agent reloads its on-disk transcript and
    /// the model retains context across `aoe serve` restarts. Cleared
    /// on cockpit_disable, session delete, or `session/load` failure.
    #[cfg(feature = "serve")]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cockpit_acp_session_id: Option<String>,

    // Runtime state (not serialized)
    #[serde(skip)]
    pub last_error_check: Option<std::time::Instant>,
    #[serde(skip)]
    pub last_start_time: Option<std::time::Instant>,
    #[serde(skip)]
    pub last_error: Option<String>,
    #[serde(skip)]
    pub session_id_poller: Option<Arc<Mutex<SessionPoller>>>,

    /// Runtime-only set of session IDs that retroactive capture must NOT
    /// re-discover from on-disk artifacts. Populated by the resume-fallback
    /// cascade with the just-crashed session ID so the Tier-2 fresh start
    /// doesn't re-import the same bad sid via filesystem scan (opencode db,
    /// vibe meta.json, codex state, etc., all keep the bad session's row
    /// after the crash for several minutes).
    ///
    /// `#[serde(skip)]` is intentional. If the daemon dies between the
    /// cascade clearing the on-disk sid and the on-disk artifact decaying
    /// (~5-10 min), the next launch starts with this set empty and the
    /// freshly-spawned poller can re-import the bad sid once. The next
    /// `start_with_resume_fallback` then re-runs the cascade and clears it
    /// again. Self-healing within one cycle; persisting a TTL set isn't
    /// worth the schema cost.
    #[serde(skip)]
    pub(crate) retroactive_capture_excludes: HashSet<String>,
}

/// Append yolo-mode flags or environment variables to a launch command.
fn apply_yolo_mode(cmd: &mut String, yolo: &crate::agents::YoloMode, is_sandboxed: bool) {
    match yolo {
        crate::agents::YoloMode::CliFlag(flag) => {
            *cmd = format!("{} {}", cmd, flag);
        }
        crate::agents::YoloMode::EnvVar(key, value) if !is_sandboxed => {
            *cmd = format_env_var_prefix(key, value, cmd);
        }
        crate::agents::YoloMode::EnvVar(..) | crate::agents::YoloMode::AlwaysYolo => {}
    }
}

fn build_resume_flags(tool: &str, session_id: &str, is_existing_session: bool) -> String {
    use crate::agents::{get_agent, ResumeStrategy};

    if !is_valid_session_id(session_id) {
        tracing::warn!(target: "session.store",
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
        tracing::debug!(target: "session.store", "Added resume flags to {} command: {}", context, resume_part);
    }
}

/// Persist an agent session ID to storage and tmux env for a given instance.
///
/// Used during synchronous pre-launch (e.g. `persist_session_id` for Claude)
/// when no poller is active yet. Post-launch persistence goes exclusively
/// through the poller channel -> `apply_session_id_updates()` in the TUI
/// thread. Concurrent calls within the same process are serialised via
/// `Storage::update`'s per-profile lock; cross-process races between TUI
/// and `aoe serve` remain a known limitation (see #1175).
fn persist_session_to_storage(profile: &str, instance_id: &str, session_id: &str) {
    if !is_valid_session_id(session_id) {
        tracing::warn!(target: "session.store",
            "Refusing to persist invalid session ID {:?} for {}",
            session_id,
            instance_id
        );
        return;
    }

    let storage = match super::storage::Storage::new(profile) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(target: "session.store", "Failed to create storage for session ID persistence: {}", e);
            return;
        }
    };

    let outcome = storage.update(|instances, _groups| {
        if let Some(inst) = instances.iter_mut().find(|i| i.id == instance_id) {
            inst.agent_session_id = Some(session_id.to_string());
            Ok(true)
        } else {
            Ok(false)
        }
    });

    match outcome {
        Ok(true) => {
            tracing::debug!(target: "session.store", "Session ID persisted for {}", instance_id);
        }
        Ok(false) => {}
        Err(e) => {
            tracing::warn!(target: "session.store", "Failed to persist session ID for {}: {}", instance_id, e);
        }
    }
}

/// Clear the persisted `agent_session_id` for an instance.
///
/// Inverse of `persist_session_to_storage`. Used by the resume-fallback
/// cascade after Tier 1 detects a crashed pane: the bad sid was already
/// flushed to `sessions.json` by the Tier-1 `finalize_launch`, so an
/// in-memory clear is not enough. If the daemon dies between Tier 1 and
/// Tier 2's `finalize_launch`, the next launch would otherwise re-load the
/// bad sid from disk and pass `--resume <bad>` again, looping.
///
/// Concurrent in-process callers (TUI tick, server `spawn_blocking` workers,
/// CLI `restart --all` JoinSet workers) are serialised via `Storage::update`'s
/// per-profile lock; cross-process races remain out of scope (see #1175).
fn clear_session_id_on_disk(profile: &str, instance_id: &str) {
    let storage = match super::storage::Storage::new(profile) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(target: "session.store", "Failed to create storage to clear session ID: {}", e);
            return;
        }
    };

    let outcome = storage.update(|instances, _groups| {
        if let Some(inst) = instances.iter_mut().find(|i| i.id == instance_id) {
            if inst.agent_session_id.is_some() {
                inst.agent_session_id = None;
                return Ok(true);
            }
        }
        Ok(false)
    });

    match outcome {
        Ok(true) => {
            tracing::debug!(target: "session.store", "Session ID cleared on disk for {}", instance_id);
        }
        Ok(false) => {}
        Err(e) => {
            tracing::warn!(target: "session.store", "Failed to clear session ID for {}: {}", instance_id, e);
        }
    }
}

/// Publish a captured session ID to the tmux environment only.
///
/// Background threads (poller on_change) call this so that
/// `build_exclusion_set()` on other instances can see the captured ID
/// without racing with the TUI thread's `save()`.
fn publish_session_to_tmux_env(tmux_session_name: &str, session_id: &str) {
    if let Err(e) = crate::tmux::env::set_hidden_env(
        tmux_session_name,
        crate::tmux::env::AOE_CAPTURED_SESSION_ID_KEY,
        session_id,
    ) {
        tracing::warn!(target: "session.store", "Failed to write captured session ID to tmux env: {}", e);
    }
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
            idle_entered_at: None,
            worktree_info: None,
            workspace_info: None,
            sandbox_info: None,
            terminal_info: None,
            agent_session_id: None,
            source_profile: String::new(),
            notify_on_waiting: None,
            notify_on_idle: None,
            notify_on_error: None,
            base_branch_override: None,
            #[cfg(feature = "serve")]
            cockpit_mode: false,
            #[cfg(feature = "serve")]
            cockpit_agent: None,
            #[cfg(feature = "serve")]
            cockpit_model: None,
            #[cfg(feature = "serve")]
            cockpit_acp_session_id: None,
            last_error_check: None,
            last_start_time: None,
            last_error: None,
            session_id_poller: None,
            retroactive_capture_excludes: HashSet::new(),
        }
    }

    /// Stamp `last_accessed_at` to the current time. Call this on
    /// user-initiated interactions (attach, send keys, etc.) so the
    /// timestamp reflects actual activity, not just status transitions.
    pub fn touch_last_accessed(&mut self) {
        self.last_accessed_at = Some(Utc::now());
    }

    /// Time elapsed since this session most recently transitioned into
    /// `Idle`. `None` for non-Idle sessions, sessions with a missing
    /// timestamp (legacy state), or sessions whose `idle_entered_at` is in
    /// the future (clock skew). Negative deltas are clamped away rather than
    /// returned as `Duration` since `chrono::Duration::to_std` rejects them.
    pub fn idle_age(&self) -> Option<std::time::Duration> {
        if self.status != Status::Idle {
            return None;
        }
        let since = self.idle_entered_at?;
        (Utc::now() - since).to_std().ok()
    }

    /// Return the profile that should drive config resolution for this
    /// instance, falling back to the user's globally configured default
    /// when `source_profile` was never populated (e.g. legacy callers).
    pub fn effective_profile(&self) -> String {
        super::config::effective_profile(&self.source_profile)
    }

    /// Resolve the effective `environment` list for this session's profile,
    /// falling back to the global list when the profile has no override.
    fn profile_host_environment(&self) -> Vec<String> {
        let profile = self.effective_profile();
        super::profile_config::resolve_config_or_warn(&profile).environment
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

    /// True when this session runs through the ACP cockpit (managed by
    /// `aoe serve`'s supervisor) rather than a tmux pane. Always false
    /// when the `serve` feature is disabled, since the field doesn't
    /// exist and no session can be in cockpit mode.
    pub fn is_cockpit_mode(&self) -> bool {
        #[cfg(feature = "serve")]
        {
            self.cockpit_mode
        }
        #[cfg(not(feature = "serve"))]
        {
            false
        }
    }

    /// Whether this agent uses a session ID poller for live tracking.
    pub fn supports_session_poller(&self) -> bool {
        crate::agents::get_agent(&self.tool).is_some_and(|a| {
            !matches!(
                a.resume_strategy,
                crate::agents::ResumeStrategy::Unsupported
            )
        })
    }

    /// Acquire a pre-launch session ID for the agent.
    ///
    /// Returns `(session_id, is_existing)`. If a persisted ID exists, returns it
    /// with `is_existing = true`. Otherwise, only Claude gets a new UUID here
    /// (it requires `--session-id <uuid>` at launch). Other agents discover
    /// their session ID post-launch via the poller (or retroactively via
    /// `try_retroactive_capture()` when an existing tmux session is reattached).
    pub fn acquire_session_id(&mut self) -> (Option<String>, bool) {
        if self.agent_session_id.is_some() {
            return (self.agent_session_id.clone(), true);
        }

        let tmux_exists = self.tmux_session().is_ok_and(|s| s.exists());
        if tmux_exists {
            if let Some(id) = self.try_retroactive_capture() {
                tracing::info!(target: "session.store",
                    "Retroactive capture found session ID for {}: {}",
                    self.tool,
                    id
                );
                self.agent_session_id = Some(id);
                return (self.agent_session_id.clone(), true);
            }
        }

        let session_id = match self.tool.as_str() {
            "claude" => Some(generate_claude_session_id()),
            "opencode" => None,
            _ => None,
        };

        if let Some(ref id) = session_id {
            tracing::debug!(target: "session.store", "Session ID for {}: {}", self.tool, id);
            self.agent_session_id = session_id.clone();
        }

        (session_id, false)
    }

    /// Full set of session IDs that retroactive capture must skip for THIS
    /// instance: the live tmux-discovered set plus any sids the
    /// resume-fallback cascade has explicitly cleared. Composed of
    /// `build_exclusion_set` (live tmux scan) and
    /// `self.retroactive_capture_excludes` (cascade memory) so the caller
    /// gets the complete picture in one call.
    fn retroactive_capture_exclusion_set(&self) -> HashSet<String> {
        super::capture::compose_exclusion(&self.id, &self.retroactive_capture_excludes)
    }

    pub(crate) fn try_retroactive_capture(&self) -> Option<String> {
        let exclusion = self.retroactive_capture_exclusion_set();
        let result: Option<String> = match self.tool.as_str() {
            "opencode" => {
                if self.is_sandboxed() {
                    let container_name = self.sandbox_info.as_ref()?.container_name.clone();
                    try_capture_opencode_session_id_in_container(
                        &container_name,
                        &self.container_workdir(),
                        &exclusion,
                        None,
                    )
                    .ok()
                } else {
                    try_capture_opencode_session_id(&self.project_path, &exclusion, None).ok()
                }
            }
            "vibe" => {
                if self.is_sandboxed() {
                    let container_name = self.sandbox_info.as_ref()?.container_name.clone();
                    try_capture_vibe_session_id_in_container(
                        &container_name,
                        &self.container_workdir(),
                        &exclusion,
                    )
                    .ok()
                } else {
                    capture_vibe_session_id(&self.project_path, &exclusion).ok()
                }
            }
            "pi" => {
                if self.is_sandboxed() {
                    let container_name = self.sandbox_info.as_ref()?.container_name.clone();
                    try_capture_pi_session_id_in_container(
                        &container_name,
                        &self.container_workdir(),
                        &exclusion,
                    )
                    .ok()
                } else {
                    capture_pi_session_id(&self.project_path, &exclusion).ok()
                }
            }
            "codex" => {
                if self.is_sandboxed() {
                    let container_name = self.sandbox_info.as_ref()?.container_name.clone();
                    try_capture_codex_session_id_in_container(
                        &container_name,
                        &self.container_workdir(),
                        &exclusion,
                    )
                    .ok()
                } else {
                    capture_codex_session_id(&self.project_path, &exclusion).ok()
                }
            }
            "gemini" => {
                if self.is_sandboxed() {
                    let container_name = self.sandbox_info.as_ref()?.container_name.clone();
                    try_capture_gemini_session_id_in_container(
                        &container_name,
                        &self.container_workdir(),
                        &exclusion,
                    )
                    .ok()
                } else {
                    capture_gemini_session_id(&self.project_path, &exclusion).ok()
                }
            }
            "hermes" => {
                if self.is_sandboxed() {
                    let container_name = self.sandbox_info.as_ref()?.container_name.clone();
                    try_capture_hermes_session_id_in_container(
                        &container_name,
                        &self.container_workdir(),
                        &exclusion,
                    )
                    .ok()
                } else {
                    capture_hermes_session_id(&self.project_path, &exclusion).ok()
                }
            }
            _ => None,
        };
        result.and_then(validated_session_id)
    }

    fn apply_session_flags(&mut self, cmd: &mut String, context: &str) {
        let (session_id, is_existing) = self.acquire_session_id();
        append_resume_flags(&self.tool, session_id.as_deref(), is_existing, cmd, context);
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

        self.terminal_info = Some(TerminalInfo { created: true });

        Ok(())
    }

    pub fn kill_terminal(&self) -> Result<()> {
        let session = self.terminal_tmux_session()?;
        if session.exists() {
            session.kill()?;
        }
        Ok(())
    }

    /// Kill the paired terminal tmux session if its pane is dead (shell
    /// exited while `remain-on-exit on` kept the session as a tombstone).
    /// Returns true if a kill happened so the caller knows to re-spawn.
    /// A missing session or a live pane both return Ok(false).
    pub fn kill_terminal_if_dead(&self) -> Result<bool> {
        let session = self.terminal_tmux_session()?;
        if session.exists() && session.is_pane_dead() {
            let _ = session.kill();
            return Ok(true);
        }
        Ok(false)
    }

    pub fn container_terminal_tmux_session(&self) -> Result<tmux::ContainerTerminalSession> {
        tmux::ContainerTerminalSession::new(&self.id, &self.title)
    }

    pub fn has_container_terminal(&self) -> bool {
        self.container_terminal_tmux_session()
            .map(|s| s.exists())
            .unwrap_or(false)
    }

    /// `exists()` alone is insufficient: a pane can exist while its agent
    /// has died. Used by recovery, status polling, and TUI reload.
    pub fn has_live_tmux_pane(&self) -> bool {
        self.tmux_session()
            .map(|s| s.exists() && !s.is_pane_dead())
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

        let env_info = build_docker_env_args(
            &self.source_profile,
            sandbox,
            std::path::Path::new(&self.project_path),
        );
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

    /// Container counterpart of [`Self::kill_terminal_if_dead`].
    pub fn kill_container_terminal_if_dead(&self) -> Result<bool> {
        let session = self.container_terminal_tmux_session()?;
        if session.exists() && session.is_pane_dead() {
            let _ = session.kill();
            return Ok(true);
        }
        Ok(false)
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
        // Cockpit-mode sessions are not backed by tmux. The cockpit
        // worker supervisor spawns the ACP agent process directly;
        // calling start() on a cockpit session is a no-op (status
        // updates flow through the ACP event channel, not tmux).
        #[cfg(feature = "serve")]
        if self.cockpit_mode {
            return Ok(());
        }

        let session = self.tmux_session()?;

        if session.exists() {
            return Ok(());
        }

        let profile = self.effective_profile();
        let on_launch_hooks = self.resolve_on_launch_hooks(skip_on_launch, &profile);

        let agent = crate::agents::get_agent(&self.tool)
            .or_else(|| crate::agents::get_agent(&self.detect_as));
        self.install_agent_status_hooks(agent);

        let cmd = if self.is_sandboxed() {
            let container = self.get_container_for_instance()?;
            if let Some(ref hook_cmds) = on_launch_hooks {
                if let Some(ref sandbox) = self.sandbox_info {
                    let workdir = self.container_workdir();
                    if let Err(e) = super::repo_config::execute_hooks_in_container(
                        hook_cmds,
                        &sandbox.container_name,
                        &workdir,
                    ) {
                        tracing::warn!(target: "session.store", "on_launch hook failed in container: {}", e);
                    }
                }
            }

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
            if let Some(instruction) = self
                .sandbox_info
                .as_ref()
                .and_then(|s| s.custom_instruction.as_ref())
                .filter(|s| !s.is_empty())
            {
                if let Some(flag_template) = agent.and_then(|a| a.instruction_flag) {
                    let escaped = shell_escape(instruction);
                    let flag = flag_template.replace("{}", &escaped);
                    tool_cmd = format!("{} {}", tool_cmd, flag);
                }
            }

            self.apply_session_flags(&mut tool_cmd, "sandboxed");

            let sandbox = self
                .sandbox_info
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("sandbox_info missing for sandboxed instance"))?;
            let env_info = build_docker_env_args(
                &self.source_profile,
                sandbox,
                std::path::Path::new(&self.project_path),
            );
            // AOE_INSTANCE_ID is not secret, goes directly in docker args
            let docker_args = format!("{} -e AOE_INSTANCE_ID={}", env_info.docker_args, self.id);
            let env_part = format!("{} ", docker_args);
            let wrapped =
                wrap_command_ignore_suspend(&container.exec_command(Some(&env_part), &tool_cmd));
            Some(prepend_exports(&env_info.exports, wrapped))
        } else {
            self.build_host_command(agent, &on_launch_hooks)
        };

        tracing::debug!(target: "session.store",
            "container cmd: {}",
            cmd.as_ref().map_or("none".to_string(), |v| {
                super::environment::redact_env_values(v)
            })
        );
        session.create_with_size(&self.project_path, cmd.as_deref(), size)?;

        self.finalize_launch(session.name(), &profile);

        Ok(())
    }

    /// Resolve on_launch hooks from the full config chain (global > profile > repo).
    ///
    /// Repo hooks go through trust verification; global/profile hooks are
    /// implicitly trusted. Returns `None` when skipped or no hooks are configured.
    pub(crate) fn resolve_on_launch_hooks(
        &self,
        skip_on_launch: bool,
        profile: &str,
    ) -> Option<Vec<String>> {
        if skip_on_launch {
            return None;
        }

        // Start with global+profile hooks as the base
        let mut resolved_on_launch = super::profile_config::resolve_config_or_warn(profile)
            .hooks
            .on_launch;

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
    /// Respects the `agent_status_hooks` config setting.
    fn install_agent_status_hooks(&self, agent: Option<&'static crate::agents::AgentDef>) {
        let profile = self.effective_profile();
        let hooks_enabled = super::profile_config::resolve_config_or_warn(&profile)
            .session
            .agent_status_hooks;
        if !hooks_enabled {
            return;
        }
        if self.tool == "settl" {
            // settl uses TOML config, not JSON settings
            if let Err(e) = crate::hooks::install_settl_hooks() {
                tracing::warn!(target: "session.store", "Failed to install settl hooks: {}", e);
            }
        } else if self.tool == "hermes" && !self.is_sandboxed() {
            // Hermes uses YAML config; sandbox path is handled by build_container_config
            if let Some(home) = dirs::home_dir() {
                let config_path = home.join(".hermes").join("config.yaml");
                if let Err(e) = crate::hooks::install_hermes_hooks(&config_path) {
                    tracing::warn!(target: "session.store", "Failed to install hermes hooks: {}", e);
                }
            }
        } else if self.tool == "kiro" && !self.is_sandboxed() {
            // Kiro uses its own JSON agent config format; sandbox path is
            // handled by build_container_config.
            if let Some(home) = dirs::home_dir() {
                let config_path = home.join(crate::hooks::KIRO_HOOKS_AGENT_FILE);
                match crate::hooks::install_kiro_hooks(&config_path) {
                    Ok(()) => crate::hooks::set_kiro_default_agent_if_builtin(),
                    Err(e) => {
                        tracing::warn!(target: "session.store", "Failed to install kiro hooks: {}", e)
                    }
                }
            }
        } else if agent.is_some_and(|a| a.name == "codex") && !self.is_sandboxed() {
            if let Some(hook_cfg) = agent.and_then(|a| a.hook_config.as_ref()) {
                match self.codex_config_path_for_launch_env() {
                    Ok(config_path) => {
                        if let Err(e) =
                            crate::hooks::install_codex_hooks(&config_path, hook_cfg.events)
                        {
                            tracing::warn!("Failed to install codex hooks: {}", e);
                        }
                    }
                    Err(e) => tracing::warn!("Failed to resolve codex config path: {}", e),
                }
            }
        } else if let Some(hook_cfg) = agent.and_then(|a| a.hook_config.as_ref()) {
            if self.is_sandboxed() {
                // For sandboxed sessions, hooks are installed via build_container_config
            } else {
                // Install hooks in the user's home directory settings
                if let Some(home) = dirs::home_dir() {
                    let settings_path = home.join(hook_cfg.settings_rel_path);
                    if let Err(e) = crate::hooks::install_hooks(&settings_path, hook_cfg.events) {
                        tracing::warn!(target: "session.store", "Failed to install agent hooks: {}", e);
                    }
                }
            }
        }
    }

    fn codex_config_path_for_launch_env(&self) -> Result<PathBuf> {
        crate::hooks::codex_config_path_for_host_environment(&self.profile_host_environment())
    }

    /// Build the tmux command for a sandboxed (Docker) session.
    ///
    /// Runs on_launch hooks inside the container, constructs the tool command
    /// with yolo mode / custom instructions / session flags, and wraps it in a
    /// `docker exec` invocation.
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
                tracing::warn!(target: "session.store", "on_launch hook failed: {}", e);
            }
        }

        // Prepend AOE_INSTANCE_ID env var if this agent supports hooks.
        let mut env_prefix = status_hook_env_prefix(&self.id, &self.tool, agent);

        // Profile-scoped host environment entries (KEY=value, KEY=$VAR,
        // KEY=$$literal, or bare KEY for passthrough). Sandboxed sessions
        // intentionally skip this injection because the entries are
        // host-side; sandbox users should configure `sandbox.environment`
        // for the in-container env list.
        let host_env = self.profile_host_environment();
        if !host_env.is_empty() {
            env_prefix = format!(
                "{}{}",
                super::environment::host_environment_prefix(&host_env),
                env_prefix
            );
        }

        if self.command.is_empty() {
            crate::agents::get_agent(&self.tool).map(|a| {
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
        if let Err(e) = crate::tmux::env::set_hidden_env(
            session_name,
            crate::tmux::env::AOE_INSTANCE_ID_KEY,
            &self.id,
        ) {
            tracing::warn!(target: "session.store", "Failed to set AOE_INSTANCE_ID in tmux env: {}", e);
        }

        self.persist_session_id(profile);
        self.maybe_start_poller();

        self.status = Status::Starting;
        self.last_start_time = Some(std::time::Instant::now());

        // Apply status bar options in a background thread to avoid blocking
        // the TUI on the multiple tmux subprocess calls they require.
        let session_name = session_name.to_string();
        let instance_id_for_log = self.id.clone();
        let title = self.title.clone();
        let branch = self.worktree_info.as_ref().map(|w| w.branch.clone());
        let sandbox = self.sandbox_display();
        match std::thread::Builder::new()
            .name(format!("finalize-tmux-{}", instance_id_for_log))
            .spawn(move || {
                if let Err(panic) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    crate::tmux::status_bar::apply_all_tmux_options(
                        &session_name,
                        &title,
                        branch.as_deref(),
                        sandbox.as_ref(),
                    );
                })) {
                    tracing::error!(target: "session.store", "finalize-tmux thread panicked: {:?}", panic);
                }
            }) {
            Ok(_handle) => {}
            Err(e) => {
                tracing::error!(target: "session.store", 
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
}

impl Instance {
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
            container_config::ContainerAgentSelection::new(&self.tool, Some(&self.detect_as)),
            self.is_yolo_mode(),
            &self.id,
            self.workspace_info.as_ref(),
            &self.source_profile,
        )
    }

    pub fn maybe_start_poller(&mut self) {
        if !self.supports_session_poller() {
            return;
        }
        let tool = self.tool.as_str();

        let tmux_session_name = self
            .tmux_session()
            .map(|s| s.name().to_string())
            .unwrap_or_default();
        let mut poller = SessionPoller::new(tmux_session_name.clone());
        let instance_id = self.id.clone();
        let initial_known = self.agent_session_id.clone();
        // Snapshot per-instance excludes (sids cleared by the resume-fallback
        // cascade) at poller-spawn time. The cascade always tears down the
        // existing poller and re-enters this function AFTER inserting into
        // `retroactive_capture_excludes` (see start_with_resume_fallback),
        // so the freshly-spawned poller's first immediate poll sees the
        // populated set and won't re-import the bad sid.
        let extra_excludes = self.retroactive_capture_excludes.clone();

        let poll_fn: Box<dyn Fn() -> Option<String> + Send + 'static> = match tool {
            "claude" => {
                if self.is_sandboxed() {
                    let container_name = match self.sandbox_info.as_ref() {
                        Some(s) => s.container_name.clone(),
                        None => return,
                    };
                    Box::new(claude_poll_fn_sandboxed(
                        container_name,
                        self.container_workdir(),
                    ))
                } else {
                    Box::new(claude_poll_fn(self.project_path.clone()))
                }
            }
            "opencode" => {
                let launch_time_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_millis() as f64)
                    .unwrap_or(0.0);
                if self.is_sandboxed() {
                    let container_name = match self.sandbox_info.as_ref() {
                        Some(s) => s.container_name.clone(),
                        None => return,
                    };
                    Box::new(opencode_poll_fn_sandboxed(
                        container_name,
                        self.container_workdir(),
                        self.id.clone(),
                        launch_time_ms,
                        extra_excludes.clone(),
                    ))
                } else {
                    Box::new(opencode_poll_fn(
                        self.project_path.clone(),
                        self.id.clone(),
                        launch_time_ms,
                        extra_excludes.clone(),
                    ))
                }
            }
            "vibe" => {
                if self.is_sandboxed() {
                    let container_name = match self.sandbox_info.as_ref() {
                        Some(s) => s.container_name.clone(),
                        None => return,
                    };
                    Box::new(vibe_poll_fn_sandboxed(
                        container_name,
                        self.container_workdir(),
                        self.id.clone(),
                        extra_excludes.clone(),
                    ))
                } else {
                    Box::new(vibe_poll_fn(
                        self.project_path.clone(),
                        self.id.clone(),
                        extra_excludes.clone(),
                    ))
                }
            }
            "pi" => {
                if self.is_sandboxed() {
                    let container_name = match self.sandbox_info.as_ref() {
                        Some(s) => s.container_name.clone(),
                        None => return,
                    };
                    Box::new(pi_poll_fn_sandboxed(
                        container_name,
                        self.container_workdir(),
                        self.id.clone(),
                        extra_excludes.clone(),
                    ))
                } else {
                    Box::new(pi_poll_fn(
                        self.project_path.clone(),
                        self.id.clone(),
                        extra_excludes.clone(),
                    ))
                }
            }
            "codex" => {
                if self.is_sandboxed() {
                    let container_name = match self.sandbox_info.as_ref() {
                        Some(s) => s.container_name.clone(),
                        None => return,
                    };
                    Box::new(codex_poll_fn_sandboxed(
                        container_name,
                        self.container_workdir(),
                        self.id.clone(),
                        extra_excludes.clone(),
                    ))
                } else {
                    Box::new(codex_poll_fn(
                        self.project_path.clone(),
                        self.id.clone(),
                        extra_excludes.clone(),
                    ))
                }
            }
            "gemini" => {
                if self.is_sandboxed() {
                    let container_name = match self.sandbox_info.as_ref() {
                        Some(s) => s.container_name.clone(),
                        None => return,
                    };
                    Box::new(gemini_poll_fn_sandboxed(
                        container_name,
                        self.container_workdir(),
                        self.id.clone(),
                        extra_excludes.clone(),
                    ))
                } else {
                    Box::new(gemini_poll_fn(
                        self.project_path.clone(),
                        self.id.clone(),
                        extra_excludes.clone(),
                    ))
                }
            }
            "hermes" => {
                if self.is_sandboxed() {
                    let container_name = match self.sandbox_info.as_ref() {
                        Some(s) => s.container_name.clone(),
                        None => return,
                    };
                    Box::new(hermes_poll_fn_sandboxed(
                        container_name,
                        self.container_workdir(),
                        self.id.clone(),
                        extra_excludes,
                    ))
                } else {
                    Box::new(hermes_poll_fn(
                        self.project_path.clone(),
                        self.id.clone(),
                        extra_excludes,
                    ))
                }
            }
            _ => return,
        };

        let cb_instance_id = self.id.clone();
        let cb_tmux_name = self
            .tmux_session()
            .map(|s| s.name().to_string())
            .unwrap_or_default();

        let on_change: Box<dyn Fn(&str) + Send + 'static> = Box::new(move |new_id: &str| {
            tracing::info!(target: "session.store", "Session ID changed for {}: {}", cb_instance_id, new_id);
            if !cb_tmux_name.is_empty() {
                publish_session_to_tmux_env(&cb_tmux_name, new_id);
            }
        });

        if poller.start(instance_id.clone(), poll_fn, on_change, initial_known) {
            self.session_id_poller = Some(Arc::new(Mutex::new(poller)));
        } else {
            tracing::warn!(target: "session.store",
                "Failed to start session poller for instance {}, poller will not be stored",
                instance_id
            );
        }
    }

    fn stop_poller(&self) {
        if let Some(ref poller_arc) = self.session_id_poller {
            match poller_arc.lock() {
                Ok(mut poller) => poller.stop(),
                Err(e) => e.into_inner().stop(),
            }
        }
    }

    pub fn restart_with_size(&mut self, size: Option<(u16, u16)>) -> Result<StartOutcome> {
        self.restart_with_size_opts(size, false)
    }

    /// Tear down the current tmux session cleanly so a fresh
    /// `start_with_size_opts` can recreate it.
    ///
    /// `remain-on-exit on` keeps the tmux session alive after the agent
    /// process exits, leaving a frozen pane. The plain kill-session +
    /// new-session flow can race against the session cache
    /// (kill_process_tree on a defunct pid stalls on macOS, and the
    /// subsequent kill can run while start's exists() check still sees the
    /// cached entry), leaving the dead pane in place. Respawning the pane
    /// into a shell first puts it back in a live state so the kill path
    /// proceeds cleanly. The kill below then sees a live pane and tears it
    /// down. Caller is responsible for the subsequent
    /// `start_with_size_opts` to recreate the session with the agent
    /// command.
    pub(crate) fn kill_clean(&self) -> Result<()> {
        let session = self.tmux_session()?;
        if !session.exists() {
            return Ok(());
        }
        if session.is_pane_dead() {
            tracing::info!(target: "session.store",
                "restart: pane dead for session {} (remain-on-exit), \
                 respawning shell before recreate",
                session.name()
            );
            let shell = super::environment::user_shell();
            if let Err(e) = session.respawn_dead_pane(&self.project_path, Some(&shell)) {
                tracing::warn!(target: "session.store",
                    "respawn_dead_pane failed for {}: {}; falling back to kill+start",
                    session.name(),
                    e
                );
            }
        }
        session.kill()?;
        std::thread::sleep(std::time::Duration::from_millis(100));
        Ok(())
    }

    /// Restart the session, optionally skipping on_launch hooks (e.g. when
    /// they already ran in the background creation poller).
    pub fn restart_with_size_opts(
        &mut self,
        size: Option<(u16, u16)>,
        skip_on_launch: bool,
    ) -> Result<StartOutcome> {
        self.stop_poller();
        self.session_id_poller = None;
        self.kill_clean()?;
        self.start_with_resume_fallback(size, skip_on_launch)
    }

    /// Settle-based pane probe used by the resume-fallback cascade.
    ///
    /// Returns `Dead` immediately if the pane dies or the session evaporates
    /// during the probe window. Returns `Alive` only after the pane has been
    /// off the boot shell for `RESUME_PROBE_POST_SHELL_GRACE` consecutive
    /// time (handles agents whose boot wrapper sits before the agent
    /// crashes on a bad sid), or charitably on full timeout for slow-start
    /// agents. `pane_dead` is the unambiguous signal we trust to fire the
    /// cascade.
    ///
    /// For instances using a shell-wrapper command (`/bin/sh -c '...'`,
    /// agent-override scripts), `is_pane_running_shell` stays true for the
    /// entire probe and the post-shell grace shortcut never fires. Such
    /// instances rely exclusively on `pane_dead`: if the wrapper exits
    /// when the agent crashes, the cascade fires correctly; if the wrapper
    /// holds the pane open past the agent crash (e.g., trailing `sleep`),
    /// the cascade misses it. Pathological shape; not worth special-casing.
    ///
    /// Latency consequence: shell-wrapper instances therefore burn the full
    /// `RESUME_PROBE_MAX` on every healthy resume. Real agents settle in
    /// ~`RESUME_PROBE_POST_SHELL_GRACE`.
    fn probe_settle(
        &self,
        max: std::time::Duration,
        poll: std::time::Duration,
    ) -> Result<ProbeResult> {
        let session = self.tmux_session()?;
        let deadline = std::time::Instant::now() + max;
        let mut first_post_shell: Option<std::time::Instant> = None;
        loop {
            if !session.exists() {
                return Ok(ProbeResult::Dead);
            }
            if session.is_pane_dead() {
                return Ok(ProbeResult::Dead);
            }
            let now = std::time::Instant::now();
            if !session.is_pane_running_shell() {
                let started = *first_post_shell.get_or_insert(now);
                if now.duration_since(started) >= RESUME_PROBE_POST_SHELL_GRACE {
                    return Ok(ProbeResult::Alive);
                }
            } else {
                first_post_shell = None;
            }
            if now >= deadline {
                return Ok(ProbeResult::Alive);
            }
            std::thread::sleep(poll);
        }
    }

    /// Start the session with a one-shot resume fallback.
    ///
    /// Cascade:
    ///   1. If a valid `agent_session_id` is set and the agent supports
    ///      resume, attempt the start (which appends `--resume <sid>` or
    ///      equivalent). Probe the pane via `probe_settle`.
    ///   2. If the pane went dead within the probe window, stop the Tier-1
    ///      poller, tear down the dead tmux session, clear the bad sid
    ///      (in memory, on disk, and from retroactive-capture re-discovery),
    ///      and retry the start with `skip_on_launch=true` so on_launch
    ///      hooks do not run twice. Probe the second pane the same way:
    ///      `start_with_size_opts` only fails on tmux-subprocess errors, so
    ///      an in-pane crash (missing binary, broken `docker exec`, agent
    ///      panic on first run) would otherwise masquerade as
    ///      `StartOutcome::Restarted` over a corpse pane.
    ///   3. If the Tier-2 start fails at the tmux level *or* its pane crashes
    ///      within the probe window, propagate `Err` so the caller's existing
    ///      `Status::Error` + `last_error` path takes over.
    ///
    /// Latency: only fires the probe when `--resume <sid>` is being passed
    /// to a freshly-created tmux session. Healthy resumes on real agents
    /// pay `RESUME_PROBE_POST_SHELL_GRACE` (~2s) once on cold start;
    /// warm sessions and non-resume launches pay nothing. Shell-wrapper
    /// command overrides pay the full `RESUME_PROBE_MAX` (~3s) on every
    /// healthy resume because `is_pane_running_shell` never clears for
    /// them; see `probe_settle`. When the cascade fires, add `kill_clean`
    /// (~100ms macOS grace) + Tier-2 spawn + a second `RESUME_PROBE_MAX`
    /// window: ~6-7s total worst-case.
    ///
    /// Cockpit-mode sessions short-circuit (no tmux pane to probe).
    /// `StartOutcome::Fresh` is honest there: cockpit's resume concept lives
    /// in `cockpit_acp_session_id` and is handled by the ACP supervisor, not
    /// by this cascade.
    pub(crate) fn start_with_resume_fallback(
        &mut self,
        size: Option<(u16, u16)>,
        skip_on_launch: bool,
    ) -> Result<StartOutcome> {
        // Clear `Status::Error` on entry so a successful relaunch from any
        // restart surface (REST `ensure_session`, TUI Enter/restart, CLI
        // `aoe session restart [id|--all]`, cockpit-mode short-circuit)
        // does not leave a stale error chip up. REST `ensure_session`
        // re-asserts `status=Starting`, `last_error=None` pre-call as
        // defense in depth.
        //
        // `last_error_check` is cleared alongside `last_error` to mirror
        // how the field is otherwise managed: `update_status` writes both
        // together when transitioning into Error (see the write sites
        // gated on the 30s rate-limit). The clear is functionally inert
        // today because the only reader is gated on `status == Error` and
        // we just left Error, but the symmetry is intentional defense
        // against a future read site that drops that gate.
        if self.status == Status::Error {
            self.status = Status::Idle;
            self.last_error = None;
            self.last_error_check = None;
        }

        #[cfg(feature = "serve")]
        if self.cockpit_mode {
            self.start_with_size_opts(size, skip_on_launch)?;
            return Ok(StartOutcome::Fresh);
        }

        let attempting_resume = should_attempt_resume(self.agent_session_id.as_deref(), &self.tool);
        // Defense in depth: every current caller runs `kill_clean()` (or
        // its equivalent) first, so this is normally false. It can still
        // be true if `kill_clean` raced the macOS tmux session cache
        // (see `Instance::kill_clean` doc): in that case
        // `start_with_size_opts` no-ops, the probe would have nothing to
        // detect, and reporting `Fresh` is the least-wrong outcome
        // (returning `Resumed` would mean lying about a `--resume <sid>`
        // that was never passed). The debug_assert surfaces the protocol
        // violation in dev/test if a future caller forgets to tear down;
        // the tracing::warn! mirrors it in release so the race is visible
        // in `aoe logs` for diagnosis. The branch on `attempting_resume`
        // separates the dangerous case (sid was passed but no probe ran,
        // pane could be left frozen) from the benign one (no resume was
        // attempted, the race is just kill_clean cache staleness).
        let pane_was_preexisting = self.tmux_session().is_ok_and(|s| s.exists());
        if pane_was_preexisting {
            if attempting_resume {
                tracing::warn!(
                    target: "session.store",
                    instance_id = %self.id,
                    "start_with_resume_fallback: tmux session still exists on \
                     entry with attempting_resume=true; cascade skipped, \
                     returning Fresh. --resume <sid> was passed to \
                     start_with_size_opts but no probe ran; if the agent \
                     crashes inside the pane, it will be left frozen.",
                );
            } else {
                tracing::warn!(
                    target: "session.store",
                    instance_id = %self.id,
                    "start_with_resume_fallback: tmux session still exists on \
                     entry (no resume attempted); cascade skipped, returning \
                     Fresh. Likely a kill_clean race or caller protocol violation.",
                );
            }
        }
        debug_assert!(
            !pane_was_preexisting,
            "start_with_resume_fallback callers must kill_clean() first; \
             tmux session for {} still exists on entry",
            self.id
        );

        self.start_with_size_opts(size, skip_on_launch)?;

        if !attempting_resume || pane_was_preexisting {
            return Ok(StartOutcome::Fresh);
        }

        // Tier-1 settle probe. On Err (rare: only when `tmux_session()`
        // fails), tear down the Tier-1 poller spawned by the
        // start_with_size_opts above before propagating, so a transient
        // tmux failure cannot leak a poller thread onto a presumed-broken
        // pane.
        let probe = match self.probe_settle(RESUME_PROBE_MAX, RESUME_PROBE_POLL) {
            Ok(p) => p,
            Err(e) => {
                self.stop_poller();
                self.session_id_poller = None;
                return Err(e);
            }
        };
        match probe {
            ProbeResult::Alive => return Ok(StartOutcome::Resumed),
            ProbeResult::Dead => {}
        }

        let stale_sid = self
            .agent_session_id
            .clone()
            .expect("attempting_resume guarantees agent_session_id is Some");
        let profile = self.effective_profile();
        tracing::warn!(
            target: "session.store",
            "start: resume with sid {} for session {} crashed pane within probe; \
             clearing sid and retrying without resume",
            stale_sid,
            self.id,
        );

        self.stop_poller();
        self.session_id_poller = None;
        self.kill_clean()
            .with_context(|| format!("kill_clean before resume fallback for {}", self.id))?;

        self.agent_session_id = None;
        clear_session_id_on_disk(&profile, &self.id);
        self.retroactive_capture_excludes.insert(stale_sid.clone());

        self.start_with_size_opts(size, true).with_context(|| {
            format!(
                "fresh restart after resume fallback failed for {} (stale sid {} was cleared)",
                self.id, stale_sid,
            )
        })?;

        // Tier-2 needs the same settle-probe as Tier-1: tmux can spawn the
        // pane successfully while the agent inside crashes immediately
        // (missing binary, gone docker image, agent panic on first run).
        // Without this, the in-pane crash class - the very class the cascade
        // exists to surface - would silently report `StartOutcome::Restarted`.
        // The dead pane is left in tmux; the next user-initiated restart
        // goes through `restart_with_size_opts` -> `kill_clean` and self-heals.
        // Tier-2 settle probe. On Err (same rare condition as Tier-1),
        // tear down the Tier-2 poller spawned by the second
        // start_with_size_opts before propagating.
        let probe = match self.probe_settle(RESUME_PROBE_MAX, RESUME_PROBE_POLL) {
            Ok(p) => p,
            Err(e) => {
                self.stop_poller();
                self.session_id_poller = None;
                return Err(e);
            }
        };
        if matches!(probe, ProbeResult::Dead) {
            // Symmetric teardown with the Tier-1->Tier-2 transition above:
            // the Tier-2 spawn already started a fresh poller via
            // `finalize_launch`, and bailing without stopping it leaves an
            // orphan thread polling a dead pane. The pane stays in tmux
            // (intentional, for `tmux attach` diagnostic on the crash),
            // but the poller handle must be torn down so callers see a
            // consistent post-error state.
            self.stop_poller();
            self.session_id_poller = None;
            anyhow::bail!(
                "fresh restart after resume fallback crashed within probe for {} \
                 (stale sid {} was cleared; underlying issue persists)",
                self.id,
                stale_sid,
            );
        }

        Ok(StartOutcome::Restarted { stale_sid })
    }

    /// Smart-send precondition: bring this session's tmux pane to a state
    /// where `send_keys_with_delay` is safe.
    ///
    /// Without this, a send to a dead pane silently writes keystrokes to a
    /// corpse with no agent to respond, and the user sees no error.
    ///
    /// Handles three states the caller would otherwise hit:
    /// - Tmux session missing: start from scratch via `start_with_size`.
    /// - Pane dead (`#{pane_dead}=1`): reuse the restart path (same path
    ///   E/F5 uses; well-tested).
    /// - Already alive: no-op.
    ///
    /// Bails on Creating/Deleting (transient lifecycle states) and on
    /// cockpit-mode sessions (no backing tmux pane).
    ///
    /// On `Started` / `Respawned`, polls briefly so keystrokes don't race the
    /// agent's startup splash. Best-effort: returns after the timeout even if
    /// the pane is still settling.
    ///
    /// Latency: `AlreadyAlive` is ~tmux RTT. The `Respawned` path routes
    /// through `restart_with_size` -> `start_with_resume_fallback`, which
    /// on a dead resume-eligible pane can block for the full Tier-1 +
    /// Tier-2 cascade window (~6-7s; see `start_with_resume_fallback` for
    /// the breakdown) plus up to 3s of `wait_for_pane_ready` polling.
    /// Smart-send, TUI Enter, and `aoe send` callers should size timeouts
    /// and spinner copy accordingly.
    ///
    /// Note: callers that mutate a clone (e.g. inside `spawn_blocking`) must
    /// sync the post-start state (`status`, `agent_session_id`,
    /// `last_start_time`, `last_error`) back onto the in-memory entry, since
    /// `finalize_launch` writes those fields and they would otherwise be
    /// dropped with the clone. See `apply_post_restart_sync`.
    pub fn ensure_pane_ready(&mut self) -> Result<EnsureReadyOutcome, EnsureReadyError> {
        if matches!(self.status, Status::Creating | Status::Deleting) {
            return Err(EnsureReadyError::Transient(self.status));
        }
        #[cfg(feature = "serve")]
        if self.cockpit_mode {
            return Err(EnsureReadyError::CockpitMode);
        }
        let session = self.tmux_session().map_err(EnsureReadyError::Tmux)?;
        if !session.exists() {
            // Route fresh starts through the cascade so a stale sid loaded
            // from disk that crashes the agent on launch is detected,
            // cleared, and retried. Without this, `aoe send` after a tmux
            // server kill or reboot resurrects the same bad sid the
            // restart paths exist to recover from.
            let outcome = self
                .start_with_resume_fallback(None, false)
                .map_err(EnsureReadyError::Tmux)?;
            self.wait_for_pane_ready(&session);
            let stale_sid = match outcome {
                StartOutcome::Restarted { stale_sid } => Some(stale_sid),
                StartOutcome::Resumed | StartOutcome::Fresh => None,
            };
            return Ok(EnsureReadyOutcome::Started { stale_sid });
        }
        if session.is_pane_dead() {
            let outcome = self
                .restart_with_size(None)
                .map_err(EnsureReadyError::Tmux)?;
            self.wait_for_pane_ready(&session);
            let stale_sid = match outcome {
                StartOutcome::Restarted { stale_sid } => Some(stale_sid),
                StartOutcome::Resumed | StartOutcome::Fresh => None,
            };
            return Ok(EnsureReadyOutcome::Respawned { stale_sid });
        }
        Ok(EnsureReadyOutcome::AlreadyAlive)
    }

    /// Best-effort wait for a freshly-started pane to settle past its initial
    /// shell/splash so subsequent `send-keys` land in the agent instead of a
    /// boot prompt. Polls up to 3s in 50ms increments; returns even on
    /// timeout so a sluggish agent doesn't block the send indefinitely.
    ///
    /// Readiness signal:
    /// - Agents that expect a shell, run a custom command override, or have
    ///   an active hook status file: just wait for the pane to not be dead.
    ///   Wrapper scripts look like shells to tmux, so `is_pane_running_shell`
    ///   would never clear for them and we would eat the full 3s every time.
    ///   This mirrors the same guard chain `ensure_session` uses.
    /// - Real agents (e.g. claude, opencode): also wait for the pane to no
    ///   longer be running a shell, so a keystroke doesn't land in the boot
    ///   prompt that runs before the agent binary takes over.
    fn wait_for_pane_ready(&self, session: &tmux::Session) {
        let shell_check_unreliable = self.expects_shell()
            || self.has_command_override()
            || crate::hooks::read_hook_status(&self.id).is_some();
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(3000);
        loop {
            if !session.exists() {
                return;
            }
            let pane_alive = !session.is_pane_dead();
            if pane_alive && (shell_check_unreliable || !session.is_pane_running_shell()) {
                return;
            }
            if std::time::Instant::now() >= deadline {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
    }

    pub fn kill(&self) -> Result<()> {
        self.stop_poller();
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
        let prev_status = self.status;
        self.update_status_with_metadata_inner(metadata);
        if self.status != prev_status {
            let now = Utc::now();
            self.last_accessed_at = Some(now);
            self.idle_entered_at = if self.status == Status::Idle {
                Some(now)
            } else {
                None
            };
        }
    }

    fn update_status_with_metadata_inner(&mut self, metadata: Option<&tmux::PaneMetadata>) {
        if matches!(
            self.status,
            Status::Stopped | Status::Deleting | Status::Creating
        ) {
            return;
        }

        // Cockpit-mode sessions are not backed by a tmux pane; the cockpit
        // worker supervisor owns their lifecycle and emits typed health
        // events over the broadcast. Probing tmux here only ever produces
        // a spurious "tmux session is gone" Error transition.
        #[cfg(feature = "serve")]
        if self.cockpit_mode {
            // Clear any stale tmux-derived error so the UI doesn't show
            // a misleading message after a session is converted or
            // restarted with cockpit_mode on.
            if self.last_error.as_deref()
                == Some("tmux session is gone. The agent process may have exited or been killed.")
            {
                self.last_error = None;
            }
            if self.status == Status::Error {
                self.status = Status::Idle;
            }
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
                tracing::trace!(target: "session.store",
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
            tracing::trace!(target: "session.store",
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

        tracing::trace!(target: "session.store",
            "status '{}': exists=true, is_dead={}, pane_cmd={:?}, tool={}, cmd_override={}",
            self.title,
            is_dead,
            pane_cmd,
            self.tool,
            self.has_command_override()
        );

        let detection_tool = if self.detect_as.is_empty() {
            &self.tool
        } else {
            &self.detect_as
        };

        if let Some(hook_status) = crate::hooks::read_hook_status(&self.id) {
            tracing::trace!(target: "session.store",
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
                self.status = if detection_tool == "codex" && hook_status == Status::Running {
                    match session.capture_pane(50) {
                        Ok(pane_content) => {
                            tmux::reconcile_codex_hook_status(hook_status, &pane_content)
                        }
                        Err(e) => {
                            tracing::trace!(
                                "status '{}': codex hook fallback pane capture failed: {}",
                                self.title,
                                e
                            );
                            hook_status
                        }
                    }
                } else {
                    hook_status
                };
                self.last_error = None;
            }
            return;
        }

        let pane_content = session.capture_pane(50).unwrap_or_default();
        let detected = tmux::detect_status_from_content(&pane_content, detection_tool);
        tracing::trace!(target: "session.store",
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
            tracing::trace!(target: "session.store",
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
                    tracing::trace!(target: "session.store",
                        "status '{}': shell stale but pane has agent content, staying Idle",
                        self.title,
                    );
                    Status::Idle
                } else {
                    tracing::trace!(target: "session.store",
                        "status '{}': shell stale, no agent content, setting Error",
                        self.title,
                    );
                    Status::Error
                }
            }
            other => other,
        };

        tracing::trace!(target: "session.store", "status '{}': final={:?}", self.title, self.status);

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
///
/// The leading `exec` ensures the tmux default shell (which may be fish, nu,
/// etc.) replaces itself with the POSIX wrapper. Without it, fish stays as the
/// pane process because fish does not exec the last command in `-c` mode. That
/// causes `#{pane_current_command}` to report "fish", which triggers a false
/// restart on reattach. See #757.
fn wrap_command_ignore_suspend(cmd: &str) -> String {
    let user = super::environment::user_shell();
    let posix = super::environment::user_posix_shell();
    let escaped = cmd.replace('\'', "'\\''");
    // Use login shell (-l) so version-manager PATHs (NVM, etc.) are available.
    // Skip -l when falling back to bash for a non-POSIX user shell (fish, nu,
    // pwsh): bash's login scripts won't contain the user's PATH setup and -l
    // may reset the inherited PATH that already has the correct entries.
    let flag = if user == posix { "-lc" } else { "-c" };
    format!(
        "exec {} {} 'stty susp undef; exec env {}'",
        posix, flag, escaped
    )
}

/// Prepend shell `export` statements to an already-wrapped sandbox command.
///
/// `wrapped` MUST be the output of `wrap_command_ignore_suspend`, which
/// guarantees a leading `exec`. This function therefore MUST NOT add another
/// `exec` of its own: in bash, `exec exec <cmd>` searches PATH for a binary
/// literally named `exec`, fails with exit 127, and kills the tmux pane on
/// every sandboxed launch. zsh-on-macOS happens to tolerate the double-exec,
/// which is why this regression hid for several days after #757 added the
/// leading `exec` to `wrap_command_ignore_suspend`. See PR #819.
fn prepend_exports(exports: &[String], wrapped: String) -> String {
    if exports.is_empty() {
        wrapped
    } else {
        format!("{}; {}", exports.join("; "), wrapped)
    }
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

    struct CodexHomeGuard(Option<String>);
    impl CodexHomeGuard {
        fn unset() -> Self {
            let prev = std::env::var("CODEX_HOME").ok();
            std::env::remove_var("CODEX_HOME");
            Self(prev)
        }
    }
    impl Drop for CodexHomeGuard {
        fn drop(&mut self) {
            match &self.0 {
                Some(v) => std::env::set_var("CODEX_HOME", v),
                None => std::env::remove_var("CODEX_HOME"),
            }
        }
    }

    #[test]
    fn test_new_instance() {
        let inst = Instance::new("test", "/tmp/test");
        assert_eq!(inst.title, "test");
        assert_eq!(inst.project_path, "/tmp/test");
        assert_eq!(inst.status, Status::Idle);
        assert_eq!(inst.id.len(), 16);
    }

    #[test]
    fn test_codex_gets_status_hook_env_prefix() {
        let agent = crate::agents::get_agent("codex");
        assert_eq!(
            status_hook_env_prefix("abc123", "codex", agent),
            "AOE_INSTANCE_ID=abc123 "
        );
    }

    #[test]
    #[serial_test::serial]
    fn test_custom_codex_detected_agent_uses_codex_hook_installer() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _codex_home_guard = CodexHomeGuard::unset();
        std::env::set_var("HOME", tmp.path());
        #[cfg(target_os = "linux")]
        std::env::set_var("XDG_CONFIG_HOME", tmp.path().join(".config"));

        let mut inst = Instance::new("wrapped", "/tmp/test");
        inst.tool = "my-codex-wrapper".to_string();
        inst.detect_as = "codex".to_string();
        inst.install_agent_status_hooks(crate::agents::get_agent(&inst.detect_as));

        let config_path = tmp.path().join(".codex").join("config.toml");
        let config = std::fs::read_to_string(config_path).unwrap();
        assert!(config.contains("[[hooks.PreToolUse]]"));
        assert!(config.contains("aoe-hooks"));
        assert!(!tmp.path().join(".codex").join("hooks.json").exists());
    }

    #[test]
    #[serial_test::serial]
    fn test_codex_hook_installer_uses_profile_codex_home() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _codex_home_guard = CodexHomeGuard::unset();
        std::env::set_var("HOME", tmp.path());
        #[cfg(target_os = "linux")]
        std::env::set_var("XDG_CONFIG_HOME", tmp.path().join(".config"));

        let codex_home = tmp.path().join("profile-codex-home");
        let profile_dir = crate::session::get_profile_dir("codex-profile").unwrap();
        std::fs::write(
            profile_dir.join("config.toml"),
            format!("environment = [\"CODEX_HOME={}\"]\n", codex_home.display()),
        )
        .unwrap();

        let mut inst = Instance::new("codex", "/tmp/test");
        inst.tool = "codex".to_string();
        inst.detect_as = "codex".to_string();
        inst.source_profile = "codex-profile".to_string();
        inst.install_agent_status_hooks(crate::agents::get_agent(&inst.detect_as));

        let config_path = codex_home.join("config.toml");
        let config = std::fs::read_to_string(config_path).unwrap();
        assert!(config.contains("[[hooks.PreToolUse]]"));
        assert!(config.contains("aoe-hooks"));
        assert!(!tmp.path().join(".codex").join("config.toml").exists());
    }

    #[test]
    #[serial_test::serial]
    fn test_codex_hook_installer_respects_profile_hooks_disabled() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _codex_home_guard = CodexHomeGuard::unset();
        std::env::set_var("HOME", tmp.path());
        #[cfg(target_os = "linux")]
        std::env::set_var("XDG_CONFIG_HOME", tmp.path().join(".config"));

        let profile_dir = crate::session::get_profile_dir("hooks-disabled").unwrap();
        std::fs::write(
            profile_dir.join("config.toml"),
            "[session]\nagent_status_hooks = false\n",
        )
        .unwrap();

        let mut inst = Instance::new("codex", "/tmp/test");
        inst.tool = "codex".to_string();
        inst.detect_as = "codex".to_string();
        inst.source_profile = "hooks-disabled".to_string();
        inst.install_agent_status_hooks(crate::agents::get_agent(&inst.detect_as));

        assert!(!tmp.path().join(".codex").join("config.toml").exists());
    }

    #[test]
    #[serial_test::serial]
    fn test_codex_hook_installer_respects_profile_hooks_enabled() {
        let tmp = tempfile::TempDir::new().unwrap();
        let _codex_home_guard = CodexHomeGuard::unset();
        std::env::set_var("HOME", tmp.path());
        #[cfg(target_os = "linux")]
        std::env::set_var("XDG_CONFIG_HOME", tmp.path().join(".config"));

        let mut global = crate::session::config::Config::default();
        global.session.agent_status_hooks = false;
        crate::session::config::save_config(&global).unwrap();

        let profile_dir = crate::session::get_profile_dir("hooks-enabled").unwrap();
        std::fs::write(
            profile_dir.join("config.toml"),
            "[session]\nagent_status_hooks = true\n",
        )
        .unwrap();

        let mut inst = Instance::new("codex", "/tmp/test");
        inst.tool = "codex".to_string();
        inst.detect_as = "codex".to_string();
        inst.source_profile = "hooks-enabled".to_string();
        inst.install_agent_status_hooks(crate::agents::get_agent(&inst.detect_as));

        let config_path = tmp.path().join(".codex").join("config.toml");
        let config = std::fs::read_to_string(config_path).unwrap();
        assert!(config.contains("[[hooks.PreToolUse]]"));
        assert!(config.contains("aoe-hooks"));
    }

    #[test]
    fn test_is_sub_session() {
        let mut inst = Instance::new("test", "/tmp/test");
        assert!(!inst.is_sub_session());

        inst.parent_session_id = Some("parent123".to_string());
        assert!(inst.is_sub_session());
    }

    #[test]
    fn test_ensure_pane_ready_bails_on_creating() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.status = Status::Creating;
        match inst.ensure_pane_ready() {
            Err(EnsureReadyError::Transient(Status::Creating)) => {}
            other => panic!("expected Transient(Creating), got {other:?}"),
        }
    }

    #[test]
    fn test_ensure_pane_ready_bails_on_deleting() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.status = Status::Deleting;
        match inst.ensure_pane_ready() {
            Err(EnsureReadyError::Transient(Status::Deleting)) => {}
            other => panic!("expected Transient(Deleting), got {other:?}"),
        }
    }

    #[cfg(feature = "serve")]
    #[test]
    fn test_ensure_pane_ready_bails_on_cockpit_mode() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.cockpit_mode = true;
        match inst.ensure_pane_ready() {
            Err(EnsureReadyError::CockpitMode) => {}
            other => panic!("expected CockpitMode, got {other:?}"),
        }
    }

    /// Real-tmux integration: an alive pane yields AlreadyAlive with no
    /// status/start_time mutations. Skipped if tmux isn't installed.
    #[test]
    fn test_ensure_pane_ready_alive_pane_is_noop() {
        if std::process::Command::new("tmux")
            .arg("-V")
            .output()
            .is_err()
        {
            eprintln!("tmux not available; skipping");
            return;
        }

        let mut inst = Instance::new("ensure_alive_test", "/tmp/test");
        let tmux_name = crate::tmux::Session::generate_name(&inst.id, &inst.title);
        let _ = std::process::Command::new("tmux")
            .args(["kill-session", "-t", &tmux_name])
            .output();
        let created = std::process::Command::new("tmux")
            .args([
                "new-session",
                "-d",
                "-s",
                &tmux_name,
                "-x",
                "80",
                "-y",
                "24",
                "sleep",
                "60",
            ])
            .status();
        if !created.map(|s| s.success()).unwrap_or(false) {
            eprintln!("tmux new-session failed; skipping");
            return;
        }
        crate::tmux::refresh_session_cache();

        inst.status = Status::Running;
        let prev_start = inst.last_start_time;
        let prev_status = inst.status;

        let outcome = inst.ensure_pane_ready().expect("ensure_pane_ready ok");
        assert_eq!(outcome, EnsureReadyOutcome::AlreadyAlive);
        assert_eq!(inst.last_start_time, prev_start);
        assert_eq!(inst.status, prev_status);

        let _ = std::process::Command::new("tmux")
            .args(["kill-session", "-t", &tmux_name])
            .output();
    }

    #[test]
    fn test_idle_age_returns_none_for_non_idle() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.status = Status::Running;
        inst.idle_entered_at = Some(Utc::now() - chrono::Duration::seconds(60));
        // A Running session never has an idle age, even if a stale
        // `idle_entered_at` timestamp is sitting around (e.g. a transition
        // that bumped from Idle → Running but missed the cleanup path).
        assert_eq!(inst.idle_age(), None);
    }

    #[test]
    fn test_idle_age_returns_none_when_no_timestamp() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.status = Status::Idle;
        inst.idle_entered_at = None;
        assert_eq!(inst.idle_age(), None);
    }

    #[test]
    fn test_idle_age_returns_positive_duration() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.status = Status::Idle;
        inst.idle_entered_at = Some(Utc::now() - chrono::Duration::seconds(5));
        let age = inst.idle_age().expect("idle age should be present");
        // Allow generous slack so the test isn't flaky on slow CI.
        assert!(age.as_secs() >= 4 && age.as_secs() <= 30);
    }

    #[test]
    fn test_idle_age_clamps_negative_to_none() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.status = Status::Idle;
        // Future timestamp (clock skew, hand-crafted state). `to_std()` on a
        // negative `chrono::Duration` returns Err, which we map to None so
        // the freshness logic sees "fully decayed" rather than panicking
        // or treating the session as freshly stopped.
        inst.idle_entered_at = Some(Utc::now() + chrono::Duration::seconds(60));
        assert_eq!(inst.idle_age(), None);
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
    fn test_prepend_exports_does_not_double_exec() {
        // Regression: `wrap_command_ignore_suspend` always emits a string
        // starting with `exec` (since #757). `prepend_exports` MUST NOT add
        // another `exec`, because bash interprets `exec exec <cmd>` as
        // "exec a binary literally named `exec`", fails with exit 127, and
        // kills the pane on every sandboxed launch. zsh-on-macOS happens
        // to tolerate the double-exec, which is why this regression hid
        // for several days after #757 merged. See PR #819.
        std::env::set_var("SHELL", "/bin/bash");
        let wrapped = wrap_command_ignore_suspend("docker exec -it container claude");
        assert!(
            wrapped.starts_with("exec "),
            "test invariant: wrapped must start with `exec ` (else this test \
             is misaligned with wrap_command_ignore_suspend's contract): {}",
            wrapped,
        );

        let exports = vec![
            "export TERM='xterm-256color'".to_string(),
            "export COLORTERM='truecolor'".to_string(),
        ];
        let session_cmd = prepend_exports(&exports, wrapped);

        assert!(
            !session_cmd.contains("exec exec"),
            "session cmd must not contain `exec exec` -- bash exits 127 on it: {}",
            session_cmd,
        );

        // Empty exports must pass through unchanged.
        let wrapped2 = wrap_command_ignore_suspend("docker exec -it container claude");
        assert_eq!(prepend_exports(&[], wrapped2.clone()), wrapped2);
    }

    #[test]
    #[serial_test::serial(shell_env)]
    fn test_wrap_command_starts_with_exec() {
        // All wrapped commands must start with `exec` so that the tmux
        // default shell (which may be fish/nu) replaces itself with the
        // POSIX wrapper. Without this, fish stays as the pane process and
        // #{pane_current_command} reports "fish", triggering false restarts
        // on reattach. See #757.
        let original = std::env::var("SHELL").ok();
        for shell in &["/bin/bash", "/bin/zsh", "/usr/bin/fish", "/usr/bin/nu"] {
            std::env::set_var("SHELL", shell);
            let wrapped = wrap_command_ignore_suspend("claude");
            assert!(
                wrapped.starts_with("exec "),
                "SHELL={}: wrapped command must start with 'exec': {}",
                shell,
                wrapped,
            );
        }
        match original {
            Some(v) => std::env::set_var("SHELL", v),
            None => std::env::remove_var("SHELL"),
        }
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
            wrapped.starts_with("exec bash -c "),
            "fish shell should produce 'exec bash -c ...': {}",
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
            wrapped.starts_with("exec bash -c "),
            "nu shell should produce 'exec bash -c ...': {}",
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
            base_branch: None,
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

    #[cfg(feature = "serve")]
    #[test]
    fn test_instance_cockpit_acp_session_id_roundtrip() {
        let mut inst = Instance::new("Test", "/tmp/test");
        inst.cockpit_mode = true;
        inst.cockpit_acp_session_id = Some("acp-uuid-1234".to_string());

        let json = serde_json::to_string(&inst).unwrap();
        assert!(json.contains("cockpit_acp_session_id"));
        let deserialized: Instance = serde_json::from_str(&json).unwrap();
        assert_eq!(
            deserialized.cockpit_acp_session_id,
            Some("acp-uuid-1234".to_string())
        );

        // None should not be serialized.
        let mut inst2 = Instance::new("Test", "/tmp/test");
        inst2.cockpit_mode = true;
        let json2 = serde_json::to_string(&inst2).unwrap();
        assert!(!json2.contains("cockpit_acp_session_id"));
    }

    #[test]
    fn test_instance_with_worktree_info() {
        let mut inst = Instance::new("Test", "/tmp/worktree");
        inst.worktree_info = Some(WorktreeInfo {
            branch: "feature/abc".to_string(),
            main_repo_path: "/tmp/main".to_string(),
            managed_by_aoe: true,
            created_at: Utc::now(),
            base_branch: None,
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
        inst.terminal_info = Some(TerminalInfo { created: true });
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
        inst.terminal_info = Some(TerminalInfo { created: false });
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

        let flags = build_resume_flags("opencode", session_id, true);
        assert_eq!(flags, "--session session-789");
    }

    #[test]
    fn test_opencode_acquire_returns_none_for_deferred_capture() {
        let mut inst = Instance::new("Test", "/nonexistent/opencode/test");
        inst.tool = "opencode".to_string();

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
    fn test_persisted_session_id_reused_when_already_set() {
        let mut inst = Instance::new("Test", "/tmp/test");
        inst.tool = "claude".to_string();
        inst.agent_session_id = Some("session-42".to_string());

        let (session_id, is_existing) = inst.acquire_session_id();

        assert_eq!(session_id, Some("session-42".to_string()));
        assert!(is_existing);
    }

    #[test]
    fn test_persisted_session_id_reused_for_unsupported_agent() {
        // The cache-hit path is generic across agents; a persisted ID is
        // returned regardless of whether the agent supports resume yet.
        let mut inst = Instance::new("Test", "/tmp/test");
        inst.tool = "codex".to_string();
        inst.agent_session_id = Some("sess-99".to_string());

        let (session_id, is_existing) = inst.acquire_session_id();

        assert_eq!(session_id, Some("sess-99".to_string()));
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
    fn test_build_unknown_tool_resume_flags() {
        let flags = build_resume_flags("mistral", "session-123", false);
        assert!(flags.is_empty());
    }

    #[test]
    fn test_build_pi_resume_flags() {
        let flags = build_resume_flags("pi", "019342ab-1234-7def-8901-abcdef012345", true);
        assert_eq!(flags, "--session 019342ab-1234-7def-8901-abcdef012345");

        let flags_new = build_resume_flags("pi", "019342ab-1234-7def-8901-abcdef012345", false);
        assert_eq!(flags_new, "--session 019342ab-1234-7def-8901-abcdef012345");
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
    fn test_status_hook_env_prefix_includes_hermes() {
        assert_eq!(
            status_hook_env_prefix("abc123", "hermes", crate::agents::get_agent("hermes")),
            "AOE_INSTANCE_ID=abc123 "
        );
        assert_eq!(
            status_hook_env_prefix("abc123", "settl", crate::agents::get_agent("settl")),
            "AOE_INSTANCE_ID=abc123 "
        );
        assert_eq!(
            status_hook_env_prefix("abc123", "claude", crate::agents::get_agent("claude")),
            "AOE_INSTANCE_ID=abc123 "
        );
        assert_eq!(
            status_hook_env_prefix("abc123", "opencode", crate::agents::get_agent("opencode")),
            ""
        );
        assert_eq!(
            status_hook_env_prefix("abc123", "kiro", crate::agents::get_agent("kiro")),
            "AOE_INSTANCE_ID=abc123 "
        );
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
    fn test_build_host_command_basic() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.tool = "codex".to_string();
        let cmd = inst.build_host_command(crate::agents::get_agent("codex"), &None);
        assert!(cmd.is_some());
        assert!(cmd.as_ref().unwrap().contains("codex"));
    }

    #[test]
    fn test_build_host_command_with_yolo() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.tool = "codex".to_string();
        inst.yolo_mode = true;
        let cmd = inst.build_host_command(crate::agents::get_agent("codex"), &None);
        let cmd_str = cmd.unwrap();
        let agent = crate::agents::get_agent("codex").unwrap();
        match agent.yolo.as_ref().unwrap() {
            crate::agents::YoloMode::CliFlag(flag) => assert!(cmd_str.contains(flag)),
            crate::agents::YoloMode::EnvVar(key, _) => assert!(cmd_str.contains(key)),
            crate::agents::YoloMode::AlwaysYolo => {}
        }
    }

    #[test]
    fn test_build_host_command_with_resume() {
        let mut inst = Instance::new("test", "/tmp/test");
        inst.tool = "claude".to_string();
        inst.agent_session_id = Some("ses_abc123def456".to_string());
        let cmd = inst.build_host_command(crate::agents::get_agent("claude"), &None);
        let cmd_str = cmd.unwrap();
        assert!(cmd_str.contains("ses_abc123def456"));
        assert!(cmd_str.contains("--session-id") || cmd_str.contains("--resume"));
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

    mod kill_terminal_if_dead {
        use super::*;
        use std::process::Command;

        fn tmux_available() -> bool {
            Command::new("tmux")
                .arg("-V")
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
        }

        /// Manually create a tmux session under `name` with `remain-on-exit on`
        /// so the session survives the inner command's exit. Used to simulate
        /// the dead-pane state without going through `start_terminal`, which
        /// would also apply unrelated tmux options.
        fn spawn_remain_on_exit(name: &str, cmd: &str) {
            let output = Command::new("tmux")
                .args([
                    "new-session",
                    "-d",
                    "-s",
                    name,
                    "-x",
                    "80",
                    "-y",
                    "24",
                    cmd,
                    ";",
                    "set-option",
                    "-p",
                    "-t",
                    name,
                    "remain-on-exit",
                    "on",
                ])
                .output()
                .expect("tmux new-session");
            assert!(
                output.status.success(),
                "tmux new-session failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            crate::tmux::refresh_session_cache();
        }

        fn cleanup(name: &str) {
            let _ = Command::new("tmux")
                .args(["kill-session", "-t", name])
                .output();
            crate::tmux::refresh_session_cache();
        }

        #[test]
        #[serial_test::serial]
        fn returns_false_when_no_session() {
            if !tmux_available() {
                eprintln!("Skipping: tmux not available");
                return;
            }
            let inst = Instance::new("ktid_missing", "/tmp");
            crate::tmux::refresh_session_cache();
            assert!(!inst.kill_terminal_if_dead().unwrap());
        }

        #[test]
        #[serial_test::serial]
        fn returns_false_when_pane_alive() {
            if !tmux_available() {
                eprintln!("Skipping: tmux not available");
                return;
            }
            let inst = Instance::new("ktid_alive", "/tmp");
            let name = crate::tmux::TerminalSession::generate_name(&inst.id, &inst.title);
            spawn_remain_on_exit(&name, "sleep 30");
            // Give tmux a moment to register the pane.
            std::thread::sleep(std::time::Duration::from_millis(200));

            let result = inst.kill_terminal_if_dead();
            cleanup(&name);

            assert!(!result.unwrap(), "live pane should not trigger a kill");
        }

        #[test]
        #[serial_test::serial]
        fn kills_dead_pane_session() {
            if !tmux_available() {
                eprintln!("Skipping: tmux not available");
                return;
            }
            let inst = Instance::new("ktid_dead", "/tmp");
            let name = crate::tmux::TerminalSession::generate_name(&inst.id, &inst.title);
            // `true` exits immediately; remain-on-exit keeps the session alive
            // with a dead pane (matches the production failure mode: shell
            // exited via Ctrl+D / `exit` / SIGHUP, session still listed).
            spawn_remain_on_exit(&name, "true");
            // Allow the pane to transition to dead.
            std::thread::sleep(std::time::Duration::from_millis(300));

            let session = inst.terminal_tmux_session().unwrap();
            assert!(
                session.exists(),
                "session should still exist via remain-on-exit"
            );
            assert!(
                session.is_pane_dead(),
                "pane should be dead after `true` exits"
            );

            let killed = inst.kill_terminal_if_dead().unwrap();
            assert!(
                killed,
                "kill_terminal_if_dead should return true for dead pane"
            );

            let session = inst.terminal_tmux_session().unwrap();
            assert!(!session.exists(), "session should be gone after kill");

            // Idempotent: second call on now-missing session returns false.
            assert!(
                !inst.kill_terminal_if_dead().unwrap(),
                "second call on missing session should return false"
            );

            cleanup(&name);
        }
    }

    mod resume_fallback {
        use super::super::{
            clear_session_id_on_disk, should_attempt_resume, Instance, StartOutcome, Status,
        };
        use serial_test::serial;
        use tempfile::tempdir;

        #[test]
        fn no_sid_does_not_attempt_resume() {
            assert!(!should_attempt_resume(None, "claude"));
            assert!(!should_attempt_resume(Some(""), "claude"));
            assert!(!should_attempt_resume(Some("   "), "claude"));
        }

        #[test]
        fn invalid_sid_does_not_attempt_resume() {
            assert!(!should_attempt_resume(Some("bad id!"), "claude"));
            assert!(!should_attempt_resume(Some("path/slash"), "claude"));
            assert!(!should_attempt_resume(Some(&"x".repeat(257)), "claude"));
        }

        #[test]
        fn valid_sid_for_resume_supporting_agent_attempts() {
            assert!(should_attempt_resume(
                Some("11111111-1111-1111-1111-111111111111"),
                "claude"
            ));
            assert!(should_attempt_resume(Some("session_abc.123"), "opencode"));
            assert!(should_attempt_resume(Some("uuid-abc-123"), "codex"));
            assert!(should_attempt_resume(Some("uuid-abc-123"), "gemini"));
        }

        #[test]
        fn unsupported_agent_does_not_attempt_resume() {
            assert!(!should_attempt_resume(
                Some("11111111-1111-1111-1111-111111111111"),
                "cursor"
            ));
            assert!(!should_attempt_resume(
                Some("11111111-1111-1111-1111-111111111111"),
                "copilot"
            ));
        }

        #[test]
        fn unknown_tool_does_not_attempt_resume() {
            assert!(!should_attempt_resume(Some("uuid-abc-123"), "nonexistent"));
        }

        #[test]
        #[serial]
        fn clear_session_id_on_disk_is_idempotent_when_already_none() {
            let temp = tempdir().unwrap();
            std::env::set_var("HOME", temp.path());
            #[cfg(target_os = "linux")]
            std::env::set_var("XDG_CONFIG_HOME", temp.path().join(".config"));

            let storage =
                crate::session::storage::Storage::new("test-profile-already-none").unwrap();
            let inst = Instance::new("title", "/tmp/x");
            let id = inst.id.clone();
            assert!(inst.agent_session_id.is_none());
            let xs = vec![inst];
            storage
                .commit(&xs, &crate::session::GroupTree::new_with_groups(&xs, &[]))
                .unwrap();

            clear_session_id_on_disk("test-profile-already-none", &id);

            let loaded = storage.load().unwrap();
            assert_eq!(loaded.len(), 1);
            assert_eq!(loaded[0].agent_session_id, None);
            assert_eq!(loaded[0].id, id);
        }

        #[test]
        #[serial]
        fn clear_session_id_on_disk_clears_persisted_value() {
            let temp = tempdir().unwrap();
            std::env::set_var("HOME", temp.path());
            #[cfg(target_os = "linux")]
            std::env::set_var("XDG_CONFIG_HOME", temp.path().join(".config"));

            let storage = crate::session::storage::Storage::new("clear-test").unwrap();
            let mut inst = Instance::new("title", "/tmp/x");
            inst.agent_session_id = Some("stale-uuid-1234".to_string());
            let id = inst.id.clone();
            let xs = vec![inst];
            storage
                .commit(&xs, &crate::session::GroupTree::new_with_groups(&xs, &[]))
                .unwrap();

            clear_session_id_on_disk("clear-test", &id);

            let loaded = storage.load().unwrap();
            assert_eq!(loaded.len(), 1);
            assert_eq!(loaded[0].agent_session_id, None);
        }

        #[cfg(feature = "serve")]
        #[test]
        #[serial]
        fn restart_outcome_for_cockpit_session_is_fresh() {
            let temp = tempdir().unwrap();
            std::env::set_var("HOME", temp.path());
            #[cfg(target_os = "linux")]
            std::env::set_var("XDG_CONFIG_HOME", temp.path().join(".config"));

            let mut inst = Instance::new("cockpit_test", "/tmp/x");
            inst.cockpit_mode = true;
            inst.agent_session_id = Some("11111111-1111-1111-1111-111111111111".to_string());
            inst.tool = "claude".to_string();

            let outcome = inst.start_with_resume_fallback(None, true).unwrap();
            assert_eq!(outcome, StartOutcome::Fresh);
        }

        #[test]
        #[serial]
        fn fallback_clears_sid_in_memory_and_on_disk_when_pane_dies() {
            if std::process::Command::new("tmux")
                .arg("-V")
                .output()
                .is_err()
            {
                eprintln!("tmux not available; skipping");
                return;
            }
            let temp = tempdir().unwrap();
            std::env::set_var("HOME", temp.path());
            #[cfg(target_os = "linux")]
            std::env::set_var("XDG_CONFIG_HOME", temp.path().join(".config"));

            let storage = crate::session::storage::Storage::new("fb-test").unwrap();

            let stale_sid = "11111111-1111-1111-1111-111111111111".to_string();
            let mut inst = Instance::new("fallback_dies_test", "/tmp/x");
            inst.tool = "claude".to_string();
            inst.source_profile = "fb-test".to_string();
            inst.command = "/bin/false".to_string();
            inst.agent_session_id = Some(stale_sid.clone());
            inst.status = Status::Idle;
            let id = inst.id.clone();

            let tmux_name = crate::tmux::Session::generate_name(&inst.id, &inst.title);
            let _ = std::process::Command::new("tmux")
                .args(["kill-session", "-t", &tmux_name])
                .output();

            let xs = vec![inst.clone()];
            storage
                .commit(&xs, &crate::session::GroupTree::new_with_groups(&xs, &[]))
                .unwrap();

            let outcome = inst.start_with_resume_fallback(None, true);

            let _ = std::process::Command::new("tmux")
                .args(["kill-session", "-t", &tmux_name])
                .output();

            let err = outcome
                .expect_err("Tier-2 with /bin/false must crash within probe and propagate Err");
            let chain = format!("{:#}", err);
            assert!(
                chain.contains("crashed within probe") && chain.contains(&stale_sid),
                "Tier-2 probe failure must surface the stale sid in its error chain, got: {chain}",
            );

            assert!(
                inst.retroactive_capture_excludes.contains(&stale_sid),
                "stale sid must be in exclusion set even when Tier-2 ultimately fails: \
                 the cleanup happens before the Tier-2 attempt and must survive the bail",
            );
            assert_ne!(
                inst.agent_session_id.as_deref(),
                Some(stale_sid.as_str()),
                "stale sid must not survive in memory after fallback, even on Tier-2 failure",
            );
            let loaded = storage.load().unwrap();
            let row = loaded.iter().find(|i| i.id == id).expect("instance");
            assert_ne!(
                row.agent_session_id.as_deref(),
                Some(stale_sid.as_str()),
                "stale sid must not survive on disk after fallback, even on Tier-2 failure",
            );
        }

        #[test]
        #[serial]
        fn fallback_returns_restarted_when_tier2_pane_lives() {
            if std::process::Command::new("tmux")
                .arg("-V")
                .output()
                .is_err()
            {
                eprintln!("tmux not available; skipping");
                return;
            }
            let temp = tempdir().unwrap();
            std::env::set_var("HOME", temp.path());
            #[cfg(target_os = "linux")]
            std::env::set_var("XDG_CONFIG_HOME", temp.path().join(".config"));

            let _storage = crate::session::storage::Storage::new("fb-test-live").unwrap();

            let stale_sid = "22222222-2222-2222-2222-222222222222".to_string();
            let mut inst = Instance::new("fallback_lives_test", "/tmp/x");
            inst.tool = "claude".to_string();
            inst.source_profile = "fb-test-live".to_string();
            // Claude regenerates a fresh UUID on Tier-2 (acquire_session_id
            // emits `--session-id <new_uuid>`), so we cannot just count argv.
            // Match the *stale* sid specifically: Tier-1 appends `--resume
            // <stale_sid>` and the script exits, firing the cascade. Tier-2
            // appends `--session-id <new_uuid>` (different value), so the
            // pattern does not match and `sleep 30` keeps the pane alive
            // past RESUME_PROBE_MAX (3s).
            //
            // Pinned: this discriminator relies on
            // `ResumeStrategy::FlagPair` appending the sid to argv (see
            // `src/agents.rs`). If a future variant ever passes the sid
            // out of band (env var, stdin, file), update the wrapper to
            // observe the new channel instead of `$*`.
            inst.command = format!(
                "/bin/sh -c 'case \"$*\" in *{stale}*) exit 1 ;; esac; exec sleep 30' --",
                stale = stale_sid,
            );
            inst.agent_session_id = Some(stale_sid.clone());
            inst.status = Status::Idle;

            let tmux_name = crate::tmux::Session::generate_name(&inst.id, &inst.title);
            let _ = std::process::Command::new("tmux")
                .args(["kill-session", "-t", &tmux_name])
                .output();

            let outcome = inst.start_with_resume_fallback(None, true);

            let _ = std::process::Command::new("tmux")
                .args(["kill-session", "-t", &tmux_name])
                .output();

            match outcome {
                Ok(StartOutcome::Restarted { stale_sid: cleared }) => {
                    assert_eq!(cleared, stale_sid);
                }
                Ok(other) => panic!(
                    "Tier-2 success path must return Restarted, got {other:?}; \
                     a different variant indicates the probe misfires"
                ),
                Err(e) => panic!(
                    "Tier-2 with a live binary must succeed: {e:#}; \
                     check probe_settle behavior on long-running shells"
                ),
            }
            assert!(inst.retroactive_capture_excludes.contains(&stale_sid));
            assert!(inst.agent_session_id.as_deref() != Some(stale_sid.as_str()));
        }

        #[test]
        #[serial]
        fn cascade_fires_when_pane_dies_inside_post_shell_grace_window() {
            if std::process::Command::new("tmux")
                .arg("-V")
                .output()
                .is_err()
            {
                eprintln!("tmux not available; skipping");
                return;
            }
            let temp = tempdir().unwrap();
            std::env::set_var("HOME", temp.path());
            #[cfg(target_os = "linux")]
            std::env::set_var("XDG_CONFIG_HOME", temp.path().join(".config"));

            let _storage = crate::session::storage::Storage::new("fb-test-grace").unwrap();

            let stale_sid = "33333333-3333-3333-3333-333333333333".to_string();
            let mut inst = Instance::new("fallback_grace_test", "/tmp/x");
            inst.tool = "claude".to_string();
            inst.source_profile = "fb-test-grace".to_string();
            // Regression guard for RESUME_PROBE_POST_SHELL_GRACE.
            //
            // The Tier-1 wrapper exits its boot shell immediately via `exec
            // sleep N`, then the replacement process exits; tmux observes
            // pane_dead at roughly t = N seconds. We pick N inside the
            // window (current grace 2000ms, sleep 1.2s) so:
            //   * with grace = 500ms, probe_settle returns Alive at t=500ms
            //     before the death at t=1200ms; cascade misses it; the test
            //     observes Resumed; assertion FAILS (regression caught).
            //   * with grace >= ~1300ms, the grace timer is still open when
            //     pane_dead fires; probe_settle returns Dead; cascade fires;
            //     the test observes Restarted; assertion PASSES.
            //
            // This pins the LOWER bound of grace; the upper bound is
            // implicitly RESUME_PROBE_MAX (3000ms) since deadline charity
            // would otherwise mask future regressions.
            //
            // Tier-2 reuses the same wrapper but its sid differs from the
            // stale one (cascade clears agent_session_id; Claude's FlagPair
            // strategy regenerates a fresh UUID), so the case match misses
            // and `exec sleep 30` keeps the pane alive past the second
            // probe window.
            inst.command = format!(
                "/bin/sh -c 'case \"$*\" in *{stale}*) exec sleep 1.2 ;; esac; exec sleep 30' --",
                stale = stale_sid,
            );
            inst.agent_session_id = Some(stale_sid.clone());
            inst.status = Status::Idle;

            let tmux_name = crate::tmux::Session::generate_name(&inst.id, &inst.title);
            let _ = std::process::Command::new("tmux")
                .args(["kill-session", "-t", &tmux_name])
                .output();

            let outcome = inst.start_with_resume_fallback(None, true);

            let _ = std::process::Command::new("tmux")
                .args(["kill-session", "-t", &tmux_name])
                .output();

            match outcome {
                Ok(StartOutcome::Restarted { stale_sid: cleared }) => {
                    assert_eq!(cleared, stale_sid);
                }
                Ok(StartOutcome::Resumed) => panic!(
                    "Tier-1 grace shortcut returned Alive before the t=1200ms pane_dead: \
                     RESUME_PROBE_POST_SHELL_GRACE is too short. \
                     Real opencode crashes at ~1000ms; raise the grace constant."
                ),
                Ok(other) => panic!(
                    "Expected Restarted or Resumed; got {other:?} (probe path is taking an unexpected branch)"
                ),
                Err(e) => panic!(
                    "Tier-2 must succeed because its wrapper does not match the stale sid: {e:#}; \
                     either the cascade is misrouting the sid or Tier-2 spawn is failing"
                ),
            }
            assert!(inst.retroactive_capture_excludes.contains(&stale_sid));
            assert!(inst.agent_session_id.as_deref() != Some(stale_sid.as_str()));
        }
    }
}
