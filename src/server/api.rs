//! REST API handlers for session management and agents.

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

const ALLOWED_SETTINGS_SECTIONS: &[&str] = &[
    "theme", "session", "tmux", "updates", "sound", "sandbox", "worktree",
];

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
    pub has_managed_worktree: bool,
    pub has_terminal: bool,
    pub profile: String,
    pub cleanup_defaults: CleanupDefaults,
    pub remote_owner: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct CleanupDefaults {
    pub delete_worktree: bool,
    pub delete_branch: bool,
    pub delete_sandbox: bool,
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
            has_managed_worktree: inst
                .worktree_info
                .as_ref()
                .is_some_and(|w| w.managed_by_aoe),
            has_terminal: inst.terminal_info.is_some(),
            profile: inst.source_profile.clone(),
            cleanup_defaults: CleanupDefaults {
                delete_worktree: true,
                delete_branch: false,
                delete_sandbox: true,
            },
            remote_owner: None,
        }
    }
}

pub async fn list_sessions(State(state): State<Arc<AppState>>) -> Json<Vec<SessionResponse>> {
    let instances = state.instances.read().await;
    let mut sessions: Vec<SessionResponse> = instances.iter().map(SessionResponse::from).collect();

    // Resolve per-profile cleanup defaults with a 30s TTL cache on AppState
    let cache = {
        let guard = state.cleanup_defaults_cache.read().await;
        if guard.0.elapsed() < std::time::Duration::from_secs(30) {
            Some(guard.1.clone())
        } else {
            None
        }
    };

    let defaults_map = if let Some(cached) = cache {
        cached
    } else {
        use std::collections::HashMap;
        let mut fresh: HashMap<String, CleanupDefaults> = HashMap::new();
        for session in &sessions {
            fresh.entry(session.profile.clone()).or_insert_with(|| {
                crate::session::resolve_config(&session.profile)
                    .map(|cfg| CleanupDefaults {
                        delete_worktree: cfg.worktree.auto_cleanup,
                        delete_branch: cfg.worktree.delete_branch_on_cleanup,
                        delete_sandbox: cfg.sandbox.auto_cleanup,
                    })
                    .unwrap_or(CleanupDefaults {
                        delete_worktree: true,
                        delete_branch: false,
                        delete_sandbox: true,
                    })
            });
        }
        *state.cleanup_defaults_cache.write().await = (std::time::Instant::now(), fresh.clone());
        fresh
    };

    // Resolve remote owners with a permanent cache on AppState
    {
        let cache = state.remote_owner_cache.read().await;
        for session in &mut sessions {
            if let Some(defaults) = defaults_map.get(&session.profile) {
                session.cleanup_defaults = defaults.clone();
            }
            let repo_path = session
                .main_repo_path
                .as_deref()
                .unwrap_or(&session.project_path);
            if let Some(owner) = cache.get(repo_path) {
                session.remote_owner = owner.clone();
            }
        }
    }

    // Fill any uncached repo paths
    let uncached: Vec<String> = sessions
        .iter()
        .filter(|s| s.remote_owner.is_none())
        .map(|s| {
            s.main_repo_path
                .clone()
                .unwrap_or_else(|| s.project_path.clone())
        })
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();

    if !uncached.is_empty() {
        let mut cache = state.remote_owner_cache.write().await;
        for path in &uncached {
            if !cache.contains_key(path.as_str()) {
                let owner = crate::git::get_remote_owner(std::path::Path::new(path));
                cache.insert(path.clone(), owner);
            }
        }
        for session in &mut sessions {
            let repo_path = session
                .main_repo_path
                .as_deref()
                .unwrap_or(&session.project_path);
            if session.remote_owner.is_none() {
                if let Some(owner) = cache.get(repo_path) {
                    session.remote_owner = owner.clone();
                }
            }
        }
    }

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
    let profile = inst.source_profile.clone();

    if let Ok(storage) = Storage::new(&profile) {
        let profile_instances: Vec<_> = instances
            .iter()
            .filter(|i| i.source_profile == profile)
            .cloned()
            .collect();
        if let Err(e) = storage.save(&profile_instances) {
            tracing::error!("Failed to save after rename: {e}");
        }
    }

    (StatusCode::OK, Json(serde_json::json!(response)))
}

// --- Delete session ---

#[derive(Default, Deserialize)]
pub struct DeleteSessionBody {
    #[serde(default)]
    pub delete_worktree: bool,
    #[serde(default)]
    pub delete_branch: bool,
    #[serde(default)]
    pub delete_sandbox: bool,
    #[serde(default)]
    pub force_delete: bool,
}

pub async fn delete_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    body: Option<Json<DeleteSessionBody>>,
) -> impl IntoResponse {
    if state.read_only {
        return (
            StatusCode::FORBIDDEN,
            Json(
                serde_json::json!({"error": "read_only", "message": "Server is in read-only mode"}),
            ),
        );
    }

    let body = body.map(|Json(b)| b).unwrap_or_default();

    // Acquire per-instance lock to serialize concurrent mutations
    let lock = state.instance_lock(&id).await;
    let _guard = lock.lock().await;

    // Find and clone the instance (need the full Instance for deletion)
    let instance = {
        let instances = state.instances.read().await;
        instances.iter().find(|i| i.id == id).cloned()
    };

    let Some(instance) = instance else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "message": "Session not found" })),
        );
    };

    let profile = instance.source_profile.clone();

    // Mark as Deleting so polling clients see the status change
    {
        let mut instances = state.instances.write().await;
        if let Some(inst) = instances.iter_mut().find(|i| i.id == id) {
            inst.status = Status::Deleting;
        }
    }

    // Run deletion on a blocking thread (may do git/docker/tmux operations)
    let deletion_id = id.clone();
    let deletion_result = tokio::task::spawn_blocking(move || {
        crate::session::deletion::perform_deletion(&crate::session::deletion::DeletionRequest {
            session_id: deletion_id,
            instance,
            delete_worktree: body.delete_worktree,
            delete_branch: body.delete_branch,
            delete_sandbox: body.delete_sandbox,
            force_delete: body.force_delete,
        })
    })
    .await;

    match deletion_result {
        Ok(result) if result.success => {
            // Remove from in-memory state and persist
            let mut instances = state.instances.write().await;
            instances.retain(|i| i.id != id);

            if let Ok(storage) = Storage::new(&profile) {
                let profile_instances: Vec<_> = instances
                    .iter()
                    .filter(|i| i.source_profile == profile)
                    .cloned()
                    .collect();
                if let Err(e) = storage.save(&profile_instances) {
                    tracing::error!("Failed to save after deletion: {e}");
                }
            }

            // Clean up per-instance lock entry
            state.instance_locks.write().await.remove(&id);

            (
                StatusCode::OK,
                Json(serde_json::json!({ "status": "deleted" })),
            )
        }
        Ok(result) => {
            // Deletion had errors; set status to Error
            let error_msg = result.error.unwrap_or_else(|| "Unknown error".to_string());
            {
                let mut instances = state.instances.write().await;
                if let Some(inst) = instances.iter_mut().find(|i| i.id == id) {
                    inst.status = Status::Error;
                    inst.last_error = Some(error_msg.clone());
                }
            }
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "deletion_failed",
                    "message": error_msg,
                })),
            )
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({
                "error": "internal",
                "message": format!("Deletion task failed: {e}"),
            })),
        ),
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
    pub profile: Option<String>,
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
    if let Some(ref profile_name) = body.profile {
        if let Err(msg) = validate_no_shell_injection(profile_name, "profile") {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "validation_failed", "message": msg})),
            )
                .into_response();
        }
        // Verify the profile exists ("default" is always valid even without a dir)
        if profile_name != "default" {
            let known = crate::session::list_profiles().unwrap_or_default();
            if !known.contains(profile_name) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(serde_json::json!({
                        "error": "profile_not_found",
                        "message": format!("Profile '{}' does not exist", profile_name)
                    })),
                )
                    .into_response();
            }
        }
    }

    let profile = body.profile.unwrap_or_else(|| state.profile.clone());
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
        instance.source_profile = profile.clone();

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

// --- Clone repository ---

#[derive(Deserialize)]
pub struct CloneRepoBody {
    pub url: String,
    pub destination: Option<String>,
    #[serde(default)]
    pub shallow: bool,
}

/// Extract a repository name from a git URL.
///
/// Handles HTTPS, SSH, and scp-style URLs:
///   https://github.com/user/repo.git       -> repo
///   git@github.com:user/repo.git           -> repo
///   ssh://git@host/user/repo               -> repo
///   ssh://git@host:2222/user/repo.git      -> repo
fn repo_name_from_url(url: &str) -> Option<String> {
    // For scheme-based URLs (https://, ssh://, git://), take the last path segment.
    // For scp-style (git@host:path), split on ':' and take the last segment of the path.
    let last_segment = if url.contains("://") {
        url.rsplit_once('/')?.1
    } else if let Some((_host, path)) = url.split_once(':') {
        // scp-style: git@github.com:user/repo.git
        path.rsplit('/').next().unwrap_or(path)
    } else {
        url.rsplit_once('/')?.1
    };
    let name = last_segment.trim_end_matches(".git");
    if name.is_empty() {
        None
    } else {
        Some(name.to_string())
    }
}

pub async fn clone_repo(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CloneRepoBody>,
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

    let url = body.url.trim().to_string();
    if url.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(
                serde_json::json!({"error": "validation_failed", "message": "URL cannot be empty"}),
            ),
        )
            .into_response();
    }

    // Basic URL scheme validation
    let looks_like_url = url.starts_with("https://")
        || url.starts_with("http://")
        || url.starts_with("git://")
        || url.starts_with("ssh://")
        || (url.contains('@') && url.contains(':') && !url.contains(' '));
    if !looks_like_url {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "validation_failed", "message": "URL does not look like a git repository URL"})),
        )
            .into_response();
    }

    // Resolve destination path
    let destination = if let Some(ref dest) = body.destination {
        let dest = dest.trim();
        if dest.is_empty() {
            None
        } else {
            Some(std::path::PathBuf::from(dest))
        }
    } else {
        None
    };

    let destination = match destination {
        Some(d) => d,
        None => {
            let repo_name = repo_name_from_url(&url).unwrap_or_else(|| "cloned-repo".to_string());
            let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/tmp"));
            home.join(&repo_name)
        }
    };

    // Security: destination must be within the home directory
    if let Some(home) = dirs::home_dir() {
        let canonical_home = home.canonicalize().unwrap_or(home);
        // For new paths that don't exist yet, check the parent
        let check_path = if destination.exists() {
            destination.canonicalize().unwrap_or(destination.clone())
        } else {
            destination
                .parent()
                .and_then(|p| p.canonicalize().ok())
                .map(|p| p.join(destination.file_name().unwrap_or_default()))
                .unwrap_or(destination.clone())
        };
        if !check_path.starts_with(&canonical_home) {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "validation_failed", "message": "Destination must be within the home directory"})),
            )
                .into_response();
        }
    }

    let dest_display = destination.display().to_string();

    // Return an actionable error if the destination already exists, before
    // spawning the blocking task, so the user knows to pick a different name.
    if destination.exists() {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "destination_exists",
                "message": format!(
                    "Directory '{}' already exists. Choose a different destination or delete it first.",
                    destination.file_name().unwrap_or_default().to_string_lossy()
                ),
                "path": dest_display,
            })),
        )
            .into_response();
    }

    let shallow = body.shallow;
    let result = tokio::task::spawn_blocking(move || {
        crate::git::clone_repo(&url, &destination, shallow)?;
        Ok::<String, crate::git::error::GitError>(destination.display().to_string())
    })
    .await;

    match result {
        Ok(Ok(path)) => {
            (StatusCode::CREATED, Json(serde_json::json!({"path": path}))).into_response()
        }
        Ok(Err(e)) => {
            let msg = e.to_string();
            tracing::warn!("Clone failed for {dest_display}: {msg}");
            (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({"error": "clone_failed", "message": msg})),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("Clone task panicked: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal", "message": "Internal server error"})),
            )
                .into_response()
        }
    }
}

// --- Ensure agent session ---

/// Ensure the main agent tmux session is alive, restarting it if dead.
///
/// Mirrors the TUI's `attach_session` restart logic: checks the actual tmux
/// state (exists / pane dead / running unexpected shell) and restarts the
/// instance when needed. Returns the resulting status so the frontend can
/// decide whether to proceed with the WebSocket attach.
///
/// Concurrency: a per-instance `tokio::sync::Mutex` serializes ensure calls
/// for the same session so two rapid POSTs don't both decide "dead" and race
/// on `tmux new-session`.
///
/// Read-only: in read-only mode, the endpoint may report `alive` but will
/// refuse to kill+restart a session. Returns 403 when a restart is needed.
pub async fn ensure_session(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let instances = state.instances.read().await;
    let Some(instance) = instances.iter().find(|i| i.id == id).cloned() else {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "not_found"})),
        )
            .into_response();
    };
    drop(instances);

    // Serialize concurrent ensure calls for the same session. The decision
    // phase reads tmux state and the restart phase mutates it; any other
    // ensure for this id must wait so both see a consistent view.
    let inst_lock = state.instance_lock(&id).await;
    let _guard = inst_lock.lock().await;

    // Inspect tmux + make the restart decision on a blocking thread. Refresh
    // the cache first so rapid re-calls see the true current state (the
    // background status poller only refreshes every 2s).
    let decision_instance = instance.clone();
    let id_for_log = id.clone();
    let decision = tokio::task::spawn_blocking(move || -> anyhow::Result<bool> {
        crate::tmux::refresh_session_cache();
        let tmux_session = decision_instance.tmux_session()?;
        let exists = tmux_session.exists();
        let pane_dead = exists && tmux_session.is_pane_dead();
        let needs_restart = if !exists || pane_dead {
            true
        } else if crate::hooks::read_hook_status(&decision_instance.id).is_some() {
            // Hook status tracks this session; shell detection is unreliable.
            false
        } else if decision_instance.has_command_override() {
            // Custom command overrides run agents through wrapper scripts that
            // look like shells to tmux. Don't restart based on shell detection.
            false
        } else {
            !decision_instance.expects_shell() && tmux_session.is_pane_running_shell()
        };
        tracing::debug!(
            session_id = id_for_log,
            exists,
            pane_dead,
            needs_restart,
            "ensure_session: restart decision"
        );
        Ok(needs_restart)
    })
    .await;

    let needs_restart = match decision {
        Ok(Ok(v)) => v,
        Ok(Err(e)) => {
            tracing::error!("ensure_session: failed to inspect tmux for {id}: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal"})),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("ensure_session inspect panicked for {id}: {e}");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal"})),
            )
                .into_response();
        }
    };

    if !needs_restart {
        return (StatusCode::OK, Json(serde_json::json!({"status": "alive"}))).into_response();
    }

    if state.read_only {
        // Read-only viewers must not kill + respawn a dead session. Signal
        // the frontend so it can show "session is stopped; ask an owner to
        // reattach" instead of silently replacing the agent process.
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({
                "error": "read_only",
                "message": "Session is stopped or errored. Restart requires write access.",
            })),
        )
            .into_response();
    }

    {
        let mut instances = state.instances.write().await;
        if let Some(inst) = instances.iter_mut().find(|i| i.id == id) {
            inst.status = crate::session::Status::Starting;
            inst.last_error = None;
        }
    }

    let restart_result = tokio::task::spawn_blocking(move || -> anyhow::Result<Instance> {
        let tmux_session = instance.tmux_session()?;
        if tmux_session.exists() {
            let _ = tmux_session.kill();
        }
        let mut inst = instance;
        inst.start_with_size_opts(None, false)?;
        Ok(inst)
    })
    .await;

    match restart_result {
        Ok(Ok(started)) => {
            let mut instances = state.instances.write().await;
            if let Some(inst) = instances.iter_mut().find(|i| i.id == id) {
                inst.status = started.status;
                inst.last_error = None;
            }
            (
                StatusCode::OK,
                Json(serde_json::json!({"status": "restarted"})),
            )
                .into_response()
        }
        Ok(Err(e)) => {
            let msg = e.to_string();
            tracing::warn!("ensure_session restart failed for {id}: {msg}");
            let mut instances = state.instances.write().await;
            if let Some(inst) = instances.iter_mut().find(|i| i.id == id) {
                inst.status = crate::session::Status::Error;
                inst.last_error = Some(msg.clone());
            }
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "error": "restart_failed",
                    "message": msg,
                })),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("ensure_session panicked for {id}: {e}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal"})),
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
    let instances = state.instances.read().await;
    let inst = match instances.iter().find(|i| i.id == id) {
        Some(i) => i.clone(),
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "not_found"})),
            )
                .into_response();
        }
    };
    drop(instances);

    // Serialize concurrent terminal-ensure calls for the same session so two
    // parallel requests don't both try to create the same tmux session
    // (the second would fail with "duplicate session").
    let inst_lock = state.instance_lock(&id).await;
    let _guard = inst_lock.lock().await;

    // Re-check after acquiring the lock; the first caller may have created it.
    {
        let instances = state.instances.read().await;
        if let Some(i) = instances.iter().find(|i| i.id == id) {
            if i.has_terminal() {
                return (
                    StatusCode::OK,
                    Json(serde_json::json!({"status": "exists"})),
                )
                    .into_response();
            }
        }
    }

    let mut inst_clone = inst;

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
    let instances = state.instances.read().await;
    let inst = match instances.iter().find(|i| i.id == id) {
        Some(i) => i.clone(),
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "not_found"})),
            )
                .into_response();
        }
    };
    drop(instances);

    let inst_lock = state.instance_lock(&id).await;
    let _guard = inst_lock.lock().await;

    {
        let instances = state.instances.read().await;
        if let Some(i) = instances.iter().find(|i| i.id == id) {
            if i.has_container_terminal() {
                return (
                    StatusCode::OK,
                    Json(serde_json::json!({"status": "exists"})),
                )
                    .into_response();
            }
        }
    }

    let mut inst_clone = inst;

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

// --- Rich Diff (per-file, merge-base aware) ---

#[derive(Serialize)]
pub struct RichDiffFileInfo {
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub old_path: Option<String>,
    pub status: String,
    pub additions: usize,
    pub deletions: usize,
}

#[derive(Serialize)]
pub struct RichDiffFilesResponse {
    pub files: Vec<RichDiffFileInfo>,
    pub base_branch: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

#[derive(Serialize)]
pub struct RichDiffLine {
    #[serde(rename = "type")]
    pub change_type: String,
    pub old_line_num: Option<usize>,
    pub new_line_num: Option<usize>,
    pub content: String,
}

#[derive(Serialize)]
pub struct RichDiffHunk {
    pub old_start: usize,
    pub old_lines: usize,
    pub new_start: usize,
    pub new_lines: usize,
    pub lines: Vec<RichDiffLine>,
}

#[derive(Serialize)]
pub struct RichFileDiffResponse {
    pub file: RichDiffFileInfo,
    pub hunks: Vec<RichDiffHunk>,
    pub is_binary: bool,
    /// True if the file was too large to diff and hunks were omitted.
    pub truncated: bool,
}

/// Max combined bytes of old+new content before we bail on diffing.
const MAX_DIFF_BYTES: usize = 2_000_000;
/// Max combined line count of old+new before we bail on diffing.
const MAX_DIFF_LINES: usize = 40_000;

/// Validate a user-supplied relative file path against a workdir.
///
/// Returns the canonicalized absolute path if the requested path is safe to
/// read (no absolute, no `..`, no symlink-escape out of the workdir) and
/// appears in `changed_files` (so only actually-diffed files are exposed).
/// Returns `Err(status, message)` otherwise.
fn validate_diff_path(
    workdir: &std::path::Path,
    requested: &std::path::Path,
    changed_files: &[crate::git::diff::DiffFile],
) -> Result<std::path::PathBuf, (StatusCode, &'static str)> {
    use std::path::Component;

    if requested.as_os_str().is_empty() {
        return Err((StatusCode::BAD_REQUEST, "empty path"));
    }
    if requested.is_absolute() {
        return Err((StatusCode::BAD_REQUEST, "absolute path not allowed"));
    }
    for comp in requested.components() {
        match comp {
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err((StatusCode::BAD_REQUEST, "path escapes workdir"));
            }
            _ => {}
        }
    }

    // Cross-check: path must be one of the currently-changed files.
    // This is the narrowest trust boundary: only files the user actually
    // modified on this branch are diffable, not arbitrary files in the worktree.
    let matches_changed = changed_files.iter().any(|f| f.path == requested);
    if !matches_changed {
        return Err((StatusCode::NOT_FOUND, "file not in changed set"));
    }

    // Canonicalize both sides and verify containment as defense in depth
    // against symlinks that might point outside the workdir.
    let canonical_workdir = workdir.canonicalize().map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "workdir canonicalize failed",
        )
    })?;
    let full = canonical_workdir.join(requested);
    // The file may not exist on disk (e.g., deleted in the working tree), in
    // which case canonicalize fails; fall back to the non-canonical path and
    // just verify textual containment.
    let final_path = match full.canonicalize() {
        Ok(c) => {
            if !c.starts_with(&canonical_workdir) {
                return Err((StatusCode::BAD_REQUEST, "path escapes workdir"));
            }
            c
        }
        Err(_) => full,
    };
    Ok(final_path)
}

/// Helper: look up a session's project_path by ID.
async fn resolve_session_path(
    state: &AppState,
    id: &str,
) -> Result<String, axum::response::Response> {
    let instances = state.instances.read().await;
    match instances.iter().find(|i| i.id == id) {
        Some(i) => Ok(i.project_path.clone()),
        None => Err((
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "not_found", "message": "Session not found"})),
        )
            .into_response()),
    }
}

pub async fn session_diff_files(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let project_path = match resolve_session_path(&state, &id).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    let result = tokio::task::spawn_blocking(move || {
        use crate::git::diff;
        let path = std::path::Path::new(&project_path);

        let base_branch = diff::get_default_branch(path).unwrap_or_else(|_| "main".to_string());
        let warning = diff::check_merge_base_status(path, &base_branch);
        let changed = diff::compute_changed_files(path, &base_branch).unwrap_or_default();

        let files: Vec<RichDiffFileInfo> = changed
            .into_iter()
            .map(|f| RichDiffFileInfo {
                path: f.path.to_string_lossy().to_string(),
                old_path: f.old_path.map(|p| p.to_string_lossy().to_string()),
                status: f.status.label().to_string(),
                additions: f.additions,
                deletions: f.deletions,
            })
            .collect();

        RichDiffFilesResponse {
            files,
            base_branch,
            warning,
        }
    })
    .await;

    match result {
        Ok(resp) => (
            StatusCode::OK,
            Json(serde_json::to_value(resp).expect("RichDiffFilesResponse is always serializable")),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Diff files panicked: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "internal", "message": "Internal server error"})),
            )
                .into_response()
        }
    }
}

#[derive(Deserialize)]
pub struct FileDiffQuery {
    pub path: String,
}

/// Response for a rejected diff request (bad path, file not changed, etc.).
enum DiffFileError {
    BadRequest(&'static str),
    NotFound(&'static str),
    Internal(anyhow::Error),
}

pub async fn session_diff_file(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<FileDiffQuery>,
) -> impl IntoResponse {
    let project_path = match resolve_session_path(&state, &id).await {
        Ok(p) => p,
        Err(resp) => return resp,
    };

    let result =
        tokio::task::spawn_blocking(move || -> Result<RichFileDiffResponse, DiffFileError> {
            use crate::git::diff;
            use similar::ChangeTag;

            let repo_path = std::path::Path::new(&project_path);
            let file_path = std::path::Path::new(&query.path);

            let base_branch =
                diff::get_default_branch(repo_path).unwrap_or_else(|_| "main".to_string());

            // Validate the requested path against the set of actually-changed files.
            // This is the primary security boundary: only files modified on this
            // branch are diffable, preventing arbitrary file reads via ?path=...
            let changed_files = diff::compute_changed_files(repo_path, &base_branch)
                .map_err(|e| DiffFileError::Internal(e.into()))?;
            match validate_diff_path(repo_path, file_path, &changed_files) {
                Ok(_) => {}
                Err((status, msg)) => {
                    return Err(if status == StatusCode::NOT_FOUND {
                        DiffFileError::NotFound(msg)
                    } else {
                        DiffFileError::BadRequest(msg)
                    });
                }
            }

            let file_diff = diff::compute_file_diff(repo_path, file_path, &base_branch, 3)
                .map_err(|e| DiffFileError::Internal(e.into()))?;

            let file = RichDiffFileInfo {
                path: file_diff.file.path.to_string_lossy().to_string(),
                old_path: file_diff
                    .file
                    .old_path
                    .map(|p| p.to_string_lossy().to_string()),
                status: file_diff.file.status.label().to_string(),
                additions: file_diff.file.additions,
                deletions: file_diff.file.deletions,
            };

            // Size cap: avoid OOM'ing the browser on huge files (minified bundles,
            // generated code, data blobs that slipped past .gitignore).
            let total_line_count: usize = file_diff.hunks.iter().map(|h| h.lines.len()).sum();
            let total_bytes: usize = file_diff
                .hunks
                .iter()
                .flat_map(|h| h.lines.iter())
                .map(|l| l.content.len())
                .sum();
            if total_line_count > MAX_DIFF_LINES || total_bytes > MAX_DIFF_BYTES {
                return Ok(RichFileDiffResponse {
                    file,
                    hunks: Vec::new(),
                    is_binary: file_diff.is_binary,
                    truncated: true,
                });
            }

            let hunks: Vec<RichDiffHunk> = file_diff
                .hunks
                .into_iter()
                .map(|h| RichDiffHunk {
                    old_start: h.old_start,
                    old_lines: h.old_lines,
                    new_start: h.new_start,
                    new_lines: h.new_lines,
                    lines: h
                        .lines
                        .into_iter()
                        .map(|l| RichDiffLine {
                            change_type: match l.tag {
                                ChangeTag::Insert => "add".to_string(),
                                ChangeTag::Delete => "delete".to_string(),
                                ChangeTag::Equal => "equal".to_string(),
                            },
                            old_line_num: l.old_line_num,
                            new_line_num: l.new_line_num,
                            content: l.content,
                        })
                        .collect(),
                })
                .collect();

            Ok(RichFileDiffResponse {
                file,
                hunks,
                is_binary: file_diff.is_binary,
                truncated: false,
            })
        })
        .await;

    match result {
        Ok(Ok(resp)) => (
            StatusCode::OK,
            Json(serde_json::to_value(resp).expect("RichFileDiffResponse is always serializable")),
        )
            .into_response(),
        Ok(Err(DiffFileError::BadRequest(msg))) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "bad_request", "message": msg})),
        )
            .into_response(),
        Ok(Err(DiffFileError::NotFound(msg))) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "not_found", "message": msg})),
        )
            .into_response(),
        Ok(Err(DiffFileError::Internal(e))) => {
            tracing::error!("File diff failed: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({"error": "diff_failed", "message": "Failed to compute file diff"})),
            )
                .into_response()
        }
        Err(e) => {
            tracing::error!("File diff panicked: {}", e);
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
        entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
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

    // ── validate_diff_path: security regression tests ──────────────────────────
    //
    // Regression for a path-traversal vulnerability in the first cut of the
    // `/api/sessions/{id}/diff/file?path=...` endpoint. Any authenticated user
    // could pass `?path=/etc/passwd` or `?path=../../etc/shadow` and have the
    // server dump the file contents in a diff response. The validator must
    // reject absolute paths, parent-dir traversal, and any path that isn't in
    // the set of actually-changed files.

    use crate::git::diff::{DiffFile, FileStatus};
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn changed(paths: &[&str]) -> Vec<DiffFile> {
        paths
            .iter()
            .map(|p| DiffFile {
                path: PathBuf::from(p),
                old_path: None,
                status: FileStatus::Modified,
                additions: 0,
                deletions: 0,
            })
            .collect()
    }

    #[test]
    fn validate_diff_path_rejects_absolute() {
        let dir = TempDir::new().unwrap();
        let err = validate_diff_path(
            dir.path(),
            std::path::Path::new("/etc/passwd"),
            &changed(&["src/main.rs"]),
        )
        .unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn validate_diff_path_rejects_parent_dir() {
        let dir = TempDir::new().unwrap();
        let err = validate_diff_path(
            dir.path(),
            std::path::Path::new("../../etc/passwd"),
            &changed(&["src/main.rs"]),
        )
        .unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn validate_diff_path_rejects_parent_dir_in_middle() {
        let dir = TempDir::new().unwrap();
        let err = validate_diff_path(
            dir.path(),
            std::path::Path::new("src/../../etc/passwd"),
            &changed(&["src/main.rs"]),
        )
        .unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn validate_diff_path_rejects_empty() {
        let dir = TempDir::new().unwrap();
        let err = validate_diff_path(dir.path(), std::path::Path::new(""), &[]).unwrap_err();
        assert_eq!(err.0, StatusCode::BAD_REQUEST);
    }

    #[test]
    fn validate_diff_path_rejects_unchanged_file() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("existing.txt"), "hello").unwrap();
        // File exists inside workdir but is not in the changed set.
        let err = validate_diff_path(
            dir.path(),
            std::path::Path::new("existing.txt"),
            &changed(&["src/main.rs"]),
        )
        .unwrap_err();
        assert_eq!(err.0, StatusCode::NOT_FOUND);
    }

    #[test]
    fn validate_diff_path_accepts_changed_file() {
        let dir = TempDir::new().unwrap();
        std::fs::write(dir.path().join("changed.txt"), "hello").unwrap();
        let ok = validate_diff_path(
            dir.path(),
            std::path::Path::new("changed.txt"),
            &changed(&["changed.txt"]),
        );
        assert!(ok.is_ok(), "expected Ok, got {:?}", ok);
    }

    #[test]
    fn validate_diff_path_accepts_deleted_file() {
        // A file that has been deleted on disk but is in the changed set
        // (status: Deleted) should still be diffable so the user can see
        // what was removed. canonicalize() on the joined path will fail,
        // so the validator must fall back to the non-canonical path.
        let dir = TempDir::new().unwrap();
        let ok = validate_diff_path(
            dir.path(),
            std::path::Path::new("deleted.txt"),
            &changed(&["deleted.txt"]),
        );
        assert!(ok.is_ok(), "expected Ok, got {:?}", ok);
    }

    // ── repo_name_from_url tests ─────────────────────────────────────────────

    #[test]
    fn repo_name_from_https_url() {
        assert_eq!(
            repo_name_from_url("https://github.com/user/my-repo.git"),
            Some("my-repo".to_string())
        );
    }

    #[test]
    fn repo_name_from_https_url_no_dotgit() {
        assert_eq!(
            repo_name_from_url("https://github.com/user/my-repo"),
            Some("my-repo".to_string())
        );
    }

    #[test]
    fn repo_name_from_scp_url() {
        assert_eq!(
            repo_name_from_url("git@github.com:user/my-repo.git"),
            Some("my-repo".to_string())
        );
    }

    #[test]
    fn repo_name_from_ssh_url() {
        assert_eq!(
            repo_name_from_url("ssh://git@github.com/user/my-repo.git"),
            Some("my-repo".to_string())
        );
    }

    #[test]
    fn repo_name_from_ssh_url_with_port() {
        assert_eq!(
            repo_name_from_url("ssh://git@host:2222/user/my-repo.git"),
            Some("my-repo".to_string())
        );
    }
}
