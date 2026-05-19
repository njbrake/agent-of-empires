//! ACP client wrapper.
//!
//! aoe is the *client* in ACP terms; the agent (claude-code, aoe-agent,
//! gemini, etc.) is the *server*. The client sends `initialize`,
//! `session/new`, `session/prompt` and handles incoming `session/update`
//! notifications and `session/request_permission` requests.
//!
//! Architecture: spawn the agent subprocess, build a `ByteStreams`
//! transport over its stdio, run `Client.builder().connect_with(...)` on
//! a background tokio task. The task drives a long-lived loop:
//! initialize once, create one ACP session, then pump commands from an
//! mpsc channel into ACP requests until shutdown.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use agent_client_protocol::schema::{
    CancelNotification, ClientCapabilities, ContentBlock, CreateTerminalRequest,
    CreateTerminalResponse, FileSystemCapabilities, InitializeRequest, KillTerminalRequest,
    KillTerminalResponse, LoadSessionRequest, NewSessionRequest, PermissionOptionKind,
    PromptRequest, ProtocolVersion, ReadTextFileRequest, ReadTextFileResponse,
    ReleaseTerminalRequest, ReleaseTerminalResponse, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, SelectedPermissionOutcome, SessionId,
    SessionNotification, SessionUpdate, SetSessionModeRequest, TerminalId, TerminalOutputRequest,
    TerminalOutputResponse, TextContent, WaitForTerminalExitRequest, WaitForTerminalExitResponse,
    WriteTextFileRequest, WriteTextFileResponse,
};
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectionTo, Responder};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{debug, error, info, trace, warn};

use super::agent_profiles;
use super::agent_registry::AgentSpec;
use super::approvals::{is_destructive, ApprovalDecision, Nonce};
use super::fs_handler::{self, FsPolicy, SandboxPathMap};
use super::permissions::build_approval;
use super::state::{
    AvailableCommand, CockpitSessionId, DiffPreview, Event, ModeInfo, Plan, PlanStep,
    PlanStepStatus, SessionMode, SessionUsage, ToolCall, UsageCost,
};
use super::terminal_handler::TerminalManager;
use crate::session::SandboxInfo;

#[derive(Debug, Error)]
pub enum AcpError {
    #[error("agent spawn failed: {0}")]
    Spawn(String),
    /// The session's working directory does not exist on disk. Distinct
    /// from a generic spawn ENOENT (which on POSIX is indistinguishable
    /// at the libc level between missing binary, missing interpreter, and
    /// missing cwd). Surfaced as its own variant so the UI can render a
    /// targeted remediation banner instead of the default "install the
    /// adapter" copy. See issue #1089.
    #[error("project path no longer exists: {path}")]
    ProjectPathMissing { path: PathBuf },
    #[error("transport error: {0}")]
    Transport(String),
    #[error("protocol violation: {0}")]
    Protocol(String),
    #[error("agent process exited unexpectedly")]
    AgentExited,
    #[error("client task is not running")]
    NotRunning,
    #[error("no pending approval with that nonce")]
    UnknownNonce,
    #[error("agent did not offer a {0:?} option")]
    NoMatchingOption(ApprovalDecision),
}

impl AcpError {
    /// Inspect a `std::io::Error` returned by `Command::spawn` against
    /// the spawn site's cwd + (resolved) command. POSIX returns ENOENT
    /// for both "binary not on PATH" and "cwd does not exist", so the
    /// disambiguation has to happen via filesystem stat. Stats only on
    /// the ENOENT branch to keep the hot path free.
    ///
    /// Belt-and-suspenders for the cwd-missing case: the supervisor
    /// pre-flights `cwd.exists()` before spawning, but the directory
    /// can race-disappear between pre-flight and exec. Without this
    /// classifier the bare ENOENT bubbles up as a generic spawn error
    /// and the UI lands on the wrong remediation banner. See #1089.
    pub fn classify_spawn_error(
        err: std::io::Error,
        cwd: &std::path::Path,
        spawn_command: &str,
    ) -> Self {
        if err.kind() == std::io::ErrorKind::NotFound && !cwd.exists() {
            return AcpError::ProjectPathMissing {
                path: cwd.to_path_buf(),
            };
        }
        AcpError::Spawn(format!("{err} (command `{spawn_command}`)"))
    }
}

/// Configuration for spawning an ACP agent.
#[derive(Debug, Clone)]
pub struct SpawnConfig {
    /// Registry key of the agent (e.g. `"claude"`, `"codex"`,
    /// `"opencode"`). Used to resolve the static `AgentProfile` that
    /// gates server-side claude-specific event synthesis and routes
    /// per-agent slash commands. Defaults to `"claude"` for legacy
    /// callers; the supervisor passes the real key when it spawns.
    pub agent_key: String,
    pub spec: AgentSpec,
    pub cwd: PathBuf,
    pub additional_dirs: Vec<PathBuf>,
    /// Provider env vars to forward (after applying the agent's allowlist).
    pub provider_env: Vec<(String, String)>,
    /// Reserved for a future agent-in-container that natively speaks
    /// the socket transport. The current cockpit sandbox path runs
    /// `docker exec` from the host-side runner (which already holds the
    /// daemon↔runner socket) and proxies the agent's stdio across the
    /// container boundary, so no bind-mount is needed today.
    pub socket_path: Option<PathBuf>,
    /// ACP session id from a previous run, captured during the last
    /// `session/new` and persisted on `Instance.cockpit_acp_session_id`.
    /// When `Some` and the agent advertises
    /// `agent_capabilities.load_session = true`, the connection task
    /// sends `LoadSessionRequest` instead of `NewSessionRequest`. On
    /// load failure the task falls back to `session/new` and emits a
    /// `SessionContextReset` event.
    pub stored_acp_session_id: Option<String>,
    /// When `Some`, the agent runs inside the named Docker container.
    /// Daemon-side spawn wraps the argv in `docker exec` and the
    /// fs/terminal handlers route across the container boundary using
    /// the container_workdir / mount map.
    pub sandbox_info: Option<SandboxInfo>,
    /// Source profile of the session. Used together with `sandbox_info`
    /// to resolve profile-level `sandbox.environment` entries so the
    /// cockpit sandbox env mirrors the tmux substrate. `None` for
    /// non-sandboxed sessions.
    pub source_profile: Option<String>,
}

/// Commands sent from `AcpClient` methods to the background connection task.
enum ClientCmd {
    Prompt(String),
    Cancel,
    SetMode(String),
    Shutdown,
}

/// How the connection task should handle the ACP handshake against the
/// agent.
///
/// `Fresh` is the standard path: send `initialize`, then either
/// `session/load` (if the agent advertises support AND we have a stored
/// id) or `session/new`.
///
/// `Resume` is used by `AcpClient::attach` on `aoe serve` restart, when
/// the per-session `aoe __cockpit-runner` shim kept the agent process
/// alive across the daemon's death. The agent is already initialized
/// and the session is already in its in-memory map; re-sending
/// `session/new` would split context onto a new session id (which the
/// in-flight turn does not address), and re-sending `session/load`
/// against an agent that advertises `loadSession: false` (e.g. the
/// bundled `aoe-agent`) would fall through to `session/new` with the
/// same split-context bug. In `Resume` mode the daemon still sends
/// `initialize` (idempotent for capabilities, cheap, lets us learn the
/// agent's caps) but skips both `session/new` and `session/load` and
/// uses the supplied `acp_session_id` as-is. `in_flight_turn` arms the
/// resume-idle watchdog described in `run_connection_task`.
#[derive(Debug, Clone)]
enum ConnectMode {
    Fresh {
        stored_acp_session_id: Option<String>,
    },
    Resume {
        acp_session_id: String,
        in_flight_turn: bool,
    },
}

/// Time without any inbound notification, after a `Resume`-mode attach
/// with `in_flight_turn = true`, before the watchdog synthesizes a
/// `Stopped { reason: "reattach_idle" }` event. LLM streams rarely have
/// intra-turn silence near this duration so the false-positive risk
/// (UI flips to Idle then back to Streaming on the next chunk) is
/// bounded.
const RESUME_IDLE_GRACE_DEFAULT: std::time::Duration = std::time::Duration::from_secs(10);

/// Grace window between the first `session/cancel` notification (sent
/// during an in-flight `session/prompt`) and the daemon declaring the
/// agent unresponsive. When this fires, the connection task ends with
/// `Stopped { reason: "agent_unresponsive" }` and the supervisor
/// SIGTERMs the runner before respawning via `session/load`. 10s is
/// long enough for claude-agent-acp to resolve a real cancel through
/// the SDK message boundary but short enough that a user who clicked
/// "Force end turn" isn't watching a frozen UI for 30s while the
/// daemon waits. See #1196.
const CANCEL_ESCALATION_GRACE: std::time::Duration = std::time::Duration::from_secs(10);

/// Monotonic counter appended to synthetic tool-call IDs so two events
/// minted within the same millisecond don't collide on the
/// `(session_id, tool_id)` keys used by the cockpit event store.
static SYNTHETIC_TOOL_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Read the resume-idle grace. In debug builds, honors
/// `AOE_RESUME_IDLE_GRACE_MS` so integration tests can short-circuit
/// the default 10s without making real failures racy. Values below
/// 100ms are clamped up so a typo can't effectively disable the
/// watchdog. Release builds always use `RESUME_IDLE_GRACE_DEFAULT`
/// so a misconfigured env var can't surface false-positive Stopped
/// events to real users.
fn resume_idle_grace() -> std::time::Duration {
    #[cfg(debug_assertions)]
    if let Ok(raw) = std::env::var("AOE_RESUME_IDLE_GRACE_MS") {
        if let Ok(ms) = raw.parse::<u64>() {
            return std::time::Duration::from_millis(ms.max(100));
        }
    }
    RESUME_IDLE_GRACE_DEFAULT
}

/// Resolution channel + the option set the agent offered. Stored in the
/// pending-responders map keyed by the cockpit's server-generated nonce.
struct PendingResponder {
    resolver: oneshot::Sender<ApprovalResolutionMessage>,
}

/// Message sent over the resolver oneshot to unblock the parked
/// `on_receive_request` callback.
enum ApprovalResolutionMessage {
    Decision { decision: ApprovalDecision },
    Cancelled,
}

type PendingResponders = Arc<Mutex<HashMap<Nonce, PendingResponder>>>;

/// Top-level ACP client. Owns the subprocess lifetime and pumps events
/// from the connection task.
pub struct AcpClient {
    pub session_id: CockpitSessionId,
    /// Inbound event receiver. Optional so the supervisor can `take()` it
    /// for the drain task, decoupling event polling from the client mutex
    /// (otherwise next_event().await would hold the mutex forever and
    /// deadlock send_prompt).
    inbound: Option<mpsc::Receiver<Event>>,
    cmd_tx: Option<mpsc::Sender<ClientCmd>>,
    pending_responders: PendingResponders,
    /// Hold the subprocess so it gets killed when the client is dropped.
    _child: Option<Arc<Mutex<tokio::process::Child>>>,
}

/// Sandbox handles a connection task needs to route ACP fs/* and
/// terminal/* requests across the container boundary.
#[derive(Debug, Clone)]
pub struct SessionSandbox {
    pub container_name: String,
    pub container_workdir: PathBuf,
    /// Snapshot of the session's sandbox info, used to re-resolve env on
    /// every `terminal/create` so the agent's shell commands see the same
    /// env entries (including any rotated host values) as the interactive
    /// tmux pane.
    pub sandbox_info: SandboxInfo,
    /// Profile the session was created under. Required for
    /// `resolved_sandbox_config` to pick up per-profile env overrides.
    pub source_profile: Option<String>,
    /// Host-side project path. `resolved_sandbox_config` walks up from
    /// here to find any repo-local config overrides.
    pub project_path: PathBuf,
}

impl SessionSandbox {
    /// Build a `SessionSandbox` + `SandboxPathMap` from a `SandboxInfo`
    /// and the session's host-side project_path. Path-map entries
    /// cover only the workspace volume(s) the container was built
    /// with; see `docs/cockpit.md` for the known-limitations note on
    /// agent-config and `extra_volumes`.
    pub fn from_info(
        sandbox: &SandboxInfo,
        project_path: &Path,
        source_profile: Option<String>,
    ) -> Result<(Self, SandboxPathMap), AcpError> {
        let project_path_str = project_path.to_string_lossy().to_string();
        let (volumes, workdir) =
            crate::session::container_config::compute_volume_paths(project_path, &project_path_str)
                .map_err(|e| AcpError::Spawn(format!("compute container workdir: {e}")))?;
        let mounts: Vec<(PathBuf, PathBuf)> = volumes
            .into_iter()
            .map(|v| (PathBuf::from(v.container_path), PathBuf::from(v.host_path)))
            .collect();
        Ok((
            Self {
                container_name: sandbox.container_name.clone(),
                container_workdir: PathBuf::from(workdir),
                sandbox_info: sandbox.clone(),
                source_profile,
                project_path: project_path.to_path_buf(),
            },
            SandboxPathMap::new(mounts),
        ))
    }

    /// Re-resolve env entries for this session's sandbox. Called on every
    /// `terminal/create` so rotated host values (e.g. refreshed tokens)
    /// reach the agent's shell commands without requiring a container
    /// recreate.
    ///
    /// A missing `source_profile` only happens for legacy `WorkerRecord`
    /// entries written before the field was persisted. Warns once per
    /// call rather than failing, since refusing resolution would break
    /// `terminal/create` for sessions that are otherwise healthy.
    pub fn current_env_entries(&self) -> Vec<crate::containers::container_interface::EnvEntry> {
        let profile = match self.source_profile.as_deref() {
            Some(p) => p,
            None => {
                tracing::warn!(
                    target: "cockpit.terminal",
                    container = %self.container_name,
                    "SessionSandbox has no source_profile (likely a legacy WorkerRecord); \
                     resolving terminal/create env against the global default profile"
                );
                ""
            }
        };
        let sandbox_config =
            crate::session::environment::resolved_sandbox_config(profile, &self.project_path);
        crate::session::environment::collect_environment(&sandbox_config, &self.sandbox_info)
    }
}

/// Per-session resources the connection task uses to handle ACP fs/* and
/// terminal/* requests delegated by the agent.
#[derive(Clone)]
struct SessionResources {
    fs_policy: Arc<FsPolicy>,
    terminals: TerminalManager,
    cwd: PathBuf,
    label: String,
    sandbox: Option<SessionSandbox>,
}

impl AcpClient {
    /// Construct a client that does not actually spawn anything. Useful
    /// for unit tests of cockpit state without a real agent.
    pub fn fake_for_test(session_id: CockpitSessionId) -> (Self, mpsc::Sender<Event>) {
        let (event_tx, event_rx) = mpsc::channel(64);
        let client = Self {
            session_id,
            inbound: Some(event_rx),
            cmd_tx: None,
            pending_responders: Arc::new(Mutex::new(HashMap::new())),
            _child: None,
        };
        (client, event_tx)
    }

    /// Spawn an ACP agent subprocess, run the handshake + create a
    /// session, and start pumping notifications into the inbound channel.
    pub async fn spawn(
        config: SpawnConfig,
        session_id: CockpitSessionId,
    ) -> Result<Self, AcpError> {
        // Pre-flight: if the session's project_path was renamed or moved
        // externally (e.g. `git worktree move` or a plain `mv`), the
        // agent process's `current_dir` will ENOENT at exec time. POSIX
        // surfaces that as the same `os error 2` as a missing binary,
        // so without the pre-flight the UI lands on the wrong "install
        // the adapter" remediation. Fail fast with a typed variant so
        // the supervisor can route to a targeted banner. See #1089.
        if !config.cwd.exists() {
            return Err(AcpError::ProjectPathMissing {
                path: config.cwd.clone(),
            });
        }
        let (cmd_tx, cmd_rx) = mpsc::channel::<ClientCmd>(16);
        let (event_tx, event_rx) = mpsc::channel::<Event>(64);
        let pending_responders: PendingResponders = Arc::new(Mutex::new(HashMap::new()));

        // Two transports:
        //  - Socket (runner-mediated): for every cockpit session in
        //    production. Spawn `aoe __cockpit-runner` detached via
        //    `setsid`; the runner binds the unix socket, spawns the
        //    agent over stdio, and survives `aoe serve --stop`. The
        //    daemon then dials the socket and runs the ACP handshake.
        //  - Stdio (in-proc): the legacy direct-spawn path. Retained for
        //    tests where we don't want to depend on `current_exe()` being
        //    a real `aoe` binary, and as a safety valve.
        let mode = ConnectMode::Fresh {
            stored_acp_session_id: config.stored_acp_session_id.clone(),
        };
        let sandbox_pair = if let Some(info) = &config.sandbox_info {
            Some(SessionSandbox::from_info(
                info,
                config.cwd.as_path(),
                config.source_profile.clone(),
            )?)
        } else {
            None
        };
        let runner_sandbox = sandbox_pair.as_ref().map(|(handle, _)| handle);
        let profile = agent_profiles::resolve(&config.agent_key);
        let install_binary = config.spec.command.clone();
        if let Some(socket_path) = config.socket_path.clone() {
            spawn_runner_detached(&config, &socket_path, session_id.0.clone(), runner_sandbox)?;
            return Self::connect_via_socket(
                socket_path,
                config.cwd,
                config.additional_dirs,
                mode,
                session_id,
                pending_responders,
                cmd_tx,
                cmd_rx,
                event_tx,
                event_rx,
                sandbox_pair,
                profile,
                install_binary,
            )
            .await;
        }

        let child = spawn_subprocess(&config)?;
        let child = Arc::new(Mutex::new(child));
        Self::start_with_stdio(
            config.cwd,
            config.additional_dirs,
            mode,
            session_id,
            child,
            pending_responders,
            cmd_tx,
            cmd_rx,
            event_tx,
            event_rx,
            sandbox_pair,
            profile,
            install_binary,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn start_with_stdio(
        cwd: PathBuf,
        additional_dirs: Vec<PathBuf>,
        mode: ConnectMode,
        session_id: CockpitSessionId,
        child: Arc<Mutex<tokio::process::Child>>,
        pending_responders: PendingResponders,
        cmd_tx: mpsc::Sender<ClientCmd>,
        cmd_rx: mpsc::Receiver<ClientCmd>,
        event_tx: mpsc::Sender<Event>,
        event_rx: mpsc::Receiver<Event>,
        sandbox: Option<(SessionSandbox, SandboxPathMap)>,
        profile: &'static agent_profiles::AgentProfile,
        install_binary: String,
    ) -> Result<Self, AcpError> {
        let (stdin, stdout) = {
            let mut guard = child.lock().await;
            let stdin = guard
                .stdin
                .take()
                .ok_or_else(|| AcpError::Spawn("no stdin handle".into()))?;
            let stdout = guard
                .stdout
                .take()
                .ok_or_else(|| AcpError::Spawn("no stdout handle".into()))?;
            (stdin, stdout)
        };

        let transport = ByteStreams::new(stdin.compat_write(), stdout.compat());
        let session_label = session_id.0.clone();
        let child_for_task = child.clone();
        let pending_for_task = pending_responders.clone();

        // Allowed fs roots: cwd + any explicit additional directories.
        let mut roots = vec![cwd.clone()];
        roots.extend(additional_dirs);
        let (sandbox_handle, fs_policy) = match sandbox {
            Some((handle, path_map)) => (
                Some(handle),
                Arc::new(FsPolicy::with_sandbox_map(roots, path_map)),
            ),
            None => (None, Arc::new(FsPolicy::new(roots))),
        };
        let resources = SessionResources {
            fs_policy,
            terminals: TerminalManager::new(),
            cwd: cwd.clone(),
            label: session_label.clone(),
            sandbox: sandbox_handle,
        };

        let (ready_tx, ready_rx) = oneshot::channel::<Result<(), AcpError>>();

        tokio::spawn(run_connection_task(
            transport,
            event_tx,
            cmd_rx,
            cwd,
            session_label.clone(),
            Some(child_for_task),
            pending_for_task,
            resources,
            None,
            mode,
            Some(ready_tx),
            profile,
        ));

        wait_for_handshake(&session_label, ready_rx, Some(&child), &install_binary).await?;

        Ok(Self {
            session_id,
            inbound: Some(event_rx),
            cmd_tx: Some(cmd_tx),
            pending_responders,
            _child: Some(child),
        })
    }

    /// Connect to a per-session runner over its unix socket. Used by the
    /// post-spawn "wait for runner to bind, then dial" path AND by the
    /// `Self::attach` reattach path on `aoe serve` startup. The runner
    /// owns the agent subprocess so this constructor returns an
    /// `AcpClient` with `_child = None`; dropping the client does not
    /// terminate the worker.
    #[allow(clippy::too_many_arguments)]
    async fn connect_via_socket(
        socket_path: PathBuf,
        cwd: PathBuf,
        additional_dirs: Vec<PathBuf>,
        mode: ConnectMode,
        session_id: CockpitSessionId,
        pending_responders: PendingResponders,
        cmd_tx: mpsc::Sender<ClientCmd>,
        cmd_rx: mpsc::Receiver<ClientCmd>,
        event_tx: mpsc::Sender<Event>,
        event_rx: mpsc::Receiver<Event>,
        sandbox: Option<(SessionSandbox, SandboxPathMap)>,
        profile: &'static agent_profiles::AgentProfile,
        install_binary: String,
    ) -> Result<Self, AcpError> {
        // Poll for the runner to finish binding the socket. The runner
        // binds before it spawns the agent so this is usually fast (a
        // few ms) but bound the wait so a wedged runner returns a typed
        // error instead of parking the supervisor.
        let stream = wait_for_socket(&socket_path, std::time::Duration::from_secs(10)).await?;
        let (read_half, write_half) = stream.into_split();
        let transport = ByteStreams::new(write_half.compat_write(), read_half.compat());

        let mut roots = vec![cwd.clone()];
        roots.extend(additional_dirs);
        let (sandbox_handle, fs_policy) = match sandbox {
            Some((handle, path_map)) => (
                Some(handle),
                Arc::new(FsPolicy::with_sandbox_map(roots, path_map)),
            ),
            None => (None, Arc::new(FsPolicy::new(roots))),
        };
        let resources = SessionResources {
            fs_policy,
            terminals: TerminalManager::new(),
            cwd: cwd.clone(),
            label: session_id.0.clone(),
            sandbox: sandbox_handle,
        };

        let session_label = session_id.0.clone();
        let pending_for_task = pending_responders.clone();

        let (ready_tx, ready_rx) = oneshot::channel::<Result<(), AcpError>>();

        tokio::spawn(run_connection_task(
            transport,
            event_tx,
            cmd_rx,
            cwd,
            session_label.clone(),
            None,
            pending_for_task,
            resources,
            None,
            mode,
            Some(ready_tx),
            profile,
        ));

        wait_for_handshake(&session_label, ready_rx, None, &install_binary).await?;

        Ok(Self {
            session_id,
            inbound: Some(event_rx),
            cmd_tx: Some(cmd_tx),
            pending_responders,
            _child: None,
        })
    }

    /// Reattach to an already-running cockpit worker over its unix
    /// socket. Used by `aoe serve` startup when a registry entry has a
    /// live PID and an existing socket file; we connect, send only the
    /// (idempotent) ACP `initialize` request, and reuse the existing
    /// `stored_acp_session_id` directly. We deliberately do NOT issue
    /// `session/new` or `session/load`: the agent process is still
    /// running (the runner kept it alive across `aoe serve --stop`) and
    /// the session is already loaded in its memory, so re-sending those
    /// requests would either split context onto a new session id (when
    /// the agent doesn't advertise `loadSession`) or double-load against
    /// a busy session.
    ///
    /// `in_flight_turn = true` tells the connection task that the
    /// session was mid-prompt when the previous daemon detached. The
    /// task arms a watchdog that emits a synthetic
    /// `Event::Stopped { reason: "reattach_idle" }` after
    /// `RESUME_IDLE_GRACE` of inbound silence, because the agent's
    /// eventual response to the orphaned `session/prompt` carries a
    /// request id this client never issued and is dropped silently by
    /// the underlying transport, leaving the UI otherwise stuck on
    /// "thinking".
    #[allow(clippy::too_many_arguments)]
    pub async fn attach(
        socket_path: PathBuf,
        cwd: PathBuf,
        additional_dirs: Vec<PathBuf>,
        stored_acp_session_id: String,
        in_flight_turn: bool,
        session_id: CockpitSessionId,
        sandbox: Option<(SessionSandbox, SandboxPathMap)>,
        agent_key: String,
    ) -> Result<Self, AcpError> {
        let (cmd_tx, cmd_rx) = mpsc::channel::<ClientCmd>(16);
        let (event_tx, event_rx) = mpsc::channel::<Event>(64);
        let pending_responders: PendingResponders = Arc::new(Mutex::new(HashMap::new()));
        let mode = ConnectMode::Resume {
            acp_session_id: stored_acp_session_id,
            in_flight_turn,
        };
        let profile = agent_profiles::resolve(&agent_key);
        // Resume path has no install hint to surface: the agent is
        // already running. Pass an empty string; the install hint is
        // only consulted on handshake-failure timeouts, which the resume
        // path treats as a different fault.
        Self::connect_via_socket(
            socket_path,
            cwd,
            additional_dirs,
            mode,
            session_id,
            pending_responders,
            cmd_tx,
            cmd_rx,
            event_tx,
            event_rx,
            sandbox,
            profile,
            String::new(),
        )
        .await
    }

    /// Send a user message to the agent (ACP `session/prompt`).
    pub async fn send_prompt(&self, text: &str) -> Result<(), AcpError> {
        let cmd_tx = self.cmd_tx.as_ref().ok_or(AcpError::NotRunning)?;
        cmd_tx
            .send(ClientCmd::Prompt(text.to_string()))
            .await
            .map_err(|_| AcpError::AgentExited)
    }

    /// Cancel the agent's currently-running turn (ACP `session/cancel`
    /// notification). Best-effort: returns Ok even if no turn is in
    /// flight, since the UI can race the agent finishing on its own.
    pub async fn cancel_prompt(&self) -> Result<(), AcpError> {
        let cmd_tx = self.cmd_tx.as_ref().ok_or(AcpError::NotRunning)?;
        cmd_tx
            .send(ClientCmd::Cancel)
            .await
            .map_err(|_| AcpError::AgentExited)
    }

    /// Switch the active session mode via ACP `session/set_mode`.
    pub async fn set_mode(&self, mode_id: &str) -> Result<(), AcpError> {
        let cmd_tx = self.cmd_tx.as_ref().ok_or(AcpError::NotRunning)?;
        cmd_tx
            .send(ClientCmd::SetMode(mode_id.to_string()))
            .await
            .map_err(|_| AcpError::AgentExited)
    }

    /// Resolve a pending permission request. Looks up the parked
    /// responder by nonce and unblocks the `on_receive_request` callback.
    pub async fn resolve_permission(
        &self,
        nonce: Nonce,
        decision: ApprovalDecision,
    ) -> Result<(), AcpError> {
        let mut map = self.pending_responders.lock().await;
        let pending = map.remove(&nonce).ok_or(AcpError::UnknownNonce)?;
        pending
            .resolver
            .send(ApprovalResolutionMessage::Decision { decision })
            .map_err(|_| AcpError::AgentExited)
    }

    /// Cancel a pending permission request. Marks it as cancelled so
    /// the agent receives a structured cancellation outcome.
    pub async fn cancel_permission(&self, nonce: Nonce) -> Result<(), AcpError> {
        let mut map = self.pending_responders.lock().await;
        let pending = map.remove(&nonce).ok_or(AcpError::UnknownNonce)?;
        pending
            .resolver
            .send(ApprovalResolutionMessage::Cancelled)
            .map_err(|_| AcpError::AgentExited)
    }

    /// Shutdown the connection task and kill the subprocess.
    pub async fn shutdown(&self) -> Result<(), AcpError> {
        let cmd_tx = self.cmd_tx.as_ref().ok_or(AcpError::NotRunning)?;
        let _ = cmd_tx.send(ClientCmd::Shutdown).await;
        Ok(())
    }

    /// Drain the next event the agent emitted. Returns None once the
    /// receiver has been moved out via `take_inbound` (the supervisor
    /// path) or the connection task has dropped its sender.
    pub async fn next_event(&mut self) -> Option<Event> {
        match self.inbound.as_mut() {
            Some(rx) => rx.recv().await,
            None => None,
        }
    }

    /// Take ownership of the inbound event receiver. The supervisor uses
    /// this so the drain task can poll events without holding the client
    /// mutex (which would deadlock send_prompt).
    pub fn take_inbound(&mut self) -> Option<mpsc::Receiver<Event>> {
        self.inbound.take()
    }
}

/// Reject `provider_env` request entries whose key would either escape
/// the agent sandbox (PATH, HOME, etc.; `always_forward` already wires
/// those from the operator's environment) or hijack the dynamic linker
/// (LD_PRELOAD, DYLD_INSERT_LIBRARIES, etc.) to run arbitrary code in
/// the child. Provider auth keys (`ANTHROPIC_API_KEY`, etc.) are
/// deliberately NOT on the denylist because per-session provider auth
/// is the legitimate use case for `provider_env`.
///
/// Returns `Some(reason)` if the key is rejected, `None` if it's safe
/// to forward. The reason string is logged as a structured field.
fn provider_env_denyreason(key: &str) -> Option<&'static str> {
    if key.is_empty() {
        return Some("empty key");
    }
    if key == "AOE_TOKEN" {
        return Some("aoe auth token, must not reach the agent");
    }
    // Infrastructure / locale keys that `always_forward` already wires
    // from the parent env. Letting `provider_env` override them lets the
    // request point the agent's binary lookup or home tree at an
    // attacker-controlled location.
    const INFRA_KEYS: &[&str] = &["PATH", "HOME", "USER", "LANG", "LC_ALL", "TERM"];
    if INFRA_KEYS.contains(&key) {
        return Some("infrastructure key, controlled by operator env");
    }
    // Dynamic linker hooks: glibc `LD_*` and macOS `DYLD_*`. Overriding
    // these causes the child process to load attacker-chosen shared
    // objects before main(), bypassing the agent binary entirely.
    if key.starts_with("LD_") || key.starts_with("DYLD_") {
        return Some("dynamic linker hook, would alter child binary load");
    }
    None
}

/// Scrub well-known secret patterns from agent stderr before it lands in
/// `debug.log`. Conservative; only redacts strings that unambiguously
/// signal a secret via prefix (Anthropic `sk-`, GitHub `ghp_`,
/// `Bearer <token>`, etc.). Catches the common case where an adapter
/// prints "auth failed: api_key=sk-ant-..."; will not catch a hand-rolled
/// secret with no recognisable shape. Users sharing logs in bug reports
/// should still scan them; see docs/cockpit.md#sharing-debug-logs.
fn scrub_stderr_secrets(line: &str) -> std::borrow::Cow<'_, str> {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(
            r"\b(sk-(?:ant-)?[A-Za-z0-9_\-]{16,}|ghp_[A-Za-z0-9]{16,}|gho_[A-Za-z0-9]{16,}|github_pat_[A-Za-z0-9_]{16,}|AKIA[A-Z0-9]{16}|Bearer\s+[A-Za-z0-9_.\-]{20,})",
        )
        .expect("static secret-scrub regex must compile")
    });
    re.replace_all(line, "<redacted-secret>")
}

/// Resolve a bare agent command name to an absolute path, scanning common
/// node-version-manager bin dirs (nvm, fnm, mise, asdf, Volta) plus the
/// usual system locations. Returns the absolute binary path and the bin
/// dir we found it in; the caller prepends that dir to the agent's PATH
/// so the adapter's own subprocesses (`node`, `npx`) can still resolve.
///
/// Re-runs per spawn (no cache) so an `nvm use <other-version>` after the
/// daemon started picks up immediately without a daemon restart. Returns
/// None when the command is already a path, contains a `${placeholder}`,
/// or isn't found anywhere we know to look.
pub fn resolve_agent_command(command: &str) -> Option<(std::path::PathBuf, std::path::PathBuf)> {
    if command.contains('/') || command.contains('\\') || command.contains("${") {
        return None;
    }

    if let Some(path) = find_in_path_env(command) {
        let parent = path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(std::path::PathBuf::new);
        return Some((path, parent));
    }

    for dir in node_search_dirs() {
        let candidate = dir.join(command);
        if candidate.is_file() {
            return Some((candidate, dir));
        }
    }
    None
}

fn find_in_path_env(binary: &str) -> Option<std::path::PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(binary);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// Best-effort enumeration of node bin dirs the user is likely to have
/// the adapter installed into. Order matters only for tie-breaking; the
/// first hit wins, but in practice each binary only lives in one place.
fn node_search_dirs() -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = dirs::home_dir() {
        // nvm: `~/.nvm/versions/node/v<ver>/bin/<binary>`
        push_subdirs(&mut out, &home.join(".nvm/versions/node"), "bin");
        // fnm: `~/.fnm/node-versions/v<ver>/installation/bin/<binary>`
        push_subdirs(
            &mut out,
            &home.join(".fnm/node-versions"),
            "installation/bin",
        );
        // mise: `~/.local/share/mise/installs/node/<ver>/bin/<binary>`
        push_subdirs(
            &mut out,
            &home.join(".local/share/mise/installs/node"),
            "bin",
        );
        // asdf: `~/.asdf/installs/nodejs/<ver>/bin/<binary>`
        push_subdirs(&mut out, &home.join(".asdf/installs/nodejs"), "bin");
        // Volta + user-scoped npm prefixes
        out.push(home.join(".volta/bin"));
        out.push(home.join(".npm-global/bin"));
        out.push(home.join(".local/bin"));
        out.push(home.join("bin"));
    }
    out.push(std::path::PathBuf::from("/usr/local/bin"));
    out.push(std::path::PathBuf::from("/opt/homebrew/bin"));
    out.push(std::path::PathBuf::from("/usr/bin"));
    out
}

fn push_subdirs(out: &mut Vec<std::path::PathBuf>, root: &std::path::Path, leaf: &str) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let bin = entry.path().join(leaf);
        if bin.is_dir() {
            out.push(bin);
        }
    }
}

/// Spawn the `aoe __cockpit-runner` shim as a detached process. The
/// runner owns the agent subprocess and outlives the daemon. We retain
/// no `Child` handle here; once the runner is up, the daemon talks to
/// it over the unix socket and the OS keeps the runner alive across
/// `aoe serve` restarts.
fn spawn_runner_detached(
    config: &SpawnConfig,
    socket_path: &std::path::Path,
    session_id: String,
    session_sandbox: Option<&SessionSandbox>,
) -> Result<(), AcpError> {
    use std::process::Command as StdCommand;
    let current_exe =
        std::env::current_exe().map_err(|e| AcpError::Spawn(format!("current_exe: {e}")))?;
    let log_path = crate::cockpit::worker_registry::log_path_for(&session_id)
        .map_err(|e| AcpError::Spawn(format!("log path: {e}")))?;
    if let Some(parent) = log_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    // Sandboxed sessions wrap the agent in `docker exec`. Host-side
    // PATH resolution is skipped because the agent binary lives inside
    // the container; the container's own PATH resolves it. The
    // container_workdir is reused from the SessionSandbox built upstream
    // so we don't redo `compute_volume_paths`.
    let sandbox_argv = match (&config.sandbox_info, session_sandbox) {
        (Some(sandbox), Some(handle)) => Some(build_sandbox_docker_argv(
            config,
            sandbox,
            handle.container_workdir.to_string_lossy().as_ref(),
        )?),
        (Some(_), None) => {
            return Err(AcpError::Spawn(
                "sandbox_info set but SessionSandbox handle missing; \
                 SessionSandbox::from_info must run before spawn_runner_detached"
                    .into(),
            ));
        }
        (None, _) => None,
    };

    // Resolve the agent binary against PATH + known node-manager dirs so
    // the runner spawns the right binary even when the daemon's frozen
    // PATH doesn't contain it. See #1048. The resolved bin dir is also
    // prepended to PATH below so the adapter's own `node`/`npx`
    // subprocesses land in the same install.
    let resolved = if sandbox_argv.is_some() {
        None
    } else {
        resolve_agent_command(&config.spec.command)
    };
    let (spawn_command, extra_path_dir) = match (&sandbox_argv, &resolved) {
        (Some(s), _) => (s.docker_binary.clone(), None),
        (None, Some((abs, dir))) => (abs.to_string_lossy().into_owned(), Some(dir.clone())),
        (None, None) => (config.spec.command.clone(), None),
    };

    let mut cmd = StdCommand::new(&current_exe);
    cmd.arg("__cockpit-runner")
        .arg("--socket")
        .arg(socket_path)
        .arg("--session-id")
        .arg(&session_id)
        .arg("--agent-name")
        .arg(&config.spec.command)
        .arg("--agent-key")
        .arg(&config.agent_key)
        .arg("--cwd")
        .arg(&config.cwd);
    if !config.additional_dirs.is_empty() {
        cmd.arg("--additional-dirs").arg(
            config
                .additional_dirs
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(","),
        );
    }
    let provider_keys: Vec<&str> = config
        .provider_env
        .iter()
        .map(|(k, _)| k.as_str())
        .collect();
    if !provider_keys.is_empty() {
        cmd.arg("--provider-env-keys").arg(provider_keys.join(","));
    }
    if let Some(profile) = config.source_profile.as_deref().filter(|s| !s.is_empty()) {
        cmd.arg("--source-profile").arg(profile);
    }
    if let Some(stored) = &config.stored_acp_session_id {
        cmd.arg("--stored-acp-session-id").arg(stored);
    }
    cmd.arg("--");
    if let Some(s) = &sandbox_argv {
        cmd.arg(&s.docker_binary);
        for a in &s.docker_args {
            cmd.arg(a);
        }
    } else {
        // Pass the resolved absolute path (or fall back to the bare command).
        // The runner spawns whatever it receives, so an absolute path bypasses
        // any PATH lookup inside the runner.
        cmd.arg(&spawn_command);
        for a in &config.spec.args {
            cmd.arg(a);
        }
    }

    // Env: apply the same allowlist + provider_env filtering that the
    // legacy in-proc path does, then hand the cleaned env to the runner.
    // The runner inherits this env when it spawns the agent (no second
    // filter pass needed). AOE_TOKEN is stripped here so it never reaches
    // either process.
    cmd.env_clear();
    apply_env_filter(&mut cmd, config);
    if let Some(s) = &sandbox_argv {
        // The agent runs inside the container; docker reads each
        // `-e KEY` flag's value from its own process env. Set the
        // corresponding values on the runner so docker (its child)
        // can forward them across the container boundary.
        for (key, value) in &s.inherit_env {
            cmd.env(key, value);
        }
    }
    if let Some(extra) = &extra_path_dir {
        // Prepend the resolved bin dir to the PATH we just forwarded so
        // the adapter's own `node`/`npx` lookups land in the same install
        // as the adapter itself, not whatever node happens to be on the
        // daemon's frozen PATH.
        let current = std::env::var("PATH").unwrap_or_default();
        let extra_s = extra.to_string_lossy();
        if !std::env::split_paths(&current).any(|p| p == *extra) {
            cmd.env("PATH", format!("{}:{}", extra_s, current));
        }
    }

    // Detach: child becomes its own session leader so a SIGTERM/SIGHUP
    // to the aoe daemon's group doesn't cascade. The runner installs its
    // own signal handlers.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                nix::unistd::setsid().map_err(std::io::Error::other)?;
                Ok(())
            });
        }
    }

    // Redirect stdio: the runner writes its own log file. Inheriting our
    // stdio would (a) pollute the shared debug.log with the per-session
    // noise and (b) keep a pipe open to the daemon, which then closes
    // when we die, making the runner observe EOF on its own stdin/stdout.
    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());

    info!(
        target: "cockpit.acp.spawn",
        session = %session_id,
        socket = %socket_path.display(),
        runner = %current_exe.display(),
        agent = %config.spec.command,
        resolved = %spawn_command,
        "spawning detached cockpit runner"
    );

    cmd.spawn().map_err(|e| {
        warn!(
            target: "cockpit.acp.spawn",
            session = %session_id,
            "runner spawn failed: {e}"
        );
        AcpError::Spawn(format!("spawn runner: {e}"))
    })?;
    // Drop the std::process::Child here. std::process::Command doesn't
    // wait on drop, so the runner stays alive. setsid + nohup-equivalent
    // make this an actual detach.
    Ok(())
}

/// Result of constructing the `docker exec` argv for a sandboxed cockpit
/// spawn. `docker_binary` is argv[0] (the docker/podman runtime);
/// `docker_args` is everything after it (including the container name
/// and the in-container agent argv). `inherit_env` is the set of
/// (key, value) pairs the parent process must export so docker can
/// forward them via the matching `-e KEY` flags already in `docker_args`.
struct SandboxArgv {
    docker_binary: String,
    docker_args: Vec<String>,
    inherit_env: Vec<(String, String)>,
}

/// Build the `docker exec` argv for a sandboxed cockpit spawn. The
/// resulting command is what the runner executes; docker proxies the
/// agent's stdio across the container boundary. Mirrors the tmux
/// substrate's env handling so the same `sandbox.environment` and
/// `extra_env` entries take effect.
///
/// `container_workdir` is the in-container working directory for the
/// session, pre-computed by `SessionSandbox::from_info` and passed
/// through to avoid re-running `compute_volume_paths`.
fn build_sandbox_docker_argv(
    config: &SpawnConfig,
    sandbox: &SandboxInfo,
    container_workdir: &str,
) -> Result<SandboxArgv, AcpError> {
    use crate::containers::container_interface::docker_env_args;

    let runtime = crate::containers::get_container_runtime();
    let docker_binary = runtime.base.binary.to_string();

    let project_path = config.cwd.as_path();
    let profile_for_env = config.source_profile.as_deref().unwrap_or("");
    let sandbox_config =
        crate::session::environment::resolved_sandbox_config(profile_for_env, project_path);
    let env_entries = crate::session::environment::collect_environment(&sandbox_config, sandbox);

    let mut docker_args: Vec<String> = vec![
        "exec".into(),
        "-i".into(),
        "-w".into(),
        container_workdir.to_string(),
    ];
    // `collect_environment` already dedupes by key, so the entry list is
    // unique. We still track `seen_keys` so the provider-auth block below
    // can skip keys we've already forwarded.
    let mut seen_keys: std::collections::HashSet<String> =
        env_entries.iter().map(|e| e.key().to_string()).collect();
    let (env_argv, inherit_pairs) = docker_env_args(&env_entries);
    docker_args.extend(env_argv);
    let mut inherit_env: Vec<(String, String)> = inherit_pairs;

    // Provider auth keys: forward into the container only when set on
    // the host AND not already in the sandbox env list. Value-typed
    // only; host filesystem paths (e.g. `CLAUDE_CONFIG_DIR`) must not
    // cross the namespace boundary because they reference paths that
    // don't exist inside the container. The agent's config dir is
    // already bind-mounted at the canonical container path via
    // `AGENT_CONFIG_MOUNTS`.
    const PROVIDER_AUTH_KEYS: &[&str] = &[
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "CLAUDE_CODE_OAUTH_TOKEN",
    ];
    for &key in PROVIDER_AUTH_KEYS {
        if seen_keys.contains(key) {
            continue;
        }
        if let Ok(value) = std::env::var(key) {
            seen_keys.insert(key.to_string());
            docker_args.push("-e".into());
            docker_args.push(key.into());
            inherit_env.push((key.into(), value));
        }
    }

    // Per-spawn provider_env entries (the request's auth payload).
    for (key, value) in &config.provider_env {
        if provider_env_denyreason(key).is_some() {
            continue;
        }
        if seen_keys.insert(key.clone()) {
            docker_args.push("-e".into());
            docker_args.push(key.clone());
            inherit_env.push((key.clone(), value.clone()));
        }
    }

    // Model override (AOE_AGENT_MODEL): the supervisor folds the
    // requested model into provider_env above, so it's already covered.

    docker_args.push(sandbox.container_name.clone());
    docker_args.push(config.spec.command.clone());
    for a in &config.spec.args {
        docker_args.push(a.clone());
    }

    Ok(SandboxArgv {
        docker_binary,
        docker_args,
        inherit_env,
    })
}

/// Apply the env_clear + allowlist + provider_env filtering used by both
/// the detached-runner path and the in-proc stdio path. Pulled out so
/// the two spawn sites share the same security posture.
fn apply_env_filter(cmd: &mut std::process::Command, config: &SpawnConfig) {
    const ALWAYS_FORWARD: &[&str] = &[
        "PATH",
        "HOME",
        "LANG",
        "LC_ALL",
        "TERM",
        "USER",
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "CLAUDE_CODE_OAUTH_TOKEN",
        "CLAUDE_CONFIG_DIR",
    ];
    for name in ALWAYS_FORWARD {
        if let Ok(value) = std::env::var(name) {
            cmd.env(name, value);
        }
    }
    if let Some(extra_allowlist) = &config.spec.env_allowlist {
        for name in extra_allowlist {
            if name == "AOE_TOKEN" {
                continue;
            }
            if let Ok(value) = std::env::var(name) {
                cmd.env(name, value);
            }
        }
    }
    for (key, value) in &config.provider_env {
        if provider_env_denyreason(key).is_some() {
            continue;
        }
        cmd.env(key, value);
    }
}

/// Poll the socket file's existence with `connect()` until a deadline.
/// Used by `connect_via_socket` to wait for the runner to finish binding
/// before the daemon dials in.
async fn wait_for_socket(
    path: &std::path::Path,
    deadline: std::time::Duration,
) -> Result<tokio::net::UnixStream, AcpError> {
    let started = std::time::Instant::now();
    let mut delay_ms = 20_u64;
    loop {
        if path.exists() {
            match tokio::net::UnixStream::connect(path).await {
                Ok(s) => return Ok(s),
                Err(e) if matches!(e.kind(), std::io::ErrorKind::ConnectionRefused) => {
                    // Listener not yet ready; back off and retry.
                }
                Err(e) => return Err(AcpError::Spawn(format!("connect {}: {e}", path.display()))),
            }
        }
        if started.elapsed() >= deadline {
            return Err(AcpError::Spawn(format!(
                "runner socket {} did not appear within {}s",
                path.display(),
                deadline.as_secs()
            )));
        }
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        delay_ms = (delay_ms * 2).min(200);
    }
}

fn spawn_subprocess(config: &SpawnConfig) -> Result<tokio::process::Child, AcpError> {
    // Resolve bare command names against PATH + known node-manager dirs.
    // `aoe serve` captures PATH at daemon-launch time and freezes it for
    // its lifetime; without this, a `nvm use` after launch leaves the
    // adapter installed but unreachable. See #1048.
    let resolved = resolve_agent_command(&config.spec.command);
    let (spawn_command, extra_path_dir) = match &resolved {
        Some((abs, dir)) => (abs.to_string_lossy().into_owned(), Some(dir.clone())),
        None => (config.spec.command.clone(), None),
    };

    let mut cmd = tokio::process::Command::new(&spawn_command);
    cmd.args(&config.spec.args)
        .current_dir(&config.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    // Env: clear, then forward an explicit allowlist + provider-specific
    // creds. AOE_TOKEN must NEVER reach the agent.
    cmd.env_clear();
    let always_forward = [
        "PATH",
        "HOME",
        "LANG",
        "LC_ALL",
        "TERM",
        "USER",
        // Provider auth: forwarded by default so users who already have
        // `ANTHROPIC_API_KEY` (or have run `claude /login` so their
        // ~/.claude credentials sit under HOME) get a working agent
        // without manual env_allowlist plumbing.
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "CLAUDE_CODE_OAUTH_TOKEN",
        "CLAUDE_CONFIG_DIR",
    ];
    let mut forwarded_keys: Vec<&str> = Vec::new();
    for name in always_forward {
        if let Ok(mut value) = std::env::var(name) {
            // Prepend the resolved bin dir to PATH so the adapter's own
            // `node`/`npx` lookups land in the same node install as the
            // adapter itself, not whatever node happens to be on the
            // daemon's frozen PATH.
            if name == "PATH" {
                if let Some(extra) = &extra_path_dir {
                    let extra_s = extra.to_string_lossy();
                    if !std::env::split_paths(&value).any(|p| p == *extra) {
                        value = format!("{}:{}", extra_s, value);
                    }
                }
            }
            cmd.env(name, value);
            forwarded_keys.push(name);
        }
    }
    if let Some(extra_allowlist) = &config.spec.env_allowlist {
        for name in extra_allowlist {
            if name == "AOE_TOKEN" {
                warn!(target: "cockpit", "ignoring AOE_TOKEN in agent env allowlist");
                continue;
            }
            if let Ok(value) = std::env::var(name) {
                cmd.env(name, value);
                forwarded_keys.push(name.as_str());
            }
        }
    }
    let mut provider_keys: Vec<&str> = Vec::new();
    for (key, value) in &config.provider_env {
        if let Some(reason) = provider_env_denyreason(key) {
            warn!(
                target: "cockpit",
                key = %key,
                reason,
                "rejecting provider_env override of protected key",
            );
            continue;
        }
        cmd.env(key, value);
        provider_keys.push(key.as_str());
    }

    // Socket-transport agents need to know where to connect. Pass the
    // path via env so the agent's bootstrap can `connect()` to it
    // instead of falling back to stdio.
    if let Some(socket_path) = &config.socket_path {
        cmd.env("AOE_ACP_SOCKET", socket_path);
    }

    info!(
        target: "cockpit.acp.spawn",
        command = %config.spec.command,
        resolved = %spawn_command,
        args = ?config.spec.args,
        cwd = %config.cwd.display(),
        transport = if config.socket_path.is_some() { "socket" } else { "stdio" },
        socket = ?config.socket_path,
        env_forwarded = ?forwarded_keys,
        provider_env = ?provider_keys,
        "spawning ACP agent subprocess"
    );

    let mut child = cmd.spawn().map_err(|e| {
        warn!(
            target: "cockpit.acp.spawn",
            command = %config.spec.command,
            resolved = %spawn_command,
            "spawn failed: {e}"
        );
        // POSIX ENOENT on `Command::spawn` is ambiguous: missing binary,
        // missing cwd, or missing interpreter all surface as the same
        // libc error. Order matters here:
        //   1. cwd missing → ProjectPathMissing (so the UI renders the
        //      "restore or rebind project_path" banner, not the
        //      install-adapter copy). See #1089.
        //   2. bare-command ENOENT with no PATH resolution → enriched
        //      Spawn message hinting at the frozen-PATH cause. See #1048.
        //   3. fallback → generic Spawn classification.
        if e.kind() == std::io::ErrorKind::NotFound && config.cwd.exists() && resolved.is_none() {
            AcpError::Spawn(format!(
                "{} (binary `{}` not found on the daemon's PATH or in any \
                 known node-manager bin dir; install it where the daemon \
                 can see it, or restart `aoe serve` from a shell where \
                 `which {}` resolves)",
                e, config.spec.command, config.spec.command
            ))
        } else {
            AcpError::classify_spawn_error(e, &config.cwd, &spawn_command)
        }
    })?;

    let pid = child.id();
    info!(
        target: "cockpit.acp.spawn",
        command = %config.spec.command,
        pid = ?pid,
        "ACP agent subprocess started"
    );

    // Drain stderr line-by-line into the tracing log. Without this the
    // child's stderr pipe fills up at ~64KB and the agent blocks on
    // write, looking like a wedged ACP handshake. Logging every line
    // also gives us a record of what the adapter said before it died.
    if let Some(stderr) = child.stderr.take() {
        let command_label = config.spec.command.clone();
        tokio::spawn(async move {
            use tokio::io::{AsyncBufReadExt, BufReader};
            let mut reader = BufReader::new(stderr).lines();
            loop {
                match reader.next_line().await {
                    Ok(Some(line)) => {
                        debug!(
                            target: "cockpit.acp.stderr",
                            command = %command_label,
                            pid = ?pid,
                            "{}",
                            scrub_stderr_secrets(&line),
                        );
                    }
                    Ok(None) => {
                        debug!(
                            target: "cockpit.acp.stderr",
                            command = %command_label,
                            pid = ?pid,
                            "stderr EOF"
                        );
                        break;
                    }
                    Err(e) => {
                        warn!(
                            target: "cockpit.acp.stderr",
                            command = %command_label,
                            pid = ?pid,
                            "stderr read error: {e}"
                        );
                        break;
                    }
                }
            }
        });
    } else {
        warn!(
            target: "cockpit.acp.spawn",
            command = %config.spec.command,
            pid = ?pid,
            "child has no stderr handle; agent crashes will be silent"
        );
    }

    Ok(child)
}

/// Translate the user's decision into the matching option_id from the
/// list the agent offered. Falls back gracefully if the agent didn't
/// offer the preferred kind.
fn pick_option_id(
    options: &[agent_client_protocol::schema::PermissionOption],
    decision: ApprovalDecision,
) -> Option<agent_client_protocol::schema::PermissionOptionId> {
    let preferred_kinds = match decision {
        ApprovalDecision::Allow => &[
            PermissionOptionKind::AllowOnce,
            PermissionOptionKind::AllowAlways,
        ][..],
        ApprovalDecision::AllowAlways => &[
            PermissionOptionKind::AllowAlways,
            PermissionOptionKind::AllowOnce,
        ][..],
        ApprovalDecision::Deny => &[
            PermissionOptionKind::RejectOnce,
            PermissionOptionKind::RejectAlways,
        ][..],
        // Synthetic decision emitted by the daemon-restart rehydration
        // sweep. Has no agent option to map to (the agent never sees
        // it); the caller falls through to `RequestPermissionOutcome::
        // Cancelled` when this returns None.
        ApprovalDecision::Cancelled => &[][..],
    };
    for kind in preferred_kinds {
        if let Some(opt) = options.iter().find(|o| &o.kind == kind) {
            return Some(opt.option_id.clone());
        }
    }
    None
}

/// True when the event would reproduce a prior turn's visible
/// transcript. Used to scope the post-`session/load` suppression
/// window: claude-agent-acp re-emits historical assistant chunks and
/// tool calls during the load handshake (which would double-render
/// against our own SQLite-restored transcript), but it ALSO emits
/// ambient state (available_commands, current_mode, usage) and
/// lifecycle events that the UI needs immediately on resume. Drop the
/// former, pass the latter through.
fn is_transcript_event(event: &Event) -> bool {
    matches!(
        event,
        Event::AgentMessageChunk { .. }
            | Event::ToolCallStarted { .. }
            | Event::ToolCallCompleted { .. }
            | Event::ToolCallContent { .. }
            | Event::ToolCallUpdated { .. }
            | Event::DiffEmitted { .. }
            | Event::PlanUpdated { .. }
            | Event::TodoListUpdated { .. }
            | Event::ThinkingStarted
            | Event::ThinkingEnded
            | Event::UserPromptSent { .. }
            | Event::ApprovalRequested { .. }
            | Event::ApprovalResolved { .. }
            | Event::RawAgentUpdate { .. }
    )
}

/// Cheap discriminant for log breadcrumbs (matches the one in
/// event_store, kept separate so this module doesn't depend on the
/// store's private helper).
fn transcript_event_kind(event: &Event) -> &'static str {
    match event {
        Event::AgentMessageChunk { .. } => "agent_message_chunk",
        Event::ToolCallStarted { .. } => "tool_call_started",
        Event::ToolCallCompleted { .. } => "tool_call_completed",
        Event::ToolCallContent { .. } => "tool_call_content",
        Event::ToolCallUpdated { .. } => "tool_call_updated",
        Event::DiffEmitted { .. } => "diff_emitted",
        Event::PlanUpdated { .. } => "plan_updated",
        Event::TodoListUpdated { .. } => "todo_list_updated",
        Event::ThinkingStarted => "thinking_started",
        Event::ThinkingEnded => "thinking_ended",
        Event::UserPromptSent { .. } => "user_prompt_sent",
        Event::ApprovalRequested { .. } => "approval_requested",
        Event::ApprovalResolved { .. } => "approval_resolved",
        Event::RawAgentUpdate { .. } => "raw_agent_update",
        _ => "other",
    }
}

/// Build a `WakeupScheduled` event from a `ScheduleWakeup` tool's
/// raw_input. Reads `delaySeconds` (number, falls back to numeric
/// string) and the optional `reason`; computes the absolute wake
/// timestamp from `Utc::now()`. Returns `None` if `delaySeconds` is
/// missing or non-finite, better to skip the event than publish a
/// wakeup at epoch zero. See #1091.
fn wakeup_event_from_raw(raw_input: &serde_json::Value) -> Option<Event> {
    let Some(delay_value) = raw_input.get("delaySeconds") else {
        debug!(
            target: "cockpit.acp.wakeup",
            "ScheduleWakeup raw_input missing `delaySeconds`; not emitting WakeupScheduled"
        );
        return None;
    };
    let Some(delay_secs) = delay_value
        .as_f64()
        .or_else(|| delay_value.as_str().and_then(|s| s.parse().ok()))
    else {
        debug!(
            target: "cockpit.acp.wakeup",
            value = %delay_value,
            "ScheduleWakeup `delaySeconds` not numeric; not emitting WakeupScheduled"
        );
        return None;
    };
    if !delay_secs.is_finite() || delay_secs < 0.0 {
        warn!(
            target: "cockpit.acp.wakeup",
            delay_secs,
            "ScheduleWakeup `delaySeconds` non-finite or negative; refusing to emit"
        );
        return None;
    }
    let delay_ms = (delay_secs * 1000.0).clamp(0.0, i64::MAX as f64) as i64;
    let at = chrono::Utc::now() + chrono::Duration::milliseconds(delay_ms);
    let reason = raw_input
        .get("reason")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());
    info!(
        target: "cockpit.acp.wakeup",
        delay_secs,
        wake_at = %at,
        reason = ?reason,
        "emitting WakeupScheduled from ScheduleWakeup tool args"
    );
    Some(Event::WakeupScheduled { at, reason })
}

/// Parse Claude's ExitPlanMode tool input into a structured `Plan`.
/// Claude ships the plan markdown in `raw_input.plan`; we extract its
/// bullet- or number-prefixed lines as `PlanStep`s with status=Pending,
/// matching the ACP `SessionUpdate::Plan` shape so the existing
/// PlanStrip renderer can consume it.
///
/// Returns `None` when the input has no `plan` key, the value isn't a
/// string, or the string has no recognisable list items; in which case
/// the generic tool card is still rendered so the user sees the raw
/// plan text. See #1059 for the upstream gap this works around.
fn extract_plan_from_switch_mode(raw_input: &serde_json::Value) -> Option<Plan> {
    let plan_text = raw_input.get("plan")?.as_str()?;
    let steps = parse_plan_steps(plan_text);
    if steps.is_empty() {
        return None;
    }
    Some(Plan {
        plan_id: format!("plan-{}", chrono::Utc::now().timestamp_millis()),
        version: 1,
        steps,
    })
}

/// Flatten plan markdown into `PlanStep`s. v1 heuristic: every line
/// starting with `-`, `*`, or `<digit>.` becomes one step. Sub-bullets
/// flatten into the parent list (PlanEntry has no nesting field in the
/// ACP spec). Strips bold/italic markers from the step title so the
/// PlanStrip doesn't render literal `**foo**`.
fn parse_plan_steps(text: &str) -> Vec<PlanStep> {
    use std::sync::OnceLock;
    static BULLET: OnceLock<regex::Regex> = OnceLock::new();
    let bullet = BULLET.get_or_init(|| {
        regex::Regex::new(r"^\s*(?:[-*]|\d+\.)\s+(.+?)\s*$")
            .expect("static plan-step regex must compile")
    });

    let mut steps = Vec::new();
    for line in text.lines() {
        if let Some(caps) = bullet.captures(line) {
            let raw_title = caps.get(1).map(|m| m.as_str()).unwrap_or("");
            let title = strip_markdown_emphasis(raw_title);
            if title.is_empty() {
                continue;
            }
            steps.push(PlanStep {
                id: format!("step-{}", steps.len()),
                title,
                detail: None,
                status: PlanStepStatus::Pending,
            });
        }
    }
    steps
}

fn strip_markdown_emphasis(s: &str) -> String {
    // Replace **bold**, __bold__, *italic*, _italic_ markers with their
    // inner text. Keep it permissive; the source is Claude's planning
    // markdown, which is usually well-formed but occasionally drops a
    // closing marker.
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        regex::Regex::new(r"\*\*(.+?)\*\*|__(.+?)__|\*([^*]+?)\*|_([^_]+?)_")
            .expect("static emphasis-strip regex must compile")
    });
    re.replace_all(s.trim(), |caps: &regex::Captures<'_>| {
        for i in 1..=4 {
            if let Some(m) = caps.get(i) {
                return m.as_str().to_string();
            }
        }
        String::new()
    })
    .into_owned()
}

/// Heuristic detector for the end of a `/compact` cycle. The Claude ACP
/// adapter emits "Compacting..." while the compaction runs and
/// "Compacting completed." once the model's context window has been
/// replaced by a summary; both as plain `agent_message_chunk`s with no
/// `_meta` flag (see #1050 for the upstream gap). String-matching on
/// the completion message is fragile to localisation but the wrong-firing
/// failure mode (an extra "context reset" divider) is harmless; it can
/// never destroy transcript data.
fn is_compact_completion(text: &str) -> bool {
    text.contains("Compacting completed.")
}

/// Map an ACP `SessionUpdate` to the cockpit's typed `Event`. Variants we
/// don't yet handle pass through as `RawAgentUpdate` so UI clients can at
/// least see them; we'll narrow these as the schema stabilises.
///
/// `profile` carries per-agent gates for claude-specific synthesis
/// (subagent linkage namespace, ExitPlanMode-to-Plan, ScheduleWakeup);
/// other agents pass these through as plain tool calls.
fn map_update_to_events(
    update: SessionUpdate,
    profile: &'static agent_profiles::AgentProfile,
) -> Vec<Event> {
    match update {
        SessionUpdate::AgentMessageChunk(chunk) => match chunk.content {
            ContentBlock::Text(text) => {
                // /compact emits a plain text chunk ("Compacting completed.")
                // and a usage_update with used=0; no typed signal. Detect
                // the literal string the adapter uses and append a typed
                // SessionContextReset event so the cockpit can render a
                // divider, otherwise the silent context replacement leaves
                // the chat looking unchanged while the model's view has
                // been swapped out underneath the user. See #1050.
                let mut events = vec![Event::AgentMessageChunk {
                    text: text.text.clone(),
                }];
                if is_compact_completion(&text.text) {
                    events.push(Event::ConversationCompacted);
                    // /compact wipes the model's tool-state alongside the
                    // chat history, so any TodoWrite plan it was tracking
                    // is gone from its perspective. The cockpit plan strip
                    // (PlanStrip + sidebar PlanProgressMini) lives in our
                    // own event log though, so without this clear it keeps
                    // showing a plan Claude no longer remembers; the user
                    // then asks "resolve the first task" and Claude
                    // responds "no task list." Emit an empty PlanUpdated
                    // so the UI matches the model's actual context.
                    events.push(Event::PlanUpdated {
                        plan: Plan {
                            plan_id: format!("plan-{}", chrono::Utc::now().timestamp_millis()),
                            version: 1,
                            steps: Vec::new(),
                        },
                    });
                }
                events
            }
            other => vec![raw_event(&other)],
        },
        SessionUpdate::AgentThoughtChunk(_) => vec![Event::ThinkingStarted],
        SessionUpdate::ToolCall(tc) => {
            let raw_args = tc.raw_input.clone().unwrap_or(serde_json::Value::Null);
            let args_preview = preview_args(&raw_args);
            let parent_tool_call_id = profile.parent_tool_use_id_from_meta(&tc.meta);
            if let Some(parent) = parent_tool_call_id.as_deref() {
                // Breadcrumb so AOE_ACP_TRACE=1 sessions can verify the
                // subagent linkage round-trip (parent Task id → child
                // tool_call id) end-to-end. See #1041 layer C.
                debug!(
                    target: "cockpit.acp",
                    child = %tc.tool_call_id.0,
                    parent,
                    kind = %tool_kind_str(&tc.kind),
                    "subagent child tool_call linked to parent via _meta.claudeCode.parentToolUseId"
                );
            }
            let tool_call = ToolCall {
                id: tc.tool_call_id.0.to_string(),
                name: tc.title.clone(),
                kind: tool_kind_str(&tc.kind),
                args_preview: args_preview.clone(),
                started_at: chrono::Utc::now(),
                parent_tool_call_id,
            };
            let mut events = vec![Event::ToolCallStarted { tool_call }];
            if is_destructive(&tc.title, &args_preview) {
                debug!(target: "cockpit.acp", "tool {} flagged destructive on tool_call ingest", tc.title);
            }
            // If the same payload carries diff content, surface it.
            if let Some(diff) = extract_diff_from_locations(&tc.locations) {
                events.push(Event::DiffEmitted { diff });
            }
            // claude-agent-acp routes Claude's built-in ExitPlanMode through
            // the tool channel (kind=switch_mode, plan markdown in
            // raw_input.plan) instead of the structured SessionUpdate::Plan
            // channel. Synthesise a PlanUpdated event so the cockpit's
            // PlanStrip and the rest of the plan-aware UI light up. See
            // #1059. Gated on the agent's profile so codex / opencode /
            // gemini mode switches don't spuriously emit empty Plans.
            if profile.supports_exit_plan_mode
                && matches!(tc.kind, agent_client_protocol::schema::ToolKind::SwitchMode)
            {
                if let Some(plan) = extract_plan_from_switch_mode(&raw_args) {
                    events.push(Event::PlanUpdated { plan });
                }
            }
            // The Claude Agent SDK's `ScheduleWakeup` tool sleeps the
            // session until `now + delaySeconds`, with `/loop` dynamic
            // mode self-firing a fresh prompt when the wake triggers.
            // Capture an absolute `at` timestamp here so the sidebar
            // countdown survives daemon restarts and never has to parse
            // the natural-language output string. See #1091. Gated on
            // the agent's profile (claude-only today) so coincidental
            // tool names on other agents don't fire a wakeup event.
            if profile.supports_wakeup_tools && tc.title == "ScheduleWakeup" {
                if let Some(event) = wakeup_event_from_raw(&raw_args) {
                    events.push(event);
                }
            }
            events
        }
        SessionUpdate::ToolCallUpdate(update) => {
            let id = update.tool_call_id.0.to_string();
            let is_error = matches!(
                update.fields.status,
                Some(agent_client_protocol::schema::ToolCallStatus::Failed)
            );
            let completed = matches!(
                update.fields.status,
                Some(agent_client_protocol::schema::ToolCallStatus::Completed)
                    | Some(agent_client_protocol::schema::ToolCallStatus::Failed)
            );
            // claude-agent-acp emits the initial `tool_call` frame
            // eagerly, often well before the underlying bash / read /
            // edit actually starts running. Use `status: InProgress` as
            // the canonical "running now" signal and re-stamp the
            // tool's `started_at` so the duration label measures real
            // tool runtime rather than adapter scheduling overhead.
            // See #1060.
            let in_progress = matches!(
                update.fields.status,
                Some(agent_client_protocol::schema::ToolCallStatus::InProgress)
            );
            let content_text = update
                .fields
                .content
                .as_ref()
                .map(|blocks| extract_tool_content_text(blocks))
                .unwrap_or_default();
            let new_args_preview = update.fields.raw_input.as_ref().map(preview_args);
            let new_title = update.fields.title.clone();
            let mut events: Vec<Event> = Vec::new();
            if new_title.is_some() || new_args_preview.is_some() || in_progress {
                events.push(Event::ToolCallUpdated {
                    tool_call_id: id.clone(),
                    title: new_title,
                    args_preview: new_args_preview,
                    started_at: if in_progress {
                        Some(chrono::Utc::now())
                    } else {
                        None
                    },
                });
            }
            if completed {
                events.push(Event::ToolCallCompleted {
                    tool_call_id: id,
                    is_error,
                    content: content_text,
                    completed_at: chrono::Utc::now(),
                });
            } else if !content_text.is_empty() {
                events.push(Event::ToolCallContent {
                    tool_call_id: id,
                    content: content_text,
                });
            } else if events.is_empty() {
                events.push(raw_event(&update));
            }
            // claude-agent-acp emits the initial `tool_call` frame for
            // ScheduleWakeup with empty `raw_input`; the actual
            // `delaySeconds` lands on a subsequent `ToolCallUpdate`. The
            // emit path in the `ToolCall` branch above therefore never
            // sees real args and `wakeup_event_from_raw` returns None,
            // so re-check here when the update carries both the title
            // and a populated raw_input. See #1091. Gated on profile so
            // non-claude agents don't fire WakeupScheduled on coincidence.
            if profile.supports_wakeup_tools
                && matches!(update.fields.title.as_deref(), Some("ScheduleWakeup"))
            {
                if let Some(raw) = update.fields.raw_input.as_ref() {
                    if let Some(event) = wakeup_event_from_raw(raw) {
                        events.push(event);
                    }
                }
            }
            events
        }
        SessionUpdate::Plan(p) => {
            // Build the structured plan + a synthetic TodoWrite tool call
            // from the same entries. claude-agent-acp routes Claude's
            // TodoWrite through the structured `SessionUpdate::Plan`
            // channel (not the tool channel), so without this synthesis
            // the cockpit's PlanStrip + sidebar light up but no tool
            // card ever renders; the user sees a plan appear "from
            // nowhere" and has no per-update record of which calls
            // produced which states. Emit a ToolCallStarted /
            // ToolCallCompleted pair shaped to match what the
            // TodoUpdateCard classifier in ToolCards.tsx expects
            // (`name = "TodoWrite"`, `args.todos = [...]`), one per
            // adapter update.
            // Append a session-local monotonic counter so two plan updates
            // arriving in the same millisecond don't share a synthetic ID
            // (which would collide in the cockpit_events row keys and
            // render as a single card instead of two).
            let ts_ms = chrono::Utc::now().timestamp_millis();
            let seq = SYNTHETIC_TOOL_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let plan_id = format!("plan-{ts_ms}-{seq}");
            let tool_id = format!("todo-{ts_ms}-{seq}");
            let todos_json: Vec<serde_json::Value> = p
                .entries
                .iter()
                .map(|e| {
                    serde_json::json!({
                        "content": e.content,
                        "status": plan_status_to_str(&e.status),
                    })
                })
                .collect();
            let args_preview = serde_json::json!({ "todos": todos_json }).to_string();
            let steps: Vec<PlanStep> = p
                .entries
                .into_iter()
                .enumerate()
                .map(|(i, e)| PlanStep {
                    id: format!("step-{i}"),
                    title: e.content,
                    detail: None,
                    status: map_plan_status(e.status),
                })
                .collect();
            let now = chrono::Utc::now();
            vec![
                Event::ToolCallStarted {
                    tool_call: ToolCall {
                        id: tool_id.clone(),
                        name: "TodoWrite".to_string(),
                        kind: "think".to_string(),
                        args_preview,
                        started_at: now,
                        parent_tool_call_id: None,
                    },
                },
                Event::PlanUpdated {
                    plan: Plan {
                        plan_id,
                        version: 1,
                        steps,
                    },
                },
                Event::ToolCallCompleted {
                    tool_call_id: tool_id,
                    is_error: false,
                    content: String::new(),
                    completed_at: now,
                },
            ]
        }
        SessionUpdate::CurrentModeUpdate(mode_update) => {
            let id = mode_update.current_mode_id.0.to_string();
            // Emit both events: CurrentModeChanged (the real id) and
            // a best-effort ModeChanged (for the legacy enum-based
            // UI, in case that path is still used somewhere).
            let mode = match id.as_str() {
                "default" => SessionMode::Default,
                "plan" => SessionMode::Plan,
                "accept_edits" | "acceptEdits" => SessionMode::AcceptEdits,
                "bypass_permissions" | "bypassPermissions" => SessionMode::BypassPermissions,
                _ => SessionMode::Default,
            };
            vec![
                Event::CurrentModeChanged {
                    current_mode_id: id,
                },
                Event::ModeChanged { mode },
            ]
        }
        SessionUpdate::UsageUpdate(u) => {
            let usage = SessionUsage {
                used: u.used,
                size: u.size,
                cost: u.cost.map(|c| UsageCost {
                    amount: c.amount,
                    currency: c.currency,
                }),
            };
            vec![Event::UsageUpdated { usage }]
        }
        SessionUpdate::AvailableCommandsUpdate(u) => {
            use agent_client_protocol::schema::AvailableCommandInput;
            let commands: Vec<AvailableCommand> = u
                .available_commands
                .into_iter()
                .map(|c| AvailableCommand {
                    name: c.name,
                    description: c.description,
                    accepts_input: matches!(c.input, Some(AvailableCommandInput::Unstructured(_))),
                })
                .collect();
            debug!(
                target: "cockpit.acp",
                count = commands.len(),
                "received AvailableCommandsUpdate from agent"
            );
            vec![Event::AvailableCommandsUpdated { commands }]
        }
        // Variants we don't have a typed mapping for yet pass through as
        // RawAgentUpdate so the UI can render best-effort and we can
        // narrow these as we go.
        other => vec![raw_event(&other)],
    }
}

fn map_plan_status(status: agent_client_protocol::schema::PlanEntryStatus) -> PlanStepStatus {
    use agent_client_protocol::schema::PlanEntryStatus;
    match status {
        PlanEntryStatus::Pending => PlanStepStatus::Pending,
        PlanEntryStatus::InProgress => PlanStepStatus::InProgress,
        PlanEntryStatus::Completed => PlanStepStatus::Done,
        // The schema is non-exhaustive; treat unknown variants as Pending.
        _ => PlanStepStatus::Pending,
    }
}

/// Lowercase string form of a PlanEntryStatus for the synthetic
/// TodoWrite args payload. Matches the values
/// `web/src/components/cockpit/ToolCards.tsx::normaliseTodoStatus`
/// accepts so the TodoUpdateCard renders the right glyph.
fn plan_status_to_str(status: &agent_client_protocol::schema::PlanEntryStatus) -> &'static str {
    use agent_client_protocol::schema::PlanEntryStatus;
    match status {
        PlanEntryStatus::Pending => "pending",
        PlanEntryStatus::InProgress => "in_progress",
        PlanEntryStatus::Completed => "completed",
        _ => "pending",
    }
}

fn raw_event<T: serde::Serialize>(value: &T) -> Event {
    Event::RawAgentUpdate {
        payload: serde_json::to_value(value).unwrap_or(serde_json::Value::Null),
    }
}

/// Stable lowercased string form of an ACP `ToolKind`. Used to drive the
/// per-tool renderer dispatch on the web side.
fn tool_kind_str(kind: &agent_client_protocol::schema::ToolKind) -> String {
    use agent_client_protocol::schema::ToolKind;
    match kind {
        ToolKind::Read => "read",
        ToolKind::Edit => "edit",
        ToolKind::Delete => "delete",
        ToolKind::Move => "move",
        ToolKind::Search => "search",
        ToolKind::Execute => "execute",
        ToolKind::Think => "think",
        ToolKind::Fetch => "fetch",
        ToolKind::SwitchMode => "switch_mode",
        _ => "other",
    }
    .into()
}

/// 16 KB cap on tool-call argument preview, with control chars stripped.
fn preview_args(raw: &serde_json::Value) -> String {
    let serialised = serde_json::to_string(raw).unwrap_or_default();
    let mut out = String::with_capacity(serialised.len().min(16 * 1024));
    for c in serialised.chars() {
        if out.len() >= 16 * 1024 {
            out.push_str("\u{2026}[truncated]");
            break;
        }
        if c.is_control() && c != '\n' && c != '\t' {
            continue;
        }
        out.push(c);
    }
    out
}

/// Concat the textual portion of a tool call's `content` array. Drops
/// non-text content blocks (images, resources, embedded terminals); the
/// per-tool renderer fall-back path only knows how to display text. Diffs
/// are surfaced separately via `extract_diff_from_locations` (and could
/// later be picked up here too via `ToolCallContent::Diff`).
fn extract_tool_content_text(blocks: &[agent_client_protocol::schema::ToolCallContent]) -> String {
    use agent_client_protocol::schema::ToolCallContent;
    let mut out = String::new();
    for block in blocks {
        if let ToolCallContent::Content(c) = block {
            if let ContentBlock::Text(t) = &c.content {
                if !out.is_empty() {
                    out.push('\n');
                }
                out.push_str(&t.text);
            }
        }
    }
    out
}

fn extract_diff_from_locations(
    _locations: &[agent_client_protocol::schema::ToolCallLocation],
) -> Option<DiffPreview> {
    // Pulling structured diffs out of a ToolCall update requires reading
    // the `content` array (ToolCallContent::Diff). Left as a follow-up;
    // the cockpit UI already reuses the existing diff viewer for this.
    None
}

#[allow(clippy::too_many_arguments)]
async fn run_connection_task<W, R>(
    transport: ByteStreams<W, R>,
    event_tx: mpsc::Sender<Event>,
    cmd_rx: mpsc::Receiver<ClientCmd>,
    cwd: PathBuf,
    session_label: String,
    child: Option<Arc<Mutex<tokio::process::Child>>>,
    pending_responders: PendingResponders,
    resources: SessionResources,
    socket_path: Option<PathBuf>,
    mode: ConnectMode,
    ready_tx: Option<oneshot::Sender<Result<(), AcpError>>>,
    profile: &'static agent_profiles::AgentProfile,
) where
    W: futures_util::AsyncWrite + Send + 'static,
    R: futures_util::AsyncRead + Send + 'static,
{
    use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

    let ready_tx = Arc::new(Mutex::new(ready_tx));
    let ready_for_block = ready_tx.clone();
    let event_tx_for_notif = event_tx.clone();
    let event_tx_for_perm = event_tx.clone();
    let event_tx_for_block = event_tx.clone();
    let pending_for_perm = pending_responders.clone();
    let mut cmd_rx = cmd_rx;
    let session_label_for_log = session_label.clone();
    let res_read = resources.clone();
    let res_write = resources.clone();
    let res_term_create = resources.clone();
    let res_term_output = resources.clone();
    let res_term_wait = resources.clone();
    let res_term_kill = resources.clone();
    let res_term_release = resources.clone();

    // After a successful `session/load`, claude-agent-acp re-emits the
    // full prior transcript as `session/update` notifications (each
    // historical assistant turn replayed as agent_message_chunk
    // events). Our SQLite event store already has those events from
    // the original run, so passing them through would double the
    // transcript on the next reload; every prior assistant bubble
    // appears once from disk replay, then again from the agent's
    // history dump. Suppress agent-side notifications during the
    // window between session/load success and the first user prompt;
    // cleared on the first ClientCmd::Prompt below.
    let suppress_history_replay = Arc::new(AtomicBool::new(false));
    let suppress_for_notif = suppress_history_replay.clone();
    let suppress_for_block = suppress_history_replay.clone();
    let session_label_for_notif = session_label.clone();

    // Watchdog inputs (only consulted when `mode` is `Resume { in_flight_turn: true }`):
    //   - `last_event_at`: epoch-ms of the last inbound notification.
    //     Updated by the notification handler below. Initialized to "now"
    //     so a session that never receives a single notification still
    //     fires Stopped after RESUME_IDLE_GRACE rather than immediately.
    //   - `prompt_sent_since_attach`: set when the user issues a prompt
    //     after attach; the user's real PromptRequest will own the next
    //     Stopped, so the watchdog must stand down.
    //   - `watchdog_fired`: ensures we synthesize Stopped at most once.
    let now_ms = chrono::Utc::now().timestamp_millis();
    let last_event_at = Arc::new(AtomicI64::new(now_ms));
    let prompt_sent_since_attach = Arc::new(AtomicBool::new(false));
    let watchdog_fired = Arc::new(AtomicBool::new(false));
    let last_event_at_for_notif = last_event_at.clone();

    let result = Client
        .builder()
        .name("aoe-cockpit")
        .on_receive_notification(
            move |notification: SessionNotification, _cx| {
                let event_tx = event_tx_for_notif.clone();
                let suppress = suppress_for_notif.clone();
                let session_label = session_label_for_notif.clone();
                let last_event_at = last_event_at_for_notif.clone();
                async move {
                    last_event_at
                        .store(chrono::Utc::now().timestamp_millis(), Ordering::Relaxed);
                    let suppressing = suppress.load(Ordering::Relaxed);
                    for event in map_update_to_events(notification.update, profile) {
                        // During the post-load replay window, drop only
                        // events that would reproduce the prior turns'
                        // visible transcript (assistant chunks, tool
                        // calls, plans, etc.). Ambient state events
                        // (mode/usage/available_commands) and lifecycle
                        // events (stopped, errors) must pass through;
                        // otherwise the composer footer and pickers
                        // stay stale until the user types something.
                        if suppressing && is_transcript_event(&event) {
                            debug!(
                                target: "cockpit.acp",
                                session = %session_label,
                                kind = transcript_event_kind(&event),
                                "dropping post-load history-replay event"
                            );
                            continue;
                        }
                        if event_tx.send(event).await.is_err() {
                            break;
                        }
                    }
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            move |request: RequestPermissionRequest,
                  responder: Responder<RequestPermissionResponse>,
                  _conn| {
                let event_tx = event_tx_for_perm.clone();
                let pending = pending_for_perm.clone();
                async move {
                    handle_permission_request(request, responder, event_tx, pending, profile)
                        .await
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            move |request: ReadTextFileRequest,
                  responder: Responder<ReadTextFileResponse>,
                  _conn| {
                let res = res_read.clone();
                async move { handle_read_text_file(request, responder, res).await }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            move |request: WriteTextFileRequest,
                  responder: Responder<WriteTextFileResponse>,
                  _conn| {
                let res = res_write.clone();
                async move { handle_write_text_file(request, responder, res).await }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            move |request: CreateTerminalRequest,
                  responder: Responder<CreateTerminalResponse>,
                  _conn| {
                let res = res_term_create.clone();
                async move { handle_create_terminal(request, responder, res).await }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            move |request: TerminalOutputRequest,
                  responder: Responder<TerminalOutputResponse>,
                  _conn| {
                let res = res_term_output.clone();
                async move { handle_terminal_output(request, responder, res).await }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            move |request: WaitForTerminalExitRequest,
                  responder: Responder<WaitForTerminalExitResponse>,
                  _conn| {
                let res = res_term_wait.clone();
                async move { handle_wait_for_terminal_exit(request, responder, res).await }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            move |request: KillTerminalRequest,
                  responder: Responder<KillTerminalResponse>,
                  _conn| {
                let res = res_term_kill.clone();
                async move { handle_kill_terminal(request, responder, res).await }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            move |request: ReleaseTerminalRequest,
                  responder: Responder<ReleaseTerminalResponse>,
                  _conn| {
                let res = res_term_release.clone();
                async move { handle_release_terminal(request, responder, res).await }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(transport, |connection: ConnectionTo<Agent>| async move {
            info!(target: "cockpit.acp", session = %session_label, "initializing ACP agent");
            let capabilities = ClientCapabilities::new()
                .fs(FileSystemCapabilities::new()
                    .read_text_file(true)
                    .write_text_file(true))
                .terminal(true);
            // `initialize` is sent in both Fresh and Resume modes.
            // It's idempotent on every ACP agent we ship against
            // (aoe-agent, claude-agent-acp); the response only carries
            // capability metadata; so re-sending it on attach is safe.
            let init = connection
                .send_request(
                    InitializeRequest::new(ProtocolVersion::V1)
                        .client_capabilities(capabilities),
                )
                .block_task()
                .await?;

            let load_session_capable = init.agent_capabilities.load_session;
            // Snapshot the watchdog-arming flag before `mode` is moved
            // into the match below.
            let arm_resume_watchdog = matches!(
                &mode,
                ConnectMode::Resume {
                    in_flight_turn: true,
                    ..
                }
            );
            info!(
                target: "cockpit.acp",
                session = %session_label,
                load_session_capable,
                ?mode,
                "initialize handshake complete"
            );

            let acp_session_id: SessionId = match mode {
                ConnectMode::Resume {
                    acp_session_id: stored,
                    in_flight_turn: _,
                } => {
                    // INVARIANT: Resume mode MUST NOT send `session/new`
                    // or `session/load`. This is the load-bearing trick
                    // that makes mid-turn continuity work across
                    // `aoe serve --stop` + `aoe serve`. Do not "fix" it
                    // by adding either call here.
                    //
                    // Why: the runner kept the agent process alive
                    // across the daemon restart, so the ACP session is
                    // still loaded in the agent's memory and addressable
                    // via its original id. `session/load` would either
                    // fail (agents that advertise loadSession=false) or
                    // double-load against a still-busy session and
                    // replay the entire transcript at the user.
                    // `session/new` would split context onto a new id
                    // the in-flight `session/prompt` doesn't address,
                    // silently orphaning the turn the user is waiting
                    // on. See issue #1037 and the
                    // `tests/cockpit_midturn_resume.rs` integration
                    // coverage.
                    info!(
                        target: "cockpit.acp",
                        session = %session_label,
                        stored_id = %stored,
                        "resume mode: reusing existing acp session id without handshake"
                    );
                    // Emit AcpSessionAssigned so the frontend reducer
                    // clears any sticky startupError/lastError from the
                    // crash. The server-side listener treats a same-id
                    // Assigned as a no-op, so this doesn't rewrite
                    // sessions.json.
                    let _ = event_tx_for_block
                        .send(Event::AcpSessionAssigned {
                            acp_session_id: stored.clone(),
                        })
                        .await;
                    SessionId::from(stored)
                }
                ConnectMode::Fresh {
                    stored_acp_session_id,
                } => {
                    // Decide whether to resume the prior agent session or create
                    // a fresh one. session/load is only attempted when the agent
                    // advertises support AND we have a stored id to feed it. On
                    // load failure (id GC'd, agent state lost, etc.) we fall
                    // through to session/new and emit SessionContextReset so the
                    // UI can show a notice and clear stale token-usage hints.
                    let mut acp_session_id: Option<SessionId> = None;
                    if load_session_capable {
                        if let Some(stored) = stored_acp_session_id.clone() {
                            info!(
                                target: "cockpit.acp",
                                session = %session_label,
                                stored_id = %stored,
                                "resuming session via session/load"
                            );
                            // Set the flag BEFORE sending the request: claude-agent-acp
                            // re-emits the prior transcript via session/update
                            // notifications *during* the load handshake, before the
                            // LoadSessionRequest response returns. Setting after .await
                            // would let those notifications leak through to the event
                            // store and produce duplicate ToolCallStarted rows on the
                            // next reload (assistant-ui then panics with "Duplicate
                            // key toolCallId-..."). Cleared on Err below if we fall
                            // back to session/new, which has no replay payload.
                            suppress_for_block.store(true, Ordering::Relaxed);
                            let req = LoadSessionRequest::new(stored.clone(), cwd.clone());
                            match connection.send_request(req).block_task().await {
                                Ok(_resp) => {
                                    info!(
                                        target: "cockpit.acp",
                                        session = %session_label,
                                        stored_id = %stored,
                                        "session/load succeeded; suppressing post-load history replay"
                                    );
                                    // Emit AcpSessionAssigned even on resume so the
                                    // frontend reducer can clear any sticky
                                    // `startupError` / `lastError` from a prior crash
                                    // (e.g. a respawn after the user's prompt hit a
                                    // dead pipe). The server-side listener treats a
                                    // same-id Assigned as a no-op, so this doesn't
                                    // rewrite sessions.json.
                                    let _ = event_tx_for_block
                                        .send(Event::AcpSessionAssigned {
                                            acp_session_id: stored.clone(),
                                        })
                                        .await;
                                    acp_session_id = Some(SessionId::from(stored));
                                }
                                Err(e) => {
                                    warn!(
                                        target: "cockpit.acp",
                                        session = %session_label,
                                        stored_id = %stored,
                                        "session/load failed, falling back to session/new: {e}"
                                    );
                                    suppress_for_block.store(false, Ordering::Relaxed);
                                    let _ = event_tx_for_block
                                        .send(Event::SessionContextReset {
                                            reason: format!("session/load failed: {e}"),
                                        })
                                        .await;
                                }
                            }
                        }
                    }

                    if let Some(id) = acp_session_id {
                        id
                    } else {
                        info!(
                            target: "cockpit.acp",
                            session = %session_label,
                            "creating fresh session via session/new"
                        );
                        let new_session = connection
                            .send_request(NewSessionRequest::new(cwd))
                            .block_task()
                            .await?;
                        let id = new_session.session_id.clone();
                        info!(
                            target: "cockpit.acp",
                            session = %session_label,
                            new_id = %id.0,
                            "session/new succeeded, captured acp_session_id"
                        );

                        // Surface the agent-advertised modes (if any) so the UI
                        // can render the actual modes the agent supports rather
                        // than the hard-coded four. Claude's adapter typically
                        // ships a mode set with ids like "default" / "plan" /
                        // "accept_edits" / "bypass_permissions".
                        if let Some(modes) = &new_session.modes {
                            let infos: Vec<ModeInfo> = modes
                                .available_modes
                                .iter()
                                .map(|m| ModeInfo {
                                    id: m.id.0.to_string(),
                                    name: m.name.clone(),
                                    description: m.description.clone(),
                                })
                                .collect();
                            let _ = event_tx_for_block
                                .send(Event::ModesAvailable {
                                    current_mode_id: modes.current_mode_id.0.to_string(),
                                    modes: infos,
                                })
                                .await;
                        }

                        // Tell the server-side listener so it can persist the
                        // new id on Instance.cockpit_acp_session_id.
                        let _ = event_tx_for_block
                            .send(Event::AcpSessionAssigned {
                                acp_session_id: id.0.to_string(),
                            })
                            .await;

                        id
                    }
                }
            };

            if let Some(tx) = ready_for_block.lock().await.take() {
                let _ = tx.send(Ok(()));
            }

            // Arm the resume-idle watchdog. The agent's response to the
            // orphaned in-flight `session/prompt` (from the previous
            // daemon) carries a request id this client never issued and
            // is dropped silently by the transport. Without this
            // synthesized Stopped, the UI's "thinking" indicator never
            // clears until the user manually sends a new prompt.
            if arm_resume_watchdog {
                let event_tx_for_watchdog = event_tx_for_block.clone();
                let last_event_at = last_event_at.clone();
                let prompt_sent_since_attach = prompt_sent_since_attach.clone();
                let watchdog_fired = watchdog_fired.clone();
                let session_label_for_watchdog = session_label.clone();
                let grace = resume_idle_grace();
                tokio::spawn(async move {
                    let grace_ms = grace.as_millis() as i64;
                    loop {
                        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                        if watchdog_fired.load(Ordering::Relaxed) {
                            return;
                        }
                        if prompt_sent_since_attach.load(Ordering::Relaxed) {
                            // User sent a new prompt; its real
                            // PromptRequest will own the next Stopped.
                            return;
                        }
                        let last = last_event_at.load(Ordering::Relaxed);
                        let now = chrono::Utc::now().timestamp_millis();
                        if now - last >= grace_ms {
                            info!(
                                target: "cockpit.acp",
                                session = %session_label_for_watchdog,
                                idle_ms = now - last,
                                "resume-idle watchdog: synthesizing Stopped for orphaned in-flight turn"
                            );
                            watchdog_fired.store(true, Ordering::Relaxed);
                            let _ = event_tx_for_watchdog
                                .send(Event::Stopped {
                                    reason: "reattach_idle".into(),
                                })
                                .await;
                            return;
                        }
                    }
                });
            }

            loop {
                let cmd = cmd_rx.recv().await;
                match cmd {
                    Some(ClientCmd::Prompt(text)) => {
                        // First user prompt after session/load: stop
                        // dropping notifications. The agent's history-
                        // replay window is over; everything from now on
                        // is live conversation.
                        if suppress_for_block.swap(false, Ordering::Relaxed) {
                            info!(
                                target: "cockpit.acp",
                                session = %session_label,
                                "first user prompt after session/load; resuming notification pump"
                            );
                        }
                        // Stand the resume-idle watchdog down: the new
                        // prompt's real Stopped will own the next status
                        // transition, so we no longer need to synthesize
                        // one for the orphaned prior turn.
                        prompt_sent_since_attach.store(true, Ordering::Relaxed);
                        info!(target: "cockpit.acp", "sending prompt ({} chars)", text.len());
                        // Drive the prompt request concurrently with the
                        // command channel so out-of-band notifications
                        // (Cancel, SetMode) can be delivered to the agent
                        // mid-turn. Per the ACP spec, session/cancel is a
                        // notification specifically designed to be sent
                        // while a session/prompt request is in flight; if
                        // we serialise the loop on the prompt's await, the
                        // cancel sits idle in the channel and only goes
                        // out after the turn already finished.
                        let prompt_fut = connection
                            .send_request(PromptRequest::new(
                                acp_session_id.clone(),
                                vec![ContentBlock::Text(TextContent::new(text))],
                            ))
                            .block_task();
                        tokio::pin!(prompt_fut);

                        let mut shutdown = false;
                        // Cancel-escalation watchdog. The first
                        // `session/cancel` sent while the prompt future is
                        // still pending arms a 10s timer; if the agent
                        // doesn't resolve the prompt before it fires (or
                        // the user submits a follow-up prompt while we're
                        // already cancelling, which means they've already
                        // clicked "Force end turn" and re-typed), we
                        // declare the agent unresponsive, end the
                        // connection task, and let the supervisor drain
                        // path SIGTERM the runner and respawn with
                        // session/load for transcript continuity. Without
                        // this, claude-agent-acp ignoring cancel in the
                        // middle of a `block: true` TaskOutput leaves the
                        // daemon's `prompt_fut` pinned forever and every
                        // follow-up prompt is silently dropped. See #1196.
                        let mut agent_unresponsive = false;
                        let mut cancelling = false;
                        let cancel_grace = tokio::time::sleep(CANCEL_ESCALATION_GRACE);
                        tokio::pin!(cancel_grace);

                        loop {
                            tokio::select! {
                                res = &mut prompt_fut => {
                                    let _ = res?;
                                    break;
                                }
                                _ = &mut cancel_grace, if cancelling => {
                                    warn!(
                                        target: "cockpit.acp",
                                        session = %session_label,
                                        grace_secs = CANCEL_ESCALATION_GRACE.as_secs(),
                                        "agent ignored session/cancel past grace window; escalating to runner restart"
                                    );
                                    agent_unresponsive = true;
                                    shutdown = true;
                                    break;
                                }
                                cmd = cmd_rx.recv() => {
                                    match cmd {
                                        Some(ClientCmd::Cancel) => {
                                            info!(
                                                target: "cockpit.acp",
                                                "sending session/cancel during in-flight prompt"
                                            );
                                            connection.send_notification(
                                                CancelNotification::new(acp_session_id.clone()),
                                            )?;
                                            // Arm the escalation watchdog on
                                            // the first cancel only; later
                                            // cancels just resend the
                                            // notification.
                                            if !cancelling {
                                                cancelling = true;
                                                cancel_grace.as_mut().reset(
                                                    tokio::time::Instant::now()
                                                        + CANCEL_ESCALATION_GRACE,
                                                );
                                            }
                                        }
                                        Some(ClientCmd::SetMode(mode_id)) => {
                                            info!(
                                                target: "cockpit.acp",
                                                "sending session/set_mode mode={mode_id} during in-flight prompt"
                                            );
                                            // Fire the request and hand the
                                            // response handling to a detached
                                            // task. Awaiting it here would
                                            // freeze this select loop for the
                                            // duration of the round-trip,
                                            // defeating the point of polling
                                            // cmd_rx concurrently; a Cancel
                                            // arriving while set_mode is in
                                            // flight would queue. The detached
                                            // task mirrors the success into the
                                            // event stream so the UI flips even
                                            // when the adapter (e.g.
                                            // claude-agent-acp) treats the
                                            // response as authoritative and
                                            // skips the follow-up
                                            // current_mode_update notification.
                                            let sent = connection.send_request(
                                                SetSessionModeRequest::new(
                                                    acp_session_id.clone(),
                                                    mode_id.clone(),
                                                ),
                                            );
                                            let tx = event_tx_for_block.clone();
                                            tokio::spawn(async move {
                                                match sent.block_task().await {
                                                    Ok(_) => {
                                                        let _ = tx
                                                            .send(Event::CurrentModeChanged {
                                                                current_mode_id: mode_id,
                                                            })
                                                            .await;
                                                    }
                                                    Err(e) => {
                                                        let reason = format!("{e}");
                                                        warn!(
                                                            target: "cockpit.acp",
                                                            "session/set_mode failed mid-turn: {reason}"
                                                        );
                                                        let _ = tx
                                                            .send(Event::ModeSwitchFailed {
                                                                mode_id,
                                                                reason,
                                                            })
                                                            .await;
                                                    }
                                                }
                                            });
                                        }
                                        Some(ClientCmd::Prompt(rejected_text)) => {
                                            // Surface the dropped prompt
                                            // to the UI so the user can
                                            // retry from a Rejected pill
                                            // instead of having their
                                            // message vanish silently.
                                            // Client-side composer queueing
                                            // is tracked separately in
                                            // #1031; this event covers the
                                            // server-side gap when a prompt
                                            // does make it to the daemon
                                            // while another is in flight.
                                            warn!(
                                                target: "cockpit.acp",
                                                "received Prompt while one is in flight; rejecting"
                                            );
                                            let _ = event_tx_for_block
                                                .send(Event::PromptRejected {
                                                    reason: "agent_busy".into(),
                                                    text: rejected_text,
                                                })
                                                .await;
                                            // A follow-up arriving while
                                            // a cancel is in flight means
                                            // the user has clicked Force
                                            // end turn (which optimistically
                                            // unlocked the composer via the
                                            // supervisor's synthetic Stopped)
                                            // and then re-typed. That's a
                                            // strong signal the agent is
                                            // wedged; escalate immediately
                                            // without waiting for the 10s
                                            // grace.
                                            if cancelling {
                                                warn!(
                                                    target: "cockpit.acp",
                                                    session = %session_label,
                                                    "follow-up prompt arrived while cancel pending; escalating to runner restart"
                                                );
                                                agent_unresponsive = true;
                                                shutdown = true;
                                                break;
                                            }
                                        }
                                        Some(ClientCmd::Shutdown) | None => {
                                            info!(
                                                target: "cockpit.acp",
                                                "shutdown received during in-flight prompt; aborting turn"
                                            );
                                            shutdown = true;
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                        // Always emit a terminal Stopped for this turn before
                        // leaving the Prompt arm, including the shutdown path.
                        // Consumers (reducer, persisted status) need a single
                        // turn-end event per turn or they sit on a stale
                        // "in flight" state forever.
                        let reason = if agent_unresponsive {
                            "agent_unresponsive"
                        } else if shutdown {
                            "shutdown"
                        } else {
                            "prompt_complete"
                        };
                        let _ = event_tx_for_block
                            .send(Event::Stopped {
                                reason: reason.into(),
                            })
                            .await;
                        if shutdown {
                            break;
                        }
                    }
                    Some(ClientCmd::Cancel) => {
                        info!(target: "cockpit.acp", "sending session/cancel (no prompt in flight)");
                        connection
                            .send_notification(CancelNotification::new(acp_session_id.clone()))?;
                    }
                    Some(ClientCmd::SetMode(mode_id)) => {
                        info!(target: "cockpit.acp", "sending session/set_mode mode={mode_id}");
                        // Detached, same shape as the mid-turn path: don't
                        // freeze the cmd_rx loop on the round-trip.
                        let sent = connection.send_request(SetSessionModeRequest::new(
                            acp_session_id.clone(),
                            mode_id.clone(),
                        ));
                        let tx = event_tx_for_block.clone();
                        tokio::spawn(async move {
                            match sent.block_task().await {
                                Ok(_) => {
                                    let _ = tx
                                        .send(Event::CurrentModeChanged {
                                            current_mode_id: mode_id,
                                        })
                                        .await;
                                }
                                Err(e) => {
                                    let reason = format!("{e}");
                                    warn!(target: "cockpit.acp", "session/set_mode failed: {reason}");
                                    let _ = tx
                                        .send(Event::ModeSwitchFailed { mode_id, reason })
                                        .await;
                                }
                            }
                        });
                    }
                    Some(ClientCmd::Shutdown) | None => {
                        info!(target: "cockpit.acp", "shutdown received, exiting connection loop");
                        break;
                    }
                }
            }
            Ok(())
        })
        .await;

    match &result {
        Err(e) => {
            error!(
                target: "cockpit.acp",
                session = %session_label_for_log,
                "ACP connection task ended with error: {:?}", e
            );
            let message = format!("ACP connection failed: {e}");
            // If the handshake never completed, hand the failure back so
            // `spawn()` can surface a typed error to the caller; otherwise
            // publish a synthetic event so the UI can show a remediation
            // hint instead of a silent dead session.
            if let Some(tx) = ready_tx.lock().await.take() {
                let _ = tx.send(Err(AcpError::Spawn(message.clone())));
            } else {
                let _ = event_tx.send(Event::AgentStartupError { message }).await;
            }
        }
        Ok(()) => {
            info!(
                target: "cockpit.acp",
                session = %session_label_for_log,
                "ACP connection task ended cleanly"
            );
        }
    }
    // In runner-managed mode (child is None) we deliberately don't kill
    // anything here: the per-worker `aoe __cockpit-runner` shim owns the
    // agent subprocess and outlives this daemon's connection. The socket
    // file also stays; the runner cleans it up on its own exit.
    if let Some(child) = child.as_ref() {
        let mut guard = child.lock().await;
        match guard.try_wait() {
            Ok(Some(status)) => info!(
                target: "cockpit.acp",
                session = %session_label_for_log,
                "agent process already exited: status={status}"
            ),
            Ok(None) => info!(
                target: "cockpit.acp",
                session = %session_label_for_log,
                "killing agent process after connection task end"
            ),
            Err(e) => warn!(
                target: "cockpit.acp",
                session = %session_label_for_log,
                "try_wait failed before kill: {e}"
            ),
        }
        let _ = guard.kill().await;
        if let Some(path) = socket_path {
            let _ = tokio::fs::remove_file(path).await;
        }
    }
}

/// Wait for the connection task to finish the ACP handshake (or fail).
/// Bounds the wait so a wedged agent (the classic `npx -y` first-run
/// download stall) returns a clear typed error instead of leaving the
/// supervisor parked indefinitely. Also watches for early child exit
/// and surfaces stderr in the message so callers see why it died.
///
/// `install_binary` is the binary name from `AgentSpec.command` so the
/// timeout message points users at the right install command for the
/// specific agent (codex-acp / opencode / gemini, not always
/// claude-agent-acp).
async fn wait_for_handshake(
    session_label: &str,
    ready_rx: oneshot::Receiver<Result<(), AcpError>>,
    child: Option<&Arc<Mutex<tokio::process::Child>>>,
    install_binary: &str,
) -> Result<(), AcpError> {
    let timeout = std::time::Duration::from_secs(30);
    match tokio::time::timeout(timeout, ready_rx).await {
        Ok(Ok(Ok(()))) => Ok(()),
        Ok(Ok(Err(e))) => {
            warn!(target: "cockpit.acp", session = %session_label, "ACP handshake failed: {e}");
            collect_child_failure(child).await;
            Err(e)
        }
        Ok(Err(_canceled)) => Err(AcpError::Spawn(
            "ACP connection task ended before completing the initialize handshake".into(),
        )),
        Err(_elapsed) => {
            warn!(
                target: "cockpit.acp",
                session = %session_label,
                "ACP handshake timed out after {}s",
                timeout.as_secs()
            );
            if let Some(child) = child {
                let mut guard = child.lock().await;
                let _ = guard.kill().await;
            }
            let install_hint = super::install_hints::install_hint_for(install_binary)
                .unwrap_or("install the adapter for the configured agent and re-run");
            Err(AcpError::Spawn(format!(
                "agent did not complete the ACP initialize handshake within {}s. \
                 Common causes: the adapter is still downloading on first run, \
                 or the configured agent command isn't a real ACP server. \
                 Try `{}` and re-run.",
                timeout.as_secs(),
                install_hint
            )))
        }
    }
}

async fn collect_child_failure(child: Option<&Arc<Mutex<tokio::process::Child>>>) {
    if let Some(child) = child {
        let mut guard = child.lock().await;
        if let Ok(Some(status)) = guard.try_wait() {
            warn!(target: "cockpit.acp", "agent process exited early: status={status}");
        }
    }
}

/// Issue #1147: monotonic ns-since-process-start, used as a thin
/// correlation token in the cockpit ACP tool-dispatch trace. Wall-clock
/// fields like `chrono::Utc::now()` jitter under NTP slew and are too
/// coarse to detect interleaved entry/exit between concurrent handlers;
/// `Instant` is monotonic and ns-resolved on every supported platform.
/// Cast to `u64` because `Instant::elapsed()` returns `Duration` whose
/// `as_nanos()` is `u128`, which `tracing` formats less compactly. A
/// `u64` of ns gives ~584 years of headroom, which is plenty.
fn enter_timestamp_ns() -> u64 {
    use std::sync::OnceLock;
    static EPOCH: OnceLock<std::time::Instant> = OnceLock::new();
    let epoch = EPOCH.get_or_init(std::time::Instant::now);
    epoch.elapsed().as_nanos() as u64
}

async fn handle_read_text_file(
    request: ReadTextFileRequest,
    responder: Responder<ReadTextFileResponse>,
    res: SessionResources,
) -> agent_client_protocol::Result<()> {
    // Issue #1147: parallel-tool-call diagnostics. The `enter_ns` value is a
    // monotonic ns-since-process-start counter; if the model dispatches N
    // tool calls in parallel, the entries should interleave (close `enter_ns`
    // values across handlers) rather than strictly increasing per-handler.
    let enter_ns = enter_timestamp_ns();
    trace!(
        target: "cockpit.acp.tool_dispatch",
        handler = "read_text_file",
        path = %request.path.display(),
        enter_ns,
        "ACP request handler entered"
    );
    let result = match fs_handler::handle_read(&res.fs_policy, &res.label, &request.path) {
        Ok(content) => {
            // Honor optional line/limit slicing for ACP semantics: 1-based.
            let sliced = if request.line.is_some() || request.limit.is_some() {
                let lines: Vec<&str> = content.lines().collect();
                let start = request
                    .line
                    .map(|l| l.saturating_sub(1) as usize)
                    .unwrap_or(0);
                let limit = request.limit.map(|n| n as usize).unwrap_or(usize::MAX);
                let end = start.saturating_add(limit).min(lines.len());
                if start >= lines.len() {
                    String::new()
                } else {
                    lines[start..end].join("\n")
                }
            } else {
                content
            };
            responder.respond(ReadTextFileResponse::new(sliced))
        }
        Err(e) => {
            responder.respond_with_error(agent_client_protocol::util::internal_error(e.to_string()))
        }
    };
    trace!(
        target: "cockpit.acp.tool_dispatch",
        handler = "read_text_file",
        enter_ns,
        elapsed_ns = enter_timestamp_ns() - enter_ns,
        "ACP request handler exited"
    );
    result
}

async fn handle_write_text_file(
    request: WriteTextFileRequest,
    responder: Responder<WriteTextFileResponse>,
    res: SessionResources,
) -> agent_client_protocol::Result<()> {
    let enter_ns = enter_timestamp_ns();
    trace!(
        target: "cockpit.acp.tool_dispatch",
        handler = "write_text_file",
        path = %request.path.display(),
        enter_ns,
        "ACP request handler entered"
    );
    let result =
        match fs_handler::handle_write(&res.fs_policy, &res.label, &request.path, &request.content)
        {
            Ok(()) => responder.respond(WriteTextFileResponse::new()),
            Err(e) => responder
                .respond_with_error(agent_client_protocol::util::internal_error(e.to_string())),
        };
    trace!(
        target: "cockpit.acp.tool_dispatch",
        handler = "write_text_file",
        enter_ns,
        elapsed_ns = enter_timestamp_ns() - enter_ns,
        "ACP request handler exited"
    );
    result
}

async fn handle_create_terminal(
    request: CreateTerminalRequest,
    responder: Responder<CreateTerminalResponse>,
    res: SessionResources,
) -> agent_client_protocol::Result<()> {
    let enter_ns = enter_timestamp_ns();
    trace!(
        target: "cockpit.acp.tool_dispatch",
        handler = "create_terminal",
        command = %request.command,
        argc = request.args.len(),
        enter_ns,
        "ACP request handler entered"
    );
    let cwd = request.cwd.clone().unwrap_or_else(|| res.cwd.clone());
    // Sandbox the cwd: must be inside session roots.
    if let Err(e) = res.fs_policy.resolve_inside(&cwd) {
        let r = responder.respond_with_error(agent_client_protocol::util::internal_error(format!(
            "terminal cwd outside session roots: {e}"
        )));
        trace!(
            target: "cockpit.acp.tool_dispatch",
            handler = "create_terminal",
            enter_ns,
            elapsed_ns = enter_timestamp_ns() - enter_ns,
            outcome = "cwd_outside_roots",
            "ACP request handler exited"
        );
        return r;
    }
    let terminal_sandbox = res
        .sandbox
        .as_ref()
        .map(|s| super::terminal_handler::TerminalSandbox {
            container_name: s.container_name.clone(),
            env_entries: s.current_env_entries(),
        });
    let result = match res
        .terminals
        .create_and_run(
            &res.label,
            &request.command,
            request.args.clone(),
            cwd,
            terminal_sandbox.as_ref(),
        )
        .await
    {
        Ok(id) => responder.respond(CreateTerminalResponse::new(TerminalId::new(id))),
        Err(e) => {
            responder.respond_with_error(agent_client_protocol::util::internal_error(e.to_string()))
        }
    };
    trace!(
        target: "cockpit.acp.tool_dispatch",
        handler = "create_terminal",
        enter_ns,
        elapsed_ns = enter_timestamp_ns() - enter_ns,
        "ACP request handler exited"
    );
    result
}

fn build_exit_status(exit_code: Option<i32>) -> agent_client_protocol::schema::TerminalExitStatus {
    use agent_client_protocol::schema::TerminalExitStatus;
    let cast = exit_code.and_then(|c| u32::try_from(c).ok());
    TerminalExitStatus::new().exit_code(cast)
}

async fn handle_terminal_output(
    request: TerminalOutputRequest,
    responder: Responder<TerminalOutputResponse>,
    res: SessionResources,
) -> agent_client_protocol::Result<()> {
    let enter_ns = enter_timestamp_ns();
    trace!(
        target: "cockpit.acp.tool_dispatch",
        handler = "terminal_output",
        terminal_id = %request.terminal_id.0,
        enter_ns,
        "ACP request handler entered"
    );
    let result = match res.terminals.output(request.terminal_id.0.as_ref()).await {
        Ok(out) => {
            let combined = format!("{}{}", out.stdout, out.stderr);
            responder.respond(
                TerminalOutputResponse::new(combined, false)
                    .exit_status(build_exit_status(out.exit_code)),
            )
        }
        Err(e) => {
            responder.respond_with_error(agent_client_protocol::util::internal_error(e.to_string()))
        }
    };
    trace!(
        target: "cockpit.acp.tool_dispatch",
        handler = "terminal_output",
        enter_ns,
        elapsed_ns = enter_timestamp_ns() - enter_ns,
        "ACP request handler exited"
    );
    result
}

async fn handle_wait_for_terminal_exit(
    request: WaitForTerminalExitRequest,
    responder: Responder<WaitForTerminalExitResponse>,
    res: SessionResources,
) -> agent_client_protocol::Result<()> {
    let enter_ns = enter_timestamp_ns();
    trace!(
        target: "cockpit.acp.tool_dispatch",
        handler = "wait_for_terminal_exit",
        terminal_id = %request.terminal_id.0,
        enter_ns,
        "ACP request handler entered"
    );
    // For our one-shot terminal model, the command has already finished by
    // the time `create_and_run` returns. So `output()` immediately yields
    // the captured exit status.
    let result = match res.terminals.output(request.terminal_id.0.as_ref()).await {
        Ok(out) => responder.respond(WaitForTerminalExitResponse::new(build_exit_status(
            out.exit_code,
        ))),
        Err(e) => {
            responder.respond_with_error(agent_client_protocol::util::internal_error(e.to_string()))
        }
    };
    trace!(
        target: "cockpit.acp.tool_dispatch",
        handler = "wait_for_terminal_exit",
        enter_ns,
        elapsed_ns = enter_timestamp_ns() - enter_ns,
        "ACP request handler exited"
    );
    result
}

async fn handle_kill_terminal(
    request: KillTerminalRequest,
    responder: Responder<KillTerminalResponse>,
    _res: SessionResources,
) -> agent_client_protocol::Result<()> {
    let enter_ns = enter_timestamp_ns();
    trace!(
        target: "cockpit.acp.tool_dispatch",
        handler = "kill_terminal",
        terminal_id = %request.terminal_id.0,
        enter_ns,
        "ACP request handler entered"
    );
    // One-shot terminals are already finished; kill is a no-op.
    let result = responder.respond(KillTerminalResponse::new());
    trace!(
        target: "cockpit.acp.tool_dispatch",
        handler = "kill_terminal",
        enter_ns,
        elapsed_ns = enter_timestamp_ns() - enter_ns,
        "ACP request handler exited"
    );
    result
}

async fn handle_release_terminal(
    request: ReleaseTerminalRequest,
    responder: Responder<ReleaseTerminalResponse>,
    res: SessionResources,
) -> agent_client_protocol::Result<()> {
    let enter_ns = enter_timestamp_ns();
    trace!(
        target: "cockpit.acp.tool_dispatch",
        handler = "release_terminal",
        terminal_id = %request.terminal_id.0,
        enter_ns,
        "ACP request handler entered"
    );
    let result = match res.terminals.release(request.terminal_id.0.as_ref()).await {
        Ok(()) => responder.respond(ReleaseTerminalResponse::new()),
        Err(e) => {
            responder.respond_with_error(agent_client_protocol::util::internal_error(e.to_string()))
        }
    };
    trace!(
        target: "cockpit.acp.tool_dispatch",
        handler = "release_terminal",
        enter_ns,
        elapsed_ns = enter_timestamp_ns() - enter_ns,
        "ACP request handler exited"
    );
    result
}

async fn handle_permission_request(
    request: RequestPermissionRequest,
    responder: Responder<RequestPermissionResponse>,
    event_tx: mpsc::Sender<Event>,
    pending: PendingResponders,
    profile: &'static agent_profiles::AgentProfile,
) -> agent_client_protocol::Result<()> {
    let enter_ns = enter_timestamp_ns();
    let tool_call_id = request.tool_call.tool_call_id.0.to_string();
    trace!(
        target: "cockpit.acp.tool_dispatch",
        handler = "permission_request",
        tool_call_id = %tool_call_id,
        enter_ns,
        "ACP request handler entered"
    );
    // Build our cockpit-side approval card.
    let title = request
        .tool_call
        .fields
        .title
        .clone()
        .unwrap_or_else(|| "tool call".into());
    let raw_args = request
        .tool_call
        .fields
        .raw_input
        .clone()
        .unwrap_or(serde_json::Value::Null);
    let args_preview = preview_args(&raw_args);
    let tool_call = ToolCall {
        id: request.tool_call.tool_call_id.0.to_string(),
        name: title,
        kind: request
            .tool_call
            .fields
            .kind
            .as_ref()
            .map(tool_kind_str)
            .unwrap_or_else(|| "other".into()),
        args_preview,
        started_at: chrono::Utc::now(),
        parent_tool_call_id: profile.parent_tool_use_id_from_meta(&request.tool_call.meta),
    };
    let approval = build_approval(tool_call);
    let nonce = approval.nonce.clone();

    let (resolve_tx, resolve_rx) = oneshot::channel::<ApprovalResolutionMessage>();
    pending.lock().await.insert(
        nonce.clone(),
        PendingResponder {
            resolver: resolve_tx,
        },
    );

    if event_tx
        .send(Event::ApprovalRequested { approval })
        .await
        .is_err()
    {
        // Receiver gone: cancel.
        pending.lock().await.remove(&nonce);
        trace!(
            target: "cockpit.acp.tool_dispatch",
            handler = "permission_request",
            tool_call_id = %tool_call_id,
            enter_ns,
            elapsed_ns = enter_timestamp_ns() - enter_ns,
            outcome = "receiver_gone",
            "ACP request handler exited"
        );
        return responder.respond(RequestPermissionResponse::new(
            RequestPermissionOutcome::Cancelled,
        ));
    }

    // Issue #1147: this `await` is the suspected serializer for the user-felt
    // slowness. Log the moment we begin awaiting so a wall-clock comparison
    // with later "responder.respond" emissions exposes how long each pending
    // approval blocked the agent's turn.
    let await_enter_ns = enter_timestamp_ns();
    trace!(
        target: "cockpit.acp.tool_dispatch",
        handler = "permission_request",
        tool_call_id = %tool_call_id,
        enter_ns,
        await_offset_ns = await_enter_ns - enter_ns,
        "awaiting approval resolution"
    );
    // Build outcome + its label together so the exit event never re-matches on
    // a foreign `#[non_exhaustive]` enum it doesn't fully own.
    let (outcome, outcome_label): (RequestPermissionOutcome, &'static str) = match resolve_rx.await
    {
        Ok(ApprovalResolutionMessage::Decision { decision }) => {
            if let Some(option_id) = pick_option_id(&request.options, decision) {
                // Surface the resolution to UI clients via the typed event channel.
                let _ = event_tx
                    .send(Event::ApprovalResolved {
                        nonce: nonce.clone(),
                        decision,
                    })
                    .await;
                (
                    RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id)),
                    "selected",
                )
            } else {
                warn!(
                    target: "cockpit.acp",
                    "agent did not offer a {decision:?}-compatible option; cancelling"
                );
                (RequestPermissionOutcome::Cancelled, "cancelled")
            }
        }
        Ok(ApprovalResolutionMessage::Cancelled) | Err(_) => {
            (RequestPermissionOutcome::Cancelled, "cancelled")
        }
    };
    let exit_ns = enter_timestamp_ns();
    trace!(
        target: "cockpit.acp.tool_dispatch",
        handler = "permission_request",
        tool_call_id = %tool_call_id,
        enter_ns,
        elapsed_ns = exit_ns - enter_ns,
        await_ns = exit_ns - await_enter_ns,
        outcome = outcome_label,
        "responding to permission request"
    );
    responder.respond(RequestPermissionResponse::new(outcome))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fake_client_round_trips_events() {
        let (mut client, tx) = AcpClient::fake_for_test(CockpitSessionId("s-1".into()));
        tx.send(Event::ThinkingStarted).await.unwrap();
        let event = client.next_event().await.expect("event delivered");
        assert!(matches!(event, Event::ThinkingStarted));
    }

    /// Sandboxed cockpit spawn must wrap the agent command in
    /// `docker exec` argv with `-i`, the container workdir, an `-e`
    /// flag per env entry, then the container name, then the agent
    /// argv. The docker binary must be argv[0]. Mirrors the tmux
    /// substrate's wrap so the same `claude-agent-acp` invocation
    /// goes inside the container instead of running on the host.
    #[test]
    fn build_sandbox_docker_argv_wraps_agent_in_docker_exec() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().to_path_buf();
        let sandbox = SandboxInfo {
            enabled: true,
            container_id: None,
            image: "alpine:latest".into(),
            container_name: "aoe-sandbox-abc12345".into(),
            extra_env: Some(vec!["MY_LITERAL=hello".into()]),
            custom_instruction: None,
        };
        let config = SpawnConfig {
            agent_key: "claude".into(),
            spec: AgentSpec {
                command: "claude-agent-acp".into(),
                args: vec!["--stdio".into()],
                description: "test".into(),
                env_allowlist: None,
            },
            cwd,
            additional_dirs: vec![],
            provider_env: vec![],
            socket_path: None,
            stored_acp_session_id: None,
            sandbox_info: Some(sandbox.clone()),
            source_profile: None,
        };
        let argv = build_sandbox_docker_argv(&config, &sandbox, "/workspace/proj")
            .expect("docker argv built");
        assert!(
            argv.docker_binary == "docker" || argv.docker_binary == "podman",
            "expected docker/podman binary, got {:?}",
            argv.docker_binary
        );
        assert_eq!(argv.docker_args[0], "exec");
        assert_eq!(argv.docker_args[1], "-i");
        assert_eq!(argv.docker_args[2], "-w");
        let cn_idx = argv
            .docker_args
            .iter()
            .position(|a| a == "aoe-sandbox-abc12345")
            .expect("container name in argv");
        let cmd_idx = cn_idx + 1;
        assert_eq!(argv.docker_args[cmd_idx], "claude-agent-acp");
        assert_eq!(argv.docker_args[cmd_idx + 1], "--stdio");
        // Literal env entry lands as `-e KEY=VALUE`.
        assert!(
            argv.docker_args.iter().any(|a| a == "MY_LITERAL=hello"),
            "literal env entry must be propagated as `-e KEY=VALUE`"
        );
        // The literal entry's KEY=VALUE form must NOT also appear in
        // `inherit_env` (that vec is for Inherit-style entries whose
        // value comes from the parent process env, not for literals).
        assert!(
            !argv.inherit_env.iter().any(|(k, _)| k == "MY_LITERAL"),
            "literal entries must not duplicate into inherit_env"
        );
    }

    /// Inherit-style env entries (provider auth keys) must lower into a
    /// pair of `-e KEY` (key only) in docker_args plus a `(KEY, VALUE)`
    /// pair in inherit_env so the runner can re-export the value and
    /// docker can forward it into the container.
    #[test]
    fn build_sandbox_docker_argv_inherit_env_shape() {
        let tmp = tempfile::tempdir().unwrap();
        let cwd = tmp.path().to_path_buf();
        let sandbox = SandboxInfo {
            enabled: true,
            container_id: None,
            image: "alpine:latest".into(),
            container_name: "aoe-sandbox-abc12345".into(),
            extra_env: None,
            custom_instruction: None,
        };
        let config = SpawnConfig {
            agent_key: "claude".into(),
            spec: AgentSpec {
                command: "claude-agent-acp".into(),
                args: vec![],
                description: "test".into(),
                env_allowlist: None,
            },
            cwd,
            additional_dirs: vec![],
            // Per-spawn provider_env entry: must end up Inherit-style.
            provider_env: vec![("ANTHROPIC_API_KEY".into(), "sk-test-value".into())],
            socket_path: None,
            stored_acp_session_id: None,
            sandbox_info: Some(sandbox.clone()),
            source_profile: None,
        };
        let argv = build_sandbox_docker_argv(&config, &sandbox, "/workspace/proj")
            .expect("docker argv built");
        // The `-e KEY` flag (without value) must appear consecutively.
        let key_flag_idx = argv
            .docker_args
            .windows(2)
            .position(|w| w[0] == "-e" && w[1] == "ANTHROPIC_API_KEY")
            .expect("ANTHROPIC_API_KEY -e flag must be present");
        // Value-typed forms like `-e ANTHROPIC_API_KEY=...` must NOT
        // appear; that would leak the secret into argv.
        assert!(
            !argv
                .docker_args
                .iter()
                .any(|a| a.starts_with("ANTHROPIC_API_KEY=")),
            "secret must not appear as `KEY=VALUE` in argv (slot {key_flag_idx})"
        );
        // The value must travel via inherit_env so the parent process
        // sets it before exec-ing docker.
        assert_eq!(
            argv.inherit_env
                .iter()
                .find(|(k, _)| k == "ANTHROPIC_API_KEY")
                .map(|(_, v)| v.as_str()),
            Some("sk-test-value"),
        );
    }

    /// `CLAUDE_CONFIG_DIR` is a host filesystem path, not a value, so
    /// it must NOT be auto-forwarded into the container even when set
    /// on the host. The agent's config dir is bind-mounted at the
    /// canonical container path by `AGENT_CONFIG_MOUNTS`.
    ///
    /// Tagged `#[serial]` because the test mutates the process-wide
    /// env; parallel readers of `std::env::var` would race.
    #[test]
    #[serial_test::serial]
    fn build_sandbox_docker_argv_drops_host_only_claude_config_dir() {
        // Set the env var to simulate the host having it; the function
        // under test must still skip it.
        let prev = std::env::var("CLAUDE_CONFIG_DIR").ok();
        std::env::set_var("CLAUDE_CONFIG_DIR", "/Users/operator/.claude");
        let tmp = tempfile::tempdir().unwrap();
        let sandbox = SandboxInfo {
            enabled: true,
            container_id: None,
            image: "alpine:latest".into(),
            container_name: "aoe-sandbox-cfgdir".into(),
            extra_env: None,
            custom_instruction: None,
        };
        let config = SpawnConfig {
            agent_key: "claude".into(),
            spec: AgentSpec {
                command: "claude-agent-acp".into(),
                args: vec![],
                description: "test".into(),
                env_allowlist: None,
            },
            cwd: tmp.path().to_path_buf(),
            additional_dirs: vec![],
            provider_env: vec![],
            socket_path: None,
            stored_acp_session_id: None,
            sandbox_info: Some(sandbox.clone()),
            source_profile: None,
        };
        let argv = build_sandbox_docker_argv(&config, &sandbox, "/workspace/proj")
            .expect("docker argv built");
        match prev {
            Some(v) => std::env::set_var("CLAUDE_CONFIG_DIR", v),
            None => std::env::remove_var("CLAUDE_CONFIG_DIR"),
        }
        assert!(
            !argv.docker_args.iter().any(|a| a == "CLAUDE_CONFIG_DIR"),
            "CLAUDE_CONFIG_DIR is a host path and must not be forwarded as `-e KEY`"
        );
        assert!(
            !argv
                .docker_args
                .iter()
                .any(|a| a.starts_with("CLAUDE_CONFIG_DIR=")),
            "CLAUDE_CONFIG_DIR must not appear as a literal `KEY=VALUE` either"
        );
        assert!(
            !argv
                .inherit_env
                .iter()
                .any(|(k, _)| k == "CLAUDE_CONFIG_DIR"),
            "CLAUDE_CONFIG_DIR must not land in inherit_env"
        );
    }

    #[tokio::test]
    async fn spawn_with_nonexistent_command_errors_cleanly() {
        let config = SpawnConfig {
            agent_key: "claude".into(),
            spec: AgentSpec {
                command: "/nonexistent/agent/binary/aoe-test".into(),
                args: vec![],
                description: "test".into(),
                env_allowlist: None,
            },
            cwd: std::env::temp_dir(),
            additional_dirs: vec![],
            provider_env: vec![],
            socket_path: None,
            stored_acp_session_id: None,
            sandbox_info: None,
            source_profile: None,
        };
        let result = AcpClient::spawn(config, CockpitSessionId("s-1".into())).await;
        assert!(matches!(result, Err(AcpError::Spawn(_))));
    }

    /// Pre-flight cwd check: when `project_path` was renamed out from
    /// under the session, the supervisor's spawn fails with a typed
    /// `ProjectPathMissing` instead of a bare ENOENT-mapped `Spawn`.
    /// See #1089.
    #[tokio::test]
    async fn spawn_returns_project_path_missing_when_cwd_does_not_exist() {
        let missing =
            std::env::temp_dir().join(format!("aoe-test-missing-cwd-{}", std::process::id()));
        // Ensure the path truly does not exist.
        let _ = std::fs::remove_dir_all(&missing);
        let config = SpawnConfig {
            agent_key: "claude".into(),
            spec: AgentSpec {
                command: "/bin/true".into(),
                args: vec![],
                description: "test".into(),
                env_allowlist: None,
            },
            cwd: missing.clone(),
            additional_dirs: vec![],
            provider_env: vec![],
            socket_path: None,
            stored_acp_session_id: None,
            sandbox_info: None,
            source_profile: None,
        };
        let result = AcpClient::spawn(config, CockpitSessionId("s-1".into())).await;
        match result {
            Err(AcpError::ProjectPathMissing { path }) => assert_eq!(path, missing),
            Err(other) => panic!("expected ProjectPathMissing, got {other:?}"),
            Ok(_) => panic!("expected ProjectPathMissing, got Ok"),
        }
    }

    /// Belt-and-suspenders: even if the pre-flight raced (cwd vanishes
    /// between `cwd.exists()` and `Command::spawn`), the classifier turns
    /// the raw ENOENT into `ProjectPathMissing` rather than the generic
    /// install-the-adapter message.
    #[test]
    fn classify_spawn_error_routes_missing_cwd_to_project_path_missing() {
        let missing =
            std::env::temp_dir().join(format!("aoe-test-classify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&missing);
        let io_err = std::io::Error::from(std::io::ErrorKind::NotFound);
        match AcpError::classify_spawn_error(io_err, &missing, "/bin/true") {
            AcpError::ProjectPathMissing { path } => assert_eq!(path, missing),
            other => panic!("expected ProjectPathMissing, got {other:?}"),
        }
    }

    #[test]
    fn classify_spawn_error_keeps_spawn_when_cwd_exists() {
        let cwd = std::env::temp_dir();
        let io_err = std::io::Error::from(std::io::ErrorKind::NotFound);
        match AcpError::classify_spawn_error(io_err, &cwd, "/nonexistent/bin/foo") {
            AcpError::Spawn(msg) => {
                assert!(
                    msg.contains("/nonexistent/bin/foo"),
                    "spawn message should echo command: {msg}"
                );
            }
            other => panic!("expected Spawn, got {other:?}"),
        }
    }

    #[test]
    fn classify_spawn_error_passes_through_non_enoent() {
        let cwd = std::env::temp_dir();
        let io_err = std::io::Error::from(std::io::ErrorKind::PermissionDenied);
        match AcpError::classify_spawn_error(io_err, &cwd, "/bin/true") {
            AcpError::Spawn(_) => {}
            other => panic!("expected Spawn for non-ENOENT, got {other:?}"),
        }
    }

    #[test]
    fn map_update_to_events_threads_parent_tool_call_id() {
        use agent_client_protocol::schema::{SessionUpdate, ToolCall as AcpToolCall};
        let mut meta = serde_json::Map::new();
        meta.insert(
            "claudeCode".to_string(),
            serde_json::json!({ "parentToolUseId": "tc-task-1" }),
        );
        let mut tc = AcpToolCall::new("tc-child-1", "Read");
        tc.raw_input = Some(serde_json::json!({"path": "x"}));
        tc.meta = Some(meta);
        let events = map_update_to_events(SessionUpdate::ToolCall(tc), &agent_profiles::CLAUDE);
        let started = events.iter().find_map(|e| match e {
            Event::ToolCallStarted { tool_call } => Some(tool_call),
            _ => None,
        });
        let started = started.expect("ToolCallStarted emitted");
        assert_eq!(started.parent_tool_call_id.as_deref(), Some("tc-task-1"),);
    }

    #[test]
    fn map_update_to_events_leaves_parent_none_when_meta_missing() {
        use agent_client_protocol::schema::{SessionUpdate, ToolCall as AcpToolCall};
        let mut tc = AcpToolCall::new("tc-1", "Read");
        tc.raw_input = Some(serde_json::json!({"path": "x"}));
        let events = map_update_to_events(SessionUpdate::ToolCall(tc), &agent_profiles::CLAUDE);
        let started = events.iter().find_map(|e| match e {
            Event::ToolCallStarted { tool_call } => Some(tool_call),
            _ => None,
        });
        assert!(started.unwrap().parent_tool_call_id.is_none());
    }

    #[test]
    fn map_update_to_events_does_not_link_parent_for_unverified_agents() {
        use agent_client_protocol::schema::{SessionUpdate, ToolCall as AcpToolCall};
        let mut meta = serde_json::Map::new();
        meta.insert(
            "claudeCode".to_string(),
            serde_json::json!({ "parentToolUseId": "tc-task-1" }),
        );
        let mut tc = AcpToolCall::new("tc-child-1", "Read");
        tc.raw_input = Some(serde_json::json!({"path": "x"}));
        tc.meta = Some(meta);
        // Codex profile lists no parent_meta_namespaces, so the linkage
        // doesn't render even when claude's namespace happens to be on
        // the wire.
        let events = map_update_to_events(SessionUpdate::ToolCall(tc), &agent_profiles::CODEX);
        let started = events.iter().find_map(|e| match e {
            Event::ToolCallStarted { tool_call } => Some(tool_call),
            _ => None,
        });
        assert!(started.unwrap().parent_tool_call_id.is_none());
    }

    #[test]
    fn parse_plan_steps_extracts_dash_and_numbered_bullets() {
        let md = "Here's the plan:\n\n- First, **read** the file\n- Then patch it\n1. Run tests\n2. Commit\n\nOther prose.";
        let steps = parse_plan_steps(md);
        let titles: Vec<&str> = steps.iter().map(|s| s.title.as_str()).collect();
        assert_eq!(
            titles,
            vec![
                "First, read the file",
                "Then patch it",
                "Run tests",
                "Commit"
            ]
        );
        for s in &steps {
            assert!(matches!(s.status, PlanStepStatus::Pending));
        }
    }

    #[test]
    fn parse_plan_steps_returns_empty_when_no_bullets() {
        assert!(parse_plan_steps("Just a paragraph with no list.").is_empty());
        assert!(parse_plan_steps("").is_empty());
    }

    #[test]
    fn extract_plan_from_switch_mode_handles_missing_plan_field() {
        let v = serde_json::json!({});
        assert!(extract_plan_from_switch_mode(&v).is_none());
        let v = serde_json::json!({ "plan": 42 });
        assert!(extract_plan_from_switch_mode(&v).is_none());
    }

    #[test]
    fn extract_plan_from_switch_mode_builds_plan_when_input_has_bullets() {
        let v = serde_json::json!({
            "plan": "- Step one\n- Step two\n- Step three"
        });
        let plan = extract_plan_from_switch_mode(&v).expect("plan should parse");
        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.steps[0].title, "Step one");
    }

    #[test]
    fn strip_markdown_emphasis_unwraps_bold_and_italic() {
        assert_eq!(strip_markdown_emphasis("**bold**"), "bold");
        assert_eq!(strip_markdown_emphasis("__bold__"), "bold");
        assert_eq!(strip_markdown_emphasis("*italic*"), "italic");
        assert_eq!(strip_markdown_emphasis("_italic_"), "italic");
        assert_eq!(
            strip_markdown_emphasis("mix of **bold** and *italic*"),
            "mix of bold and italic"
        );
        assert_eq!(strip_markdown_emphasis("plain"), "plain");
    }

    #[test]
    fn is_compact_completion_matches_adapter_string() {
        assert!(is_compact_completion("Compacting completed."));
        assert!(is_compact_completion("\n\nCompacting completed.\n"));
        assert!(!is_compact_completion("Compacting..."));
        assert!(!is_compact_completion("compact done"));
        assert!(!is_compact_completion(""));
    }

    #[test]
    fn resolve_agent_command_returns_none_for_absolute_path() {
        assert!(resolve_agent_command("/usr/local/bin/claude-agent-acp").is_none());
        assert!(resolve_agent_command("./relative/path").is_none());
    }

    #[test]
    fn resolve_agent_command_returns_none_for_placeholder() {
        assert!(resolve_agent_command("${aoe_data_dir}/cockpit-worker/dist/aoe-agent").is_none());
    }

    #[test]
    #[serial_test::serial]
    fn resolve_agent_command_finds_binary_in_path_env() {
        // Build a temp dir with a fake binary, point PATH at it.
        // Tagged `#[serial]` because the test mutates the process-wide
        // PATH; any concurrent test that reads PATH (e.g. resolves a
        // real binary) would race.
        let dir = tempfile::TempDir::new().unwrap();
        let bin = dir.path().join("aoe-test-resolver-fake");
        std::fs::write(&bin, "#!/bin/sh\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let prev = std::env::var_os("PATH");
        let new_path = format!(
            "{}:{}",
            dir.path().display(),
            prev.as_ref()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default()
        );
        // SAFETY: this test mutates the process-wide PATH. Other PATH
        // readers in the same test binary would race; `#[serial]` keeps
        // them apart.
        unsafe {
            std::env::set_var("PATH", &new_path);
        }
        let resolved = resolve_agent_command("aoe-test-resolver-fake");
        if let Some(prev) = prev {
            unsafe {
                std::env::set_var("PATH", prev);
            }
        }
        let (path, parent) = resolved.expect("binary should resolve from PATH");
        assert_eq!(path, bin);
        assert_eq!(parent, dir.path());
    }

    #[test]
    fn pick_option_id_finds_allow_once() {
        use agent_client_protocol::schema::{PermissionOption, PermissionOptionId};
        let options = vec![
            PermissionOption::new(
                PermissionOptionId::new("yes"),
                "Allow this once",
                PermissionOptionKind::AllowOnce,
            ),
            PermissionOption::new(
                PermissionOptionId::new("no"),
                "Reject",
                PermissionOptionKind::RejectOnce,
            ),
        ];
        let id = pick_option_id(&options, ApprovalDecision::Allow).unwrap();
        assert_eq!(id.0.as_ref(), "yes");
    }

    #[test]
    fn pick_option_id_falls_back() {
        use agent_client_protocol::schema::{PermissionOption, PermissionOptionId};
        let options = vec![PermissionOption::new(
            PermissionOptionId::new("always"),
            "Always",
            PermissionOptionKind::AllowAlways,
        )];
        // We asked for Allow (prefers AllowOnce); the agent only offered
        // AllowAlways. Falls back gracefully.
        let id = pick_option_id(&options, ApprovalDecision::Allow).unwrap();
        assert_eq!(id.0.as_ref(), "always");
    }

    #[test]
    fn preview_args_caps_to_16k() {
        let big = serde_json::Value::String("x".repeat(20_000));
        let preview = preview_args(&big);
        assert!(preview.len() <= 16 * 1024 + 32);
        assert!(preview.contains("[truncated]"));
    }

    #[test]
    fn extract_tool_content_text_concats_text_blocks() {
        use agent_client_protocol::schema::{Content, ToolCallContent};
        let blocks = vec![
            ToolCallContent::Content(Content::new("stdout line 1")),
            ToolCallContent::Content(Content::new("stdout line 2")),
        ];
        let text = extract_tool_content_text(&blocks);
        assert_eq!(text, "stdout line 1\nstdout line 2");
    }

    #[test]
    fn extract_tool_content_text_empty_for_no_text_blocks() {
        // No content → empty string. The reducer falls back to the
        // status word ("completed" / "tool failed") in that case so
        // the card still conveys state.
        assert_eq!(extract_tool_content_text(&[]), "");
    }

    #[test]
    fn map_tool_call_update_completed_carries_content() {
        use agent_client_protocol::schema::{
            Content, ToolCallContent, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
        };
        let fields = ToolCallUpdateFields::new()
            .status(ToolCallStatus::Completed)
            .content(vec![ToolCallContent::Content(Content::new(
                "abc1234 first commit",
            ))]);
        let update = ToolCallUpdate::new("tc-1", fields);
        let events = map_update_to_events(
            SessionUpdate::ToolCallUpdate(update),
            &agent_profiles::CLAUDE,
        );
        assert_eq!(events.len(), 1);
        match &events[0] {
            Event::ToolCallCompleted {
                tool_call_id,
                is_error,
                content,
                completed_at: _,
            } => {
                assert_eq!(tool_call_id, "tc-1");
                assert!(!*is_error);
                assert_eq!(content, "abc1234 first commit");
            }
            other => panic!("expected ToolCallCompleted, got {other:?}"),
        }
    }

    #[test]
    fn map_tool_call_update_in_progress_with_content_emits_streaming_event() {
        use agent_client_protocol::schema::{
            Content, ToolCallContent, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
        };
        let fields = ToolCallUpdateFields::new()
            .status(ToolCallStatus::InProgress)
            .content(vec![ToolCallContent::Content(Content::new(
                "partial output",
            ))]);
        let update = ToolCallUpdate::new("tc-2", fields);
        let events = map_update_to_events(
            SessionUpdate::ToolCallUpdate(update),
            &agent_profiles::CLAUDE,
        );
        // InProgress now emits a ToolCallUpdated re-stamping started_at
        // (#1060 follow-up) plus the streaming ToolCallContent.
        assert_eq!(events.len(), 2);
        match &events[0] {
            Event::ToolCallUpdated {
                tool_call_id,
                started_at,
                ..
            } => {
                assert_eq!(tool_call_id, "tc-2");
                assert!(started_at.is_some());
            }
            other => panic!("expected ToolCallUpdated, got {other:?}"),
        }
        match &events[1] {
            Event::ToolCallContent {
                tool_call_id,
                content,
            } => {
                assert_eq!(tool_call_id, "tc-2");
                assert_eq!(content, "partial output");
            }
            other => panic!("expected ToolCallContent, got {other:?}"),
        }
    }

    #[test]
    fn map_tool_call_update_in_progress_restamps_started_at() {
        use agent_client_protocol::schema::{ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields};
        let fields = ToolCallUpdateFields::new().status(ToolCallStatus::InProgress);
        let update = ToolCallUpdate::new("tc-3", fields);
        let events = map_update_to_events(
            SessionUpdate::ToolCallUpdate(update),
            &agent_profiles::CLAUDE,
        );
        assert_eq!(events.len(), 1);
        match &events[0] {
            Event::ToolCallUpdated {
                tool_call_id,
                started_at,
                title,
                args_preview,
            } => {
                assert_eq!(tool_call_id, "tc-3");
                assert!(
                    started_at.is_some(),
                    "InProgress must carry a re-stamped started_at"
                );
                assert!(title.is_none());
                assert!(args_preview.is_none());
            }
            other => panic!("expected ToolCallUpdated, got {other:?}"),
        }
    }

    #[test]
    fn map_tool_call_update_emits_wakeup_when_title_and_raw_input_land_in_update() {
        // claude-agent-acp sends the initial `ToolCall` for ScheduleWakeup
        // with `raw_input = {}`; the real `delaySeconds` arrives on a
        // follow-up `ToolCallUpdate` that carries both `title` and
        // `raw_input`. The initial-path emit therefore returns `None`
        // from `wakeup_event_from_raw`, and the update-path must pick up
        // the slack so `Event::WakeupScheduled` lands in the store
        // (sidebar `⏰ in Nm` chip + cockpit "Asleep until…" banner
        // depend on it). Regression for #1091.
        use agent_client_protocol::schema::{ToolCallUpdate, ToolCallUpdateFields};
        let fields = ToolCallUpdateFields::new()
            .title("ScheduleWakeup".to_string())
            .raw_input(serde_json::json!({
                "delaySeconds": 600,
                "prompt": "Wake-up fired. Confirm.",
                "reason": "Test 10-minute wake-up card countdown",
            }));
        let update = ToolCallUpdate::new("toolu_test", fields);
        let events = map_update_to_events(
            SessionUpdate::ToolCallUpdate(update),
            &agent_profiles::CLAUDE,
        );
        let wakeup = events
            .iter()
            .find(|e| matches!(e, Event::WakeupScheduled { .. }))
            .expect(
                "ToolCallUpdate with title=ScheduleWakeup + delaySeconds must emit WakeupScheduled",
            );
        match wakeup {
            Event::WakeupScheduled { at, reason } => {
                let delta = (*at - chrono::Utc::now()).num_seconds();
                assert!(
                    (590..=610).contains(&delta),
                    "wakeup `at` should be ~600s in the future, got {delta}s",
                );
                assert_eq!(
                    reason.as_deref(),
                    Some("Test 10-minute wake-up card countdown"),
                );
            }
            other => panic!("expected WakeupScheduled, got {other:?}"),
        }
    }

    #[test]
    fn map_tool_call_update_skips_wakeup_when_raw_input_missing() {
        // Title-only update (the initial frame's mirror, before
        // raw_input arrives) must NOT emit a WakeupScheduled, otherwise
        // we'd publish a "wakeup at epoch zero" placeholder.
        use agent_client_protocol::schema::{ToolCallUpdate, ToolCallUpdateFields};
        let fields = ToolCallUpdateFields::new().title("ScheduleWakeup".to_string());
        let update = ToolCallUpdate::new("toolu_test", fields);
        let events = map_update_to_events(
            SessionUpdate::ToolCallUpdate(update),
            &agent_profiles::CLAUDE,
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, Event::WakeupScheduled { .. })),
            "no WakeupScheduled should fire without delaySeconds",
        );
    }

    #[test]
    fn map_usage_update_emits_typed_usage_event() {
        use agent_client_protocol::schema::{Cost, UsageUpdate};
        let u = UsageUpdate::new(12_345, 200_000).cost(Cost::new(0.42, "USD"));
        let events = map_update_to_events(SessionUpdate::UsageUpdate(u), &agent_profiles::CLAUDE);
        assert_eq!(events.len(), 1);
        match &events[0] {
            Event::UsageUpdated { usage } => {
                assert_eq!(usage.used, 12_345);
                assert_eq!(usage.size, 200_000);
                let cost = usage.cost.as_ref().expect("cost present");
                assert!((cost.amount - 0.42).abs() < f64::EPSILON);
                assert_eq!(cost.currency, "USD");
            }
            other => panic!("expected UsageUpdated, got {other:?}"),
        }
    }

    #[test]
    fn map_available_commands_update_emits_typed_event() {
        use agent_client_protocol::schema::{
            AvailableCommand as AcpAvailableCommand, AvailableCommandInput,
            AvailableCommandsUpdate, UnstructuredCommandInput,
        };
        let cmds = vec![
            AcpAvailableCommand::new("review", "Review changes").input(
                AvailableCommandInput::Unstructured(UnstructuredCommandInput::new("PR url")),
            ),
            AcpAvailableCommand::new("clear", "Reset context"),
        ];
        let update = AvailableCommandsUpdate::new(cmds);
        let events = map_update_to_events(
            SessionUpdate::AvailableCommandsUpdate(update),
            &agent_profiles::CLAUDE,
        );
        assert_eq!(events.len(), 1);
        match &events[0] {
            Event::AvailableCommandsUpdated { commands } => {
                assert_eq!(commands.len(), 2);
                assert_eq!(commands[0].name, "review");
                assert!(commands[0].accepts_input);
                assert_eq!(commands[1].name, "clear");
                assert!(!commands[1].accepts_input);
            }
            other => panic!("expected AvailableCommandsUpdated, got {other:?}"),
        }
    }

    #[test]
    fn preview_args_strips_control_chars() {
        // Build the preview string by hand-injecting raw control chars
        // *into* the result of to_string (simulating agents that send
        // pre-serialised non-utf8 noise through). The function should
        // strip BEL/BS/etc. but preserve `\n` and `\t`.
        let arg = serde_json::Value::String("hello\x07world".into());
        let preview = preview_args(&arg);
        // The literal BEL (0x07) inside the string-data part of the JSON
        // gets escaped by to_string, so the preview never sees a raw
        // control char in this path. That's fine: the assertion we care
        // about is that the preview doesn't carry any unprintable bytes.
        for c in preview.chars() {
            assert!(
                !c.is_control() || c == '\n' || c == '\t',
                "unexpected control char {:?} in preview",
                c
            );
        }
        assert!(preview.contains("hello"));
        assert!(preview.contains("world"));
    }

    #[test]
    fn provider_env_denyreason_blocks_infra_and_linker_keys() {
        assert!(provider_env_denyreason("AOE_TOKEN").is_some());
        assert!(provider_env_denyreason("PATH").is_some());
        assert!(provider_env_denyreason("HOME").is_some());
        assert!(provider_env_denyreason("LD_PRELOAD").is_some());
        assert!(provider_env_denyreason("LD_LIBRARY_PATH").is_some());
        assert!(provider_env_denyreason("DYLD_INSERT_LIBRARIES").is_some());
        assert!(provider_env_denyreason("").is_some());
    }

    #[test]
    fn provider_env_denyreason_allows_provider_auth_keys() {
        // The legitimate use case: per-session auth override.
        assert!(provider_env_denyreason("ANTHROPIC_API_KEY").is_none());
        assert!(provider_env_denyreason("CLAUDE_CODE_OAUTH_TOKEN").is_none());
        assert!(provider_env_denyreason("OPENAI_API_KEY").is_none());
        assert!(provider_env_denyreason("AOE_AGENT_MODEL").is_none());
        // Custom provider keys should pass through.
        assert!(provider_env_denyreason("MY_CUSTOM_VAR").is_none());
    }

    #[test]
    fn scrub_stderr_secrets_redacts_known_prefixes() {
        let cases = [
            ("auth failed: sk-ant-abcdefghijklmnop1234567890", true),
            ("Bearer abcdefghijklmnop1234567890.signature", true),
            ("GitHub PAT: ghp_abcdefghijklmnop1234567890", true),
            ("legacy fine grained: github_pat_abcdefghijklmnop1234", true),
            ("AWS: AKIAIOSFODNN7EXAMPLE", true),
        ];
        for (input, should_redact) in cases {
            let scrubbed = scrub_stderr_secrets(input);
            if should_redact {
                assert!(
                    scrubbed.contains("<redacted-secret>"),
                    "expected redaction in {input:?}, got {scrubbed:?}"
                );
            } else {
                assert_eq!(scrubbed, input);
            }
        }
    }

    #[test]
    fn scrub_stderr_secrets_leaves_innocuous_lines_alone() {
        // Common-case debug lines that must not get false-positive
        // redaction or the log loses diagnostic value.
        let lines = [
            "agent connected at /tmp/aoe.sock",
            "session/initialize ok, capabilities: load_session=true",
            "user prompt: please refactor src/main.rs to use anyhow",
            // Even though "sk-" appears, the literal isn't long enough
            // to match the secret regex.
            "the variable sk-test is fine",
        ];
        for line in lines {
            assert_eq!(scrub_stderr_secrets(line), line);
        }
    }
}
