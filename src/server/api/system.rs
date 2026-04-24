//! Misc system endpoints: agents, settings, themes, profiles, filesystem,
//! groups, docker status, devices, about.

use std::sync::Arc;

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use serde::{Deserialize, Serialize};

use super::AppState;
use super::{validate_profile_name, ALLOWED_SETTINGS_SECTIONS, SESSION_BLOCKED_FIELDS};

// --- Agents ---

#[derive(Serialize)]
pub struct AgentInfo {
    pub name: String,
    pub binary: String,
    pub host_only: bool,
    pub installed: bool,
    pub install_hint: String,
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
                install_hint: a.install_hint.to_string(),
            })
            .collect::<Vec<_>>()
    })
    .await
    .unwrap_or_default();
    Json(result)
}

// --- Settings ---

#[derive(Deserialize)]
pub struct SettingsQuery {
    pub profile: Option<String>,
}

pub async fn get_settings(
    axum::extract::Query(query): axum::extract::Query<SettingsQuery>,
) -> impl IntoResponse {
    let config_result = if let Some(ref profile_name) = query.profile {
        crate::session::resolve_config(profile_name)
    } else {
        crate::session::Config::load()
    };

    match config_result {
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

pub async fn update_settings(
    State(state): State<Arc<AppState>>,
    Json(body): Json<serde_json::Value>,
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

pub async fn list_devices(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<crate::server::DeviceInfo>> {
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
    // Treat empty profile (server launched without --profile) as "default"
    let active = if state.profile.is_empty() {
        "default"
    } else {
        &state.profile
    };
    let mut result: Vec<ProfileInfo> = profiles
        .into_iter()
        .map(|name| {
            let is_default = name == active;
            ProfileInfo { name, is_default }
        })
        .collect();
    // Ensure the active profile appears even if list_profiles missed it
    if !active.is_empty() && !result.iter().any(|p| p.name == active) {
        result.insert(
            0,
            ProfileInfo {
                name: active.to_string(),
                is_default: true,
            },
        );
    }
    Json(result)
}

#[derive(Deserialize)]
pub struct BrowseQuery {
    pub path: String,
    pub limit: Option<usize>,
}

#[derive(Serialize)]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_git_repo: bool,
}

#[derive(Serialize)]
struct BrowseResponse {
    entries: Vec<DirEntry>,
    has_more: bool,
}

pub async fn filesystem_home() -> impl IntoResponse {
    match dirs::home_dir() {
        Some(home) => (
            StatusCode::OK,
            Json(serde_json::json!({"path": home.to_string_lossy()})),
        )
            .into_response(),
        None => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Could not determine home directory"})),
        )
            .into_response(),
    }
}

pub async fn browse_filesystem(
    axum::extract::Query(query): axum::extract::Query<BrowseQuery>,
) -> impl IntoResponse {
    let result = tokio::task::spawn_blocking(move || {
        let limit = query.limit.unwrap_or(100);
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
        // Cached: avoids re-allocating the lowercase String on every comparison
        // (sort_by_key calls the keyfn O(n log n) times, sort_by_cached_key calls it O(n)).
        entries.sort_by_cached_key(|e| e.name.to_lowercase());
        let has_more = entries.len() > limit;
        entries.truncate(limit);
        Ok(BrowseResponse { entries, has_more })
    })
    .await;

    match result {
        Ok(Ok(resp)) => (StatusCode::OK, Json(serde_json::to_value(resp).unwrap())).into_response(),
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

#[derive(Serialize)]
pub struct ServerAbout {
    pub version: String,
    pub auth_required: bool,
    pub passphrase_enabled: bool,
    pub read_only: bool,
    pub behind_tunnel: bool,
    pub profile: String,
}

pub async fn get_about(State(state): State<Arc<AppState>>) -> Json<ServerAbout> {
    let auth_required = !state.token_manager.is_no_auth().await;
    Json(ServerAbout {
        version: env!("CARGO_PKG_VERSION").to_string(),
        auth_required,
        passphrase_enabled: state.login_manager.is_enabled(),
        read_only: state.read_only,
        behind_tunnel: state.behind_tunnel,
        profile: state.profile.clone(),
    })
}

// --- Profile management ---

#[derive(Deserialize)]
pub struct CreateProfileBody {
    pub name: String,
}

pub async fn create_profile(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CreateProfileBody>,
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
    if let Err(e) = validate_profile_name(&body.name) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "validation_failed", "message": e})),
        )
            .into_response();
    }
    match tokio::task::spawn_blocking(move || crate::session::create_profile(&body.name)).await {
        Ok(Ok(())) => (StatusCode::CREATED, Json(serde_json::json!({"ok": true}))).into_response(),
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

pub async fn delete_profile(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(name): axum::extract::Path<String>,
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
    if let Err(e) = validate_profile_name(&name) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "validation_failed", "message": e})),
        )
            .into_response();
    }
    if name == state.profile {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "active_profile", "message": "Cannot delete the active profile"})),
        )
            .into_response();
    }
    match tokio::task::spawn_blocking(move || crate::session::delete_profile(&name)).await {
        Ok(Ok(())) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Ok(Err(e)) => (
            StatusCode::BAD_REQUEST,
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

#[derive(Deserialize)]
pub struct RenameProfileBody {
    pub new_name: String,
}

pub async fn rename_profile(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(name): axum::extract::Path<String>,
    Json(body): Json<RenameProfileBody>,
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
    if let Err(e) = validate_profile_name(&name) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "validation_failed", "message": e})),
        )
            .into_response();
    }
    if let Err(e) = validate_profile_name(&body.new_name) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "validation_failed", "message": e})),
        )
            .into_response();
    }
    let old = name;
    let new = body.new_name;
    match tokio::task::spawn_blocking(move || crate::session::rename_profile(&old, &new)).await {
        Ok(Ok(())) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
        Ok(Err(e)) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "rename_failed", "message": e.to_string()})),
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
pub struct DefaultProfileBody {
    pub name: String,
}

pub async fn default_profile(
    State(state): State<Arc<AppState>>,
    Json(body): Json<DefaultProfileBody>,
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
    if let Err(e) = validate_profile_name(&body.name) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "validation_failed", "message": e})),
        )
            .into_response();
    }
    let name = body.name;
    match tokio::task::spawn_blocking(move || crate::session::set_default_profile(&name)).await {
        Ok(Ok(())) => (StatusCode::OK, Json(serde_json::json!({"ok": true}))).into_response(),
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

pub async fn get_profile_settings(
    axum::extract::Path(name): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Err(e) = validate_profile_name(&name) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "validation_failed", "message": e})),
        )
            .into_response();
    }
    let result =
        tokio::task::spawn_blocking(move || crate::session::load_profile_config(&name)).await;
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
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "load_failed", "message": e.to_string()})),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "internal", "message": e.to_string()})),
        )
            .into_response(),
    }
}

pub async fn update_profile_settings(
    State(state): State<Arc<AppState>>,
    axum::extract::Path(name): axum::extract::Path<String>,
    Json(body): Json<serde_json::Value>,
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
    if let Err(e) = validate_profile_name(&name) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "validation_failed", "message": e})),
        )
            .into_response();
    }
    // Validate allowed sections
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
        let config = crate::session::load_profile_config(&name).unwrap_or_default();
        let mut current = serde_json::to_value(&config)?;
        if let (Some(current_obj), Some(update_obj)) = (current.as_object_mut(), body.as_object()) {
            for (key, value) in update_obj {
                let mut value = value.clone();
                if key == "session" {
                    if let Some(session_obj) = value.as_object_mut() {
                        for blocked in SESSION_BLOCKED_FIELDS {
                            session_obj.remove(*blocked);
                        }
                    }
                }
                // Deep merge within sections so that sending a single field
                // (e.g. {"session": {"yolo_mode_default": true}}) only sets
                // that field as a profile override, preserving other existing
                // overrides instead of replacing the entire section.
                if let Some(existing) = current_obj.get_mut(key) {
                    if let (Some(existing_obj), Some(new_obj)) =
                        (existing.as_object_mut(), value.as_object())
                    {
                        for (k, v) in new_obj {
                            existing_obj.insert(k.clone(), v.clone());
                        }
                    } else {
                        current_obj.insert(key.clone(), value);
                    }
                } else {
                    current_obj.insert(key.clone(), value);
                }
            }
        }
        let config: crate::session::ProfileConfig = serde_json::from_value(current)?;
        crate::session::save_profile_config(&name, &config)?;
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

// --- Sounds ---

pub async fn list_sounds() -> Json<Vec<String>> {
    Json(crate::sound::list_available_sounds())
}
