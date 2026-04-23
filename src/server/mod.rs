//! Web dashboard for remote agent session access
//!
//! Provides an embedded axum web server that serves a responsive dashboard
//! for monitoring and interacting with agent sessions from any browser.

pub mod api;
pub mod auth;
pub mod login;
pub mod push;
pub mod push_send;
pub mod rate_limit;
pub mod tunnel;
pub mod ws;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use rust_embed::Embed;
use serde::Serialize;
use tokio::sync::{broadcast, RwLock};
use tracing::info;

use self::push::{PushState, StatusChange, STATUS_CHANNEL_CAPACITY};

use crate::session::Instance;
use crate::session::Storage;

use self::rate_limit::RateLimiter;

#[derive(Embed)]
#[folder = "web/dist/"]
struct StaticAssets;

// ── DeviceInfo ──────────────────────────────────────────────────────────────

/// A device that has connected to the dashboard.
#[derive(Clone, Serialize)]
pub struct DeviceInfo {
    pub ip: String,
    pub user_agent: String,
    pub first_seen: chrono::DateTime<chrono::Utc>,
    pub last_seen: chrono::DateTime<chrono::Utc>,
    pub request_count: u64,
}

// ── TokenManager ────────────────────────────────────────────────────────────

struct TokenState {
    current: Option<String>,
    previous: Option<String>,
    grace_expires: Option<tokio::time::Instant>,
    lifetime: Duration,
}

/// Manages auth tokens with rotation and grace periods.
pub struct TokenManager {
    state: RwLock<TokenState>,
}

impl TokenManager {
    pub fn new(initial_token: Option<String>, lifetime: Duration) -> Self {
        Self {
            state: RwLock::new(TokenState {
                current: initial_token,
                previous: None,
                grace_expires: None,
                lifetime,
            }),
        }
    }

    /// Check if auth is disabled (no-auth mode).
    pub async fn is_no_auth(&self) -> bool {
        self.state.read().await.current.is_none()
    }

    /// Validate a token against current and previous (grace period).
    /// Returns `(is_valid, needs_cookie_upgrade)`.
    pub async fn validate(&self, token: &str) -> (bool, bool) {
        let state = self.state.read().await;

        if let Some(ref current) = state.current {
            if auth::constant_time_eq(token, current) {
                return (true, false);
            }
        }

        // Check previous token within grace period
        if let Some(ref previous) = state.previous {
            if let Some(grace_expires) = state.grace_expires {
                if tokio::time::Instant::now() < grace_expires
                    && auth::constant_time_eq(token, previous)
                {
                    return (true, true);
                }
            }
        }

        (false, false)
    }

    /// Get the current token value (for setting cookies).
    pub async fn current_token(&self) -> Option<String> {
        self.state.read().await.current.clone()
    }

    pub async fn lifetime_secs(&self) -> u64 {
        self.state.read().await.lifetime.as_secs()
    }

    /// Clear the previous token after the grace period has expired.
    /// Used by the rotation task after the 5-minute grace window.
    pub async fn clear_previous(&self) {
        let mut state = self.state.write().await;
        state.previous = None;
        state.grace_expires = None;
    }

    /// Rotate: generate new token, move current to previous with grace period.
    pub async fn rotate(&self) {
        let mut state = self.state.write().await;
        let new_token = generate_token();

        state.previous = state.current.take();
        state.current = Some(new_token.clone());
        state.grace_expires = Some(tokio::time::Instant::now() + Duration::from_secs(300));

        // Persist to disk
        if let Ok(app_dir) = crate::session::get_app_dir() {
            write_secret_file(&app_dir.join("serve.token"), &new_token);
        }

        info!("Auth token rotated (previous token valid for 5 more minutes)");
    }

    /// Spawn a background rotation task (only in remote mode).
    pub fn spawn_rotation_task(self: &Arc<Self>) {
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                let lifetime = manager.state.read().await.lifetime;
                tokio::time::sleep(lifetime).await;
                manager.rotate().await;

                // After grace period, clear previous
                tokio::time::sleep(Duration::from_secs(300)).await;
                {
                    let mut state = manager.state.write().await;
                    state.previous = None;
                    state.grace_expires = None;
                }
            }
        });
    }
}

// ── AppState ────────────────────────────────────────────────────────────────

/// Per-profile cleanup defaults with a refresh timestamp. Re-resolved from
/// disk after `CLEANUP_DEFAULTS_TTL`.
pub struct CleanupDefaultsCache {
    pub refreshed_at: std::time::Instant,
    pub entries: std::collections::HashMap<String, api::CleanupDefaults>,
}

pub const CLEANUP_DEFAULTS_TTL: std::time::Duration = std::time::Duration::from_secs(30);

impl CleanupDefaultsCache {
    pub fn stale(&self) -> bool {
        self.refreshed_at.elapsed() >= CLEANUP_DEFAULTS_TTL
    }
}

/// Shared application state accessible by all request handlers.
pub struct AppState {
    pub profile: String,
    pub read_only: bool,
    pub instances: RwLock<Vec<Instance>>,
    pub token_manager: Arc<TokenManager>,
    pub login_manager: Arc<login::LoginManager>,
    pub rate_limiter: Arc<RateLimiter>,
    pub devices: RwLock<Vec<DeviceInfo>>,
    pub behind_tunnel: bool,
    /// Per-instance mutex guarding mutations that must not interleave
    /// (e.g. `ensure_session` decide-and-restart). Entries are created on
    /// first use and live for the lifetime of the process — there are only
    /// as many as the user has sessions.
    pub instance_locks: RwLock<std::collections::HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Cached per-profile cleanup defaults for the delete dialog, with a
    /// timestamp so we re-resolve after config changes (see
    /// `CLEANUP_DEFAULTS_TTL`).
    pub cleanup_defaults_cache: RwLock<CleanupDefaultsCache>,
    /// Cached remote owner per repo path. Remote owners don't change, so
    /// entries live for the lifetime of the process.
    pub remote_owner_cache: RwLock<std::collections::HashMap<String, Option<String>>>,
    /// Broadcasts session status transitions to consumers (currently the
    /// push-notification module). Emitted from `status_poll_loop` after
    /// each tmux scrape when `old != new`. Keep the Sender around even
    /// when no receivers exist so callers can emit without checking.
    pub status_tx: broadcast::Sender<StatusChange>,
    /// Web Push state: VAPID keypair, subscription store, VAPID subject.
    /// None when `web.notifications_enabled` is false at startup (the
    /// feature is fully off and endpoints return 404).
    pub push: Option<Arc<PushState>>,
    /// Cached value of `web.notifications_enabled` at startup. Changes
    /// to the config flag require a server restart to take effect; this
    /// is a documented limitation of the toggle for v1.
    pub push_enabled: bool,
    /// Snapshot of the resolved WebConfig at startup. Consumed by the
    /// push consumer task to evaluate per-event-type defaults.
    pub web_config: crate::session::config::WebConfig,
    /// Per-tmux-session primary WebSocket client. Maps tmux session name
    /// to the client ID that most recently sent keyboard input. Only the
    /// primary client's resize messages are applied to its PTY, preventing
    /// multiple browser viewports from fighting over the tmux window size.
    pub session_primaries: Arc<RwLock<std::collections::HashMap<String, String>>>,
    /// Per-tmux-session refcount of clients currently asking the pane's
    /// process tree to be paused (SIGSTOP). Incremented by `pause_output`,
    /// decremented by `resume_output` and on WebSocket disconnect. The
    /// pane's process is SIGSTOP-ed on 0→N transitions and SIGCONT-ed on
    /// N→0, so two mobile clients scrolling concurrently don't have one's
    /// `resume_output` un-pause the other's scrollback read.
    pub session_pause_counts: Arc<tokio::sync::Mutex<std::collections::HashMap<String, u32>>>,
    /// Epoch-millis timestamp of the most recent authenticated API request.
    /// Updated by auth middleware on every successful auth. The push consumer
    /// checks this to suppress notifications when someone is actively using
    /// the web dashboard (on any device).
    pub last_web_activity: std::sync::atomic::AtomicI64,
}

impl AppState {
    /// Get or create the per-instance serialization mutex. The outer
    /// `RwLock` is only held long enough to insert/lookup the `Arc<Mutex>`;
    /// the caller awaits the inner mutex without holding the map lock.
    pub async fn instance_lock(&self, id: &str) -> Arc<tokio::sync::Mutex<()>> {
        {
            let guard = self.instance_locks.read().await;
            if let Some(lock) = guard.get(id) {
                return lock.clone();
            }
        }
        let mut guard = self.instance_locks.write().await;
        guard
            .entry(id.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    /// Record that an authenticated web client just made a request.
    pub fn touch_web_activity(&self) {
        self.last_web_activity
            .store(epoch_millis(), std::sync::atomic::Ordering::Relaxed);
    }

    /// Returns true if an authenticated web request arrived within `threshold`.
    pub fn web_active_within(&self, threshold: std::time::Duration) -> bool {
        let last = self
            .last_web_activity
            .load(std::sync::atomic::Ordering::Relaxed);
        if last == 0 {
            return false;
        }
        let elapsed_ms = epoch_millis() - last;
        elapsed_ms >= 0 && (elapsed_ms as u64) < threshold.as_millis() as u64
    }
}

fn epoch_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

// ── Server ──────────────────────────────────────────────────────────────────

pub struct ServerConfig<'a> {
    pub profile: &'a str,
    pub host: &'a str,
    pub port: u16,
    pub no_auth: bool,
    pub read_only: bool,
    pub remote: bool,
    pub tunnel_name: Option<&'a str>,
    pub tunnel_url: Option<&'a str>,
    pub no_tailscale: bool,
    pub is_daemon: bool,
    pub passphrase: Option<&'a str>,
}

pub async fn start_server(config: ServerConfig<'_>) -> anyhow::Result<()> {
    let ServerConfig {
        profile,
        host,
        port,
        no_auth,
        read_only,
        remote,
        tunnel_name,
        tunnel_url,
        no_tailscale,
        is_daemon,
        passphrase,
    } = config;
    let instances = load_all_instances()?;

    // Load or generate auth token
    let auth_token = if no_auth {
        eprintln!(
            "WARNING: Running without authentication. \
             Anyone with network access to this port can control your agent sessions."
        );
        None
    } else {
        Some(load_or_generate_token()?)
    };

    let token_lifetime = if remote {
        Duration::from_secs(4 * 60 * 60) // 4 hours
    } else {
        Duration::from_secs(24 * 60 * 60) // 24 hours (existing behavior)
    };

    let token_manager = Arc::new(TokenManager::new(auth_token.clone(), token_lifetime));
    let login_manager = Arc::new(login::LoginManager::new(passphrase));
    let rate_limiter = Arc::new(RateLimiter::new());

    if login_manager.is_enabled() {
        info!("Passphrase login enabled (second-factor authentication)");
    }

    // Persist the plaintext passphrase so the TUI can display it on
    // reopen, including after a TUI restart or when the daemon was
    // started from the CLI. Owner-only perms; cleaned up on shutdown.
    if let Some(pp) = passphrase {
        if let Ok(app_dir) = crate::session::get_app_dir() {
            write_secret_file(&app_dir.join("serve.passphrase"), pp);
        }
    }

    // Push notifications: initialize only when the operator flag is on at
    // startup. Flipping it later requires a server restart to take effect.
    let config = crate::session::resolve_config(profile).unwrap_or_default();
    let push_enabled = config.web.notifications_enabled;
    let push_state = if push_enabled {
        match crate::session::get_app_dir() {
            Ok(dir) => match PushState::init(&dir) {
                Ok(s) => Some(Arc::new(s)),
                Err(e) => {
                    tracing::warn!(
                        "Push notifications disabled: failed to init VAPID/state: {}",
                        e
                    );
                    None
                }
            },
            Err(e) => {
                tracing::warn!("Push notifications disabled: app_dir unavailable: {}", e);
                None
            }
        }
    } else {
        info!("Push notifications disabled by web.notifications_enabled=false");
        None
    };

    let state = Arc::new(AppState {
        profile: profile.to_string(),
        read_only,
        instances: RwLock::new(instances),
        token_manager: Arc::clone(&token_manager),
        login_manager: Arc::clone(&login_manager),
        rate_limiter: Arc::clone(&rate_limiter),
        devices: RwLock::new(Vec::new()),
        behind_tunnel: remote,
        instance_locks: RwLock::new(std::collections::HashMap::new()),
        cleanup_defaults_cache: RwLock::new(CleanupDefaultsCache {
            // Seed with an already-stale timestamp so the first request
            // forces a fresh resolve instead of handing out an empty map.
            refreshed_at: std::time::Instant::now() - CLEANUP_DEFAULTS_TTL,
            entries: std::collections::HashMap::new(),
        }),
        remote_owner_cache: RwLock::new(std::collections::HashMap::new()),
        session_primaries: Arc::new(RwLock::new(std::collections::HashMap::new())),
        session_pause_counts: Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new())),
        status_tx: broadcast::channel(STATUS_CHANNEL_CAPACITY).0,
        push: push_state,
        push_enabled,
        web_config: config.web.clone(),
        last_web_activity: std::sync::atomic::AtomicI64::new(0),
    });

    let app = build_router(state.clone());
    let addr = format!("{}:{}", host, port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    let local_port = listener.local_addr()?.port();

    // Start tunnel if remote mode. Preference order:
    //  1. User-specified named Cloudflare tunnel (stable, explicit choice).
    //  2. Tailscale Funnel if tailscale is installed and logged in
    //     (stable .ts.net URL, installable PWAs keep working).
    //  3. Cloudflare quick tunnel (fallback; URL rotates per restart,
    //     which breaks installed PWAs).
    // Capture the Tailscale probe result before the branch so the
    // debug log shows why we did or didn't take the Tailscale path.
    // The probe itself also logs details about each underlying call.
    let tailscale_ok = if remote && !no_tailscale {
        let available = tunnel::tailscale_available().await;
        tracing::debug!(
            no_tailscale,
            tailscale_available = available,
            "tunnel: choosing transport"
        );
        available
    } else {
        if remote && no_tailscale {
            tracing::debug!("tunnel: --no-tailscale set, skipping Tailscale auto-detection");
        }
        false
    };

    let tunnel_handle = if remote {
        let handle = if let (Some(name), Some(url)) = (tunnel_name, tunnel_url) {
            tunnel::TunnelHandle::spawn_named(name, url, local_port).await?
        } else if tailscale_ok {
            info!("Tailscale detected; using Tailscale Funnel for stable HTTPS origin");
            // Do NOT fall back to Cloudflare on Tailscale failure: the
            // user is on the Tailscale path because they want the
            // stable-URL benefit, and silently downgrading to a rotating
            // Cloudflare URL would break the feature they wanted. Bail
            // with the real error; the user fixes Tailscale or passes
            // --no-tailscale to explicitly opt into Cloudflare.
            tunnel::TunnelHandle::spawn_tailscale(local_port)
                .await
                .map_err(|e| {
                    anyhow::anyhow!(
                        "Tailscale Funnel setup failed: {e}\n\n\
                         aoe detected a logged-in Tailscale on this host and did not \
                         fall back to Cloudflare, because doing so silently would \
                         give you a rotating URL that breaks installed PWAs (the \
                         reason Tailscale is the preferred transport).\n\n\
                         Ways to move forward:\n  \
                         - Fix the Tailscale issue above and re-run `aoe serve --remote`.\n  \
                         - Re-run with `aoe serve --remote --no-tailscale` to use \
                         Cloudflare intentionally (quick-tunnel URL rotates on restart).\n  \
                         - Re-run with `--tunnel-name <name> --tunnel-url <host>` \
                         to use a named Cloudflare tunnel."
                    )
                })?
        } else {
            tunnel::TunnelHandle::spawn_quick(local_port).await?
        };

        let tunnel_url_with_token = if let Some(ref token) = auth_token {
            format!("{}/?token={}", handle.url, token)
        } else {
            handle.url.clone()
        };

        // Print QR code unless running as daemon
        if !is_daemon {
            eprintln!(
                "Remote access via {} (URL is {}).",
                match handle.mode_label() {
                    "tailscale" => "Tailscale Funnel",
                    "tunnel" => "Cloudflare tunnel",
                    other => other,
                },
                if handle.is_stable_origin() {
                    "stable across restarts"
                } else {
                    "temporary; rotates on restart"
                }
            );
            tunnel::print_qr_code(&tunnel_url_with_token);
            if !handle.is_stable_origin() {
                eprintln!(
                    "\nNote: this Cloudflare quick tunnel URL changes on every restart.\n\
                     Installed PWAs (home-screen apps) break when the URL changes.\n\
                     For a stable installable dashboard, install Tailscale and run\n\
                     `tailscale up` on this host before `aoe serve --remote`, or use\n\
                     a named Cloudflare tunnel via --tunnel-name/--tunnel-url.\n"
                );
            }
        }

        // Write tunnel URL for daemon discovery. Single-line content:
        // backward-compatible with any consumer that does `head -1 serve.url`,
        // and the TUI parses both single- and multi-URL formats.
        if let Ok(app_dir) = crate::session::get_app_dir() {
            write_secret_file(&app_dir.join("serve.url"), &tunnel_url_with_token);
            // serve.mode lets the TUI reattach to a running daemon and
            // render the right transport label: "tunnel" for Cloudflare,
            // "tailscale" for Tailscale Funnel, "local" for local-only.
            let mode = format!("{}\n", handle.mode_label());
            let _ = std::fs::write(app_dir.join("serve.mode"), mode);
        }

        // Start health monitor (uses CancellationToken internally)
        handle.spawn_health_monitor();

        Some(handle)
    } else {
        // Local mode: print URLs as before.
        let make_url = |h: &str| {
            if let Some(ref token) = auth_token {
                format!("http://{}:{}/?token={}", h, port, token)
            } else {
                format!("http://{}:{}/", h, port)
            }
        };

        // Collect labeled URLs in preference order (Tailscale > LAN > localhost).
        // When bound to 0.0.0.0 we're reachable on all three; on a specific
        // host we just surface that one.
        let labeled_urls: Vec<(IpKind, String)> = if host == "0.0.0.0" {
            let mut urls: Vec<(IpKind, String)> = discover_tagged_ips()
                .into_iter()
                .map(|(kind, ip)| (kind, make_url(&ip.to_string())))
                .collect();
            urls.push((IpKind::Loopback, make_url("localhost")));
            urls
        } else {
            vec![(IpKind::Loopback, make_url(host))]
        };

        println!("aoe web dashboard running at:");
        for (_, u) in &labeled_urls {
            println!("  {}", u);
        }
        if auth_token.is_some() {
            println!();
            println!(
                "Open any URL above in a browser. Share it to access from other devices on your network."
            );
        }

        // serve.url: primary URL on line 1 (unlabeled, backward-compatible
        // with any `head -1 serve.url` consumer). Alternates below as
        // `kind\turl` so the TUI can cycle them. Always owner-only perms
        // since the URL embeds the auth token.
        if let Ok(app_dir) = crate::session::get_app_dir() {
            let mut contents = String::new();
            if let Some((_, primary)) = labeled_urls.first() {
                contents.push_str(primary);
                contents.push('\n');
            }
            for (kind, url) in labeled_urls.iter().skip(1) {
                contents.push_str(kind.label());
                contents.push('\t');
                contents.push_str(url);
                contents.push('\n');
            }
            write_secret_file(&app_dir.join("serve.url"), &contents);
            let _ = std::fs::write(app_dir.join("serve.mode"), "local\n");
        }

        None
    };

    // Spawn background tasks
    let poll_state = state.clone();
    tokio::spawn(async move {
        status_poll_loop(poll_state).await;
    });

    // Push-notification consumer: subscribes to status_tx, applies
    // dwell + cooldown, sends pushes. No-op when push_state is None
    // (feature disabled via web.notifications_enabled=false).
    push::spawn_consumer(state.clone());

    rate_limiter.spawn_cleanup_task();
    login_manager.spawn_cleanup_task();

    if remote {
        // Inline the rotation loop here rather than calling
        // token_manager.spawn_rotation_task() so we can also invalidate
        // push subscriptions whose owner hash is no longer valid after
        // rotation. Behavior otherwise matches the original: wait one
        // lifetime, rotate, wait 300s grace, clear previous.
        let rot_state = state.clone();
        tokio::spawn(async move {
            loop {
                let lifetime = rot_state.token_manager.lifetime_secs().await;
                tokio::time::sleep(std::time::Duration::from_secs(lifetime)).await;

                // Capture the hashes of the current and (about-to-be)
                // previous tokens BEFORE rotating, so we know which
                // owner-hashes are still valid in the store.
                let pre_rotate_current = rot_state.token_manager.current_token().await;
                rot_state.token_manager.rotate().await;
                let post_rotate_current = rot_state.token_manager.current_token().await;

                if let Some(push) = rot_state.push.as_ref() {
                    let mut valid_hashes: Vec<[u8; 32]> = Vec::new();
                    if let Some(t) = &post_rotate_current {
                        valid_hashes.push(push::sha256_token(t));
                    }
                    if let Some(t) = &pre_rotate_current {
                        // The old token remains in the grace period (5m)
                        // so devices that haven't yet picked up the new
                        // token should keep receiving pushes.
                        valid_hashes.push(push::sha256_token(t));
                    }
                    // In no-auth mode the token is None and we use a
                    // zero hash; preserve that so zero-hash subs survive.
                    if valid_hashes.is_empty() {
                        valid_hashes.push([0u8; 32]);
                    }
                    match push.store.retain_owners(&valid_hashes).await {
                        Ok(0) => {}
                        Ok(n) => tracing::info!(
                            removed = n,
                            "push: dropped subscriptions whose owner-hash is no longer valid after rotation"
                        ),
                        Err(e) => tracing::warn!(error = %e, "push: retain_owners failed"),
                    }
                }

                // After grace period, the previous token becomes invalid.
                // Clear it AND drop any subscriptions that were bound
                // only to the old hash (retain_owners with only the new).
                tokio::time::sleep(std::time::Duration::from_secs(300)).await;
                // Clear previous token inside TokenManager. Reuse its
                // internal state access via a tiny helper on the manager.
                rot_state.token_manager.clear_previous().await;

                if let Some(push) = rot_state.push.as_ref() {
                    let mut valid_hashes: Vec<[u8; 32]> = Vec::new();
                    if let Some(t) = rot_state.token_manager.current_token().await {
                        valid_hashes.push(push::sha256_token(&t));
                    }
                    if valid_hashes.is_empty() {
                        valid_hashes.push([0u8; 32]);
                    }
                    let _ = push.store.retain_owners(&valid_hashes).await;
                }
            }
        });
    }

    // Graceful shutdown: SIGINT (Ctrl-C), SIGTERM (`aoe serve --stop`),
    // and SIGHUP (parent session died). Without these, the default handler
    // kills the process immediately, skipping PID/URL file cleanup.
    let shutdown_signal = async {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigterm = signal(SignalKind::terminate()).ok();
            let mut sighup = signal(SignalKind::hangup()).ok();
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {
                    info!("Received SIGINT, shutting down...");
                }
                _ = async { match sigterm { Some(ref mut s) => { s.recv().await; } None => std::future::pending().await } } => {
                    info!("Received SIGTERM, shutting down...");
                }
                _ = async { match sighup { Some(ref mut s) => { s.recv().await; } None => std::future::pending().await } } => {
                    info!("Received SIGHUP, shutting down...");
                }
            }
        }
        #[cfg(not(unix))]
        {
            let _ = tokio::signal::ctrl_c().await;
            info!("Shutting down...");
        }
    };

    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown_signal)
    .await?;

    // Clean up tunnel (cancels health monitor, then sends SIGTERM to cloudflared)
    if let Some(handle) = tunnel_handle {
        handle.shutdown().await;
    }

    if let Ok(app_dir) = crate::session::get_app_dir() {
        let _ = std::fs::remove_file(app_dir.join("serve.passphrase"));
    }

    Ok(())
}

fn build_router(state: Arc<AppState>) -> Router {
    use axum::routing::{get, patch, post};

    Router::new()
        // Sessions
        .route(
            "/api/sessions",
            get(api::list_sessions).post(api::create_session),
        )
        .route(
            "/api/sessions/{id}",
            patch(api::rename_session).delete(api::delete_session),
        )
        .route(
            "/api/sessions/{id}/diff/files",
            get(api::session_diff_files),
        )
        .route("/api/sessions/{id}/diff/file", get(api::session_diff_file))
        .route("/api/sessions/{id}/ensure", post(api::ensure_session))
        .route(
            "/api/sessions/{id}/notifications",
            patch(api::update_session_notifications),
        )
        .route("/api/sessions/{id}/terminal", post(api::ensure_terminal))
        .route(
            "/api/sessions/{id}/container-terminal",
            post(api::ensure_container_terminal),
        )
        // Agents
        .route("/api/agents", get(api::list_agents))
        // Wizard support
        .route("/api/profiles", get(api::list_profiles))
        .route("/api/filesystem/browse", get(api::browse_filesystem))
        .route("/api/filesystem/home", get(api::filesystem_home))
        .route("/api/git/branches", get(api::list_branches))
        .route("/api/git/clone", post(api::clone_repo))
        .route("/api/groups", get(api::list_groups))
        .route("/api/docker/status", get(api::docker_status))
        // Settings + themes
        .route(
            "/api/settings",
            get(api::get_settings).patch(api::update_settings),
        )
        .route("/api/themes", get(api::list_themes))
        // Push notifications
        .route("/api/push/status", get(push::get_status))
        .route(
            "/api/push/vapid-public-key",
            get(push::get_vapid_public_key),
        )
        .route("/api/push/subscribe", post(push::subscribe))
        .route("/api/push/unsubscribe", post(push::unsubscribe))
        .route("/api/push/test", post(push::test))
        // Login (second-factor auth)
        .route("/api/login", post(login::login_handler))
        .route("/api/logout", post(login::logout_handler))
        .route("/api/login/status", get(login::login_status_handler))
        // Devices
        .route("/api/devices", get(api::list_devices))
        // About (version, auth status, read-only state)
        .route("/api/about", get(api::get_about))
        // Terminal WebSockets
        .route("/sessions/{id}/ws", get(ws::terminal_ws))
        .route("/sessions/{id}/terminal/ws", get(ws::paired_terminal_ws))
        .route(
            "/sessions/{id}/container-terminal/ws",
            get(ws::container_terminal_ws),
        )
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
        .layer(axum::middleware::from_fn(security_headers))
        .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024))
        .with_state(state)
}

/// Content-Security-Policy for the dashboard.
///
/// - `default-src 'self'`: deny everything we don't explicitly allow.
/// - `script-src 'self' 'wasm-unsafe-eval'`: wterm compiles WebAssembly;
///   the `wasm-unsafe-eval` source is the CSP3 opt-in for WASM compilation.
/// - `style-src 'self' 'unsafe-inline'`: React writes to element.style at
///   runtime (terminal theme vars, font-size updates) and Tailwind v4 emits
///   inline `<style>` blocks in dev. Blocking inline styles breaks wterm.
/// - `img-src 'self' data: https://github.com https://avatars.githubusercontent.com`:
///   repo-owner avatars are loaded from `github.com/{user}.png` which 302s
///   to `avatars.githubusercontent.com`; CSP checks both URLs across the
///   redirect, so both hosts must be allowed. `data:` covers inline icons.
/// - `font-src 'self'`: Geist fonts are bundled under /fonts/.
/// - `connect-src 'self' ws: wss:`: REST + PTY WebSocket to same origin.
/// - `frame-ancestors 'none'`: CSP-native equivalent of X-Frame-Options.
/// - `base-uri 'self'`, `form-action 'self'`, `object-src 'none'`: tighten
///   the usual attack surfaces on injection bugs.
const CSP: &str = "default-src 'self'; \
    script-src 'self' 'wasm-unsafe-eval'; \
    style-src 'self' 'unsafe-inline'; \
    img-src 'self' data: https://github.com https://avatars.githubusercontent.com; \
    font-src 'self'; \
    connect-src 'self' ws: wss:; \
    frame-ancestors 'none'; \
    base-uri 'self'; \
    form-action 'self'; \
    object-src 'none'";

/// Middleware that adds security headers to all responses.
async fn security_headers(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert("x-frame-options", "DENY".parse().unwrap());
    headers.insert("x-content-type-options", "nosniff".parse().unwrap());
    headers.insert("referrer-policy", "no-referrer".parse().unwrap());
    headers.insert("content-security-policy", CSP.parse().unwrap());
    response
}

async fn serve_index(uri: axum::http::Uri) -> impl axum::response::IntoResponse {
    use axum::response::IntoResponse;

    let path = uri.path().trim_start_matches('/');
    if !path.is_empty() && path != "index.html" && path.contains('.') {
        if let Some(file) = StaticAssets::get(path) {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            return (
                axum::http::StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, mime.as_ref().to_string())],
                file.data.to_vec(),
            )
                .into_response();
        }
    }
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

/// Kind tag for a local IPv4 address. Ordering in this enum is also the
/// preference order for picking the "primary" URL to show in a QR: when
/// the user serves on a Tailnet, that's almost always the one they want
/// a phone (on cellular) to scan, not the LAN IP behind their NAT.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum IpKind {
    Tailscale,
    Lan,
    Loopback,
}

impl IpKind {
    pub fn label(self) -> &'static str {
        match self {
            IpKind::Tailscale => "tailscale",
            IpKind::Lan => "lan",
            IpKind::Loopback => "localhost",
        }
    }
}

/// Classify a v4 address into Tailscale (CGNAT 100.64.0.0/10, which is
/// what Tailscale hands out), regular LAN (RFC1918), or loopback.
/// Public non-RFC1918 / non-CGNAT addresses are rare on an `aoe serve`
/// host (would mean serving directly on the open internet) and fall
/// through to `Lan` so we still surface them.
pub fn classify_ip(ip: std::net::Ipv4Addr) -> IpKind {
    let octets = ip.octets();
    if ip.is_loopback() {
        return IpKind::Loopback;
    }
    // CGNAT 100.64.0.0/10 (RFC 6598). Second octet is 64..=127.
    if octets[0] == 100 && (64..=127).contains(&octets[1]) {
        return IpKind::Tailscale;
    }
    IpKind::Lan
}

/// Discover non-loopback IPv4 addresses on all network interfaces,
/// tagged by kind and sorted so the preferred URL (Tailscale > LAN)
/// is first. Caller decides whether to include loopback.
pub fn discover_tagged_ips() -> Vec<(IpKind, std::net::Ipv4Addr)> {
    let mut out: Vec<(IpKind, std::net::Ipv4Addr)> = Vec::new();
    if let Ok(addrs) = nix::ifaddrs::getifaddrs() {
        for ifaddr in addrs {
            if let Some(addr) = ifaddr.address {
                if let Some(sockaddr) = addr.as_sockaddr_in() {
                    let ip = sockaddr.ip();
                    if ip.is_loopback() {
                        continue;
                    }
                    if !out.iter().any(|(_, existing)| *existing == ip) {
                        out.push((classify_ip(ip), ip));
                    }
                }
            }
        }
    }
    out.sort_by_key(|(k, _)| *k);
    out
}

/// Write a file with owner-only permissions (0600) to protect secrets.
#[cfg(unix)]
fn write_secret_file(path: &std::path::Path, contents: &str) {
    use std::os::unix::fs::OpenOptionsExt;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
    {
        use std::io::Write;
        let _ = file.write_all(contents.as_bytes());
    }
}

#[cfg(not(unix))]
fn write_secret_file(path: &std::path::Path, contents: &str) {
    let _ = std::fs::write(path, contents);
}

/// Generate a cryptographically random 64-character hex token (256 bits of entropy).
pub(crate) fn generate_token() -> String {
    use rand::RngExt;
    let mut bytes = [0u8; 32];
    rand::rng().fill(&mut bytes);
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Validate that a token matches the expected format.
/// Accepts 64-char hex (new) or 32-char alphanumeric (legacy).
fn is_valid_token_format(token: &str) -> bool {
    let len = token.len();
    (len == 64 || len == 32)
        && token
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c.is_ascii_lowercase())
}

/// Load an existing auth token from disk if it's less than 24 hours old,
/// otherwise generate a fresh one and persist it.
fn load_or_generate_token() -> anyhow::Result<String> {
    let app_dir = crate::session::get_app_dir()?;
    let token_path = app_dir.join("serve.token");

    // Try to reuse existing token if fresh enough
    if let Ok(metadata) = std::fs::metadata(&token_path) {
        if let Ok(modified) = metadata.modified() {
            let age = std::time::SystemTime::now()
                .duration_since(modified)
                .unwrap_or_default();
            if age < std::time::Duration::from_secs(24 * 60 * 60) {
                if let Ok(token) = std::fs::read_to_string(&token_path) {
                    let token = token.trim().to_string();
                    if !token.is_empty() && is_valid_token_format(&token) {
                        return Ok(token);
                    }
                }
            }
        }
    }

    let token = generate_token();
    write_secret_file(&token_path, &token);
    Ok(token)
}

/// Load sessions from all profiles, matching the TUI's "all profiles" view.
fn load_all_instances() -> anyhow::Result<Vec<Instance>> {
    let profiles = crate::session::list_profiles().unwrap_or_default();
    let mut all = Vec::new();
    for profile in &profiles {
        if let Ok(storage) = Storage::new(profile) {
            if let Ok(mut instances) = storage.load() {
                for inst in &mut instances {
                    inst.source_profile = profile.clone();
                }
                all.extend(instances);
            }
        }
    }
    // Also load from the default profile if it wasn't in the list
    if !profiles.iter().any(|p| p == "default") {
        if let Ok(storage) = Storage::new("default") {
            if let Ok(mut instances) = storage.load() {
                for inst in &mut instances {
                    inst.source_profile = "default".to_string();
                }
                all.extend(instances);
            }
        }
    }
    Ok(all)
}

/// Background task that periodically refreshes session statuses. On each
/// tick, diffs pre- and post-refresh statuses and emits a `StatusChange`
/// on `state.status_tx` for every transition. Keeping the diff here,
/// rather than pushing it into `Instance::update_status_with_metadata`,
/// leaves the session module free of any broadcast-channel dependency
/// and keeps TUI/CLI callers unchanged.
async fn status_poll_loop(state: Arc<AppState>) {
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(2));
    loop {
        interval.tick().await;

        // Snapshot prior statuses so we can detect transitions without
        // holding the lock across the blocking tmux work.
        let prev: std::collections::HashMap<String, crate::session::Status> = {
            let instances = state.instances.read().await;
            instances.iter().map(|i| (i.id.clone(), i.status)).collect()
        };

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
            // Emit transitions before swapping in the new snapshot so
            // consumers see events in the same order regardless of when
            // they read state.instances themselves.
            let now = chrono::Utc::now();
            for inst in &instances {
                if let Some(old) = prev.get(&inst.id) {
                    if *old != inst.status {
                        // send() errors only when there are no receivers;
                        // that's fine, we emit best-effort.
                        let _ = state.status_tx.send(StatusChange {
                            instance_id: inst.id.clone(),
                            instance_title: inst.title.clone(),
                            old: *old,
                            new: inst.status,
                            at: now,
                        });
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

    #[test]
    fn generate_token_correct_length_and_charset() {
        let token = generate_token();
        assert_eq!(token.len(), 64);
        assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn valid_token_format_accepts_hex_64() {
        assert!(is_valid_token_format(
            "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2"
        ));
    }

    #[test]
    fn valid_token_format_accepts_legacy_32() {
        assert!(is_valid_token_format("abcdef0123456789abcdef0123456789"));
    }

    #[test]
    fn valid_token_format_rejects_garbage() {
        assert!(!is_valid_token_format("short"));
        assert!(!is_valid_token_format(""));
        assert!(!is_valid_token_format("ZZZZ0000111122223333444455556666"));
    }

    #[test]
    fn classify_ip_recognizes_tailscale_cgnat() {
        use std::net::Ipv4Addr;
        // CGNAT range 100.64.0.0/10 = second octet 64..=127.
        assert_eq!(classify_ip(Ipv4Addr::new(100, 64, 0, 1)), IpKind::Tailscale);
        assert_eq!(
            classify_ip(Ipv4Addr::new(100, 100, 50, 50)),
            IpKind::Tailscale
        );
        assert_eq!(
            classify_ip(Ipv4Addr::new(100, 127, 255, 254)),
            IpKind::Tailscale
        );
        // Boundary: 100.63.x.x is NOT CGNAT, it's just regular public
        // space — classify as LAN so we still surface it (rare but
        // possible on a weird home network).
        assert_eq!(classify_ip(Ipv4Addr::new(100, 63, 0, 1)), IpKind::Lan);
        // Boundary: 100.128.x.x is also not CGNAT.
        assert_eq!(classify_ip(Ipv4Addr::new(100, 128, 0, 1)), IpKind::Lan);
    }

    #[test]
    fn classify_ip_recognizes_rfc1918_lan() {
        use std::net::Ipv4Addr;
        assert_eq!(classify_ip(Ipv4Addr::new(192, 168, 1, 42)), IpKind::Lan);
        assert_eq!(classify_ip(Ipv4Addr::new(10, 0, 0, 1)), IpKind::Lan);
        assert_eq!(classify_ip(Ipv4Addr::new(172, 16, 5, 10)), IpKind::Lan);
    }

    #[test]
    fn classify_ip_recognizes_loopback() {
        use std::net::Ipv4Addr;
        assert_eq!(classify_ip(Ipv4Addr::new(127, 0, 0, 1)), IpKind::Loopback);
        assert_eq!(classify_ip(Ipv4Addr::new(127, 1, 2, 3)), IpKind::Loopback);
    }

    #[test]
    fn ip_kind_ordering_prefers_tailscale() {
        // This is the "Tailscale first in QR" contract. If the sort order
        // ever flips, the user's phone would scan a LAN IP from cellular
        // and hit a timeout — regression test locks it in.
        let mut v = [IpKind::Loopback, IpKind::Lan, IpKind::Tailscale];
        v.sort();
        assert_eq!(v, [IpKind::Tailscale, IpKind::Lan, IpKind::Loopback]);
    }

    #[test]
    fn csp_parses_as_valid_header_value() {
        // Catches typos that would make the header unparseable.
        // security_headers() calls `.parse().unwrap()` at request time;
        // this test surfaces any regression at `cargo test` time instead.
        let parsed: axum::http::HeaderValue = CSP.parse().expect("CSP must parse");
        let rendered = parsed.to_str().expect("CSP must be ASCII");
        // Spot-check load-bearing directives so a future edit that
        // accidentally drops one fails loudly.
        for needle in [
            "default-src 'self'",
            "'wasm-unsafe-eval'",
            "img-src 'self' data: https://github.com https://avatars.githubusercontent.com",
            "connect-src 'self' ws: wss:",
            "frame-ancestors 'none'",
        ] {
            assert!(
                rendered.contains(needle),
                "CSP is missing required directive fragment `{needle}`"
            );
        }
    }

    #[test]
    fn cleanup_defaults_cache_stale_within_ttl_is_false() {
        let cache = CleanupDefaultsCache {
            refreshed_at: std::time::Instant::now(),
            entries: std::collections::HashMap::new(),
        };
        assert!(!cache.stale());
    }

    #[test]
    fn cleanup_defaults_cache_stale_past_ttl_is_true() {
        let cache = CleanupDefaultsCache {
            refreshed_at: std::time::Instant::now()
                - CLEANUP_DEFAULTS_TTL
                - std::time::Duration::from_millis(1),
            entries: std::collections::HashMap::new(),
        };
        assert!(cache.stale());
    }

    #[tokio::test]
    async fn token_manager_validates_current() {
        let mgr = TokenManager::new(Some("abc123".to_string()), Duration::from_secs(3600));
        let (valid, upgrade) = mgr.validate("abc123").await;
        assert!(valid);
        assert!(!upgrade);
    }

    #[tokio::test]
    async fn token_manager_rejects_invalid() {
        let mgr = TokenManager::new(Some("abc123".to_string()), Duration::from_secs(3600));
        let (valid, _) = mgr.validate("wrong").await;
        assert!(!valid);
    }

    #[tokio::test]
    async fn token_manager_validates_previous_in_grace() {
        let mgr = TokenManager::new(Some("old_token".to_string()), Duration::from_secs(3600));
        mgr.rotate().await;

        // Old token should still be valid during grace period
        let (valid, upgrade) = mgr.validate("old_token").await;
        assert!(valid);
        assert!(upgrade); // needs cookie upgrade

        // New token should also be valid
        let current = mgr.current_token().await.unwrap();
        let (valid, upgrade) = mgr.validate(&current).await;
        assert!(valid);
        assert!(!upgrade);
    }

    #[tokio::test]
    async fn token_manager_rotate_changes_token() {
        let mgr = TokenManager::new(Some("original".to_string()), Duration::from_secs(3600));
        let before = mgr.current_token().await;
        mgr.rotate().await;
        let after = mgr.current_token().await;
        assert_ne!(before, after);
    }

    #[tokio::test]
    async fn token_manager_no_auth_mode() {
        let mgr = TokenManager::new(None, Duration::from_secs(3600));
        assert!(mgr.is_no_auth().await);
    }
}
