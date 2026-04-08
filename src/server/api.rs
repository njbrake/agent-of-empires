//! REST API handlers for session management.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Serialize;

use crate::session::{Instance, Status};

use super::AppState;

/// API response DTO for session data.
/// Decouples the API contract from the internal Instance struct.
#[derive(Serialize)]
pub struct SessionResponse {
    pub id: String,
    pub title: String,
    pub project_path: String,
    pub group_path: String,
    pub tool: String,
    pub status: String,
    pub yolo_mode: bool,
    pub created_at: String,
    pub last_accessed_at: Option<String>,
    pub last_error: Option<String>,
    pub branch: Option<String>,
    pub is_sandboxed: bool,
    pub has_terminal: bool,
}

impl From<&Instance> for SessionResponse {
    fn from(inst: &Instance) -> Self {
        Self {
            id: inst.id.clone(),
            title: inst.title.clone(),
            project_path: inst.project_path.clone(),
            group_path: inst.group_path.clone(),
            tool: inst.tool.clone(),
            status: format!("{:?}", inst.status),
            yolo_mode: inst.yolo_mode,
            created_at: inst.created_at.to_rfc3339(),
            last_accessed_at: inst.last_accessed_at.map(|t| t.to_rfc3339()),
            last_error: inst.last_error.clone(),
            branch: inst.worktree_info.as_ref().map(|w| w.branch.clone()),
            is_sandboxed: inst.is_sandboxed(),
            has_terminal: inst.terminal_info.is_some(),
        }
    }
}

pub async fn list_sessions(State(state): State<Arc<AppState>>) -> Json<Vec<SessionResponse>> {
    let instances = state.instances.read().await;
    let sessions: Vec<SessionResponse> = instances.iter().map(SessionResponse::from).collect();
    Json(sessions)
}

pub async fn get_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let instances = state.instances.read().await;
    match instances.iter().find(|i| i.id == id) {
        Some(inst) => (
            StatusCode::OK,
            Json(serde_json::to_value(SessionResponse::from(inst)).unwrap()),
        )
            .into_response(),
        None => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "not_found", "message": "Session not found"})),
        )
            .into_response(),
    }
}

pub async fn stop_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let instances = state.instances.read().await;
    let inst = match instances.iter().find(|i| i.id == id) {
        Some(i) => i.clone(),
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "not_found", "message": "Session not found"})),
            )
                .into_response();
        }
    };
    drop(instances);

    // Run the blocking stop operation in a dedicated thread
    let result = tokio::task::spawn_blocking(move || inst.stop()).await;

    match result {
        Ok(Ok(())) => {
            // Update status in our cache
            let mut instances = state.instances.write().await;
            if let Some(inst) = instances.iter_mut().find(|i| i.id == id) {
                inst.status = Status::Stopped;
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({"status": "stopped"})),
            )
                .into_response()
        }
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "stop_failed", "message": e.to_string()})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal", "message": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn restart_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let mut instances = state.instances.write().await;
    let inst = match instances.iter_mut().find(|i| i.id == id) {
        Some(i) => i,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "not_found", "message": "Session not found"})),
            )
                .into_response();
        }
    };

    let mut inst_clone = inst.clone();
    drop(instances);

    let result = tokio::task::spawn_blocking(move || inst_clone.start()).await;

    match result {
        Ok(Ok(())) => {
            let mut instances = state.instances.write().await;
            if let Some(inst) = instances.iter_mut().find(|i| i.id == id) {
                inst.status = Status::Starting;
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({"status": "starting"})),
            )
                .into_response()
        }
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "restart_failed", "message": e.to_string()})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal", "message": e.to_string()})),
        )
            .into_response(),
    }
}
