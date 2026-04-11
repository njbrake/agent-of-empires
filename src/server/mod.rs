//! Web dashboard for remote agent session access
//!
//! Provides an embedded axum web server that serves a responsive dashboard
//! for monitoring and interacting with agent sessions from any browser.

pub mod api;
pub mod auth;
pub mod ws;

use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axum::extract::ConnectInfo;
use axum::Router;
use rust_embed::Embed;
use tokio::sync::RwLock;

use crate::session::Instance;
use crate::session::Storage;

#[derive(Embed)]
#[folder = "web/dist/"]
struct StaticAssets;

/// Shared application state accessible by all request handlers.
pub struct AppState {
    pub profile: String,
    pub auth_token: Option<String>,
    pub read_only: bool,
    pub instances: RwLock<Vec<Instance>>,
    /// Controls whether non-local connections are accepted.
    /// None = all connections allowed (CLI mode, no filtering).
    /// Some(false) = only localhost (127.0.0.1, ::1) allowed.
    /// Some(true) = all authenticated connections allowed (remote access on).
    pub remote_enabled: Option<Arc<AtomicBool>>,
    /// Broadcast channel for session status change events.
    /// Desktop app subscribes to this for notifications.
    pub status_events: Option<tokio::sync::broadcast::Sender<Vec<StatusChange>>>,
}

/// Describes a session status transition detected by the poll loop.
#[derive(Clone, Debug)]
pub struct StatusChange {
    pub session_id: String,
    pub title: String,
    pub project_path: String,
    pub tool: String,
    pub old_status: String,
    pub new_status: String,
}

/// Configuration for starting the web server.
/// The CLI builds a default config; the desktop app customizes it.
pub struct ServerConfig {
    pub profile: String,
    pub host: String,
    pub port: u16,
    pub no_auth: bool,
    pub read_only: bool,
    /// Remote access guard. None = no filtering (CLI default).
    pub remote_enabled: Option<Arc<AtomicBool>>,
    /// Whether to print the access URL banner to stdout.
    pub print_banner: bool,
    /// Whether to write the URL to ~/.agent-of-empires/serve.url.
    pub write_url_file: bool,
    /// Signal sent after the TCP listener binds successfully.
    /// The desktop app waits on this before opening the webview.
    pub ready_signal: Option<tokio::sync::oneshot::Sender<String>>,
    /// Broadcast channel sender for status change events.
    /// If provided, the poll loop will send status transitions through it.
    pub status_events: Option<tokio::sync::broadcast::Sender<Vec<StatusChange>>>,
    /// Pre-generated auth token. If None, a new one is generated.
    /// The desktop app passes its own token so it can share it with the webview and QR code.
    pub auth_token: Option<String>,
}

/// Generate a 32-character alphanumeric auth token.
pub fn generate_auth_token() -> String {
    use rand::RngExt;
    let mut rng = rand::rng();
    (0..32)
        .map(|_| {
            let idx = rng.random_range(0..36u8);
            if idx < 10 {
                (b'0' + idx) as char
            } else {
                (b'a' + idx - 10) as char
            }
        })
        .collect()
}

/// Start the web server with the given configuration.
/// Returns the auth token (or empty string if no_auth).
pub async fn start_server_with_config(config: ServerConfig) -> anyhow::Result<String> {
    let instances = load_all_instances()?;

    let auth_token = if config.no_auth {
        eprintln!(
            "WARNING: Running without authentication. \
             Anyone with network access to this port can control your agent sessions."
        );
        None
    } else {
        Some(config.auth_token.unwrap_or_else(generate_auth_token))
    };

    let token_result = auth_token.clone().unwrap_or_default();

    let state = Arc::new(AppState {
        profile: config.profile,
        auth_token: auth_token.clone(),
        read_only: config.read_only,
        instances: RwLock::new(instances),
        remote_enabled: config.remote_enabled,
        status_events: config.status_events,
    });

    let app = build_router(state.clone());

    let addr = format!("{}:{}", config.host, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    // Build access URL
    let display_host = if config.host == "0.0.0.0" {
        "localhost"
    } else {
        &config.host
    };
    let url = if let Some(ref token) = auth_token {
        format!("http://{}:{}/?token={}", display_host, config.port, token)
    } else {
        format!("http://{}:{}/", display_host, config.port)
    };

    if config.print_banner {
        println!("aoe web dashboard running at:");
        println!("  {}", url);
        if auth_token.is_some() {
            println!();
            println!(
                "Open this URL in any browser. Share it to access from other devices on your network."
            );
        }
    }

    if config.write_url_file {
        if let Ok(app_dir) = crate::session::get_app_dir() {
            let _ = std::fs::write(app_dir.join("serve.url"), &url);
        }
    }

    // Fire ready signal so the desktop app knows the server is listening
    if let Some(tx) = config.ready_signal {
        let _ = tx.send(url);
    }

    // Spawn background status polling task
    let poll_state = state.clone();
    tokio::spawn(async move {
        status_poll_loop(poll_state).await;
    });

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await?;
    Ok(token_result)
}

/// Axum middleware that enforces remote access control.
/// Checks AppState.remote_enabled and the client's IP address.
pub async fn remote_access_middleware(
    state: axum::extract::State<Arc<AppState>>,
    connect_info: ConnectInfo<SocketAddr>,
    request: axum::http::Request<axum::body::Body>,
    next: axum::middleware::Next,
) -> axum::response::Response {
    use axum::http::StatusCode;
    use axum::response::IntoResponse;

    if let Some(ref remote_flag) = state.remote_enabled {
        if !remote_flag.load(Ordering::Relaxed) {
            let ip = connect_info.0.ip();
            let is_local = ip.is_loopback();
            if !is_local {
                return (
                    StatusCode::FORBIDDEN,
                    "Remote access is disabled. Enable it from the desktop app menu bar.",
                )
                    .into_response();
            }
        }
    }
    // remote_enabled is None (CLI mode) or true (remote on): pass through
    next.run(request).await
}

fn build_router(state: Arc<AppState>) -> Router {
    use axum::routing::{delete, get, patch, post};

    Router::new()
        // Session CRUD
        .route(
            "/api/sessions",
            get(api::list_sessions).post(api::create_session),
        )
        .route("/api/sessions/{id}", get(api::get_session))
        .route("/api/sessions/{id}/stop", post(api::stop_session))
        .route("/api/sessions/{id}/restart", post(api::restart_session))
        .route("/api/sessions/{id}", delete(api::delete_session))
        .route("/api/sessions/{id}", patch(api::update_session))
        .route("/api/sessions/{id}/diff", get(api::session_diff))
        // Agents
        .route("/api/agents", get(api::list_agents))
        // Groups
        .route("/api/groups", get(api::list_groups))
        // Profiles
        .route("/api/profiles", get(api::list_profiles))
        .route("/api/profiles", post(api::create_profile))
        .route("/api/profiles/{name}", delete(api::delete_profile))
        // Settings + themes
        .route(
            "/api/settings",
            get(api::get_settings).patch(api::update_settings),
        )
        .route("/api/themes", get(api::list_themes))
        // Worktrees
        .route("/api/worktrees", get(api::list_worktrees))
        // Terminal
        .route("/sessions/{id}/ws", get(ws::terminal_ws))
        // Static assets (Vite build output: assets/, manifest.json, sw.js, icons)
        .route("/assets/{*path}", get(serve_asset))
        .route("/manifest.json", get(serve_public_file))
        .route("/sw.js", get(serve_public_file))
        .route("/icon-192.png", get(serve_public_file))
        .route("/icon-512.png", get(serve_public_file))
        // SPA fallback: all other GET routes serve index.html
        .fallback(get(serve_index))
        // Remote access middleware (checks before auth)
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            remote_access_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ))
        .with_state(state)
}

async fn serve_index() -> impl axum::response::IntoResponse {
    serve_embedded_file("index.html")
}

async fn serve_asset(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> impl axum::response::IntoResponse {
    serve_embedded_file(&format!("assets/{}", path))
}

async fn serve_public_file(uri: axum::http::Uri) -> impl axum::response::IntoResponse {
    // Strip leading slash to match rust-embed paths
    let path = uri.path().trim_start_matches('/');
    serve_embedded_file(path)
}

fn serve_embedded_file(path: &str) -> axum::response::Response {
    use axum::http::{header, StatusCode};
    use axum::response::IntoResponse;

    match StaticAssets::get(path) {
        Some(file) => {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            (
                StatusCode::OK,
                [(header::CONTENT_TYPE, mime.as_ref().to_string())],
                file.data.to_vec(),
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}

/// Load sessions from all profiles, matching the TUI's "all profiles" view.
fn load_all_instances() -> anyhow::Result<Vec<Instance>> {
    let profiles = crate::session::list_profiles().unwrap_or_default();
    let mut all = Vec::new();
    for profile in &profiles {
        if let Ok(storage) = Storage::new(profile) {
            if let Ok(instances) = storage.load() {
                all.extend(instances);
            }
        }
    }
    // Also load from the default profile if it wasn't in the list
    if !profiles.iter().any(|p| p == "default") {
        if let Ok(storage) = Storage::new("default") {
            if let Ok(instances) = storage.load() {
                all.extend(instances);
            }
        }
    }
    Ok(all)
}

/// Background task that periodically refreshes session statuses.
/// Emits status change events through the broadcast channel when sessions transition.
async fn status_poll_loop(state: Arc<AppState>) {
    use std::collections::HashMap;

    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    let mut previous_statuses: HashMap<String, String> = HashMap::new();

    loop {
        interval.tick().await;
        let updated = tokio::task::spawn_blocking(move || {
            let mut instances = load_all_instances().unwrap_or_default();

            crate::tmux::refresh_session_cache();
            let pane_metadata = crate::tmux::batch_pane_metadata();

            for inst in &mut instances {
                let session_name = crate::tmux::Session::generate_name(&inst.id, &inst.title);
                let metadata = pane_metadata.get(&session_name);
                inst.update_status_with_metadata(metadata);
            }

            instances
        })
        .await;

        if let Ok(instances) = updated {
            // Detect status transitions and emit events
            if state.status_events.is_some() {
                let mut changes = Vec::new();
                for inst in &instances {
                    let current_status = format!("{:?}", inst.status);
                    let old_status = previous_statuses.get(&inst.id).cloned().unwrap_or_default();
                    if !old_status.is_empty() && old_status != current_status {
                        changes.push(StatusChange {
                            session_id: inst.id.clone(),
                            title: inst.title.clone(),
                            project_path: inst.project_path.clone(),
                            tool: inst.tool.clone(),
                            old_status: old_status.clone(),
                            new_status: current_status.clone(),
                        });
                    }
                    previous_statuses.insert(inst.id.clone(), current_status);
                }
                // Remove stale entries for sessions that no longer exist
                let current_ids: std::collections::HashSet<&str> =
                    instances.iter().map(|i| i.id.as_str()).collect();
                previous_statuses.retain(|id, _| current_ids.contains(id.as_str()));

                if !changes.is_empty() {
                    if let Some(ref tx) = state.status_events {
                        // Ignore send errors (no subscribers is fine)
                        let _ = tx.send(changes);
                    }
                }
            }

            *state.instances.write().await = instances;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
    use std::sync::atomic::AtomicBool;

    #[allow(dead_code)]
    fn make_state(remote_enabled: Option<Arc<AtomicBool>>) -> Arc<AppState> {
        Arc::new(AppState {
            profile: "default".to_string(),
            auth_token: Some("testtoken".to_string()),
            read_only: false,
            instances: RwLock::new(vec![]),
            remote_enabled,
            status_events: None,
        })
    }

    /// Helper: returns true if the middleware would allow this IP.
    fn is_allowed(remote_enabled: Option<Arc<AtomicBool>>, addr: SocketAddr) -> bool {
        if let Some(ref flag) = remote_enabled {
            if !flag.load(Ordering::Relaxed) {
                return addr.ip().is_loopback();
            }
        }
        true
    }

    #[test]
    fn test_remote_middleware_local_ipv4_always_passes() {
        let flag = Arc::new(AtomicBool::new(false));
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 12345);
        assert!(is_allowed(Some(flag), addr));
    }

    #[test]
    fn test_remote_middleware_local_ipv6_always_passes() {
        let flag = Arc::new(AtomicBool::new(false));
        let addr = SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), 12345);
        assert!(is_allowed(Some(flag), addr));
    }

    #[test]
    fn test_remote_middleware_remote_ip_blocked_when_off() {
        let flag = Arc::new(AtomicBool::new(false));
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), 12345);
        assert!(!is_allowed(Some(flag), addr));
    }

    #[test]
    fn test_remote_middleware_remote_ip_allowed_when_on() {
        let flag = Arc::new(AtomicBool::new(true));
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 100)), 12345);
        assert!(is_allowed(Some(flag), addr));
    }

    #[test]
    fn test_remote_middleware_none_allows_all() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 50)), 12345);
        assert!(is_allowed(None, addr));
    }

    #[test]
    fn test_server_config_default_matches_cli() {
        let config = ServerConfig {
            profile: "default".to_string(),
            host: "127.0.0.1".to_string(),
            port: 8080,
            no_auth: false,
            read_only: false,
            remote_enabled: None,
            print_banner: true,
            write_url_file: true,
            ready_signal: None,
            status_events: None,
            auth_token: None,
        };
        assert!(config.print_banner);
        assert!(config.write_url_file);
        assert!(config.remote_enabled.is_none());
        assert!(config.ready_signal.is_none());
    }

    #[test]
    fn test_server_config_desktop_mode() {
        let flag = Arc::new(AtomicBool::new(false));
        let (tx, _rx) = tokio::sync::oneshot::channel();
        let (events_tx, _events_rx) = tokio::sync::broadcast::channel(16);
        let config = ServerConfig {
            profile: "default".to_string(),
            host: "0.0.0.0".to_string(),
            port: 8080,
            no_auth: false,
            read_only: false,
            remote_enabled: Some(flag.clone()),
            print_banner: false,
            write_url_file: false,
            ready_signal: Some(tx),
            status_events: Some(events_tx),
            auth_token: Some("mytoken123".to_string()),
        };
        assert!(!config.print_banner);
        assert!(!config.write_url_file);
        assert!(config.remote_enabled.is_some());
        assert!(config.auth_token.is_some());
    }

    #[test]
    fn test_broadcast_no_subscribers_no_panic() {
        let (tx, rx) = tokio::sync::broadcast::channel::<Vec<StatusChange>>(16);
        // Drop the receiver, then send. Should not panic.
        drop(rx);
        let result = tx.send(vec![StatusChange {
            session_id: "test".to_string(),
            title: "test".to_string(),
            project_path: "/tmp".to_string(),
            tool: "claude".to_string(),
            old_status: "Running".to_string(),
            new_status: "Waiting".to_string(),
        }]);
        // SendError is expected when no receivers, but no panic
        assert!(result.is_err());
    }

    #[test]
    fn test_generate_auth_token_length() {
        let token = generate_auth_token();
        assert_eq!(token.len(), 32);
        assert!(token.chars().all(|c| c.is_ascii_alphanumeric()));
    }
}
