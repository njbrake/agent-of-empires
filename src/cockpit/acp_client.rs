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
//!
//! ## What this slice does
//!
//! - Spawns a real ACP agent subprocess.
//! - Initialises the protocol and creates one session.
//! - Sends prompts via `send_prompt`.
//! - Forwards inbound `session/update` notifications to the cockpit's
//!   typed `Event` channel.
//! - Permission requests are auto-approved (yolo-style) for now. The
//!   responder side-channel that lets the cockpit UI gate approvals
//!   lands in the next slice.

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use agent_client_protocol::schema::{
    ContentBlock, InitializeRequest, NewSessionRequest, ProtocolVersion, PromptRequest,
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome, SessionNotification, TextContent,
};
use agent_client_protocol::{Agent, ByteStreams, Client, ConnectionTo, Responder};
use thiserror::Error;
use tokio::sync::{mpsc, Mutex};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::{error, info, warn};

use super::agent_registry::AgentSpec;
use super::approvals::{ApprovalDecision, Nonce};
use super::state::{CockpitSessionId, Event};

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
}

/// Configuration for spawning an ACP agent.
#[derive(Debug, Clone)]
pub struct SpawnConfig {
    pub spec: AgentSpec,
    pub cwd: PathBuf,
    pub additional_dirs: Vec<PathBuf>,
    /// Provider env vars to forward (after applying the agent's allowlist).
    pub provider_env: Vec<(String, String)>,
}

/// Commands sent from `AcpClient` methods to the background connection task.
enum ClientCmd {
    Prompt(String),
    Shutdown,
}

/// Top-level ACP client. Owns the subprocess lifetime and pumps events
/// from the connection task.
pub struct AcpClient {
    pub session_id: CockpitSessionId,
    inbound: mpsc::Receiver<Event>,
    cmd_tx: Option<mpsc::Sender<ClientCmd>>,
    /// Hold the subprocess so it gets killed when the client is dropped.
    /// `Arc<Mutex<...>>` so the spawn task can also reach it (to log
    /// exit) without taking ownership away from the client.
    _child: Option<Arc<Mutex<tokio::process::Child>>>,
}

impl AcpClient {
    /// Construct a client that does not actually spawn anything. Useful
    /// for unit tests of cockpit state without a real agent.
    pub fn fake_for_test(session_id: CockpitSessionId) -> (Self, mpsc::Sender<Event>) {
        let (event_tx, event_rx) = mpsc::channel(64);
        let client = Self {
            session_id,
            inbound: event_rx,
            cmd_tx: None,
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

        let child = spawn_subprocess(&config)?;
        let child = Arc::new(Mutex::new(child));

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
        let cwd = config.cwd.clone();
        let session_label = session_id.0.clone();
        let child_for_task = child.clone();

        tokio::spawn(run_connection_task(
            transport,
            event_tx,
            cmd_rx,
            cwd,
            session_label,
            child_for_task,
        ));

        Ok(Self {
            session_id,
            inbound: event_rx,
            cmd_tx: Some(cmd_tx),
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

    /// Resolve a pending permission request. Stub for now; the responder
    /// side-channel that wires this into the connection task lands in the
    /// next slice.
    pub async fn resolve_permission(
        &self,
        _nonce: Nonce,
        _decision: ApprovalDecision,
        _message: Option<String>,
    ) -> Result<(), AcpError> {
        Err(AcpError::Protocol(
            "resolve_permission not yet wired (auto-approved in this slice)".into(),
        ))
    }

    /// Shutdown the connection task and kill the subprocess.
    pub async fn shutdown(&self) -> Result<(), AcpError> {
        let cmd_tx = self.cmd_tx.as_ref().ok_or(AcpError::NotRunning)?;
        let _ = cmd_tx.send(ClientCmd::Shutdown).await;
        Ok(())
    }

    /// Drain the next event the agent emitted.
    pub async fn next_event(&mut self) -> Option<Event> {
        self.inbound.recv().await
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
    let always_forward = ["PATH", "HOME", "LANG", "LC_ALL", "TERM", "USER"];
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

    cmd.spawn().map_err(|e| AcpError::Spawn(e.to_string()))
}

async fn run_connection_task(
    transport: ByteStreams<
        tokio_util::compat::Compat<tokio::process::ChildStdin>,
        tokio_util::compat::Compat<tokio::process::ChildStdout>,
    >,
    event_tx: mpsc::Sender<Event>,
    cmd_rx: mpsc::Receiver<ClientCmd>,
    cwd: PathBuf,
    session_label: String,
    child: Arc<Mutex<tokio::process::Child>>,
) {
    let event_tx_for_notif = event_tx.clone();
    let event_tx_for_perm = event_tx.clone();
    let cmd_rx = Arc::new(Mutex::new(cmd_rx));

    let result = Client
        .builder()
        .name("aoe-cockpit")
        .on_receive_notification(
            move |notification: SessionNotification, _cx| {
                let event_tx = event_tx_for_notif.clone();
                async move {
                    let payload = serde_json::to_value(&notification.update)
                        .unwrap_or(serde_json::Value::Null);
                    let _ = event_tx.send(Event::RawAgentUpdate { payload }).await;
                    Ok(())
                }
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            move |request: RequestPermissionRequest,
                  responder: Responder<RequestPermissionResponse>,
                  _conn| {
                // Yolo auto-approve until the responder side-channel lands.
                let event_tx = event_tx_for_perm.clone();
                async move {
                    warn!(
                        target: "cockpit.acp",
                        "permission request auto-approved (responder side-channel not yet wired): {:?}",
                        request.tool_call
                    );
                    // Surface a passthrough event so the UI at least sees that an
                    // approval was requested + auto-resolved.
                    let payload = serde_json::to_value(&request.tool_call)
                        .unwrap_or(serde_json::Value::Null);
                    let _ = event_tx
                        .send(Event::RawAgentUpdate { payload })
                        .await;
                    let option_id = request.options.first().map(|o| o.option_id.clone());
                    if let Some(id) = option_id {
                        responder.respond(RequestPermissionResponse::new(
                            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(id)),
                        ))
                    } else {
                        responder.respond(RequestPermissionResponse::new(
                            RequestPermissionOutcome::Cancelled,
                        ))
                    }
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(transport, |connection: ConnectionTo<Agent>| async move {
            info!(target: "cockpit.acp", session = %session_label, "initializing ACP agent");
            let _init = connection
                .send_request(InitializeRequest::new(ProtocolVersion::V1))
                .block_task()
                .await?;

            info!(target: "cockpit.acp", session = %session_label, "creating ACP session");
            let new_session = connection
                .send_request(NewSessionRequest::new(cwd))
                .block_task()
                .await?;
            let acp_session_id = new_session.session_id;

            // Command loop: pump prompts/cancels from the AcpClient until
            // shutdown.
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
                        let _ = event_tx
                            .send(Event::Stopped {
                                reason: "prompt_complete".into(),
                            })
                            .await;
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

    if let Err(e) = result {
        error!(target: "cockpit.acp", "ACP connection task ended with error: {:?}", e);
    }
    // Clean up the subprocess regardless of how we exited.
    let mut guard = child.lock().await;
    let _ = guard.kill().await;
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
        };
        let result = AcpClient::spawn(config, CockpitSessionId("s-1".into())).await;
        assert!(matches!(result, Err(AcpError::Spawn(_))));
    }
}
