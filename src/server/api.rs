//! REST API handlers for session management, groups, profiles, and agents.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::session::{Instance, Status, Storage};

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
            Json(
                serde_json::to_value(SessionResponse::from(inst))
                    .expect("SessionResponse is always serializable"),
            ),
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
    if state.read_only {
        return (
            StatusCode::FORBIDDEN,
            Json(
                serde_json::json!({"error": "read_only", "message": "Server is in read-only mode"}),
            ),
        )
            .into_response();
    }

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
    if state.read_only {
        return (
            StatusCode::FORBIDDEN,
            Json(
                serde_json::json!({"error": "read_only", "message": "Server is in read-only mode"}),
            ),
        )
            .into_response();
    }

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

// --- Create session ---

#[derive(Deserialize)]
pub struct CreateSessionBody {
    pub title: Option<String>,
    pub path: String,
    pub tool: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub yolo_mode: bool,
    pub worktree_branch: Option<String>,
    #[serde(default)]
    pub create_new_branch: bool,
    #[serde(default)]
    pub sandbox: bool,
    #[serde(default)]
    pub extra_args: String,
}

pub async fn create_session(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateSessionBody>,
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

    let profile = state.profile.clone();
    let instances = state.instances.read().await;
    let existing_titles: Vec<String> = instances.iter().map(|i| i.title.clone()).collect();
    drop(instances);

    let result = tokio::task::spawn_blocking(move || {
        use crate::session::builder::{self, InstanceParams};
        use crate::session::Config;

        let config = Config::load().unwrap_or_default();
        let sandbox_image = if config.sandbox.default_image.is_empty() {
            "ubuntu:latest".to_string()
        } else {
            config.sandbox.default_image.clone()
        };

        let title_refs: Vec<&str> = existing_titles.iter().map(|s| s.as_str()).collect();
        let params = InstanceParams {
            title: body.title.unwrap_or_default(),
            path: body.path,
            group: body.group,
            tool: body.tool,
            worktree_branch: body.worktree_branch,
            create_new_branch: body.create_new_branch,
            sandbox: body.sandbox,
            sandbox_image,
            yolo_mode: body.yolo_mode,
            extra_env: vec![],
            extra_args: body.extra_args,
            command_override: String::new(),
            extra_repo_paths: vec![],
        };

        let build_result = builder::build_instance(params, &title_refs, &profile)?;
        let mut instance = build_result.instance;

        // Save to disk
        let storage = Storage::new(&profile)?;
        let mut all = storage.load().unwrap_or_default();
        all.push(instance.clone());
        storage.save(&all)?;

        // Start the session
        instance.start()?;

        Ok::<Instance, anyhow::Error>(instance)
    })
    .await;

    match result {
        Ok(Ok(instance)) => {
            let resp = SessionResponse::from(&instance);
            // Update in-memory cache
            let mut instances = state.instances.write().await;
            instances.push(instance);
            (
                StatusCode::CREATED,
                Json(serde_json::to_value(resp).expect("SessionResponse is always serializable")),
            )
                .into_response()
        }
        Ok(Err(e)) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "create_failed", "message": e.to_string()})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal", "message": e.to_string()})),
        )
            .into_response(),
    }
}

// --- Delete session ---

#[derive(Deserialize)]
pub struct DeleteOptions {
    #[serde(default)]
    pub delete_worktree: bool,
    #[serde(default)]
    pub delete_branch: bool,
    #[serde(default)]
    pub delete_sandbox: bool,
}

pub async fn delete_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    body: Option<Json<DeleteOptions>>,
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

    let mut instances = state.instances.write().await;
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

    let profile = inst.source_profile.clone();
    let _opts = body.map(|b| b.0);

    // Remove from in-memory cache
    instances.retain(|i| i.id != id);
    let remaining: Vec<Instance> = instances
        .iter()
        .filter(|i| i.source_profile == profile)
        .cloned()
        .collect();
    drop(instances);

    // Persist removal to disk
    let result = tokio::task::spawn_blocking(move || {
        if let Ok(storage) = Storage::new(&profile) {
            storage.save(&remaining)?;
        }
        // Kill tmux session
        let _ = inst.stop();
        Ok::<_, anyhow::Error>(())
    })
    .await;

    match result {
        Ok(Ok(())) => (StatusCode::OK, Json(serde_json::json!({"deleted": true}))).into_response(),
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "delete_failed", "message": e.to_string()})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal", "message": e.to_string()})),
        )
            .into_response(),
    }
}

// --- Update (rename/move) session ---

#[derive(Deserialize)]
pub struct UpdateSessionBody {
    pub title: Option<String>,
    pub group_path: Option<String>,
}

pub async fn update_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<UpdateSessionBody>,
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

    let mut instances = state.instances.write().await;
    let found = instances.iter().any(|i| i.id == id);
    if !found {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "not_found", "message": "Session not found"})),
        )
            .into_response();
    }

    // Apply updates
    for inst in instances.iter_mut() {
        if inst.id == id {
            if let Some(title) = &body.title {
                inst.title = title.clone();
            }
            if let Some(group) = &body.group_path {
                inst.group_path = group.clone();
            }
        }
    }

    let inst = instances
        .iter()
        .find(|i| i.id == id)
        .expect("instance must exist after any() check above");
    let profile = inst.source_profile.clone();
    let resp = SessionResponse::from(inst);
    let all_for_profile: Vec<Instance> = instances
        .iter()
        .filter(|i| i.source_profile == profile)
        .cloned()
        .collect();
    drop(instances);

    // Persist to disk
    let _ = tokio::task::spawn_blocking(move || {
        if let Ok(storage) = Storage::new(&profile) {
            let _ = storage.save(&all_for_profile);
        }
    })
    .await;

    (
        StatusCode::OK,
        Json(serde_json::to_value(resp).expect("SessionResponse is always serializable")),
    )
        .into_response()
}

// --- Diff ---

#[derive(Serialize)]
pub struct DiffResponse {
    pub files: Vec<DiffFileInfo>,
    pub raw: String,
}

#[derive(Serialize)]
pub struct DiffFileInfo {
    pub path: String,
    pub status: String,
}

pub async fn session_diff(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let instances = state.instances.read().await;
    let project_path = match instances.iter().find(|i| i.id == id) {
        Some(i) => i.project_path.clone(),
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "not_found", "message": "Session not found"})),
            )
                .into_response();
        }
    };
    drop(instances);

    let result = tokio::task::spawn_blocking(move || {
        let output = std::process::Command::new("git")
            .args(["diff", "HEAD"])
            .current_dir(&project_path)
            .output()?;
        let raw = String::from_utf8_lossy(&output.stdout).to_string();

        // Get changed file list
        let status_output = std::process::Command::new("git")
            .args(["diff", "HEAD", "--name-status"])
            .current_dir(&project_path)
            .output()?;
        let files: Vec<DiffFileInfo> = String::from_utf8_lossy(&status_output.stdout)
            .lines()
            .filter_map(|line| {
                let parts: Vec<&str> = line.splitn(2, '\t').collect();
                if parts.len() == 2 {
                    Some(DiffFileInfo {
                        status: parts[0].to_string(),
                        path: parts[1].to_string(),
                    })
                } else {
                    None
                }
            })
            .collect();

        Ok::<_, anyhow::Error>(DiffResponse { files, raw })
    })
    .await;

    match result {
        Ok(Ok(diff)) => (
            StatusCode::OK,
            Json(serde_json::to_value(diff).expect("DiffResponse is always serializable")),
        )
            .into_response(),
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "diff_failed", "message": e.to_string()})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal", "message": e.to_string()})),
        )
            .into_response(),
    }
}

// --- Agents ---

#[derive(Serialize)]
pub struct AgentInfo {
    pub name: String,
    pub binary: String,
}

pub async fn list_agents() -> Json<Vec<AgentInfo>> {
    let agents: Vec<AgentInfo> = crate::agents::AGENTS
        .iter()
        .map(|a| AgentInfo {
            name: a.name.to_string(),
            binary: a.binary.to_string(),
        })
        .collect();
    Json(agents)
}

// --- Groups ---

#[derive(Serialize)]
pub struct GroupInfo {
    pub path: String,
    pub session_count: usize,
}

pub async fn list_groups(State(state): State<Arc<AppState>>) -> Json<Vec<GroupInfo>> {
    let instances = state.instances.read().await;
    let mut groups: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

    for inst in instances.iter() {
        if !inst.group_path.is_empty() {
            *groups.entry(inst.group_path.clone()).or_default() += 1;
            // Also count parent paths so hierarchy is visible
            let parts: Vec<&str> = inst.group_path.split('/').collect();
            for i in 1..parts.len() {
                let parent = parts[..i].join("/");
                groups.entry(parent).or_default();
            }
        }
    }

    let mut result: Vec<GroupInfo> = groups
        .into_iter()
        .map(|(path, session_count)| GroupInfo {
            path,
            session_count,
        })
        .collect();
    result.sort_by(|a, b| a.path.cmp(&b.path));
    Json(result)
}

// --- Profiles ---

pub async fn list_profiles() -> impl IntoResponse {
    match crate::session::list_profiles() {
        Ok(profiles) => (StatusCode::OK, Json(serde_json::json!(profiles))).into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "list_failed", "message": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct CreateProfileBody {
    pub name: String,
}

pub async fn create_profile(Json(body): Json<CreateProfileBody>) -> impl IntoResponse {
    match crate::session::create_profile(&body.name) {
        Ok(()) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"created": true, "name": body.name})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "create_failed", "message": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn delete_profile(Path(name): Path<String>) -> impl IntoResponse {
    match crate::session::delete_profile(&name) {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({"deleted": true}))).into_response(),
        Err(e) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "delete_failed", "message": e.to_string()})),
        )
            .into_response(),
    }
}

// --- Settings ---

pub async fn get_settings() -> impl IntoResponse {
    match crate::session::Config::load() {
        Ok(config) => match serde_json::to_value(&config) {
            Ok(val) => (StatusCode::OK, Json(val)).into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "serialize_failed", "message": e.to_string()})),
            )
                .into_response(),
        },
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "load_failed", "message": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn update_settings(Json(body): Json<serde_json::Value>) -> impl IntoResponse {
    // Load current config, merge updates, save
    let result = tokio::task::spawn_blocking(move || {
        let mut config = crate::session::Config::load().unwrap_or_default();

        // Merge the incoming JSON into the existing config
        let mut current = serde_json::to_value(&config)?;
        if let (Some(current_obj), Some(update_obj)) = (current.as_object_mut(), body.as_object()) {
            for (key, value) in update_obj {
                current_obj.insert(key.clone(), value.clone());
            }
        }
        config = serde_json::from_value(current)?;
        crate::session::save_config(&config)?;
        Ok::<_, anyhow::Error>(config)
    })
    .await;

    match result {
        Ok(Ok(config)) => match serde_json::to_value(&config) {
            Ok(val) => (StatusCode::OK, Json(val)).into_response(),
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "serialize_failed", "message": e.to_string()})),
            )
                .into_response(),
        },
        Ok(Err(e)) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "update_failed", "message": e.to_string()})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal", "message": e.to_string()})),
        )
            .into_response(),
    }
}

// --- Themes ---

pub async fn list_themes() -> Json<Vec<String>> {
    Json(
        crate::tui::styles::available_themes()
            .into_iter()
            .map(|s| s.to_string())
            .collect(),
    )
}

// --- Worktrees ---

#[derive(Serialize)]
pub struct WorktreeInfo {
    pub session_id: String,
    pub session_title: String,
    pub branch: String,
    pub main_repo_path: String,
    pub managed_by_aoe: bool,
}

pub async fn list_worktrees(State(state): State<Arc<AppState>>) -> Json<Vec<WorktreeInfo>> {
    let instances = state.instances.read().await;
    let worktrees: Vec<WorktreeInfo> = instances
        .iter()
        .filter_map(|inst| {
            inst.worktree_info.as_ref().map(|wt| WorktreeInfo {
                session_id: inst.id.clone(),
                session_title: inst.title.clone(),
                branch: wt.branch.clone(),
                main_repo_path: wt.main_repo_path.clone(),
                managed_by_aoe: wt.managed_by_aoe,
            })
        })
        .collect();
    Json(worktrees)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_instance() -> Instance {
        let mut inst = Instance::new("test-session", "/tmp/test-project");
        inst.tool = "claude".to_string();
        inst.status = Status::Running;
        inst.group_path = "work/projects".to_string();
        inst
    }

    #[test]
    fn session_response_from_instance() {
        let inst = make_test_instance();
        let resp = SessionResponse::from(&inst);

        assert_eq!(resp.id, inst.id);
        assert_eq!(resp.title, "test-session");
        assert_eq!(resp.project_path, "/tmp/test-project");
        assert_eq!(resp.tool, "claude");
        assert_eq!(resp.status, "Running");
        assert_eq!(resp.group_path, "work/projects");
        assert!(!resp.is_sandboxed);
        assert!(!resp.has_terminal);
    }

    #[test]
    fn session_response_status_variants() {
        let mut inst = make_test_instance();

        for (status, expected) in [
            (Status::Running, "Running"),
            (Status::Waiting, "Waiting"),
            (Status::Error, "Error"),
            (Status::Stopped, "Stopped"),
            (Status::Idle, "Idle"),
            (Status::Starting, "Starting"),
        ] {
            inst.status = status;
            assert_eq!(SessionResponse::from(&inst).status, expected);
        }
    }

    #[test]
    fn session_response_branch_from_worktree() {
        let mut inst = make_test_instance();
        assert!(SessionResponse::from(&inst).branch.is_none());

        inst.worktree_info = Some(crate::session::WorktreeInfo {
            branch: "feature/test".to_string(),
            main_repo_path: "/tmp/repo".to_string(),
            managed_by_aoe: true,
            created_at: chrono::Utc::now(),
        });
        assert_eq!(
            SessionResponse::from(&inst).branch.as_deref(),
            Some("feature/test")
        );
    }

    #[test]
    fn session_response_serializes_to_json() {
        let inst = make_test_instance();
        let json = serde_json::to_value(SessionResponse::from(&inst)).unwrap();

        assert!(json.get("id").is_some());
        assert_eq!(json["tool"], "claude");
        assert_eq!(json["status"], "Running");
        assert_eq!(json["is_sandboxed"], false);
    }
}
