//! Passphrase-based login as a second authentication factor.
//!
//! When a passphrase is configured, users must enter it after token auth
//! to access the dashboard. Login sessions are tracked server-side with
//! IP binding and a 30-day sliding expiry window.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::State;
use axum::http::{header, StatusCode};
use axum::response::IntoResponse;
use axum::Json;
use serde::Deserialize;
use tokio::sync::RwLock;

use super::auth::resolve_client_ip;
use super::AppState;

/// 30-day session lifetime (sliding window).
const SESSION_LIFETIME: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// Maximum concurrent login sessions before evicting the oldest.
const MAX_SESSIONS: usize = 50;

/// Minimum recommended passphrase length.
const MIN_PASSPHRASE_LENGTH: usize = 8;

struct LoginSession {
    expires_at: Instant,
    ip: IpAddr,
    #[allow(dead_code)]
    user_agent: String,
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

    /// Create a new login session. Returns the session ID (64-char hex).
    pub async fn create_session(&self, ip: IpAddr, user_agent: &str) -> String {
        let session_id = super::generate_token();
        let session = LoginSession {
            expires_at: Instant::now() + SESSION_LIFETIME,
            ip,
            user_agent: user_agent.to_string(),
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

    /// Validate a session. Checks existence, expiry, and IP match.
    /// On success, extends the sliding window.
    pub async fn validate_session(&self, session_id: &str, client_ip: IpAddr) -> bool {
        if session_id.is_empty() {
            return false;
        }

        let mut sessions = self.sessions.write().await;
        let Some(session) = sessions.get_mut(session_id) else {
            return false;
        };

        if Instant::now() > session.expires_at {
            sessions.remove(session_id);
            return false;
        }

        if session.ip != client_ip {
            return false;
        }

        // Sliding window: extend expiry on each valid access
        session.expires_at = Instant::now() + SESSION_LIFETIME;
        true
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
}

/// POST /api/login
pub async fn login_handler(
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

    // Extract body
    let body = match axum::body::to_bytes(request.into_body(), 1024).await {
        Ok(b) => b,
        Err(_) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "error": "bad_request",
                    "message": "Invalid request body"
                })),
            )
                .into_response();
        }
    };

    let login_req: LoginRequest = match serde_json::from_slice(&body) {
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

    tracing::debug!(
        ip = %client_ip,
        passphrase_len = login_req.passphrase.len(),
        "Login attempt"
    );

    if state.login_manager.verify_passphrase(&login_req.passphrase) {
        state.rate_limiter.record_success(client_ip).await;

        let user_agent = addr.to_string(); // simplified
        let session_id = state
            .login_manager
            .create_session(client_ip, &user_agent)
            .await;

        tracing::info!(ip = %client_ip, "Login successful");

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
        tracing::warn!(ip = %client_ip, locked = locked, "Login failed: incorrect passphrase");

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
pub async fn login_status_handler(
    State(state): State<Arc<AppState>>,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    request: axum::extract::Request,
) -> Json<serde_json::Value> {
    let required = state.login_manager.is_enabled();

    let authenticated = if required {
        if let Some(session_id) = extract_login_session(&request) {
            let client_ip = resolve_client_ip(addr, request.headers());
            state
                .login_manager
                .validate_session(&session_id, client_ip)
                .await
        } else {
            false
        }
    } else {
        true
    };

    Json(serde_json::json!({
        "required": required,
        "authenticated": authenticated
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
fn build_login_cookie(session_id: &str, secure: bool) -> String {
    let mut cookie = format!(
        "aoe_session={}; HttpOnly; SameSite=Strict; Path=/; Max-Age=2592000",
        session_id
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

/// Build a Set-Cookie header that refreshes the sliding window.
pub fn refresh_login_cookie(session_id: &str, secure: bool) -> String {
    build_login_cookie(session_id, secure)
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

    #[tokio::test]
    async fn create_and_validate_session() {
        let mgr = LoginManager::new(Some("test"));
        let ip: IpAddr = "1.2.3.4".parse().unwrap();
        let session_id = mgr.create_session(ip, "test-agent").await;

        assert!(mgr.validate_session(&session_id, ip).await);
    }

    #[tokio::test]
    async fn validate_rejects_wrong_ip() {
        let mgr = LoginManager::new(Some("test"));
        let ip: IpAddr = "1.2.3.4".parse().unwrap();
        let other_ip: IpAddr = "5.6.7.8".parse().unwrap();
        let session_id = mgr.create_session(ip, "test-agent").await;

        assert!(!mgr.validate_session(&session_id, other_ip).await);
    }

    #[tokio::test]
    async fn validate_rejects_unknown_session() {
        let mgr = LoginManager::new(Some("test"));
        let ip: IpAddr = "1.2.3.4".parse().unwrap();

        assert!(!mgr.validate_session("nonexistent", ip).await);
    }

    #[tokio::test]
    async fn validate_rejects_empty_session_id() {
        let mgr = LoginManager::new(Some("test"));
        let ip: IpAddr = "1.2.3.4".parse().unwrap();

        assert!(!mgr.validate_session("", ip).await);
    }

    #[tokio::test]
    async fn invalidate_session_removes_it() {
        let mgr = LoginManager::new(Some("test"));
        let ip: IpAddr = "1.2.3.4".parse().unwrap();
        let session_id = mgr.create_session(ip, "test-agent").await;

        mgr.invalidate_session(&session_id).await;
        assert!(!mgr.validate_session(&session_id, ip).await);
    }

    #[tokio::test]
    async fn invalidate_unknown_session_is_noop() {
        let mgr = LoginManager::new(Some("test"));
        mgr.invalidate_session("nonexistent").await;
        // No panic, no error
    }

    #[tokio::test]
    async fn max_sessions_evicts_oldest() {
        let mgr = LoginManager::new(Some("test"));
        let ip: IpAddr = "1.2.3.4".parse().unwrap();

        let mut first_id = String::new();
        for i in 0..MAX_SESSIONS {
            let id = mgr.create_session(ip, &format!("agent-{}", i)).await;
            if i == 0 {
                first_id = id;
            }
        }

        // First session should still be valid (at capacity, not over)
        assert!(mgr.validate_session(&first_id, ip).await);

        // Adding one more should evict the oldest (which is now the second one,
        // since first_id just had its expiry refreshed by validate_session above)
        let _new_id = mgr.create_session(ip, "agent-overflow").await;
        let sessions = mgr.sessions.read().await;
        assert_eq!(sessions.len(), MAX_SESSIONS);
    }

    #[tokio::test]
    async fn cleanup_expired_removes_stale() {
        let mgr = LoginManager::new(Some("test"));
        let ip: IpAddr = "1.2.3.4".parse().unwrap();
        let session_id = mgr.create_session(ip, "test-agent").await;

        // Manually expire the session
        {
            let mut sessions = mgr.sessions.write().await;
            if let Some(s) = sessions.get_mut(&session_id) {
                s.expires_at = Instant::now() - Duration::from_secs(1);
            }
        }

        mgr.cleanup_expired().await;

        assert!(!mgr.validate_session(&session_id, ip).await);
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
