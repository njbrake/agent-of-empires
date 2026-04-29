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
    let instances = state.instances.read().await;
    let Some(instance) = instances.iter().find(|i| i.id == id).cloned() else {
        return (StatusCode::NOT_FOUND, "session not found").into_response();
    };
    drop(instances);

    let agent = req
        .agent
        .or_else(|| instance.cockpit_agent.clone())
        .unwrap_or_else(|| {
            // Default agent picks: claude-code for "claude", aoe-agent
            // otherwise. Settings-side default is consulted via the
            // existing config when the supervisor is wired into the TUI
            // creation flow. For the REST surface we keep it simple.
            if instance.tool == "claude" {
                "claude-code".into()
            } else {
                "aoe-agent".into()
            }
        });

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
