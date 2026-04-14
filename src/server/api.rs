//! REST API handlers for session management and agents.

use std::sync::Arc;

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::{Deserialize, Serialize};

use crate::session::{Instance, Storage};

#[cfg(test)]
use crate::session::Status;

use super::AppState;

const SHELL_METACHARACTERS: &[char] = &[
    ';', '&', '|', '$', '`', '(', ')', '{', '}', '<', '>', '\n', '\r', '\\', '"', '\'', '!', '#',
    '*', '?', '[', ']', '~', '\t', '\0',
];

fn validate_no_shell_injection(value: &str, field_name: &str) -> Result<(), String> {
    if let Some(c) = value.chars().find(|c| SHELL_METACHARACTERS.contains(c)) {
        return Err(format!(
            "Invalid character '{}' in {}. Shell metacharacters are not allowed.",
            c, field_name
        ));
    }
    Ok(())
}

const ALLOWED_SETTINGS_SECTIONS: &[&str] = &["theme", "session", "tmux", "updates", "sound"];

const SESSION_BLOCKED_FIELDS: &[&str] =
    &["agent_command_override", "agent_extra_args", "extra_env"];

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
    pub main_repo_path: Option<String>,
    pub is_sandboxed: bool,
    pub has_terminal: bool,
    pub profile: String,
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
            main_repo_path: inst
                .worktree_info
                .as_ref()
                .map(|w| w.main_repo_path.clone()),
            is_sandboxed: inst.is_sandboxed(),
            has_terminal: inst.terminal_info.is_some(),
            profile: inst.source_profile.clone(),
        }
    }
}

pub async fn list_sessions(State(state): State<Arc<AppState>>) -> Json<Vec<SessionResponse>> {
    let instances = state.instances.read().await;
    let sessions: Vec<SessionResponse> = instances.iter().map(SessionResponse::from).collect();
    Json(sessions)
}

// --- Rename session ---

#[derive(Deserialize)]
pub struct RenameSessionBody {
    pub title: String,
}

pub async fn rename_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Json(body): Json<RenameSessionBody>,
) -> impl IntoResponse {
    let title = body.title.trim().to_string();
    if title.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "message": "Title cannot be empty" })),
        );
    }
    if let Err(msg) = validate_no_shell_injection(&title, "title") {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "message": msg })),
        );
    }

    let mut instances = state.instances.write().await;
    let Some(inst) = instances.iter_mut().find(|i| i.id == id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "message": "Session not found" })),
        );
    };

    inst.title = title.clone();
    // Also update the worktree branch name in metadata (cosmetic only;
    // the actual git branch is not renamed on disk).
    if let Some(ref mut wt) = inst.worktree_info {
        wt.branch = title;
    }

    let response = SessionResponse::from(&*inst);

    let profile = state.profile.clone();
    if let Ok(storage) = Storage::new(&profile) {
        if let Err(e) = storage.save(&instances) {
            tracing::error!("Failed to save after rename: {e}");
        }
    }

    (StatusCode::OK, Json(serde_json::json!(response)))
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
    #[serde(default)]
    pub sandbox_image: Option<String>,
    #[serde(default)]
    pub extra_env: Vec<String>,
    #[serde(default)]
    pub extra_repo_paths: Vec<String>,
    #[serde(default)]
    pub command_override: String,
    #[serde(default)]
    pub custom_instruction: Option<String>,
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

    // Validate user inputs for shell injection
    for (value, name) in [
        (body.extra_args.as_str(), "extra_args"),
        (body.tool.as_str(), "tool"),
        (body.group.as_str(), "group"),
        (body.path.as_str(), "path"),
    ] {
        if let Err(msg) = validate_no_shell_injection(value, name) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "validation_failed", "message": msg})),
            )
                .into_response();
        }
    }
    if let Some(ref title) = body.title {
        if let Err(msg) = validate_no_shell_injection(title, "title") {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "validation_failed", "message": msg})),
            )
                .into_response();
        }
    }
    if let Some(ref branch) = body.worktree_branch {
        if let Err(msg) = validate_no_shell_injection(branch, "worktree_branch") {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "validation_failed", "message": msg})),
            )
                .into_response();
        }
    }

    let profile = state.profile.clone();
    let instances = state.instances.read().await;
    let existing_titles: Vec<String> = instances.iter().map(|i| i.title.clone()).collect();
    drop(instances);

    let result = tokio::task::spawn_blocking(move || {
        use crate::session::builder::{self, InstanceParams};
        use crate::session::Config;

        let config = Config::load().unwrap_or_default();
        let sandbox_image = body.sandbox_image.unwrap_or_else(|| {
            if config.sandbox.default_image.is_empty() {
                "ubuntu:latest".to_string()
            } else {
                config.sandbox.default_image.clone()
            }
        });

        let title_refs: Vec<&str> = existing_titles.iter().map(|s| s.as_str()).collect();
        let extra_repo_paths: Vec<String> = body
            .extra_repo_paths
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect();

        // When worktree_branch is empty string, generate a name from civilizations.
        // The generated name is used as both title and branch.
        let title = body.title.unwrap_or_default();
        let worktree_branch = match body.worktree_branch {
            Some(b) if b.is_empty() => {
                let generated = crate::session::civilizations::generate_random_title(&title_refs);
                Some(generated)
            }
            other => other,
        };
        // If title is empty and we generated a branch name, use it as the title too
        let title = if title.is_empty() {
            worktree_branch.clone().unwrap_or_default()
        } else {
            title
        };

        let params = InstanceParams {
            title,
            path: body.path,
            group: body.group,
            tool: body.tool,
            worktree_branch,
            create_new_branch: body.create_new_branch,
            sandbox: body.sandbox,
            sandbox_image,
            yolo_mode: body.yolo_mode,
            extra_env: body.extra_env,
            extra_args: body.extra_args,
            command_override: body.command_override,
            extra_repo_paths,
        };

        let build_result = builder::build_instance(params, &title_refs, &profile)?;
        let mut instance = build_result.instance;

        // Apply per-session sandbox overrides from the request body.
        if let Some(ref mut sandbox) = instance.sandbox_info {
            if body.custom_instruction.is_some() {
                sandbox.custom_instruction = body.custom_instruction;
            }
        }

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
            let mut instances = state.instances.write().await;
            instances.push(instance);
            (
                StatusCode::CREATED,
                Json(serde_json::to_value(resp).expect("SessionResponse is always serializable")),
            )
                .into_response()
        }
        Ok(Err(e)) => {
            tracing::warn!("Session creation failed: {}", e);
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "create_failed", "message": "Failed to create session"})),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("Session creation panicked: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal", "message": "Internal server error"})),
            )
                .into_response()
        }
    }
}

// --- Paired terminal ---

pub async fn ensure_terminal(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let mut instances = state.instances.write().await;
    let inst = match instances.iter_mut().find(|i| i.id == id) {
        Some(i) => i,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "not_found"})),
            )
                .into_response();
        }
    };

    if inst.has_terminal() {
        return (
            StatusCode::OK,
            Json(serde_json::json!({"status": "exists"})),
        )
            .into_response();
    }

    let mut inst_clone = inst.clone();
    drop(instances);

    let result = tokio::task::spawn_blocking(move || inst_clone.start_terminal()).await;

    match result {
        Ok(Ok(())) => {
            // Update in-memory cache
            let mut instances = state.instances.write().await;
            if let Some(inst) = instances.iter_mut().find(|i| i.id == id) {
                inst.terminal_info = Some(crate::session::TerminalInfo {
                    created: true,
                    created_at: Some(chrono::Utc::now()),
                });
            }
            (
                StatusCode::CREATED,
                Json(serde_json::json!({"status": "created"})),
            )
                .into_response()
        }
        Ok(Err(e)) => {
            tracing::error!("Terminal creation failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "create_failed", "message": "Failed to create terminal"})),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("Terminal creation panicked: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal", "message": "Internal server error"})),
            )
                .into_response()
        }
    }
}

pub async fn ensure_container_terminal(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let mut instances = state.instances.write().await;
    let inst = match instances.iter_mut().find(|i| i.id == id) {
        Some(i) => i,
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "not_found"})),
            )
                .into_response();
        }
    };

    if inst.has_container_terminal() {
        return (
            StatusCode::OK,
            Json(serde_json::json!({"status": "exists"})),
        )
            .into_response();
    }

    let mut inst_clone = inst.clone();
    drop(instances);

    let result =
        tokio::task::spawn_blocking(move || inst_clone.start_container_terminal_with_size(None))
            .await;

    match result {
        Ok(Ok(())) => (
            StatusCode::CREATED,
            Json(serde_json::json!({"status": "created"})),
        )
            .into_response(),
        Ok(Err(e)) => {
            tracing::error!("Container terminal creation failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "create_failed", "message": "Failed to create container terminal"})),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("Container terminal creation panicked: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal", "message": "Internal server error"})),
            )
                .into_response()
        }
    }
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
        Ok(Err(e)) => {
            tracing::error!("Diff failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "diff_failed", "message": "Failed to compute diff"})),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("Diff panicked: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal", "message": "Internal server error"})),
            )
                .into_response()
        }
    }
}

// --- Agents ---

#[derive(Serialize)]
pub struct AgentInfo {
    pub name: String,
    pub binary: String,
    pub host_only: bool,
    pub installed: bool,
}

pub async fn list_agents() -> Json<Vec<AgentInfo>> {
    let result = tokio::task::spawn_blocking(|| {
        let tools = crate::tmux::AvailableTools::detect();
        let available = tools.available_list();
        crate::agents::AGENTS
            .iter()
            .map(|a| AgentInfo {
                name: a.name.to_string(),
                binary: a.binary.to_string(),
                host_only: a.host_only,
                installed: available.iter().any(|s| s == a.name),
            })
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    Json(result)
}

// --- Settings ---

pub async fn get_settings() -> impl IntoResponse {
    match crate::session::Config::load() {
        Ok(config) => match serde_json::to_value(&config) {
            Ok(val) => (StatusCode::OK, Json(val)).into_response(),
            Err(e) => {
                tracing::error!("Settings serialization failed: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "serialize_failed", "message": "Failed to serialize settings"})),
                )
                    .into_response()
            }
        },
        Err(e) => {
            tracing::error!("Settings load failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "load_failed", "message": "Failed to load settings"})),
            )
                .into_response()
        }
    }
}

pub async fn update_settings(Json(body): Json<serde_json::Value>) -> impl IntoResponse {
    // Validate that only allowed sections are being updated
    if let Some(obj) = body.as_object() {
        for key in obj.keys() {
            if !ALLOWED_SETTINGS_SECTIONS.contains(&key.as_str()) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": "validation_failed",
                        "message": format!("Settings section '{}' is not allowed via the web API.", key)
                    })),
                )
                    .into_response();
            }
        }
    }

    let result = tokio::task::spawn_blocking(move || {
        let config = crate::session::Config::load().unwrap_or_default();
        let mut current = serde_json::to_value(&config)?;
        if let (Some(current_obj), Some(update_obj)) = (current.as_object_mut(), body.as_object()) {
            for (key, value) in update_obj {
                let mut value = value.clone();
                // Strip blocked fields from session section
                if key == "session" {
                    if let Some(session_obj) = value.as_object_mut() {
                        for blocked in SESSION_BLOCKED_FIELDS {
                            session_obj.remove(*blocked);
                        }
                    }
                }
                current_obj.insert(key.clone(), value);
            }
        }
        let config: crate::session::Config = serde_json::from_value(current)?;
        crate::session::save_config(&config)?;
        Ok::<_, anyhow::Error>(config)
    })
    .await;

    match result {
        Ok(Ok(config)) => match serde_json::to_value(&config) {
            Ok(val) => (StatusCode::OK, Json(val)).into_response(),
            Err(e) => {
                tracing::error!("Settings serialization failed: {}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(serde_json::json!({"error": "serialize_failed", "message": "Failed to serialize settings"})),
                )
                    .into_response()
            }
        },
        Ok(Err(e)) => {
            tracing::warn!("Settings update failed: {}", e);
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "update_failed", "message": "Failed to update settings"})),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("Settings update panicked: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal", "message": "Internal server error"})),
            )
                .into_response()
        }
    }
}

// --- Devices ---

pub async fn list_devices(State(state): State<Arc<AppState>>) -> Json<Vec<super::DeviceInfo>> {
    let devices = state.devices.read().await;
    Json(devices.clone())
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

// --- Wizard support ---

#[derive(Serialize)]
pub struct ProfileInfo {
    pub name: String,
    pub is_default: bool,
}

pub async fn list_profiles(State(state): State<Arc<AppState>>) -> Json<Vec<ProfileInfo>> {
    let profiles = crate::session::list_profiles().unwrap_or_default();
    let active = &state.profile;
    let mut result: Vec<ProfileInfo> = profiles
        .into_iter()
        .map(|name| {
            let is_default = name == *active;
            ProfileInfo { name, is_default }
        })
        .collect();
    // Ensure the active profile appears even if list_profiles missed it
    if !result.iter().any(|p| p.name == *active) {
        result.insert(
            0,
            ProfileInfo {
                name: active.clone(),
                is_default: true,
            },
        );
    }
    Json(result)
}

#[derive(Deserialize)]
pub struct BrowseQuery {
    pub path: String,
}

#[derive(Serialize)]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_git_repo: bool,
}

pub async fn browse_filesystem(
    axum::extract::Query(query): axum::extract::Query<BrowseQuery>,
) -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(move || {
        let path = std::path::Path::new(&query.path);
        let canonical = path.canonicalize().map_err(|_| "Path does not exist")?;

        if !canonical.is_dir() {
            return Err("Path is not a directory");
        }

        // Security: restrict browsing to the user's home directory
        if let Some(home) = dirs::home_dir() {
            if !canonical.starts_with(&home) {
                return Err("Path is outside the home directory");
            }
        }

        let mut entries: Vec<DirEntry> = Vec::new();
        let read_dir = std::fs::read_dir(&canonical).map_err(|_| "Cannot read directory")?;

        for entry in read_dir.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let entry_path = entry.path();
            let is_dir = entry_path.is_dir();
            if !is_dir {
                continue;
            }
            let is_git_repo = entry_path.join(".git").exists();
            entries.push(DirEntry {
                name,
                path: entry_path.to_string_lossy().to_string(),
                is_dir,
                is_git_repo,
            });
        }
        entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
        Ok(entries)
    })
    .await;

    match result {
        Ok(Ok(entries)) => {
            (StatusCode::OK, Json(serde_json::to_value(entries).unwrap())).into_response()
        }
        Ok(Err(msg)) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "browse_failed", "message": msg})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal", "message": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Deserialize)]
pub struct BranchesQuery {
    pub path: String,
}

#[derive(Serialize)]
pub struct BranchInfo {
    pub name: String,
    pub is_current: bool,
}

pub async fn list_branches(
    axum::extract::Query(query): axum::extract::Query<BranchesQuery>,
) -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(move || {
        let path = std::path::Path::new(&query.path);
        if !crate::git::GitWorktree::is_git_repo(path) {
            return Err("Path is not a git repository".to_string());
        }

        let branches = crate::git::diff::list_branches(path).map_err(|e| e.to_string())?;

        let current = std::process::Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(path)
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim().to_string())
            .unwrap_or_default();

        let mut result: Vec<BranchInfo> = branches
            .into_iter()
            .take(200)
            .map(|name| {
                let is_current = name == current;
                BranchInfo { name, is_current }
            })
            .collect();

        result.sort_by(|a, b| b.is_current.cmp(&a.is_current));

        Ok(result)
    })
    .await;

    match result {
        Ok(Ok(branches)) => (
            StatusCode::OK,
            Json(serde_json::to_value(branches).unwrap()),
        )
            .into_response(),
        Ok(Err(msg)) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "not_a_repo", "message": msg})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal", "message": e.to_string()})),
        )
            .into_response(),
    }
}

#[derive(Serialize)]
pub struct GroupInfo {
    pub path: String,
    pub session_count: usize,
}

pub async fn list_groups(State(state): State<Arc<AppState>>) -> impl IntoResponse {
    let instances = state.instances.read().await;
    let mut group_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for inst in instances.iter() {
        if !inst.group_path.is_empty() {
            *group_counts.entry(inst.group_path.clone()).or_default() += 1;
        }
    }
    let groups: Vec<GroupInfo> = group_counts
        .into_iter()
        .map(|(path, session_count)| GroupInfo {
            path,
            session_count,
        })
        .collect();
    Json(groups)
}

#[derive(Serialize)]
pub struct DockerStatus {
    pub available: bool,
    pub runtime: Option<String>,
}

pub async fn docker_status() -> Json<DockerStatus> {
    let result = tokio::task::spawn_blocking(|| {
        use crate::containers::ContainerRuntimeInterface;
        let runtime = crate::containers::get_container_runtime();
        let available = runtime.is_available() && runtime.is_daemon_running();
        let runtime_name = if available {
            let config = crate::session::Config::load().unwrap_or_default();
            Some(
                serde_json::to_value(config.sandbox.container_runtime)
                    .ok()
                    .and_then(|v| v.as_str().map(String::from))
                    .unwrap_or_else(|| "docker".to_string()),
            )
        } else {
            None
        };
        DockerStatus {
            available,
            runtime: runtime_name,
        }
    })
    .await
    .unwrap_or(DockerStatus {
        available: false,
        runtime: None,
    });
    Json(result)
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
