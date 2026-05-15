//! Passphrase-based login as a second authentication factor.
//!
//! When a passphrase is configured, users must enter it after token auth
//! to access the dashboard. Login sessions are tracked server-side with
//! a device-binding secret (replaces the prior strict IP binding, see
//! #1131) and a 30-day sliding expiry window.
//!
//! The device-binding model: the client generates 32 random bytes via
//! `crypto.getRandomValues`, stores them in `localStorage`, and presents
//! them on every authenticated request. The server stores only the
//! SHA-256 hash and uses a constant-time compare. A leaked session
//! cookie alone is therefore insufficient, the attacker also needs the
//! binding secret. Mobile IP rotation no longer logs anyone out because
//! IP is now telemetry only.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use tokio::sync::RwLock;

use super::auth::resolve_client_ip;
use super::AppState;

/// 30-day session lifetime (sliding window).
const SESSION_LIFETIME: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// Step-up elevation window. Required for high-risk operations
/// (terminal attach, cockpit command execution, file writes,
/// destructive session ops). See #1131.
const ELEVATION_LIFETIME: Duration = Duration::from_secs(15 * 60);

/// Maximum concurrent login sessions before evicting the oldest.
const MAX_SESSIONS: usize = 50;

/// Minimum recommended passphrase length.
const MIN_PASSPHRASE_LENGTH: usize = 8;

/// Length in raw bytes of the client-generated device binding secret.
/// 32 bytes (256 bits) of entropy from `crypto.getRandomValues`. We
/// reject shorter or longer payloads to catch typos and tampering.
const BINDING_SECRET_BYTES: usize = 32;

struct LoginSession {
    expires_at: Instant,
    /// SHA-256 hash of the client-presented device binding secret.
    /// Constant-time compared on validation. We never store or log
    /// the raw secret; a server-side leak of `LoginManager` state
    /// must not be replayable.
    binding_hash: [u8; 32],
    /// Step-up elevation deadline. `None` (or in the past) means the
    /// session can browse the dashboard but cannot reach the
    /// high-risk routes guarded by `is_elevated`. See #1131.
    elevated_until: Option<Instant>,
}

/// Manages passphrase verification and login session lifecycle.
pub struct LoginManager {
    passphrase_hash: Option<String>,
    sessions: RwLock<HashMap<String, LoginSession>>,
}

impl LoginManager {
    /// Create a new login manager. If `passphrase` is `Some`, hash it with argon2.
    pub fn new(passphrase: Option<&str>) -> Self {
        let passphrase_hash = passphrase.map(|p| {
            use argon2::password_hash::SaltString;
            use argon2::{Argon2, PasswordHasher};
            use rand::RngExt;

            let mut salt_bytes = [0u8; 16];
            rand::rng().fill(&mut salt_bytes);
            let salt = SaltString::encode_b64(&salt_bytes).expect("salt encoding must succeed");
            Argon2::default()
                .hash_password(p.as_bytes(), &salt)
                .expect("argon2 hashing must not fail")
                .to_string()
        });

        Self {
            passphrase_hash,
            sessions: RwLock::new(HashMap::new()),
        }
    }

    /// Whether passphrase login is enabled.
    pub fn is_enabled(&self) -> bool {
        self.passphrase_hash.is_some()
    }

    /// Verify a passphrase against the stored hash.
    pub fn verify_passphrase(&self, input: &str) -> bool {
        let Some(ref hash) = self.passphrase_hash else {
            return false;
        };

        use argon2::password_hash::PasswordHash;
        use argon2::{Argon2, PasswordVerifier};

        let parsed = match PasswordHash::new(hash) {
            Ok(h) => h,
            Err(_) => return false,
        };

        Argon2::default()
            .verify_password(input.as_bytes(), &parsed)
            .is_ok()
    }

    /// Create a new login session bound to a device. Returns the
    /// session ID (64-char hex). `binding_secret_bytes` is the raw 32
    /// random bytes the client generated; only its SHA-256 hash is
    /// retained.
    pub async fn create_session(&self, binding_secret_bytes: &[u8]) -> String {
        let session_id = super::generate_token();
        let session = LoginSession {
            expires_at: Instant::now() + SESSION_LIFETIME,
            binding_hash: hash_binding_secret(binding_secret_bytes),
            elevated_until: None,
        };

        let mut sessions = self.sessions.write().await;

        // Evict oldest if at capacity
        if sessions.len() >= MAX_SESSIONS {
            if let Some(oldest_id) = sessions
                .iter()
                .min_by_key(|(_, s)| s.expires_at)
                .map(|(id, _)| id.clone())
            {
                sessions.remove(&oldest_id);
            }
        }

        sessions.insert(session_id.clone(), session);
        session_id
    }

    /// Validate a session. Checks existence, expiry, and a
    /// constant-time match against the stored device binding hash.
    /// On success, extends the sliding window. IP is no longer
    /// consulted, mobile network rotation is a normal pattern and
    /// the device-binding secret carries the identity instead. See
    /// #1131.
    pub async fn validate_session(&self, session_id: &str, presented_binding: &[u8]) -> bool {
        if session_id.is_empty() || presented_binding.len() != BINDING_SECRET_BYTES {
            return false;
        }

        let presented_hash = hash_binding_secret(presented_binding);

        let mut sessions = self.sessions.write().await;
        let Some(session) = sessions.get_mut(session_id) else {
            return false;
        };

        if Instant::now() > session.expires_at {
            sessions.remove(session_id);
            return false;
        }

        // Constant-time compare. `Choice::unwrap_u8()` gives a 0/1 we
        // can interpret as `bool` without branching on the comparison
        // result.
        if session.binding_hash.ct_eq(&presented_hash).unwrap_u8() == 0 {
            return false;
        }

        // Sliding window: extend expiry on each valid access
        session.expires_at = Instant::now() + SESSION_LIFETIME;
        true
    }

    /// Mark a session as elevated (passphrase confirmed) for
    /// `ELEVATION_LIFETIME`. Caller is responsible for verifying the
    /// passphrase before calling. See #1131.
    pub async fn elevate_session(&self, session_id: &str) -> bool {
        if session_id.is_empty() {
            return false;
        }
        let mut sessions = self.sessions.write().await;
        let Some(session) = sessions.get_mut(session_id) else {
            return false;
        };
        if Instant::now() > session.expires_at {
            return false;
        }
        session.elevated_until = Some(Instant::now() + ELEVATION_LIFETIME);
        true
    }

    /// Read elevation state. Returns `(elevated, elevated_until_secs)`:
    /// the bool reflects whether the elevation window is still open,
    /// the optional seconds-from-now value is what `/api/login/status`
    /// surfaces to the client. Returns `(false, None)` for an unknown
    /// or expired session.
    pub async fn elevation_state(&self, session_id: &str) -> (bool, Option<u64>) {
        if session_id.is_empty() {
            return (false, None);
        }
        let sessions = self.sessions.read().await;
        let Some(session) = sessions.get(session_id) else {
            return (false, None);
        };
        let now = Instant::now();
        if now > session.expires_at {
            return (false, None);
        }
        let Some(deadline) = session.elevated_until else {
            return (false, None);
        };
        if now > deadline {
            return (false, None);
        }
        let remaining = deadline.saturating_duration_since(now).as_secs();
        (true, Some(remaining))
    }

    /// Whether the session is currently elevated. Auth middleware
    /// calls this to gate sensitive routes.
    pub async fn is_elevated(&self, session_id: &str) -> bool {
        self.elevation_state(session_id).await.0
    }

    /// Invalidate a session (logout).
    pub async fn invalidate_session(&self, session_id: &str) {
        self.sessions.write().await.remove(session_id);
    }

    /// Remove expired sessions. Called periodically.
    pub async fn cleanup_expired(&self) {
        let mut sessions = self.sessions.write().await;
        let now = Instant::now();
        sessions.retain(|_, s| now < s.expires_at);
    }

    /// Spawn periodic cleanup (piggybacks on the rate limiter's interval).
    pub fn spawn_cleanup_task(self: &Arc<Self>) {
        let manager = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(60));
            loop {
                interval.tick().await;
                manager.cleanup_expired().await;
            }
        });
    }
}

/// Hash a device binding secret with SHA-256. The input has 256 bits
/// of entropy from the client's `crypto.getRandomValues`, so plain
/// SHA-256 is sufficient and avoids needing a process-scoped secret.
fn hash_binding_secret(secret: &[u8]) -> [u8; 32] {
    Sha256::digest(secret).into()
}

/// Decode a base64url-encoded device binding secret from the wire.
/// Returns the raw bytes only when they decode to exactly
/// `BINDING_SECRET_BYTES`. Both padded and unpadded base64url are
/// accepted because browser base64url emitters disagree on padding.
pub fn decode_binding_secret(s: &str) -> Option<Vec<u8>> {
    use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};
    use base64::Engine;
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return None;
    }
    let decoded = URL_SAFE_NO_PAD
        .decode(trimmed)
        .or_else(|_| URL_SAFE.decode(trimmed))
        .ok()?;
    if decoded.len() == BINDING_SECRET_BYTES {
        Some(decoded)
    } else {
        None
    }
}

/// Check if passphrase meets minimum length. Returns a warning message if not.
pub fn check_passphrase_strength(passphrase: &str) -> Option<String> {
    if passphrase.len() < MIN_PASSPHRASE_LENGTH {
        Some(format!(
            "WARNING: Passphrase is only {} characters. \
             Consider using at least {} characters for better security.",
            passphrase.len(),
            MIN_PASSPHRASE_LENGTH
        ))
    } else {
        None
    }
}

// ── Handlers ───────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct LoginRequest {
    passphrase: String,
    /// Base64url encoding of 32 random bytes the client persists in
    /// `localStorage`. Required since #1131; without it the session
    /// cannot be device-bound and the response is 400.
    device_binding_secret: String,
}

/// POST /api/login
pub async fn login_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    headers: axum::http::HeaderMap,
    login_body: Result<Json<LoginRequest>, axum::extract::rejection::JsonRejection>,
) -> axum::response::Response {
    let client_ip = resolve_client_ip(addr, &headers);

    if !state.login_manager.is_enabled() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "not_found",
                "message": "Login is not enabled"
            })),
        )
            .into_response();
    }

    // Rate limit check
    if let Some(remaining) = state.rate_limiter.check_locked(client_ip).await {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [("Retry-After", remaining.to_string())],
            Json(serde_json::json!({
                "error": "rate_limited",
                "message": format!("Too many failed attempts. Try again in {} seconds.", remaining)
            })),
        )
            .into_response();
    }

    let login_req = match login_body {
        Ok(Json(req)) => req,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "bad_request",
                    "message": "Missing or invalid passphrase / device_binding_secret"
                })),
            )
                .into_response();
        }
    };

    let Some(binding_bytes) = decode_binding_secret(&login_req.device_binding_secret) else {
        // Treat malformed bindings as a usage error (the client sent
        // garbage), not a failed login attempt: no rate-limiter
        // increment, no audit log.
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "error": "bad_request",
                "message": format!(
                    "device_binding_secret must be base64url of {} random bytes",
                    BINDING_SECRET_BYTES
                )
            })),
        )
            .into_response();
    };

    tracing::debug!(
        ip = %client_ip,
        passphrase_len = login_req.passphrase.len(),
        "Login attempt"
    );

    if state.login_manager.verify_passphrase(&login_req.passphrase) {
        state.rate_limiter.record_success(client_ip).await;

        let session_id = state.login_manager.create_session(&binding_bytes).await;

        tracing::info!(target: "auth.passphrase", ip = %client_ip, "passphrase login successful");

        let cookie = build_login_cookie(&session_id, state.behind_tunnel);
        let mut response = Json(serde_json::json!({
            "ok": true
        }))
        .into_response();

        response.headers_mut().insert(
            header::SET_COOKIE,
            cookie.parse().expect("cookie format must be valid"),
        );

        response
    } else {
        let locked = state.rate_limiter.record_failure(client_ip).await;
        tracing::warn!(
            target: "auth.passphrase",
            ip = %client_ip,
            locked = locked,
            reason = "incorrect_passphrase",
            "passphrase login failed"
        );

        (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "unauthorized",
                "message": "Incorrect passphrase"
            })),
        )
            .into_response()
    }
}

#[derive(Deserialize)]
pub struct ElevateRequest {
    passphrase: String,
}

/// POST /api/login/elevate
///
/// Re-verifies the passphrase against the configured hash and, on
/// success, sets the calling session's elevation window. Sensitive
/// routes (terminal attach, cockpit command execution, file writes)
/// gate on the resulting `is_elevated` flag in the auth middleware.
/// Already requires a valid token, login session cookie, and device
/// binding by the time the handler runs (the middleware enforces all
/// of those). See #1131.
pub async fn elevate_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    request: axum::extract::Request,
) -> axum::response::Response {
    let client_ip = resolve_client_ip(addr, request.headers());

    if !state.login_manager.is_enabled() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "not_found",
                "message": "Login is not enabled"
            })),
        )
            .into_response();
    }

    let Some(session_id) = extract_login_session(&request) else {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "unauthorized",
                "message": "No active login session"
            })),
        )
            .into_response();
    };

    if let Some(remaining) = state.rate_limiter.check_locked(client_ip).await {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [("Retry-After", remaining.to_string())],
            Json(serde_json::json!({
                "error": "rate_limited",
                "message": format!("Too many failed attempts. Try again in {} seconds.", remaining)
            })),
        )
            .into_response();
    }

    // Body is a small JSON object; read it eagerly. The middleware
    // already authenticated the request so we can safely buffer it.
    let body_bytes = match axum::body::to_bytes(request.into_body(), 4096).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "bad_request",
                    "message": "Could not read request body"
                })),
            )
                .into_response();
        }
    };

    let elevate_req: ElevateRequest = match serde_json::from_slice(&body_bytes) {
        Ok(r) => r,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "bad_request",
                    "message": "Missing or invalid passphrase field"
                })),
            )
                .into_response();
        }
    };

    if !state
        .login_manager
        .verify_passphrase(&elevate_req.passphrase)
    {
        let locked = state.rate_limiter.record_failure(client_ip).await;
        tracing::warn!(
            target: "auth.passphrase",
            ip = %client_ip,
            locked = locked,
            reason = "incorrect_passphrase_on_elevate",
            "elevation failed"
        );
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "unauthorized",
                "message": "Incorrect passphrase"
            })),
        )
            .into_response();
    }

    state.rate_limiter.record_success(client_ip).await;
    let elevated = state.login_manager.elevate_session(&session_id).await;
    if !elevated {
        return (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({
                "error": "unauthorized",
                "message": "Login session expired"
            })),
        )
            .into_response();
    }

    let (_, remaining_secs) = state.login_manager.elevation_state(&session_id).await;
    tracing::info!(
        target: "auth.passphrase",
        ip = %client_ip,
        "session elevated"
    );

    Json(serde_json::json!({
        "ok": true,
        "elevated_until_secs": remaining_secs,
    }))
    .into_response()
}

/// POST /api/logout
pub async fn logout_handler(
    State(state): State<Arc<AppState>>,
    request: axum::extract::Request,
) -> axum::response::Response {
    // Extract session cookie
    if let Some(session_id) = extract_login_session(&request) {
        state.login_manager.invalidate_session(&session_id).await;
    }

    let clear_cookie = format!(
        "aoe_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0{}",
        if state.behind_tunnel { "; Secure" } else { "" }
    );

    let mut response = Json(serde_json::json!({ "ok": true })).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        clear_cookie.parse().expect("cookie format must be valid"),
    );

    response
}

/// GET /api/login/status
///
/// Returns whether passphrase login is required, whether the caller
/// currently holds a valid login session, and the elevation state
/// (used by the frontend to decide whether to prompt for the
/// passphrase again before a high-risk action). `authenticated` is
/// only true when both the session cookie AND the device binding
/// secret match, mirroring the auth middleware's enforcement (#1131).
pub async fn login_status_handler(
    State(state): State<Arc<AppState>>,
    request: axum::extract::Request,
) -> Json<serde_json::Value> {
    let required = state.login_manager.is_enabled();

    if !required {
        return Json(serde_json::json!({
            "required": false,
            "authenticated": true,
            "elevated": true,
            "elevated_until_secs": null,
        }));
    }

    let session_id = extract_login_session(&request);
    let presented_binding = super::auth::extract_device_binding(&request);

    let (authenticated, session_id_for_elevation) = match (session_id, presented_binding) {
        (Some(sid), Some(secret)) => {
            let ok = state.login_manager.validate_session(&sid, &secret).await;
            (ok, if ok { Some(sid) } else { None })
        }
        _ => (false, None),
    };

    let (elevated, elevated_secs) = match session_id_for_elevation {
        Some(sid) => state.login_manager.elevation_state(&sid).await,
        None => (false, None),
    };

    Json(serde_json::json!({
        "required": required,
        "authenticated": authenticated,
        "elevated": elevated,
        "elevated_until_secs": elevated_secs,
    }))
}

/// Extract the `aoe_session` cookie value from a request.
pub fn extract_login_session(request: &axum::extract::Request) -> Option<String> {
    let cookie_header = request.headers().get(header::COOKIE)?;
    let cookie_str = cookie_header.to_str().ok()?;
    for cookie in cookie_str.split(';') {
        let cookie = cookie.trim();
        if let Some(value) = cookie.strip_prefix("aoe_session=") {
            if !value.is_empty() {
                return Some(value.to_string());
            }
        }
    }
    None
}

/// Build a Set-Cookie header for the login session.
pub fn build_login_cookie(session_id: &str, secure: bool) -> String {
    let mut cookie = format!(
        "aoe_session={}; HttpOnly; SameSite=Strict; Path=/; Max-Age=2592000",
        session_id
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn login_manager_disabled_when_no_passphrase() {
        let mgr = LoginManager::new(None);
        assert!(!mgr.is_enabled());
    }

    #[test]
    fn login_manager_enabled_when_passphrase_set() {
        let mgr = LoginManager::new(Some("test123"));
        assert!(mgr.is_enabled());
    }

    #[test]
    fn verify_correct_passphrase() {
        let mgr = LoginManager::new(Some("my_secret"));
        assert!(mgr.verify_passphrase("my_secret"));
    }

    #[test]
    fn verify_incorrect_passphrase() {
        let mgr = LoginManager::new(Some("my_secret"));
        assert!(!mgr.verify_passphrase("wrong"));
    }

    #[test]
    fn verify_empty_passphrase() {
        let mgr = LoginManager::new(Some("my_secret"));
        assert!(!mgr.verify_passphrase(""));
    }

    #[test]
    fn verify_fails_when_disabled() {
        let mgr = LoginManager::new(None);
        assert!(!mgr.verify_passphrase("anything"));
    }

    fn binding(byte: u8) -> Vec<u8> {
        vec![byte; BINDING_SECRET_BYTES]
    }

    #[tokio::test]
    async fn create_and_validate_session() {
        let mgr = LoginManager::new(Some("test"));
        let secret = binding(0xAA);
        let session_id = mgr.create_session(&secret).await;
        assert!(mgr.validate_session(&session_id, &secret).await);
    }

    #[tokio::test]
    async fn validate_rejects_wrong_binding() {
        let mgr = LoginManager::new(Some("test"));
        let secret = binding(0xAA);
        let other = binding(0xBB);
        let session_id = mgr.create_session(&secret).await;
        assert!(!mgr.validate_session(&session_id, &other).await);
    }

    #[tokio::test]
    async fn validate_accepts_after_ip_change_when_binding_matches() {
        // Regression for #1131: a mobile client whose public IP rotates
        // (Wi-Fi -> cellular handoff, CGNAT, iCloud Private Relay) must
        // not be logged out as long as the device-binding secret still
        // matches. The session has no IP field anymore; just verify
        // back-to-back validations on the same secret keep working.
        let mgr = LoginManager::new(Some("test"));
        let secret = binding(0xCC);
        let session_id = mgr.create_session(&secret).await;
        assert!(mgr.validate_session(&session_id, &secret).await);
        assert!(mgr.validate_session(&session_id, &secret).await);
    }

    #[tokio::test]
    async fn validate_rejects_missing_or_empty() {
        let mgr = LoginManager::new(Some("test"));
        let secret = binding(0xDD);
        let _session_id = mgr.create_session(&secret).await;
        assert!(!mgr.validate_session("nonexistent", &secret).await);
        assert!(!mgr.validate_session("", &secret).await);
    }

    #[tokio::test]
    async fn validate_rejects_wrong_length_binding() {
        let mgr = LoginManager::new(Some("test"));
        let secret = binding(0xEE);
        let session_id = mgr.create_session(&secret).await;
        // 31 bytes -> rejected even though the prefix matches.
        let short = vec![0xEE; BINDING_SECRET_BYTES - 1];
        assert!(!mgr.validate_session(&session_id, &short).await);
    }

    #[tokio::test]
    async fn invalidate_session_removes_it() {
        let mgr = LoginManager::new(Some("test"));
        let secret = binding(0x11);
        let session_id = mgr.create_session(&secret).await;
        mgr.invalidate_session(&session_id).await;
        assert!(!mgr.validate_session(&session_id, &secret).await);
    }

    #[tokio::test]
    async fn invalidate_unknown_session_is_noop() {
        let mgr = LoginManager::new(Some("test"));
        mgr.invalidate_session("nonexistent").await;
    }

    #[tokio::test]
    async fn elevation_starts_false_and_can_be_set() {
        let mgr = LoginManager::new(Some("test"));
        let secret = binding(0x22);
        let session_id = mgr.create_session(&secret).await;
        assert!(!mgr.is_elevated(&session_id).await);
        assert!(mgr.elevate_session(&session_id).await);
        let (elevated, remaining) = mgr.elevation_state(&session_id).await;
        assert!(elevated);
        assert!(remaining.is_some());
    }

    #[tokio::test]
    async fn elevation_rejects_unknown_session() {
        let mgr = LoginManager::new(Some("test"));
        assert!(!mgr.elevate_session("nope").await);
        assert!(!mgr.is_elevated("nope").await);
    }

    #[tokio::test]
    async fn elevation_expires() {
        let mgr = LoginManager::new(Some("test"));
        let secret = binding(0x33);
        let session_id = mgr.create_session(&secret).await;
        assert!(mgr.elevate_session(&session_id).await);
        // Manually rewind the deadline into the past.
        {
            let mut sessions = mgr.sessions.write().await;
            if let Some(s) = sessions.get_mut(&session_id) {
                s.elevated_until = Some(Instant::now() - Duration::from_secs(1));
            }
        }
        assert!(!mgr.is_elevated(&session_id).await);
    }

    #[tokio::test]
    async fn max_sessions_evicts_oldest() {
        let mgr = LoginManager::new(Some("test"));
        let secret = binding(0x44);
        let mut first_id = String::new();
        for i in 0..MAX_SESSIONS {
            let id = mgr.create_session(&secret).await;
            if i == 0 {
                first_id = id;
            }
        }
        assert!(mgr.validate_session(&first_id, &secret).await);
        let _new_id = mgr.create_session(&secret).await;
        let sessions = mgr.sessions.read().await;
        assert_eq!(sessions.len(), MAX_SESSIONS);
    }

    #[tokio::test]
    async fn cleanup_expired_removes_stale() {
        let mgr = LoginManager::new(Some("test"));
        let secret = binding(0x55);
        let session_id = mgr.create_session(&secret).await;
        {
            let mut sessions = mgr.sessions.write().await;
            if let Some(s) = sessions.get_mut(&session_id) {
                s.expires_at = Instant::now() - Duration::from_secs(1);
            }
        }
        mgr.cleanup_expired().await;
        assert!(!mgr.validate_session(&session_id, &secret).await);
    }

    #[test]
    fn decode_binding_secret_accepts_url_safe_no_pad() {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        let raw = [0xAB; BINDING_SECRET_BYTES];
        let encoded = URL_SAFE_NO_PAD.encode(raw);
        let decoded = decode_binding_secret(&encoded).expect("decodes");
        assert_eq!(decoded, raw);
    }

    #[test]
    fn decode_binding_secret_rejects_wrong_length() {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        let too_short = URL_SAFE_NO_PAD.encode([0xAB; 16]);
        assert!(decode_binding_secret(&too_short).is_none());
        let too_long = URL_SAFE_NO_PAD.encode([0xAB; 64]);
        assert!(decode_binding_secret(&too_long).is_none());
    }

    #[test]
    fn decode_binding_secret_rejects_garbage() {
        assert!(decode_binding_secret("").is_none());
        assert!(decode_binding_secret("!@#$%^&*()").is_none());
    }

    #[test]
    fn passphrase_strength_short() {
        assert!(check_passphrase_strength("short").is_some());
    }

    #[test]
    fn passphrase_strength_adequate() {
        assert!(check_passphrase_strength("longenough").is_none());
    }

    #[test]
    fn build_cookie_without_secure() {
        let cookie = build_login_cookie("abc123", false);
        assert!(cookie.contains("aoe_session=abc123"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
        assert!(cookie.contains("Max-Age=2592000"));
        assert!(!cookie.contains("Secure"));
    }

    #[test]
    fn build_cookie_with_secure() {
        let cookie = build_login_cookie("abc123", true);
        assert!(cookie.contains("Secure"));
    }

    #[test]
    fn extract_session_from_cookie_header() {
        let request = axum::http::Request::builder()
            .header(header::COOKIE, "aoe_token=foo; aoe_session=bar123")
            .body(axum::body::Body::empty())
            .unwrap();

        assert_eq!(extract_login_session(&request), Some("bar123".to_string()));
    }

    #[test]
    fn extract_session_missing_cookie() {
        let request = axum::http::Request::builder()
            .header(header::COOKIE, "aoe_token=foo")
            .body(axum::body::Body::empty())
            .unwrap();

        assert_eq!(extract_login_session(&request), None);
    }

    #[test]
    fn extract_session_no_cookie_header() {
        let request = axum::http::Request::builder()
            .body(axum::body::Body::empty())
            .unwrap();

        assert_eq!(extract_login_session(&request), None);
    }
}
