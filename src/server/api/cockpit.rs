//! REST endpoints for cockpit sessions.
//!
//! Spawn / shutdown / send-prompt / resolve-approval. The cockpit
//! WebSocket carries the read side; this module is the write side.

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::cockpit::approvals::{ApprovalDecision, Nonce};
use crate::cockpit::supervisor::SupervisorError;
use crate::server::AppState;

#[derive(Debug, Deserialize)]
pub struct SpawnCockpitRequest {
    /// Optional override; falls back to the cockpit_default_agent
    /// setting / aoe-agent.
    pub agent: Option<String>,
    /// Optional model override; forwarded to aoe-agent as
    /// AOE_AGENT_MODEL env var.
    pub model: Option<String>,
    /// Optional additional dirs the agent may read/write through
    /// fs/*. The session's worktree is always allowed.
    #[serde(default)]
    pub additional_dirs: Vec<PathBuf>,
    /// Provider env vars to forward (e.g., ANTHROPIC_API_KEY). Will be
    /// filtered against the agent's allowlist.
    #[serde(default)]
    pub provider_env: Vec<EnvPair>,
}

#[derive(Debug, Deserialize)]
pub struct EnvPair {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Serialize)]
pub struct SpawnCockpitResponse {
    pub session_id: String,
    pub agent: String,
    pub status: &'static str,
}

pub async fn spawn_cockpit(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<SpawnCockpitRequest>,
) -> impl IntoResponse {
    if !state.cockpit_master_enabled {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "cockpit is disabled (config.toml `cockpit.enabled = false`); \
             toggle the field and restart `aoe serve` to use",
        )
            .into_response();
    }
    let instances = state.instances.read().await;
    let Some(instance) = instances.iter().find(|i| i.id == id).cloned() else {
        return (StatusCode::NOT_FOUND, "session not found").into_response();
    };
    drop(instances);

    // Pick the cockpit agent: explicit request override > stored
    // cockpit_agent on the instance > registry entry keyed on the
    // tool name (so tool="opencode" → opencode-acp, etc).
    let explicit = req.agent.clone().or_else(|| instance.cockpit_agent.clone());
    let agent = state
        .cockpit_supervisor
        .pick_agent_for_tool(&instance.tool, explicit.as_deref())
        .await;

    let cwd = PathBuf::from(&instance.project_path);
    let provider_env: Vec<(String, String)> = req
        .provider_env
        .into_iter()
        .map(|p| (p.key, p.value))
        .collect();
    let model = req.model.or_else(|| instance.cockpit_model.clone());

    match state
        .cockpit_supervisor
        .spawn(
            id.clone(),
            &agent,
            cwd,
            req.additional_dirs,
            provider_env,
            model,
        )
        .await
    {
        Ok(()) => Json(SpawnCockpitResponse {
            session_id: id,
            agent,
            status: "running",
        })
        .into_response(),
        Err(SupervisorError::AlreadyRunning(_)) => {
            (StatusCode::CONFLICT, "cockpit already running for session").into_response()
        }
        Err(SupervisorError::UnknownAgent(name)) => (
            StatusCode::BAD_REQUEST,
            format!("unknown cockpit agent: {name}"),
        )
            .into_response(),
        Err(e @ SupervisorError::CapacityFull { .. }) => {
            (StatusCode::SERVICE_UNAVAILABLE, format!("{e}")).into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("spawn failed: {e}"),
        )
            .into_response(),
    }
}

pub async fn shutdown_cockpit(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.cockpit_supervisor.shutdown(&id).await {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(SupervisorError::UnknownSession(_)) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("shutdown failed: {e}"),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct PromptRequest {
    pub text: String,
}

pub async fn cockpit_prompt(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<PromptRequest>,
) -> impl IntoResponse {
    match state.cockpit_supervisor.send_prompt(&id, &req.text).await {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(SupervisorError::UnknownSession(_)) => {
            (StatusCode::NOT_FOUND, "session has no running cockpit").into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("prompt failed: {e}"),
        )
            .into_response(),
    }
}

pub async fn cockpit_cancel(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    match state.cockpit_supervisor.cancel_prompt(&id).await {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(SupervisorError::UnknownSession(_)) => {
            (StatusCode::NOT_FOUND, "session has no running cockpit").into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("cancel failed: {e}"),
        )
            .into_response(),
    }
}

#[derive(Debug, Serialize)]
pub struct FilesResponse {
    pub files: Vec<String>,
    pub truncated: bool,
}

/// List workspace files for the @-mention picker. Walks the session's
/// project_path tree, skipping VCS/build dirs and dot-files at the
/// top level. Capped at 5000 entries.
pub async fn cockpit_files(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let instances = state.instances.read().await;
    let Some(inst) = instances.iter().find(|i| i.id == id).cloned() else {
        return (StatusCode::NOT_FOUND, "session not found").into_response();
    };
    drop(instances);

    let root = std::path::PathBuf::from(&inst.project_path);
    let result = tokio::task::spawn_blocking(move || list_files(&root, 5000)).await;
    match result {
        Ok(Ok((files, truncated))) => Json(FilesResponse { files, truncated }).into_response(),
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("file listing failed: {e}"),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("blocking task failed: {e}"),
        )
            .into_response(),
    }
}

fn list_files(root: &std::path::Path, cap: usize) -> std::io::Result<(Vec<String>, bool)> {
    // Names we never want to recurse into. Top-level only — a deep
    // `node_modules` inside a sub-package would still show up via its
    // parent path which is fine.
    const SKIP_DIRS: &[&str] = &[
        ".git",
        "node_modules",
        "target",
        "dist",
        "build",
        ".next",
        ".venv",
        ".cache",
        ".turbo",
        ".idea",
        ".vscode",
    ];
    let mut out: Vec<String> = Vec::new();
    let mut stack: Vec<std::path::PathBuf> = vec![root.to_path_buf()];
    let mut truncated = false;
    while let Some(dir) = stack.pop() {
        if out.len() >= cap {
            truncated = true;
            break;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with('.') {
                continue;
            }
            if SKIP_DIRS.iter().any(|d| *d == name_str.as_ref()) {
                continue;
            }
            let path = entry.path();
            let ft = match entry.file_type() {
                Ok(t) => t,
                Err(_) => continue,
            };
            if ft.is_dir() {
                stack.push(path);
            } else if ft.is_file() {
                if let Ok(rel) = path.strip_prefix(root) {
                    out.push(rel.to_string_lossy().to_string());
                    if out.len() >= cap {
                        truncated = true;
                        break;
                    }
                }
            }
        }
    }
    out.sort();
    Ok((out, truncated))
}

/* ── Substrate switching: cockpit ↔ tmux ─────────────────────── */

#[derive(Debug, Serialize)]
pub struct SubstrateSwitchResponse {
    pub session_id: String,
    pub cockpit_mode: bool,
}

/// Switch a tmux-mode session to cockpit. Idempotent: a session that
/// is already cockpit-mode returns 200 with no work done.
///
/// History is destroyed in the swap: the tmux scrollback is dropped
/// when the pane is killed; cockpit starts with an empty conversation.
/// The frontend warns the user before calling this endpoint.
pub async fn cockpit_enable(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if !state.cockpit_master_enabled {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "cockpit is disabled (config.toml `cockpit.enabled = false`); \
             toggle the field and restart `aoe serve` to use",
        )
            .into_response();
    }
    if !crate::cockpit::experimental_enabled() {
        return (
            StatusCode::FORBIDDEN,
            "cockpit is experimental; set AOE_EXPERIMENTAL_COCKPIT=1 to enable",
        )
            .into_response();
    }
    let (mut instance, profile) = {
        let instances = state.instances.read().await;
        let Some(inst) = instances.iter().find(|i| i.id == id).cloned() else {
            return (StatusCode::NOT_FOUND, "session not found").into_response();
        };
        let profile = inst.source_profile.clone();
        (inst, profile)
    };

    if instance.cockpit_mode {
        return Json(SubstrateSwitchResponse {
            session_id: id,
            cockpit_mode: true,
        })
        .into_response();
    }

    // Verify the tool has an ACP-capable registry entry. Otherwise
    // there's no agent to spawn and the swap would just produce a
    // dead cockpit. Falls back to "tool not in registry" → 400.
    let agent_name = state
        .cockpit_supervisor
        .pick_agent_for_tool(&instance.tool, instance.cockpit_agent.as_deref())
        .await;
    let registry = state.cockpit_supervisor.registry_snapshot().await;
    if registry.get(&agent_name).is_none() {
        return (
            StatusCode::BAD_REQUEST,
            format!("no cockpit agent registered for tool {:?}", instance.tool),
        )
            .into_response();
    }

    // Tear down the tmux side. Best-effort: a stale tmux name should
    // not block the swap.
    if let Err(e) = instance.kill() {
        tracing::warn!(target: "cockpit.switch", session = %id, "kill tmux failed: {e}");
    }
    instance.cockpit_mode = true;

    // Persist before spawning so a crash mid-swap leaves us in the
    // declared end state, not a half-broken intermediate.
    {
        let mut instances = state.instances.write().await;
        if let Some(slot) = instances.iter_mut().find(|i| i.id == id) {
            *slot = instance.clone();
        }
        if let Ok(storage) = crate::session::Storage::new(&profile) {
            let scoped: Vec<_> = instances
                .iter()
                .filter(|i| i.source_profile == profile)
                .cloned()
                .collect();
            if let Err(e) = storage.save(&scoped) {
                tracing::error!(target: "cockpit.switch", "save after enable: {e}");
            }
        }
    }

    // Spawn the cockpit worker. If this fails the supervisor publishes
    // an AgentStartupError that the UI surfaces as the red banner; we
    // still return 200 because the substrate swap itself succeeded.
    let cwd = std::path::PathBuf::from(&instance.project_path);
    let supervisor = state.cockpit_supervisor.clone();
    let session_id = id.clone();
    let model = instance.cockpit_model.clone();
    tokio::spawn(async move {
        if let Err(e) = supervisor
            .spawn(session_id.clone(), &agent_name, cwd, vec![], vec![], model)
            .await
        {
            let message = format!("Failed to start cockpit agent {agent_name:?}: {e}");
            tracing::warn!(target: "cockpit.switch", session = %session_id, "spawn after enable: {message}");
            supervisor.publish_startup_error(&session_id, message);
        }
    });

    Json(SubstrateSwitchResponse {
        session_id: id,
        cockpit_mode: true,
    })
    .into_response()
}

/// Switch a cockpit session back to tmux. Idempotent: a session that
/// is already tmux-mode returns 200 with no work done.
///
/// History is destroyed in the swap: the cockpit conversation log
/// (still in the broadcast replay buffer) is dropped, and tmux comes
/// back with an empty pane that the agent fills as it runs.
pub async fn cockpit_disable(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let (mut instance, profile) = {
        let instances = state.instances.read().await;
        let Some(inst) = instances.iter().find(|i| i.id == id).cloned() else {
            return (StatusCode::NOT_FOUND, "session not found").into_response();
        };
        let profile = inst.source_profile.clone();
        (inst, profile)
    };

    if !instance.cockpit_mode {
        return Json(SubstrateSwitchResponse {
            session_id: id,
            cockpit_mode: false,
        })
        .into_response();
    }

    // Tear down the cockpit worker. UnknownSession is fine — the
    // supervisor may not have a worker if startup never completed.
    match state.cockpit_supervisor.shutdown(&id).await {
        Ok(()) | Err(SupervisorError::UnknownSession(_)) => {}
        Err(e) => {
            tracing::warn!(target: "cockpit.switch", session = %id, "shutdown cockpit failed: {e}");
        }
    }
    // Drop per-session bookkeeping so a future re-enable starts a
    // fresh conversation (seq counter from 1, empty replay buffer).
    // Without this, the next cockpit_enable's first event would
    // collide on a stale seq with the buffer entry from this
    // conversation, and the client-side dedupe would silently eat it.
    state.cockpit_supervisor.forget_session(&id);
    if let Ok(mut guard) = state.cockpit_replay.lock() {
        guard.remove(&id);
    }
    instance.cockpit_mode = false;

    // Persist + start tmux. start() now no longer short-circuits for
    // cockpit_mode, so it will create a fresh tmux session and run
    // the agent CLI in the pane.
    {
        let mut instances = state.instances.write().await;
        if let Some(slot) = instances.iter_mut().find(|i| i.id == id) {
            *slot = instance.clone();
        }
        if let Ok(storage) = crate::session::Storage::new(&profile) {
            let scoped: Vec<_> = instances
                .iter()
                .filter(|i| i.source_profile == profile)
                .cloned()
                .collect();
            if let Err(e) = storage.save(&scoped) {
                tracing::error!(target: "cockpit.switch", "save after disable: {e}");
            }
        }
    }

    let start_result = tokio::task::spawn_blocking(move || instance.start()).await;
    match start_result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            tracing::warn!(target: "cockpit.switch", session = %id, "tmux start after disable: {e}");
        }
        Err(e) => {
            tracing::error!(target: "cockpit.switch", session = %id, "spawn_blocking failed: {e}");
        }
    }

    Json(SubstrateSwitchResponse {
        session_id: id,
        cockpit_mode: false,
    })
    .into_response()
}

#[derive(Debug, Deserialize)]
pub struct SetModeRequest {
    pub mode_id: String,
}

/// Set the active session mode (Default / Plan / AcceptEdits /
/// BypassPermissions). Sends an ACP `session/set_mode` request.
pub async fn cockpit_set_mode(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<SetModeRequest>,
) -> impl IntoResponse {
    match state.cockpit_supervisor.set_mode(&id, &req.mode_id).await {
        Ok(()) => StatusCode::ACCEPTED.into_response(),
        Err(SupervisorError::UnknownSession(_)) => {
            (StatusCode::NOT_FOUND, "session has no running cockpit").into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("set_mode failed: {e}"),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct ResolveApprovalRequest {
    pub decision: ApprovalDecisionWire,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum ApprovalDecisionWire {
    Allow,
    AllowAlways,
    Deny,
}

impl From<ApprovalDecisionWire> for ApprovalDecision {
    fn from(d: ApprovalDecisionWire) -> Self {
        match d {
            ApprovalDecisionWire::Allow => ApprovalDecision::Allow,
            ApprovalDecisionWire::AllowAlways => ApprovalDecision::AllowAlways,
            ApprovalDecisionWire::Deny => ApprovalDecision::Deny,
        }
    }
}

pub async fn resolve_approval(
    State(state): State<Arc<AppState>>,
    Path((id, nonce_str)): Path<(String, String)>,
    Json(req): Json<ResolveApprovalRequest>,
) -> impl IntoResponse {
    let nonce = Nonce(nonce_str);
    match state
        .cockpit_supervisor
        .resolve_permission(&id, nonce, req.decision.into())
        .await
    {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(SupervisorError::UnknownSession(_)) => {
            (StatusCode::NOT_FOUND, "session has no running cockpit").into_response()
        }
        Err(SupervisorError::Acp(crate::cockpit::acp_client::AcpError::UnknownNonce)) => {
            (StatusCode::NOT_FOUND, "no pending approval with that nonce").into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("resolve failed: {e}"),
        )
            .into_response(),
    }
}

#[derive(Debug, Deserialize)]
pub struct ReplayQuery {
    /// Last seq the client has applied. The endpoint returns frames
    /// strictly newer than this. Defaults to 0 (full replay).
    #[serde(default)]
    pub since: u64,
}

#[derive(Debug, Serialize)]
pub struct ReplayResponse {
    /// Frames the client missed, in publish order. Empty when the
    /// client is already caught up.
    pub frames: Vec<crate::server::CockpitBroadcastFrame>,
    /// True when the requested `since` predates what's still in the
    /// buffer (the client missed events that have since been evicted).
    /// Clients should treat the conversation log as truncated and
    /// request a fresh start, e.g. by reloading.
    pub lost: bool,
    /// Highest seq the buffer has seen, even if it's been evicted.
    /// Lets the client decide whether reloading is worth it.
    pub highest_seq: u64,
}

/// Reconnect/snapshot endpoint. Mobile clients drop their WebSocket
/// briefly any time a screen lock fires; this lets them resync without
/// a full page reload by replaying the buffered frames they missed.
///
/// Gating note: only the standard auth middleware applies — no master-
/// switch check. History is read-only and contains nothing the live
/// channel didn't already broadcast, so flipping `cockpit.enabled` off
/// (which requires a daemon restart and clears the buffers) is the
/// right way to stop history reads, not gating each request.
pub async fn cockpit_replay(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    axum::extract::Query(q): axum::extract::Query<ReplayQuery>,
) -> impl IntoResponse {
    use crate::cockpit::replay_buffer::BufferedEvent;

    let guard = match state.cockpit_replay.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let Some(buf) = guard.get(&id) else {
        return Json(ReplayResponse {
            frames: vec![],
            lost: false,
            highest_seq: 0,
        })
        .into_response();
    };
    let highest_seq = buf.highest_seq();
    let entries = match buf.replay_from(q.since) {
        Some(items) => items,
        None => {
            return Json(ReplayResponse {
                frames: vec![],
                lost: true,
                highest_seq,
            })
            .into_response();
        }
    };
    let frames: Vec<crate::server::CockpitBroadcastFrame> = entries
        .into_iter()
        .filter_map(|item| match item {
            BufferedEvent::Event { seq, event } => Some(crate::server::CockpitBroadcastFrame {
                session_id: id.clone(),
                seq,
                event: serde_json::to_value(&event).unwrap_or(serde_json::Value::Null),
            }),
            // Gap markers are surfaced via the `lost` flag if they
            // block the replay; otherwise (gap older than `since`)
            // they're not interesting to the client.
            BufferedEvent::Gap { .. } => None,
        })
        .collect();
    Json(ReplayResponse {
        frames,
        lost: false,
        highest_seq,
    })
    .into_response()
}
