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

use crate::cockpit::approvals::Nonce;
use crate::cockpit::protocol::{
    ContextPrimerQuery, ContextPrimerResponse, PromptRequest, ReplayQuery, ReplayResponse,
    ResolveApprovalRequest,
};
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

/// 403 helper for `aoe serve --read-only`. Matches the response shape used
/// by `sessions.rs` write endpoints so the read-only contract is uniform
/// across the API surface.
pub(crate) fn read_only_block(state: &AppState) -> Option<axum::response::Response> {
    if state.read_only {
        return Some(
            (
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({
                    "error": "read_only",
                    "message": "Server is in read-only mode",
                })),
            )
                .into_response(),
        );
    }
    None
}

/// Single chokepoint for cockpit-availability checks. The persistent
/// master switch (`cockpit.enabled` in config.toml, toggleable via
/// `PATCH /api/cockpit/master`) must be on for any cockpit-spawning
/// endpoint to succeed.
pub(crate) fn cockpit_gate(state: &AppState) -> Result<(), (StatusCode, &'static str)> {
    if !state
        .cockpit_master_enabled
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            "cockpit is disabled (config.toml `cockpit.enabled = false`); \
             enable it from the web settings or set the field to true",
        ));
    }
    Ok(())
}

pub async fn spawn_cockpit(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<SpawnCockpitRequest>,
) -> impl IntoResponse {
    if let Some(resp) = read_only_block(&state) {
        return resp;
    }
    if let Err(reason) = cockpit_gate(&state) {
        return reason.into_response();
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
    let stored_acp_session_id = instance.cockpit_acp_session_id.clone();
    let yolo_mode = instance.yolo_mode;

    let inst_lock = state.instance_lock(&id).await;
    let sandbox_info = match crate::cockpit::sandbox::ensure_container_for_session(
        &state.instances,
        &inst_lock,
        &id,
        false,
    )
    .await
    {
        Ok(info) => info,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("sandbox container ensure failed: {e}"),
            )
                .into_response();
        }
    };
    let source_profile = sandbox_info
        .as_ref()
        .map(|_| instance.source_profile.clone());
    let agent_for_response = agent.clone();
    match state
        .cockpit_supervisor
        .spawn(crate::cockpit::supervisor::SpawnRequest {
            session_id: id.clone(),
            agent,
            cwd,
            additional_dirs: req.additional_dirs,
            provider_env,
            model,
            stored_acp_session_id,
            sandbox_info,
            source_profile,
            yolo_mode,
        })
        .await
    {
        Ok(()) => Json(SpawnCockpitResponse {
            session_id: id,
            agent: agent_for_response,
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
    if let Some(resp) = read_only_block(&state) {
        return resp;
    }
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

pub async fn cockpit_prompt(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(req): Json<PromptRequest>,
) -> impl IntoResponse {
    if let Some(resp) = read_only_block(&state) {
        return resp;
    }
    // Publish the user's prompt into the event stream BEFORE forwarding
    // to the agent so the replay buffer / on-disk store captures it
    // even if the agent forward fails. The frontend treats UserPromptSent
    // as authoritative and dedupes against its own optimistic row.
    state
        .cockpit_supervisor
        .publish_user_prompt(&id, req.text.clone())
        .await;
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
    if let Some(resp) = read_only_block(&state) {
        return resp;
    }
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

/// Escape hatch for the "stuck spinner" failure mode (#1100). Publishes
/// a synthetic `Stopped { reason: "user_forced" }` so every connected UI
/// drops `turnActive`, then best-effort cancels any in-flight agent
/// turn. Always 202: the publish is idempotent and the cancel is
/// fire-and-forget; any genuine read-only mode is rejected upstream.
pub async fn cockpit_force_end_turn(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if let Some(resp) = read_only_block(&state) {
        return resp;
    }
    state.cockpit_supervisor.force_end_turn(&id).await;
    StatusCode::ACCEPTED.into_response()
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
    if let Some(resp) = read_only_block(&state) {
        return resp;
    }
    if let Err(reason) = cockpit_gate(&state) {
        return reason.into_response();
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
    //
    // The on-disk and in-memory updates mutate ONLY the cockpit-specific
    // field (`cockpit_mode = true`). Wholesale replacement with a
    // pre-lock snapshot would clobber concurrent writes to other
    // fields (status, last_accessed, agent_session_id) made by the
    // status poll loop or other handlers between the snapshot and the
    // lock acquisition.
    {
        let mut instances = state.instances.write().await;
        if let Some(slot) = instances.iter_mut().find(|i| i.id == id) {
            slot.cockpit_mode = true;
        }
    }
    let id_for_save = id.clone();
    let profile_for_save = profile.clone();
    let save_result = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let storage = crate::session::Storage::new(&profile_for_save)?;
        storage.update(|all, _groups| {
            if let Some(slot) = all.iter_mut().find(|i| i.id == id_for_save) {
                slot.cockpit_mode = true;
            }
            Ok(())
        })?;
        Ok(())
    })
    .await;
    match save_result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            tracing::error!(target: "cockpit.switch", "save after enable: {e}");
        }
        Err(join_err) => {
            tracing::error!(target: "cockpit.switch", "save task panicked after enable: {join_err}");
        }
    }

    // Spawn the cockpit worker. If this fails the supervisor publishes
    // an AgentStartupError that the UI surfaces as the red banner; we
    // still return 200 because the substrate swap itself succeeded.
    // Container ensure runs inside the spawned task so the HTTP
    // response isn't held open through a docker pull/create.
    let cwd = std::path::PathBuf::from(&instance.project_path);
    let supervisor = state.cockpit_supervisor.clone();
    let session_id = id.clone();
    let model = instance.cockpit_model.clone();
    let stored_acp_session_id = instance.cockpit_acp_session_id.clone();
    let yolo_mode = instance.yolo_mode;
    let profile_for_spawn = profile.clone();
    let state_for_spawn = state.clone();
    tokio::spawn(async move {
        let inst_lock = state_for_spawn.instance_lock(&session_id).await;
        let sandbox_info = match crate::cockpit::sandbox::ensure_container_for_session(
            &state_for_spawn.instances,
            &inst_lock,
            &session_id,
            false,
        )
        .await
        {
            Ok(info) => info,
            Err(e) => {
                let message = format!("container start failed: {e}");
                tracing::warn!(target: "cockpit.switch", session = %session_id, "container ensure failed: {e}");
                supervisor.publish_startup_error(&session_id, message);
                return;
            }
        };
        let source_profile = sandbox_info.as_ref().map(|_| profile_for_spawn);
        if let Err(e) = supervisor
            .spawn(crate::cockpit::supervisor::SpawnRequest {
                session_id: session_id.clone(),
                agent: agent_name.clone(),
                cwd,
                additional_dirs: vec![],
                provider_env: vec![],
                model,
                stored_acp_session_id,
                sandbox_info,
                source_profile,
                yolo_mode,
            })
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
    if let Some(resp) = read_only_block(&state) {
        return resp;
    }
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
    // Drop on-disk history so the next cockpit_enable starts truly
    // fresh — without this, the seq=1 first publish would collide
    // with a row already on disk and INSERT OR IGNORE would silently
    // drop it.
    state.cockpit_event_store.delete_session(&id);
    instance.cockpit_mode = false;
    // Clear the stored ACP session id: the agent's transcript is
    // tied to the cockpit-mode lifecycle. If the user re-enables
    // cockpit later, the agent should start a fresh session/new
    // rather than try to resume an id that's no longer relevant.
    if instance.cockpit_acp_session_id.is_some() {
        tracing::debug!(
            target: "cockpit.switch",
            session = %id,
            "clearing cockpit_acp_session_id on disable"
        );
        instance.cockpit_acp_session_id = None;
    }

    // Persist + start tmux. start() now no longer short-circuits for
    // cockpit_mode, so it will create a fresh tmux session and run
    // the agent CLI in the pane.
    //
    // The on-disk and in-memory updates mutate ONLY the cockpit-specific
    // fields (`cockpit_mode = false`, `cockpit_acp_session_id = None`).
    // Wholesale replacement with a pre-lock snapshot would clobber
    // concurrent writes to other fields made by the status poll loop or
    // other handlers between the snapshot and the lock acquisition.
    {
        let mut instances = state.instances.write().await;
        if let Some(slot) = instances.iter_mut().find(|i| i.id == id) {
            slot.cockpit_mode = false;
            slot.cockpit_acp_session_id = None;
        }
    }
    let id_for_save = id.clone();
    let profile_for_save = profile.clone();
    let save_result = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let storage = crate::session::Storage::new(&profile_for_save)?;
        storage.update(|all, _groups| {
            if let Some(slot) = all.iter_mut().find(|i| i.id == id_for_save) {
                slot.cockpit_mode = false;
                slot.cockpit_acp_session_id = None;
            }
            Ok(())
        })?;
        Ok(())
    })
    .await;
    match save_result {
        Ok(Ok(())) => {}
        Ok(Err(e)) => {
            tracing::error!(target: "cockpit.switch", "save after disable: {e}");
        }
        Err(join_err) => {
            tracing::error!(target: "cockpit.switch", "save task panicked after disable: {join_err}");
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
    if let Some(resp) = read_only_block(&state) {
        return resp;
    }
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

pub async fn resolve_approval(
    State(state): State<Arc<AppState>>,
    Path((id, nonce_str)): Path<(String, String)>,
    Json(req): Json<ResolveApprovalRequest>,
) -> impl IntoResponse {
    if let Some(resp) = read_only_block(&state) {
        return resp;
    }
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

/// Build a markdown context primer from the persisted cockpit event
/// log. Used after a `session/load` failure: the agent's model
/// context is empty, but the visible transcript is intact in SQLite,
/// so the user can opt in to sending a compact recap as their next
/// prompt. See #1004.
pub async fn cockpit_context_primer(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    axum::extract::Query(q): axum::extract::Query<ContextPrimerQuery>,
) -> impl IntoResponse {
    let events = state.cockpit_event_store.replay_before(&id, q.before_seq);
    let primer = crate::cockpit::context_primer::build_context_primer(
        &events,
        crate::cockpit::context_primer::PrimerOptions {
            before_seq: Some(q.before_seq),
            ..Default::default()
        },
    );
    Json(ContextPrimerResponse {
        primer: primer.text,
        included_event_count: primer.included_event_count,
        included_turn_count: primer.included_turn_count,
        truncated: primer.truncated,
        max_chars: primer.max_chars,
    })
    .into_response()
}

/// Reconnect/snapshot endpoint. Mobile clients drop their WebSocket
/// briefly any time a screen lock fires; this lets them resync without
/// a full page reload by replaying the buffered frames they missed.
///
/// Gating note: only the standard auth middleware applies, no master-
/// switch check. History is read-only and contains nothing the live
/// channel didn't already broadcast, so flipping `cockpit.enabled` off
/// (which requires a daemon restart and clears the buffers) is the
/// right way to stop history reads, not gating each request.
pub async fn cockpit_replay(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    axum::extract::Query(q): axum::extract::Query<ReplayQuery>,
) -> impl IntoResponse {
    // Reads from the disk-backed event store so reload, session-switch,
    // and `aoe serve` restart all reconstruct the full conversation
    // (subject to the per-session retention cap). The in-memory replay
    // buffer is still consulted on WS connect for the hot path; this
    // endpoint backstops that when the in-memory ring is cold (server
    // just restarted) or the client lagged far enough to need older
    // events than the ring holds.
    let highest_seq = state.cockpit_event_store.highest_seq(&id);
    let lowest_seq = state.cockpit_event_store.lowest_seq(&id);
    let entries = state.cockpit_event_store.replay_from(&id, q.since);
    let frames: Vec<crate::server::CockpitBroadcastFrame> = entries
        .into_iter()
        .map(|(seq, event)| crate::server::CockpitBroadcastFrame {
            session_id: id.clone(),
            seq,
            event: Arc::new(event),
        })
        .collect();
    // `lost = true` when the client's `since` cursor predates the oldest
    // seq still on disk. The retention cap can evict older events, so a
    // client that returns after a long absence may legitimately need a
    // full reload. With no events on disk yet, nothing is lost.
    let lost = match lowest_seq {
        Some(lo) => q.since < lo.saturating_sub(1),
        None => false,
    };
    Json(ReplayResponse {
        frames,
        lost,
        highest_seq,
        lowest_seq,
    })
    .into_response()
}

#[derive(Debug, Deserialize)]
pub struct SetMasterRequest {
    pub enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct MasterStateResponse {
    pub master_enabled: bool,
}

/// Toggle `config.cockpit.enabled` from the web UI. Persists to
/// `config.toml` and updates the live atomic so the reconciler and
/// gating endpoints pick up the new value without a server restart.
pub async fn set_cockpit_master(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SetMasterRequest>,
) -> impl IntoResponse {
    if state.read_only {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "read_only",
                "message": "Server is in read-only mode",
            })),
        )
            .into_response();
    }
    let new_value = req.enabled;
    // The atomic is the live source of truth — the reconciler and
    // every gating REST handler reads it. Flip it FIRST so an
    // in-flight `cockpit_enable` arriving in the disk-write window
    // sees the declared end state, not the previous one. If the
    // disk write fails we restore the previous atomic value.
    let prev = state
        .cockpit_master_enabled
        .swap(new_value, std::sync::atomic::Ordering::Relaxed);
    let result = tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        let mut config = crate::session::Config::load_or_warn();
        config.cockpit.enabled = new_value;
        crate::session::save_config(&config)?;
        Ok(())
    })
    .await;
    match result {
        Ok(Ok(())) => (
            StatusCode::OK,
            Json(MasterStateResponse {
                master_enabled: new_value,
            }),
        )
            .into_response(),
        Ok(Err(e)) => {
            // Persist failed: roll the atomic back so the live state
            // matches what's actually on disk. A subsequent gating
            // call won't be misled by the in-memory value.
            state
                .cockpit_master_enabled
                .store(prev, std::sync::atomic::Ordering::Relaxed);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "save_failed",
                    "message": e.to_string(),
                })),
            )
                .into_response()
        }
        Err(e) => {
            state
                .cockpit_master_enabled
                .store(prev, std::sync::atomic::Ordering::Relaxed);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "internal",
                    "message": e.to_string(),
                })),
            )
                .into_response()
        }
    }
}
