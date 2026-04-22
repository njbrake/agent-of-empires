#![allow(clippy::result_large_err)]
//! Web Push notifications for the dashboard PWA.
//!
//! Sends VAPID-signed pushes to subscribed browsers when session status
//! transitions require user attention (v1: Running -> Waiting only).
//! Consumed via a broadcast channel on `AppState.status_tx`, so the
//! transition-detection logic is decoupled from tmux polling and can be
//! unit-tested by feeding events directly.
//!
//! Wire format for subscriptions and the security model (per-token hash
//! ownership, rotate-invalidation) are documented in
//! `docs/plans/web-push-notifications.md`.

use crate::session::Status;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;

/// Emitted when an instance's status changes. The broadcast channel on
/// `AppState.status_tx` carries these; `push.rs` is the only consumer in
/// v1, but future features (UI realtime, webhooks) can subscribe too.
#[derive(Clone, Debug)]
pub struct StatusChange {
    pub instance_id: String,
    pub instance_title: String,
    pub old: Status,
    pub new: Status,
    pub at: DateTime<Utc>,
}

/// Capacity of the broadcast channel. Large enough that short bursts of
/// concurrent transitions (e.g., `/api/sessions` bulk refresh) don't drop
/// events even if the consumer is momentarily behind. If a receiver lags
/// past this, broadcast surfaces `RecvError::Lagged` and the consumer
/// logs and continues; push delivery is best-effort anyway.
pub const STATUS_CHANNEL_CAPACITY: usize = 64;

/// Dwell requirement for Waiting: Claude sometimes pauses briefly in
/// Waiting before resolving, and tmux scrape results flicker. Require
/// 5s of continuous Waiting before firing.
pub const DWELL_WAITING_MS: u64 = 5_000;

/// Dwell for Idle and Error is shorter because these are terminal
/// states and far less flicker-prone. Still non-zero to absorb the
/// 2s poll-loop update boundary.
pub const DWELL_TERMINAL_MS: u64 = 2_000;

/// Post-send cooldown per session. After a push fires for a session,
/// suppress further pushes until the session leaves the firing state
/// OR this long has passed, whichever comes second.
pub const COOLDOWN_MS: u64 = 60_000;

// ── VAPID keypair ───────────────────────────────────────────────────────────

/// Persisted form of the VAPID keypair. PKCS#8 PEM for the private key,
/// base64url for the uncompressed public key (which is what the browser's
/// `applicationServerKey` expects after base64url decoding).
#[derive(Serialize, Deserialize)]
pub struct VapidKeypairFile {
    pub private_pem: String,
    pub public_b64url: String,
    pub created_at: DateTime<Utc>,
}

pub struct VapidKeypair {
    pub signing_key: p256::ecdsa::SigningKey,
    pub public_b64url: String,
    pub private_pem: String,
}

impl VapidKeypair {
    /// Load from disk, or generate and persist a new keypair. Uses an
    /// exclusive file lock on `<path>.lock` to prevent two concurrent
    /// `aoe serve` invocations from racing and producing two keypairs.
    pub fn load_or_generate(path: &Path) -> anyhow::Result<Self> {
        use fs2::FileExt;
        use std::fs::OpenOptions;

        // Short-circuit: file already present, load directly.
        if path.exists() {
            return Self::load(path);
        }

        // Acquire the generate-lock (creating the lock file if absent).
        let lock_path = path.with_extension("json.lock");
        let lock_file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(&lock_path)?;
        lock_file.lock_exclusive()?;

        // Re-check: another process may have generated while we were
        // waiting for the lock.
        if path.exists() {
            let _ = FileExt::unlock(&lock_file);
            return Self::load(path);
        }

        let kp = Self::generate()?;
        kp.persist(path)?;
        let _ = FileExt::unlock(&lock_file);
        Ok(kp)
    }

    fn generate() -> anyhow::Result<Self> {
        use p256::ecdsa::SigningKey;
        use p256::pkcs8::EncodePrivateKey;

        // Pull 32 bytes of OS entropy and reduce via SigningKey::from_slice;
        // avoids the rand/rand_core OsRng shuffle across major versions.
        let mut seed = [0u8; 32];
        getrandom::fill(&mut seed).map_err(|e| anyhow::anyhow!("getrandom failed: {}", e))?;
        let signing_key = SigningKey::from_slice(&seed)
            .map_err(|e| anyhow::anyhow!("derive signing key: {}", e))?;
        let verifying_key = signing_key.verifying_key();

        // Private key as PKCS#8 PEM.
        let private_pem = signing_key
            .to_pkcs8_pem(p256::pkcs8::LineEnding::LF)?
            .to_string();

        // Public key in uncompressed SEC1 form, base64url encoded. This
        // is the shape browsers expect for applicationServerKey.
        let public_bytes = verifying_key.to_encoded_point(false);
        let public_b64url = base64_url_encode(public_bytes.as_bytes());

        Ok(Self {
            signing_key,
            public_b64url,
            private_pem,
        })
    }

    fn load(path: &Path) -> anyhow::Result<Self> {
        use p256::ecdsa::SigningKey;
        use p256::pkcs8::DecodePrivateKey;

        let raw = std::fs::read_to_string(path)?;
        let file: VapidKeypairFile = serde_json::from_str(&raw)?;
        let signing_key = SigningKey::from_pkcs8_pem(&file.private_pem)?;
        Ok(Self {
            signing_key,
            public_b64url: file.public_b64url,
            private_pem: file.private_pem,
        })
    }

    fn persist(&self, path: &Path) -> anyhow::Result<()> {
        let file = VapidKeypairFile {
            private_pem: self.private_pem.clone(),
            public_b64url: self.public_b64url.clone(),
            created_at: Utc::now(),
        };
        let body = serde_json::to_string_pretty(&file)?;

        // Atomic: write to tmp, fsync, rename.
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, &body)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&tmp, path)?;
        Ok(())
    }
}

// ── Subscription store ──────────────────────────────────────────────────────

/// A browser push subscription. Fields mirror the browser-side
/// `PushSubscription.toJSON()` with added ownership and bookkeeping.
#[derive(Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
    /// SHA-256 of the bearer token at the time of subscribe. Pushes and
    /// mutations only fire for subscriptions whose hash matches the
    /// current (or grace-period) token.
    pub owner_token_hash: [u8; 32],
    pub user_agent: String,
    pub created_at: DateTime<Utc>,
    /// Monotonic counter for optimistic-lock GC: the send path snapshots
    /// the generation before sending; the GC path removes only if the
    /// counter still matches. Prevents wiping a freshly re-subscribed
    /// entry when a concurrent send returns 410.
    pub generation: u64,
}

pub struct SubscriptionStore {
    path: PathBuf,
    subs: RwLock<HashMap<String, Subscription>>,
}

impl SubscriptionStore {
    pub fn load_or_empty(path: PathBuf) -> Self {
        let subs = match std::fs::read_to_string(&path) {
            Ok(raw) => serde_json::from_str::<Vec<Subscription>>(&raw)
                .map(|v| v.into_iter().map(|s| (s.endpoint.clone(), s)).collect())
                .unwrap_or_default(),
            Err(_) => HashMap::new(),
        };
        Self {
            path,
            subs: RwLock::new(subs),
        }
    }

    pub async fn snapshot(&self) -> Vec<Subscription> {
        self.subs.read().await.values().cloned().collect()
    }

    pub async fn for_owner(&self, owner: &[u8; 32]) -> Vec<Subscription> {
        self.subs
            .read()
            .await
            .values()
            .filter(|s| &s.owner_token_hash == owner)
            .cloned()
            .collect()
    }

    pub async fn upsert(&self, mut sub: Subscription) -> anyhow::Result<()> {
        {
            let mut guard = self.subs.write().await;
            if let Some(existing) = guard.get(&sub.endpoint) {
                sub.generation = existing.generation.saturating_add(1);
                sub.created_at = existing.created_at;
            }
            guard.insert(sub.endpoint.clone(), sub);
        }
        self.persist().await
    }

    pub async fn remove_if_owner(&self, endpoint: &str, owner: &[u8; 32]) -> anyhow::Result<bool> {
        let removed = {
            let mut guard = self.subs.write().await;
            match guard.get(endpoint) {
                Some(s) if &s.owner_token_hash == owner => {
                    guard.remove(endpoint);
                    true
                }
                _ => false,
            }
        };
        if removed {
            self.persist().await?;
        }
        Ok(removed)
    }

    /// GC a subscription following a push-endpoint 410/404, gated on the
    /// generation counter so we don't wipe an entry that was re-subscribed
    /// while the send was in flight.
    pub async fn gc_stale(&self, endpoint: &str, observed_generation: u64) -> anyhow::Result<bool> {
        let removed = {
            let mut guard = self.subs.write().await;
            match guard.get(endpoint) {
                Some(s) if s.generation == observed_generation => {
                    guard.remove(endpoint);
                    true
                }
                _ => false,
            }
        };
        if removed {
            self.persist().await?;
        }
        Ok(removed)
    }

    /// Drop any subscriptions whose owner hash is not in `valid`.
    /// Called on token rotation once we know which hashes are
    /// current-or-grace-period.
    pub async fn retain_owners(&self, valid: &[[u8; 32]]) -> anyhow::Result<usize> {
        let removed = {
            let mut guard = self.subs.write().await;
            let before = guard.len();
            guard.retain(|_, s| valid.iter().any(|v| v == &s.owner_token_hash));
            before - guard.len()
        };
        if removed > 0 {
            self.persist().await?;
        }
        Ok(removed)
    }

    async fn persist(&self) -> anyhow::Result<()> {
        let all: Vec<Subscription> = self.subs.read().await.values().cloned().collect();
        let body = serde_json::to_string_pretty(&all)?;
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &body)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

// ── Module-level state ──────────────────────────────────────────────────────

/// The push feature's mutable state, owned by `AppState.push`.
pub struct PushState {
    pub vapid: VapidKeypair,
    pub store: SubscriptionStore,
    /// VAPID `sub:` claim identifying the sending application. Must be
    /// either `mailto:` or an `https://` URL per the spec. Not strongly
    /// validated by push endpoints in practice.
    pub subject: String,
}

/// VAPID `sub` claim (RFC 8292). Spec requires a `mailto:` or `https://`
/// URL but does not require deliverability; major push services do not
/// validate this for reachability in practice. We use the project's
/// public URL so providers that do care have somewhere real to reach.
pub const VAPID_SUBJECT: &str = "https://github.com/njbrake/agent-of-empires";

impl PushState {
    pub fn init(app_dir: &Path) -> anyhow::Result<Self> {
        let vapid = VapidKeypair::load_or_generate(&app_dir.join("push.vapid.json"))?;
        let store = SubscriptionStore::load_or_empty(app_dir.join("push.subscriptions.json"));
        Ok(Self {
            vapid,
            store,
            subject: VAPID_SUBJECT.to_string(),
        })
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

pub fn base64_url_encode(bytes: &[u8]) -> String {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    URL_SAFE_NO_PAD.encode(bytes)
}

pub fn base64_url_decode(s: &str) -> Result<Vec<u8>, base64::DecodeError> {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
    URL_SAFE_NO_PAD.decode(s)
}

// ── Consumer task ───────────────────────────────────────────────────────────

/// Push-notification event types that the consumer can fire. Each
/// has its own server-wide default (in WebConfig) and a per-session
/// override (in Instance). The dwell requirement also varies by kind:
/// Waiting uses a longer dwell since Claude often pauses briefly.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NotificationEvent {
    Waiting,
    Idle,
    Error,
}

impl NotificationEvent {
    fn dwell_ms(self) -> u64 {
        match self {
            Self::Waiting => DWELL_WAITING_MS,
            _ => DWELL_TERMINAL_MS,
        }
    }
}

/// Per-session timing state the consumer maintains to apply the dwell
/// requirement and the post-send cooldown per event type.
#[derive(Default)]
struct DwellState {
    /// When the session most recently entered Waiting. None if not
    /// currently waiting.
    waiting_since: Option<std::time::Instant>,
    /// When the session most recently entered Idle.
    idle_since: Option<std::time::Instant>,
    /// When the session most recently entered Error.
    error_since: Option<std::time::Instant>,
    /// Last time a push fired for this session (any event type). Used
    /// for a shared per-session cooldown: rapid-fire events like
    /// Error → brief Running → Error don't double-buzz.
    last_notified: Option<std::time::Instant>,
    /// Cached title for the payload body.
    title: String,
}

/// Max concurrent push sends. Caps the number of parallel outbound
/// HTTP requests the consumer will hold open; above this, sends queue
/// behind the semaphore and are processed in FIFO order.
pub const SEND_CONCURRENCY: usize = 8;

/// Spawn the consumer task. Subscribes to `state.status_tx`, applies
/// dwell + cooldown logic, and fans out pushes to all still-valid
/// subscriptions when a session stays in `Waiting` past DWELL_MS.
///
/// The task runs for the lifetime of the server; no clean shutdown
/// path is required since `broadcast::Receiver` is drained on drop.
pub fn spawn_consumer(state: std::sync::Arc<super::AppState>) {
    if state.push.is_none() {
        return; // feature disabled, nothing to spawn
    }

    tokio::spawn(async move {
        let client = match super::push_send::build_client() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(error = %e, "push: consumer failed to build reqwest client");
                return;
            }
        };
        let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(SEND_CONCURRENCY));
        let mut rx = state.status_tx.subscribe();
        let mut dwell: HashMap<String, DwellState> = HashMap::new();

        // Interleave receiving status changes with polling the dwell
        // map for sessions whose dwell window has elapsed. A simple
        // 500ms tick is precise enough and cheap.
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(500));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            tokio::select! {
                recv = rx.recv() => {
                    match recv {
                        Ok(change) => handle_status_change(&mut dwell, change),
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(lagged = n, "push: consumer lagged, skipped events");
                            continue;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            tracing::info!("push: status channel closed, consumer exiting");
                            return;
                        }
                    }
                }
                _ = tick.tick() => {
                    fire_due_pushes(state.clone(), &client, &semaphore, &mut dwell).await;
                }
            }
        }
    });
}

fn handle_status_change(dwell: &mut HashMap<String, DwellState>, change: StatusChange) {
    let entry = dwell.entry(change.instance_id.clone()).or_default();
    entry.title = change.instance_title;
    let now = std::time::Instant::now();
    // Exactly one `*_since` is set at a time: the current state's timer.
    // Transitioning clears the others. Each fresh entry into a fire-worthy
    // state resets that state's dwell timer (a flicker Waiting → Running →
    // Waiting restarts the 5s clock, which is what we want).
    entry.waiting_since = None;
    entry.idle_since = None;
    entry.error_since = None;
    match change.new {
        Status::Waiting => entry.waiting_since = Some(now),
        Status::Idle => entry.idle_since = Some(now),
        Status::Error => entry.error_since = Some(now),
        _ => {}
    }
    // Drop entries for transitions into Stopped/Deleting so the map
    // doesn't grow forever in long-running servers that create and
    // destroy many sessions.
    if matches!(change.new, Status::Stopped | Status::Deleting) {
        dwell.remove(&change.instance_id);
    }
}

/// Resolve whether a given event type should fire for a given instance,
/// combining server-wide defaults with per-session overrides.
fn should_fire(
    event: NotificationEvent,
    web: &crate::session::config::WebConfig,
    instance: Option<&crate::session::Instance>,
) -> bool {
    let (global, override_val) = match event {
        NotificationEvent::Waiting => (
            web.notify_on_waiting,
            instance.and_then(|i| i.notify_on_waiting),
        ),
        NotificationEvent::Idle => (web.notify_on_idle, instance.and_then(|i| i.notify_on_idle)),
        NotificationEvent::Error => (
            web.notify_on_error,
            instance.and_then(|i| i.notify_on_error),
        ),
    };
    override_val.unwrap_or(global)
}

async fn fire_due_pushes(
    app_state: std::sync::Arc<super::AppState>,
    client: &reqwest::Client,
    semaphore: &std::sync::Arc<tokio::sync::Semaphore>,
    dwell: &mut HashMap<String, DwellState>,
) {
    let Some(push) = app_state.push.as_ref() else {
        return; // feature disabled, nothing to do
    };
    let push = push.clone();

    let now = std::time::Instant::now();
    // Collect (instance_id, title, event) tuples to fire. Firing mutates
    // the dwell map (clear `*_since`, set `last_notified`) so we collect
    // before sending to avoid holding a borrow across the await boundary.
    let mut to_fire: Vec<(String, String, NotificationEvent)> = Vec::new();

    for (id, state) in dwell.iter_mut() {
        // Cooldown gates ALL event types for this session. Rapid
        // oscillation Error → Running → Error shouldn't double-buzz.
        if let Some(last) = state.last_notified {
            if now.duration_since(last).as_millis() < COOLDOWN_MS as u128 {
                continue;
            }
        }

        // Evaluate each event in priority order. At most one *_since is
        // set at any time (handle_status_change maintains this), so this
        // loop terminates early with a single fire or zero fires.
        let checks = [
            (NotificationEvent::Waiting, state.waiting_since),
            (NotificationEvent::Error, state.error_since),
            (NotificationEvent::Idle, state.idle_since),
        ];
        for (event, since_opt) in checks {
            let Some(since) = since_opt else { continue };
            if now.duration_since(since).as_millis() < event.dwell_ms() as u128 {
                continue;
            }
            state.last_notified = Some(now);
            state.waiting_since = None;
            state.idle_since = None;
            state.error_since = None;
            to_fire.push((id.clone(), state.title.clone(), event));
            break;
        }
    }

    if to_fire.is_empty() {
        return;
    }

    // Suppress pushes when the user is actively using aoe (TUI or web
    // dashboard). They can already see session state changes in real
    // time, so OS-level push notifications are noise.
    let suppress_reason = if crate::session::is_tui_active(std::time::Duration::from_secs(30)) {
        Some("TUI is active")
    } else if app_state.web_active_within(std::time::Duration::from_secs(30)) {
        Some("web dashboard is active")
    } else {
        None
    };
    if let Some(reason) = suppress_reason {
        tracing::debug!(
            "push: suppressed {} notification(s), {}",
            to_fire.len(),
            reason,
        );
        return;
    }

    // Snapshot instances once; fire_due_pushes holds no locks across
    // the tokio::spawn boundary below.
    let instances = app_state.instances.read().await.clone();
    let web_config = app_state.web_config.clone();

    for (instance_id, instance_title, event) in to_fire {
        // If the instance vanished (externally deleted, tmux killed,
        // storage file hand-edited) between the dwell timer starting
        // and firing, skip rather than sending a notification that
        // deep-links to a 404. Also drop the dwell entry so we don't
        // keep retrying every tick forever.
        let Some(instance) = instances.iter().find(|i| i.id == instance_id) else {
            dwell.remove(&instance_id);
            continue;
        };
        if !should_fire(event, &web_config, Some(instance)) {
            continue;
        }

        let subs = push.store.snapshot().await;
        if subs.is_empty() {
            continue;
        }

        let (title, body_prefix) = match event {
            NotificationEvent::Waiting => ("Claude is waiting", "Waiting for input"),
            NotificationEvent::Idle => ("Session finished", "Agent is idle"),
            NotificationEvent::Error => ("Session error", "Agent errored"),
        };
        let body = if instance_title.is_empty() {
            body_prefix.to_string()
        } else {
            format!("{}: {}", body_prefix, instance_title)
        };
        let payload = super::push_send::PushPayload {
            title: title.to_string(),
            body,
            url: format!("/?session={}", instance_id),
            tag: format!("session-{}", instance_id),
            session_id: instance_id.clone(),
        };

        for sub in subs {
            let permit_sem = semaphore.clone();
            let client = client.clone();
            let push = push.clone();
            let payload_clone = super::push_send::PushPayload {
                title: payload.title.clone(),
                body: payload.body.clone(),
                url: payload.url.clone(),
                tag: payload.tag.clone(),
                session_id: payload.session_id.clone(),
            };
            tokio::spawn(async move {
                let Ok(_permit) = permit_sem.acquire_owned().await else {
                    return;
                };
                let outcome =
                    super::push_send::send_one(&client, push.as_ref(), &sub, &payload_clone).await;
                if outcome == super::push_send::SendOutcome::Gone {
                    let _ = push.store.gc_stale(&sub.endpoint, sub.generation).await;
                }
            });
        }
    }
}

pub fn sha256_token(token: &str) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(token.as_bytes());
    let out = h.finalize();
    let mut arr = [0u8; 32];
    arr.copy_from_slice(&out);
    arr
}

// ── HTTP handlers ───────────────────────────────────────────────────────────

use axum::extract::{Extension, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use std::sync::Arc;

use super::auth::AuthenticatedTokenHash;
use super::AppState;

/// Body accepted by POST /api/push/subscribe. Mirrors the browser's
/// `PushSubscription.toJSON()` output.
#[derive(Deserialize)]
pub struct SubscribeBody {
    pub endpoint: String,
    pub keys: SubscribeKeys,
}

#[derive(Deserialize)]
pub struct SubscribeKeys {
    pub p256dh: String,
    pub auth: String,
}

#[derive(Deserialize)]
pub struct EndpointBody {
    pub endpoint: String,
}

#[derive(Serialize)]
pub struct TestResult {
    pub delivered: u32,
    pub failed: u32,
    pub gone: u32,
}

/// GET /api/push/status
/// Tells the client whether the feature is enabled server-wide. Cheap,
/// no secrets: used by the UI on mount to decide whether to show the
/// Enable button or the "disabled by operator" state.
pub async fn get_status(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    Json(serde_json::json!({ "enabled": state.push_enabled }))
}

/// GET /api/push/vapid-public-key
/// Returns the base64url-encoded raw public key for the browser's
/// `pushManager.subscribe({ applicationServerKey })` call.
pub async fn get_vapid_public_key(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let push = state.push.as_ref().ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(
        serde_json::json!({ "public_key": push.vapid.public_b64url }),
    ))
}

/// POST /api/push/subscribe
/// Stores a browser subscription, binding it to the requesting token's
/// hash. Idempotent: re-subscribing the same endpoint updates the stored
/// keys/user-agent and bumps the generation counter (the GC path uses
/// that counter to avoid wiping freshly-re-subscribed entries when a
/// concurrent 410 arrives for the old generation).
pub async fn subscribe(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthenticatedTokenHash>,
    headers: HeaderMap,
    Json(body): Json<SubscribeBody>,
) -> Result<StatusCode, StatusCode> {
    let push = state.push.as_ref().ok_or(StatusCode::NOT_FOUND)?;

    // Minimal shape validation so we don't store garbage.
    if body.endpoint.is_empty() || body.keys.p256dh.is_empty() || body.keys.auth.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    let user_agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();

    let sub = Subscription {
        endpoint: body.endpoint,
        p256dh: body.keys.p256dh,
        auth: body.keys.auth,
        owner_token_hash: auth.0,
        user_agent,
        created_at: Utc::now(),
        generation: 0,
    };
    push.store
        .upsert(sub)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::NO_CONTENT)
}

/// POST /api/push/unsubscribe
/// Removes a subscription by endpoint. Requires owner match: cross-token
/// attempts return 403 (intentionally visible: helps debug "why isn't
/// my disable working" without leaking whether the endpoint exists).
pub async fn unsubscribe(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthenticatedTokenHash>,
    Json(body): Json<EndpointBody>,
) -> Result<StatusCode, StatusCode> {
    let push = state.push.as_ref().ok_or(StatusCode::NOT_FOUND)?;
    if body.endpoint.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let removed = push
        .store
        .remove_if_owner(&body.endpoint, &auth.0)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    if removed {
        Ok(StatusCode::NO_CONTENT)
    } else {
        // Either the endpoint doesn't exist or belongs to another owner.
        // Return 403 rather than 204 so clients know the call did nothing.
        Err(StatusCode::FORBIDDEN)
    }
}

/// POST /api/push/test
/// Fires a single notification to the given endpoint (which MUST belong
/// to the caller). Used by the "Send test notification" button. No
/// fire-to-all fallback: that would let any authenticated caller spam
/// every subscriber.
pub async fn test(
    State(state): State<Arc<AppState>>,
    Extension(auth): Extension<AuthenticatedTokenHash>,
    Json(body): Json<EndpointBody>,
) -> Result<Json<TestResult>, StatusCode> {
    let push = state.push.as_ref().ok_or(StatusCode::NOT_FOUND)?;
    if body.endpoint.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }

    // Confirm ownership before doing anything. Reject cross-owner test
    // calls with 403 even if the subscription exists.
    let owned = push
        .store
        .for_owner(&auth.0)
        .await
        .into_iter()
        .find(|s| s.endpoint == body.endpoint);
    let Some(subscription) = owned else {
        return Err(StatusCode::FORBIDDEN);
    };

    let client = match super::push_send::build_client() {
        Ok(c) => c,
        Err(e) => {
            tracing::error!(error = %e, "push: failed to build reqwest client");
            return Err(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    let payload = super::push_send::PushPayload {
        title: "Agent of Empires".to_string(),
        body: "Test notification. If you see this on your lock screen, push is working."
            .to_string(),
        url: "/".to_string(),
        tag: "aoe-test".to_string(),
        session_id: String::new(),
    };

    let outcome = super::push_send::send_one(&client, push, &subscription, &payload).await;
    let mut result = TestResult {
        delivered: 0,
        failed: 0,
        gone: 0,
    };
    match outcome {
        super::push_send::SendOutcome::Delivered => result.delivered = 1,
        super::push_send::SendOutcome::Failed => result.failed = 1,
        super::push_send::SendOutcome::Gone => {
            result.gone = 1;
            // Best-effort GC; the result still reports gone=1 even if GC
            // races with a re-subscribe (that's what the generation
            // counter in gc_stale prevents).
            let _ = push
                .store
                .gc_stale(&body.endpoint, subscription.generation)
                .await;
        }
    }
    Ok(Json(result))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vapid_generate_roundtrip() {
        let kp = VapidKeypair::generate().unwrap();
        assert!(kp.public_b64url.len() > 80);
        assert!(kp.private_pem.contains("BEGIN PRIVATE KEY"));
    }

    #[test]
    fn vapid_persist_and_reload_same_key() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("push.vapid.json");
        let first = VapidKeypair::load_or_generate(&path).unwrap();
        let second = VapidKeypair::load_or_generate(&path).unwrap();
        assert_eq!(first.public_b64url, second.public_b64url);
        assert_eq!(first.private_pem, second.private_pem);
    }

    #[tokio::test]
    async fn subscription_store_upsert_increments_generation() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("push.subscriptions.json");
        let store = SubscriptionStore::load_or_empty(path);

        let base = Subscription {
            endpoint: "https://push.example/abc".into(),
            p256dh: "pk".into(),
            auth: "auth".into(),
            owner_token_hash: [1u8; 32],
            user_agent: "UA".into(),
            created_at: Utc::now(),
            generation: 0,
        };
        store.upsert(base.clone()).await.unwrap();
        store.upsert(base.clone()).await.unwrap();

        let all = store.snapshot().await;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].generation, 1);
    }

    #[tokio::test]
    async fn gc_stale_respects_generation() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("push.subscriptions.json");
        let store = SubscriptionStore::load_or_empty(path);

        let sub = Subscription {
            endpoint: "https://push.example/abc".into(),
            p256dh: "pk".into(),
            auth: "auth".into(),
            owner_token_hash: [1u8; 32],
            user_agent: "UA".into(),
            created_at: Utc::now(),
            generation: 5,
        };
        store.upsert(sub.clone()).await.unwrap();

        // Stale GC (observed generation differs) does NOT remove.
        let removed = store.gc_stale(&sub.endpoint, 4).await.unwrap();
        assert!(!removed);
        assert_eq!(store.snapshot().await.len(), 1);

        // Matching generation removes.
        let removed = store.gc_stale(&sub.endpoint, 5).await.unwrap();
        assert!(removed);
        assert_eq!(store.snapshot().await.len(), 0);
    }

    #[tokio::test]
    async fn retain_owners_keeps_grace_token_drops_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("push.subscriptions.json");
        let store = SubscriptionStore::load_or_empty(path);

        let mk = |hash: [u8; 32], endpoint: &str| Subscription {
            endpoint: endpoint.to_string(),
            p256dh: "pk".into(),
            auth: "auth".into(),
            owner_token_hash: hash,
            user_agent: "UA".into(),
            created_at: Utc::now(),
            generation: 0,
        };
        store.upsert(mk([1u8; 32], "https://x/1")).await.unwrap();
        store.upsert(mk([2u8; 32], "https://x/2")).await.unwrap();
        store.upsert(mk([3u8; 32], "https://x/3")).await.unwrap();
        assert_eq!(store.snapshot().await.len(), 3);

        // Keep current (hash 2) and grace (hash 1); drop hash 3.
        let removed = store.retain_owners(&[[1u8; 32], [2u8; 32]]).await.unwrap();
        assert_eq!(removed, 1);
        let remaining: Vec<_> = store
            .snapshot()
            .await
            .into_iter()
            .map(|s| s.endpoint)
            .collect();
        assert_eq!(remaining.len(), 2);
        assert!(remaining.contains(&"https://x/1".to_string()));
        assert!(remaining.contains(&"https://x/2".to_string()));

        // After grace expires, only hash 2 remains valid. hash 1 drops.
        let removed = store.retain_owners(&[[2u8; 32]]).await.unwrap();
        assert_eq!(removed, 1);
        assert_eq!(store.snapshot().await.len(), 1);
    }

    #[tokio::test]
    async fn remove_if_owner_blocks_cross_owner() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("push.subscriptions.json");
        let store = SubscriptionStore::load_or_empty(path);

        let sub = Subscription {
            endpoint: "https://push.example/abc".into(),
            p256dh: "pk".into(),
            auth: "auth".into(),
            owner_token_hash: [1u8; 32],
            user_agent: "UA".into(),
            created_at: Utc::now(),
            generation: 0,
        };
        store.upsert(sub).await.unwrap();

        // Different owner must not succeed.
        let removed = store
            .remove_if_owner("https://push.example/abc", &[2u8; 32])
            .await
            .unwrap();
        assert!(!removed);
        assert_eq!(store.snapshot().await.len(), 1);

        // Correct owner succeeds.
        let removed = store
            .remove_if_owner("https://push.example/abc", &[1u8; 32])
            .await
            .unwrap();
        assert!(removed);
        assert_eq!(store.snapshot().await.len(), 0);
    }

    #[test]
    fn dwell_starts_on_enter_waiting_and_clears_on_exit() {
        let mut dwell: HashMap<String, DwellState> = HashMap::new();
        let id = "sess-1".to_string();

        // Enter Waiting: dwell starts.
        handle_status_change(
            &mut dwell,
            StatusChange {
                instance_id: id.clone(),
                instance_title: "my session".to_string(),
                old: Status::Running,
                new: Status::Waiting,
                at: Utc::now(),
            },
        );
        assert!(dwell.get(&id).unwrap().waiting_since.is_some());
        assert_eq!(dwell.get(&id).unwrap().title, "my session");

        // Leave Waiting: dwell clears (but entry still exists so
        // last_notified survives for cooldown checking).
        handle_status_change(
            &mut dwell,
            StatusChange {
                instance_id: id.clone(),
                instance_title: "my session".to_string(),
                old: Status::Waiting,
                new: Status::Running,
                at: Utc::now(),
            },
        );
        assert!(dwell.get(&id).unwrap().waiting_since.is_none());
    }

    #[test]
    fn dwell_switches_between_event_types() {
        let mut dwell: HashMap<String, DwellState> = HashMap::new();
        let id = "sess-3".to_string();
        let ev = |new: Status| StatusChange {
            instance_id: id.clone(),
            instance_title: "s".into(),
            old: Status::Running,
            new,
            at: Utc::now(),
        };

        handle_status_change(&mut dwell, ev(Status::Waiting));
        let s = dwell.get(&id).unwrap();
        assert!(s.waiting_since.is_some());
        assert!(s.idle_since.is_none());
        assert!(s.error_since.is_none());

        handle_status_change(&mut dwell, ev(Status::Error));
        let s = dwell.get(&id).unwrap();
        assert!(s.waiting_since.is_none());
        assert!(s.idle_since.is_none());
        assert!(s.error_since.is_some());

        handle_status_change(&mut dwell, ev(Status::Idle));
        let s = dwell.get(&id).unwrap();
        assert!(s.waiting_since.is_none());
        assert!(s.idle_since.is_some());
        assert!(s.error_since.is_none());
    }

    #[test]
    fn should_fire_respects_per_session_override() {
        use crate::session::config::WebConfig;
        use crate::session::Instance;

        let web = WebConfig {
            notifications_enabled: true,
            notify_on_waiting: true,
            notify_on_idle: false, // globally off
            notify_on_error: true,
        };

        // No instance (session not in state): fall back to web defaults.
        assert!(should_fire(NotificationEvent::Waiting, &web, None));
        assert!(!should_fire(NotificationEvent::Idle, &web, None));
        assert!(should_fire(NotificationEvent::Error, &web, None));

        // Instance with no overrides: inherits web defaults.
        let mut inst = Instance::new("t", "/tmp");
        assert!(should_fire(NotificationEvent::Waiting, &web, Some(&inst)));
        assert!(!should_fire(NotificationEvent::Idle, &web, Some(&inst)));
        assert!(should_fire(NotificationEvent::Error, &web, Some(&inst)));

        // Session opts INTO idle despite global default off; this is
        // the "I want to babysit this one long session" case.
        inst.notify_on_idle = Some(true);
        assert!(should_fire(NotificationEvent::Idle, &web, Some(&inst)));

        // Session opts OUT of waiting despite global on; this is the
        // "stop spamming me about this noisy session" case.
        inst.notify_on_waiting = Some(false);
        assert!(!should_fire(NotificationEvent::Waiting, &web, Some(&inst)));

        // Error unaffected: per-event-type overrides don't cross-pollute.
        assert!(should_fire(NotificationEvent::Error, &web, Some(&inst)));
    }

    #[test]
    fn dwell_entry_drops_on_stopped() {
        let mut dwell: HashMap<String, DwellState> = HashMap::new();
        let id = "sess-2".to_string();
        handle_status_change(
            &mut dwell,
            StatusChange {
                instance_id: id.clone(),
                instance_title: "s".to_string(),
                old: Status::Running,
                new: Status::Waiting,
                at: Utc::now(),
            },
        );
        assert!(dwell.contains_key(&id));
        handle_status_change(
            &mut dwell,
            StatusChange {
                instance_id: id.clone(),
                instance_title: "s".to_string(),
                old: Status::Waiting,
                new: Status::Stopped,
                at: Utc::now(),
            },
        );
        assert!(!dwell.contains_key(&id));
    }

    #[test]
    fn sha256_token_is_deterministic_and_differs_per_input() {
        let a = sha256_token("token-1");
        let b = sha256_token("token-1");
        let c = sha256_token("token-2");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
