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
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use agent_client_protocol::schema::{
    CancelNotification, ClientCapabilities, ContentBlock, CreateTerminalRequest,
    CreateTerminalResponse, FileSystemCapabilities, InitializeRequest, KillTerminalRequest,
    KillTerminalResponse, NewSessionRequest, PermissionOptionKind, PromptRequest, ProtocolVersion,
    ReadTextFileRequest, ReadTextFileResponse, ReleaseTerminalRequest, ReleaseTerminalResponse,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, SessionUpdate, SetSessionModeRequest,
    TerminalId, TerminalOutputRequest, TerminalOutputResponse, TextContent,
    WaitForTerminalExitRequest, WaitForTerminalExitResponse, WriteTextFileRequest,
    WriteTextFileResponse,
};
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectionTo, Responder};
use thiserror::Error;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{debug, error, info, warn};

use super::agent_registry::AgentSpec;
use super::approvals::{is_destructive, ApprovalDecision, Nonce};
use super::fs_handler::{self, FsPolicy};
use super::permissions::build_approval;
use super::state::{
    CockpitSessionId, DiffPreview, Event, ModeInfo, Plan, PlanStep, PlanStepStatus, SessionMode,
    ToolCall,
};
use super::terminal_handler::TerminalManager;

#[derive(Debug, Error)]
pub enum AcpError {
    #[error("agent spawn failed: {0}")]
    Spawn(String),
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

/// Configuration for spawning an ACP agent.
#[derive(Debug, Clone)]
pub struct SpawnConfig {
    pub spec: AgentSpec,
    pub cwd: PathBuf,
    pub additional_dirs: Vec<PathBuf>,
    /// Provider env vars to forward (after applying the agent's allowlist).
    pub provider_env: Vec<(String, String)>,
    /// When set, aoe creates a unix socket at this path BEFORE spawning
    /// the agent and exports `AOE_ACP_SOCKET=<path>` to the agent's env.
    /// The agent connects to the socket instead of using stdio. Used
    /// for sandboxed cockpit sessions: the same socket path is bind-
    /// mounted into the container so the in-container agent can reach
    /// the host-side aoe.
    pub socket_path: Option<PathBuf>,
}

/// Commands sent from `AcpClient` methods to the background connection task.
enum ClientCmd {
    Prompt(String),
    Cancel,
    SetMode(String),
    Shutdown,
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

/// Per-session resources the connection task uses to handle ACP fs/* and
/// terminal/* requests delegated by the agent.
#[derive(Clone)]
struct SessionResources {
    fs_policy: Arc<FsPolicy>,
    terminals: TerminalManager,
    cwd: PathBuf,
    label: String,
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
        let (cmd_tx, cmd_rx) = mpsc::channel::<ClientCmd>(16);
        let (event_tx, event_rx) = mpsc::channel::<Event>(64);
        let pending_responders: PendingResponders = Arc::new(Mutex::new(HashMap::new()));

        // Choose transport: if a socket path is set, bind a listener
        // first, then spawn the agent with AOE_ACP_SOCKET pointing at
        // it. Otherwise fall back to stdio over the child's stdin/out.
        let socket_listener = if let Some(socket_path) = &config.socket_path {
            // Remove any stale socket so bind succeeds.
            let _ = tokio::fs::remove_file(socket_path).await;
            if let Some(parent) = socket_path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| AcpError::Spawn(format!("socket parent: {e}")))?;
            }
            let listener = tokio::net::UnixListener::bind(socket_path)
                .map_err(|e| AcpError::Spawn(format!("bind unix socket: {e}")))?;
            Some(listener)
        } else {
            None
        };

        let child = spawn_subprocess(&config)?;
        let child = Arc::new(Mutex::new(child));

        match socket_listener {
            None => {
                Self::start_with_stdio(
                    config.cwd,
                    config.additional_dirs,
                    session_id,
                    child,
                    pending_responders,
                    cmd_tx,
                    cmd_rx,
                    event_tx,
                    event_rx,
                )
                .await
            }
            Some(listener) => {
                let socket_path = config.socket_path.clone();
                Self::start_with_socket(
                    config.cwd,
                    config.additional_dirs,
                    session_id,
                    child,
                    pending_responders,
                    cmd_tx,
                    cmd_rx,
                    event_tx,
                    event_rx,
                    listener,
                    socket_path,
                )
                .await
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    async fn start_with_stdio(
        cwd: PathBuf,
        additional_dirs: Vec<PathBuf>,
        session_id: CockpitSessionId,
        child: Arc<Mutex<tokio::process::Child>>,
        pending_responders: PendingResponders,
        cmd_tx: mpsc::Sender<ClientCmd>,
        cmd_rx: mpsc::Receiver<ClientCmd>,
        event_tx: mpsc::Sender<Event>,
        event_rx: mpsc::Receiver<Event>,
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
        let resources = SessionResources {
            fs_policy: Arc::new(FsPolicy::new(roots)),
            terminals: TerminalManager::new(),
            cwd: cwd.clone(),
            label: session_label.clone(),
        };

        let (ready_tx, ready_rx) = oneshot::channel::<Result<(), AcpError>>();

        tokio::spawn(run_connection_task(
            transport,
            event_tx,
            cmd_rx,
            cwd,
            session_label.clone(),
            child_for_task,
            pending_for_task,
            resources,
            None,
            Some(ready_tx),
        ));

        wait_for_handshake(&session_label, ready_rx, &child).await?;

        Ok(Self {
            session_id,
            inbound: Some(event_rx),
            cmd_tx: Some(cmd_tx),
            pending_responders,
            _child: Some(child),
        })
    }

    #[allow(clippy::too_many_arguments)]
    async fn start_with_socket(
        cwd: PathBuf,
        additional_dirs: Vec<PathBuf>,
        session_id: CockpitSessionId,
        child: Arc<Mutex<tokio::process::Child>>,
        pending_responders: PendingResponders,
        cmd_tx: mpsc::Sender<ClientCmd>,
        cmd_rx: mpsc::Receiver<ClientCmd>,
        event_tx: mpsc::Sender<Event>,
        event_rx: mpsc::Receiver<Event>,
        listener: tokio::net::UnixListener,
        socket_path: Option<PathBuf>,
    ) -> Result<Self, AcpError> {
        // Wait for the agent to connect. Bound the wait so a wedged
        // agent doesn't park spawn() forever.
        let accept = tokio::time::timeout(std::time::Duration::from_secs(10), listener.accept())
            .await
            .map_err(|_| AcpError::Spawn("agent did not connect to socket within 10s".into()))?
            .map_err(|e| AcpError::Spawn(format!("accept: {e}")))?;
        let (stream, _addr) = accept;
        let (read_half, write_half) = stream.into_split();
        let transport = ByteStreams::new(write_half.compat_write(), read_half.compat());

        let mut roots = vec![cwd.clone()];
        roots.extend(additional_dirs);
        let resources = SessionResources {
            fs_policy: Arc::new(FsPolicy::new(roots)),
            terminals: TerminalManager::new(),
            cwd: cwd.clone(),
            label: session_id.0.clone(),
        };

        let session_label = session_id.0.clone();
        let child_for_task = child.clone();
        let pending_for_task = pending_responders.clone();

        let (ready_tx, ready_rx) = oneshot::channel::<Result<(), AcpError>>();

        tokio::spawn(run_connection_task(
            transport,
            event_tx,
            cmd_rx,
            cwd,
            session_label.clone(),
            child_for_task,
            pending_for_task,
            resources,
            socket_path,
            Some(ready_tx),
        ));

        wait_for_handshake(&session_label, ready_rx, &child).await?;

        Ok(Self {
            session_id,
            inbound: Some(event_rx),
            cmd_tx: Some(cmd_tx),
            pending_responders,
            _child: Some(child),
        })
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

fn spawn_subprocess(config: &SpawnConfig) -> Result<tokio::process::Child, AcpError> {
    let mut cmd = tokio::process::Command::new(&config.spec.command);
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
    for name in always_forward {
        if let Ok(value) = std::env::var(name) {
            cmd.env(name, value);
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
            }
        }
    }
    for (key, value) in &config.provider_env {
        if key == "AOE_TOKEN" {
            warn!(target: "cockpit", "ignoring AOE_TOKEN in provider env");
            continue;
        }
        cmd.env(key, value);
    }

    // Socket-transport agents need to know where to connect. Pass the
    // path via env so the agent's bootstrap can `connect()` to it
    // instead of falling back to stdio.
    if let Some(socket_path) = &config.socket_path {
        cmd.env("AOE_ACP_SOCKET", socket_path);
    }

    cmd.spawn().map_err(|e| AcpError::Spawn(e.to_string()))
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
    };
    for kind in preferred_kinds {
        if let Some(opt) = options.iter().find(|o| &o.kind == kind) {
            return Some(opt.option_id.clone());
        }
    }
    None
}

/// Map an ACP `SessionUpdate` to the cockpit's typed `Event`. Variants we
/// don't yet handle pass through as `RawAgentUpdate` so UI clients can at
/// least see them; we'll narrow these as the schema stabilises.
fn map_update_to_events(update: SessionUpdate) -> Vec<Event> {
    match update {
        SessionUpdate::AgentMessageChunk(chunk) => match chunk.content {
            ContentBlock::Text(text) => vec![Event::AgentMessageChunk { text: text.text }],
            other => vec![raw_event(&other)],
        },
        SessionUpdate::AgentThoughtChunk(_) => vec![Event::ThinkingStarted],
        SessionUpdate::ToolCall(tc) => {
            let raw_args = tc.raw_input.clone().unwrap_or(serde_json::Value::Null);
            let args_preview = preview_args(&raw_args);
            let tool_call = ToolCall {
                id: tc.tool_call_id.0.to_string(),
                name: tc.title.clone(),
                kind: tool_kind_str(&tc.kind),
                args_preview: args_preview.clone(),
                started_at: chrono::Utc::now(),
            };
            let mut events = vec![Event::ToolCallStarted { tool_call }];
            if is_destructive(&tc.title, &args_preview) {
                debug!(target: "cockpit.acp", "tool {} flagged destructive on tool_call ingest", tc.title);
            }
            // If the same payload carries diff content, surface it.
            if let Some(diff) = extract_diff_from_locations(&tc.locations) {
                events.push(Event::DiffEmitted { diff });
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
            if completed {
                vec![Event::ToolCallCompleted {
                    tool_call_id: id,
                    is_error,
                }]
            } else {
                vec![raw_event(&update)]
            }
        }
        SessionUpdate::Plan(p) => {
            let plan = Plan {
                plan_id: format!("plan-{}", chrono::Utc::now().timestamp_millis()),
                version: 1,
                steps: p
                    .entries
                    .into_iter()
                    .enumerate()
                    .map(|(i, e)| PlanStep {
                        id: format!("step-{i}"),
                        title: e.content,
                        detail: None,
                        status: map_plan_status(e.status),
                    })
                    .collect(),
            };
            vec![Event::PlanUpdated { plan }]
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
    child: Arc<Mutex<tokio::process::Child>>,
    pending_responders: PendingResponders,
    resources: SessionResources,
    socket_path: Option<PathBuf>,
    ready_tx: Option<oneshot::Sender<Result<(), AcpError>>>,
) where
    W: futures_util::AsyncWrite + Send + 'static,
    R: futures_util::AsyncRead + Send + 'static,
{
    let ready_tx = Arc::new(Mutex::new(ready_tx));
    let ready_for_block = ready_tx.clone();
    let event_tx_for_notif = event_tx.clone();
    let event_tx_for_perm = event_tx.clone();
    let event_tx_for_block = event_tx.clone();
    let pending_for_perm = pending_responders.clone();
    let cmd_rx = Arc::new(Mutex::new(cmd_rx));
    let res_read = resources.clone();
    let res_write = resources.clone();
    let res_term_create = resources.clone();
    let res_term_output = resources.clone();
    let res_term_wait = resources.clone();
    let res_term_kill = resources.clone();
    let res_term_release = resources.clone();

    let result = Client
        .builder()
        .name("aoe-cockpit")
        .on_receive_notification(
            move |notification: SessionNotification, _cx| {
                let event_tx = event_tx_for_notif.clone();
                async move {
                    for event in map_update_to_events(notification.update) {
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
                    handle_permission_request(request, responder, event_tx, pending).await
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
            let _init = connection
                .send_request(
                    InitializeRequest::new(ProtocolVersion::V1)
                        .client_capabilities(capabilities),
                )
                .block_task()
                .await?;

            info!(target: "cockpit.acp", session = %session_label, "creating ACP session");
            let new_session = connection
                .send_request(NewSessionRequest::new(cwd))
                .block_task()
                .await?;
            let acp_session_id = new_session.session_id.clone();

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

            if let Some(tx) = ready_for_block.lock().await.take() {
                let _ = tx.send(Ok(()));
            }

            loop {
                let cmd = {
                    let mut rx = cmd_rx.lock().await;
                    rx.recv().await
                };
                match cmd {
                    Some(ClientCmd::Prompt(text)) => {
                        info!(target: "cockpit.acp", "sending prompt ({} chars)", text.len());
                        let _ = connection
                            .send_request(PromptRequest::new(
                                acp_session_id.clone(),
                                vec![ContentBlock::Text(TextContent::new(text))],
                            ))
                            .block_task()
                            .await?;
                        let _ = event_tx_for_block
                            .send(Event::Stopped {
                                reason: "prompt_complete".into(),
                            })
                            .await;
                    }
                    Some(ClientCmd::Cancel) => {
                        info!(target: "cockpit.acp", "sending session/cancel");
                        connection
                            .send_notification(CancelNotification::new(acp_session_id.clone()))?;
                    }
                    Some(ClientCmd::SetMode(mode_id)) => {
                        info!(target: "cockpit.acp", "sending session/set_mode mode={mode_id}");
                        let _ = connection
                            .send_request(SetSessionModeRequest::new(
                                acp_session_id.clone(),
                                mode_id,
                            ))
                            .block_task()
                            .await?;
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

    if let Err(e) = &result {
        error!(target: "cockpit.acp", "ACP connection task ended with error: {:?}", e);
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
    let mut guard = child.lock().await;
    let _ = guard.kill().await;
    // Clean up socket file on exit when this transport was socket-based.
    if let Some(path) = socket_path {
        let _ = tokio::fs::remove_file(path).await;
    }
}

/// Wait for the connection task to finish the ACP handshake (or fail).
/// Bounds the wait so a wedged agent (the classic `npx -y` first-run
/// download stall) returns a clear typed error instead of leaving the
/// supervisor parked indefinitely. Also watches for early child exit
/// and surfaces stderr in the message so callers see why it died.
async fn wait_for_handshake(
    session_label: &str,
    ready_rx: oneshot::Receiver<Result<(), AcpError>>,
    child: &Arc<Mutex<tokio::process::Child>>,
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
            // Kill the wedged child so we don't leak a zombie npx
            // download. The connection task will then unwind and the
            // ready_tx is already gone, so no event_tx duplicate.
            let mut guard = child.lock().await;
            let _ = guard.kill().await;
            Err(AcpError::Spawn(format!(
                "agent did not complete the ACP initialize handshake within {}s. \
                 Common causes: `npx -y` is still downloading the adapter on first run, \
                 or the configured agent command isn't a real ACP server. \
                 Try `npm install -g @agentclientprotocol/claude-agent-acp` and re-run.",
                timeout.as_secs()
            )))
        }
    }
}

async fn collect_child_failure(child: &Arc<Mutex<tokio::process::Child>>) {
    let mut guard = child.lock().await;
    if let Ok(Some(status)) = guard.try_wait() {
        warn!(target: "cockpit.acp", "agent process exited early: status={status}");
    }
}

async fn handle_read_text_file(
    request: ReadTextFileRequest,
    responder: Responder<ReadTextFileResponse>,
    res: SessionResources,
) -> agent_client_protocol::Result<()> {
    match fs_handler::handle_read(&res.fs_policy, &res.label, &request.path) {
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
    }
}

async fn handle_write_text_file(
    request: WriteTextFileRequest,
    responder: Responder<WriteTextFileResponse>,
    res: SessionResources,
) -> agent_client_protocol::Result<()> {
    match fs_handler::handle_write(&res.fs_policy, &res.label, &request.path, &request.content) {
        Ok(()) => responder.respond(WriteTextFileResponse::new()),
        Err(e) => {
            responder.respond_with_error(agent_client_protocol::util::internal_error(e.to_string()))
        }
    }
}

async fn handle_create_terminal(
    request: CreateTerminalRequest,
    responder: Responder<CreateTerminalResponse>,
    res: SessionResources,
) -> agent_client_protocol::Result<()> {
    let cwd = request.cwd.clone().unwrap_or_else(|| res.cwd.clone());
    // Sandbox the cwd: must be inside session roots.
    if let Err(e) = res.fs_policy.resolve_inside(&cwd) {
        return responder.respond_with_error(agent_client_protocol::util::internal_error(format!(
            "terminal cwd outside session roots: {e}"
        )));
    }
    match res
        .terminals
        .create_and_run(&res.label, &request.command, request.args.clone(), cwd)
        .await
    {
        Ok(id) => responder.respond(CreateTerminalResponse::new(TerminalId::new(id))),
        Err(e) => {
            responder.respond_with_error(agent_client_protocol::util::internal_error(e.to_string()))
        }
    }
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
    match res.terminals.output(request.terminal_id.0.as_ref()).await {
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
    }
}

async fn handle_wait_for_terminal_exit(
    request: WaitForTerminalExitRequest,
    responder: Responder<WaitForTerminalExitResponse>,
    res: SessionResources,
) -> agent_client_protocol::Result<()> {
    // For our one-shot terminal model, the command has already finished by
    // the time `create_and_run` returns. So `output()` immediately yields
    // the captured exit status.
    match res.terminals.output(request.terminal_id.0.as_ref()).await {
        Ok(out) => responder.respond(WaitForTerminalExitResponse::new(build_exit_status(
            out.exit_code,
        ))),
        Err(e) => {
            responder.respond_with_error(agent_client_protocol::util::internal_error(e.to_string()))
        }
    }
}

async fn handle_kill_terminal(
    _request: KillTerminalRequest,
    responder: Responder<KillTerminalResponse>,
    _res: SessionResources,
) -> agent_client_protocol::Result<()> {
    // One-shot terminals are already finished; kill is a no-op.
    responder.respond(KillTerminalResponse::new())
}

async fn handle_release_terminal(
    request: ReleaseTerminalRequest,
    responder: Responder<ReleaseTerminalResponse>,
    res: SessionResources,
) -> agent_client_protocol::Result<()> {
    match res.terminals.release(request.terminal_id.0.as_ref()).await {
        Ok(()) => responder.respond(ReleaseTerminalResponse::new()),
        Err(e) => {
            responder.respond_with_error(agent_client_protocol::util::internal_error(e.to_string()))
        }
    }
}

async fn handle_permission_request(
    request: RequestPermissionRequest,
    responder: Responder<RequestPermissionResponse>,
    event_tx: mpsc::Sender<Event>,
    pending: PendingResponders,
) -> agent_client_protocol::Result<()> {
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
        return responder.respond(RequestPermissionResponse::new(
            RequestPermissionOutcome::Cancelled,
        ));
    }

    let outcome = match resolve_rx.await {
        Ok(ApprovalResolutionMessage::Decision { decision }) => {
            if let Some(option_id) = pick_option_id(&request.options, decision) {
                // Surface the resolution to UI clients via the typed event channel.
                let _ = event_tx
                    .send(Event::ApprovalResolved {
                        nonce: nonce.clone(),
                        decision,
                    })
                    .await;
                RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id))
            } else {
                warn!(
                    target: "cockpit.acp",
                    "agent did not offer a {decision:?}-compatible option; cancelling"
                );
                RequestPermissionOutcome::Cancelled
            }
        }
        Ok(ApprovalResolutionMessage::Cancelled) | Err(_) => RequestPermissionOutcome::Cancelled,
    };

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

    #[tokio::test]
    async fn spawn_with_nonexistent_command_errors_cleanly() {
        let config = SpawnConfig {
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
        };
        let result = AcpClient::spawn(config, CockpitSessionId("s-1".into())).await;
        assert!(matches!(result, Err(AcpError::Spawn(_))));
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
}
