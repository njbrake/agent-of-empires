//! Web dashboard for remote agent session access
//!
//! Provides an embedded axum web server that serves a responsive dashboard
//! for monitoring and interacting with agent sessions from any browser.

pub mod api;
pub mod auth;
pub mod ws;

use std::sync::Arc;

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
}

pub async fn start_server(
    profile: &str,
    host: &str,
    port: u16,
    no_auth: bool,
    read_only: bool,
) -> anyhow::Result<()> {
    // Load initial session data from all profiles
    let instances = load_all_instances()?;

    // Generate auth token
    let auth_token = if no_auth {
        eprintln!(
            "WARNING: Running without authentication. \
             Anyone with network access to this port can control your agent sessions."
        );
        None
    } else {
        use rand::RngExt;
        let mut rng = rand::rng();
        let token: String = (0..32)
            .map(|_| {
                let idx = rng.random_range(0..36u8);
                if idx < 10 {
                    (b'0' + idx) as char
                } else {
                    (b'a' + idx - 10) as char
                }
            })
            .collect();
        Some(token)
    };

    let state = Arc::new(AppState {
        profile: profile.to_string(),
        auth_token: auth_token.clone(),
        read_only,
        instances: RwLock::new(instances),
    });

    // Build router
    let app = build_router(state.clone());

    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    // Build and print access URL
    let display_host = if host == "0.0.0.0" { "localhost" } else { host };
    let url = if let Some(ref token) = auth_token {
        format!("http://{}:{}/?token={}", display_host, port, token)
    } else {
        format!("http://{}:{}/", display_host, port)
    };

    println!("aoe web dashboard running at:");
    println!("  {}", url);
    if auth_token.is_some() {
        println!();
        println!(
            "Open this URL in any browser. Share it to access from other devices on your network."
        );
    }

    // Write URL to file so daemon users can retrieve it with `cat ~/.agent-of-empires/serve.url`
    if let Ok(app_dir) = crate::session::get_app_dir() {
        let _ = std::fs::write(app_dir.join("serve.url"), &url);
    }

    // Spawn background status polling task
    let poll_state = state.clone();
    tokio::spawn(async move {
        status_poll_loop(poll_state).await;
    });

    axum::serve(listener, app).await?;
    Ok(())
}

fn build_router(state: Arc<AppState>) -> Router {
    use axum::routing::{get, post};

    Router::new()
        // API + WebSocket routes
        .route("/api/sessions", get(api::list_sessions))
        .route("/api/sessions/{id}", get(api::get_session))
        .route("/api/sessions/{id}/stop", post(api::stop_session))
        .route("/api/sessions/{id}/restart", post(api::restart_session))
        .route("/sessions/{id}/ws", get(ws::terminal_ws))
        // Static assets (Vite build output: assets/, manifest.json, sw.js, icons)
        .route("/assets/{*path}", get(serve_asset))
        .route("/manifest.json", get(serve_public_file))
        .route("/sw.js", get(serve_public_file))
        .route("/icon-192.png", get(serve_public_file))
        .route("/icon-512.png", get(serve_public_file))
        // SPA fallback: all other GET routes serve index.html
        .fallback(get(serve_index))
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
async fn status_poll_loop(state: Arc<AppState>) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
        interval.tick().await;
        // Run blocking tmux subprocess calls in a dedicated thread
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
            *state.instances.write().await = instances;
        }
    }
}
