//! Web CRUD for the project registry. Backs the dashboard's Projects page
//! and feeds the session-creation wizard's multi-select picker.

use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::git::GitWorktree;
use crate::session::projects::{self, RegistryError};
use crate::session::{Project, ProjectScope};

use super::AppState;

#[derive(Serialize)]
pub struct ProjectResponse {
    pub name: String,
    pub path: String,
    pub scope: String,
}

impl From<Project> for ProjectResponse {
    fn from(p: Project) -> Self {
        Self {
            name: p.name,
            path: p.path,
            scope: p.scope.as_str().to_string(),
        }
    }
}

#[derive(Deserialize)]
pub struct ListQuery {
    /// Optional scope filter: "global", "profile", or omitted (= all).
    #[serde(default)]
    pub scope: Option<String>,
}

pub async fn list_projects(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ListQuery>,
) -> impl IntoResponse {
    let result: anyhow::Result<Vec<Project>> = match q.scope.as_deref() {
        Some("global") => projects::load_global(),
        Some("profile") => projects::load_profile(&state.profile),
        Some(other) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "bad_scope",
                    "message": format!("Unknown scope '{}'. Use 'global', 'profile', or omit.", other),
                })),
            )
                .into_response();
        }
        None => projects::load_merged(&state.profile),
    };

    match result {
        Ok(list) => Json(
            list.into_iter()
                .map(ProjectResponse::from)
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "load_failed", "message": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct CreateProjectBody {
    pub path: String,
    #[serde(default)]
    pub name: Option<String>,
    /// "global" (default) or "profile".
    #[serde(default)]
    pub scope: Option<String>,
    /// When true, allow registering this path even if it already exists in
    /// the other scope. Defaults to false; cross-scope path collisions
    /// otherwise return 409.
    #[serde(default)]
    pub allow_override: bool,
}

pub async fn create_project(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateProjectBody>,
) -> impl IntoResponse {
    if state.read_only {
        return (
            StatusCode::FORBIDDEN,
            Json(
                serde_json::json!({"error": "read_only", "message": "Server is in read-only mode"}),
            ),
        )
            .into_response();
    }

    let scope = match body.scope.as_deref() {
        Some("profile") => ProjectScope::Profile,
        Some("global") | None => ProjectScope::Global,
        Some(other) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "bad_scope",
                    "message": format!("Unknown scope '{}'. Use 'global' or 'profile'.", other),
                })),
            )
                .into_response();
        }
    };

    let path_buf = std::path::PathBuf::from(&body.path);
    let canonical = path_buf.canonicalize().unwrap_or_else(|_| path_buf.clone());
    if !GitWorktree::is_git_repo(&canonical) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "not_a_git_repo",
                "message": format!("Path is not a git repository: {}", canonical.display()),
            })),
        )
            .into_response();
    }

    let name = body.name.unwrap_or_else(|| {
        canonical
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "project".to_string())
    });

    let project = Project::new(name, canonical.to_string_lossy(), scope);
    match projects::add(&state.profile, scope, project, body.allow_override) {
        Ok(saved) => (StatusCode::CREATED, Json(ProjectResponse::from(saved))).into_response(),
        Err(RegistryError::Conflict(msg)) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "conflict", "message": msg})),
        )
            .into_response(),
        Err(RegistryError::NotFound(msg)) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "not_found", "message": msg})),
        )
            .into_response(),
        Err(RegistryError::Other(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "add_failed", "message": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct DeleteQuery {
    /// "global" (default) or "profile".
    #[serde(default)]
    pub scope: Option<String>,
}

pub async fn delete_project(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
    Query(q): Query<DeleteQuery>,
) -> impl IntoResponse {
    if state.read_only {
        return (
            StatusCode::FORBIDDEN,
            Json(
                serde_json::json!({"error": "read_only", "message": "Server is in read-only mode"}),
            ),
        )
            .into_response();
    }

    let scope = match q.scope.as_deref() {
        Some("profile") => ProjectScope::Profile,
        Some("global") | None => ProjectScope::Global,
        Some(other) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "bad_scope",
                    "message": format!("Unknown scope '{}'. Use 'global' or 'profile'.", other),
                })),
            )
                .into_response();
        }
    };

    match projects::remove(&state.profile, scope, &name) {
        Ok(removed) => (StatusCode::OK, Json(ProjectResponse::from(removed))).into_response(),
        Err(RegistryError::NotFound(msg)) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "not_found", "message": msg})),
        )
            .into_response(),
        Err(RegistryError::Conflict(msg)) => (
            StatusCode::CONFLICT,
            Json(serde_json::json!({"error": "conflict", "message": msg})),
        )
            .into_response(),
        Err(RegistryError::Other(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "remove_failed", "message": e.to_string()})),
        )
            .into_response(),
    }
}
