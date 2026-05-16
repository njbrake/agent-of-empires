//! Token-based authentication middleware for the web dashboard.
//!
//! Accepts the auth token via:
//! - Cookie: `aoe_token=<token>`
//! - Query parameter: `?token=<token>` (sets the cookie for future requests)
//! - WebSocket protocol header: `Sec-WebSocket-Protocol: <token>`
//! - Authorization header: `Authorization: Bearer <token>` (used by the PWA,
//!   which persists the token in localStorage since iOS `start_url` strips
//!   the query param on home-screen relaunch)
//!
//! Includes rate limiting (5 failed attempts = 15 min lockout) and device tracking.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use axum::{
    extract::{ConnectInfo, Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use super::AppState;

/// Constant-time string comparison to prevent timing attacks on token values.
pub(crate) fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// Resolve the real client IP, trusting X-Forwarded-For only from loopback
/// (i.e., only when the request came through the cloudflared proxy).
pub(crate) fn resolve_client_ip(
    socket_addr: SocketAddr,
    headers: &axum::http::HeaderMap,
) -> IpAddr {
    let socket_ip = socket_addr.ip();
    if socket_ip.is_loopback() {
        if let Some(cf_ip) = headers.get("cf-connecting-ip") {
            if let Ok(ip_str) = cf_ip.to_str() {
                if let Ok(ip) = ip_str.trim().parse::<IpAddr>() {
                    return ip;
                }
            }
        }
        if let Some(xff) = headers.get("x-forwarded-for") {
            if let Ok(xff_str) = xff.to_str() {
                if let Some(last) = xff_str.rsplit(',').next() {
                    if let Ok(ip) = last.trim().parse::<IpAddr>() {
                        return ip;
                    }
                }
            }
        }
    }
    socket_ip
}

/// Build a Set-Cookie header value with optional Secure flag for HTTPS tunnels.
fn build_cookie(token: &str, secure: bool, max_age_secs: u64) -> String {
    let mut cookie = format!(
        "aoe_token={}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}",
        token, max_age_secs
    );
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

/// Write the `aoe_token` Set-Cookie and the companion `x-aoe-token`
/// header into a response's header map.
///
/// Uses `.append` for `Set-Cookie` because the handler that just ran
/// may already have set its own (`login_handler` sets `aoe_session`
/// on successful login; `logout_handler` clears it). `.insert` would
/// replace those values and the browser would only see our
/// `aoe_token` cookie, silently dropping the session state the
/// handler intended to set. The two cookie names don't collide, so
/// the browser processes both Set-Cookie values. `.insert` is fine
/// for `x-aoe-token` because no handler writes that header.
fn write_token_headers(
    headers: &mut axum::http::HeaderMap,
    token: &str,
    behind_tunnel: bool,
    max_age_secs: u64,
) {
    let cookie = build_cookie(token, behind_tunnel, max_age_secs);
    headers.append(
        header::SET_COOKIE,
        cookie.parse().expect("cookie format must be valid"),
    );
    if let Ok(value) = token.parse() {
        headers.insert("x-aoe-token", value);
    }
}

/// Attach both the Set-Cookie and X-Aoe-Token headers to a response. The
/// cookie covers the browser flow; X-Aoe-Token lets the PWA update its
/// localStorage-cached token when the server rotates. Without the header,
/// a rotated token would brick the PWA until the user manually re-visits
/// with a fresh `?token=` URL.
async fn attach_token_headers(response: &mut Response, state: &AppState) {
    let Some(current) = state.token_manager.current_token().await else {
        return;
    };
    let max_age = state.token_manager.lifetime_secs().await;
    write_token_headers(
        response.headers_mut(),
        &current,
        state.behind_tunnel,
        max_age,
    );
}

const MAX_DEVICES: usize = 100;

/// Record a successful device connection for tracking.
async fn record_device(state: &AppState, ip: IpAddr, user_agent: &str) {
    let ip_str = ip.to_string();
    let ua = user_agent.to_string();
    let mut devices = state.devices.write().await;
    if let Some(device) = devices
        .iter_mut()
        .find(|d| d.ip == ip_str && d.user_agent == ua)
    {
        device.last_seen = chrono::Utc::now();
        device.request_count += 1;
    } else {
        if devices.len() >= MAX_DEVICES {
            if let Some(oldest_idx) = devices
                .iter()
                .enumerate()
                .min_by_key(|(_, d)| d.last_seen)
                .map(|(i, _)| i)
            {
                devices.remove(oldest_idx);
            }
        }
        devices.push(super::DeviceInfo {
            ip: ip_str,
            user_agent: ua,
            first_seen: chrono::Utc::now(),
            last_seen: chrono::Utc::now(),
            request_count: 1,
        });
    }
}

/// Extract all token candidates from the request (cookie, query parameter, and
/// Authorization header). Returns them in priority order so callers can try
/// each until one validates. A stale cookie must not prevent a valid query
/// param or Bearer token from being tried.
fn extract_tokens(request: &Request) -> Vec<(&str, TokenSource)> {
    let mut tokens = Vec::new();

    // Check cookie
    if let Some(cookie_header) = request.headers().get(header::COOKIE) {
        if let Ok(cookie_str) = cookie_header.to_str() {
            for cookie in cookie_str.split(';') {
                let cookie = cookie.trim();
                if let Some(value) = cookie.strip_prefix("aoe_token=") {
                    tokens.push((value, TokenSource::Cookie));
                }
            }
        }
    }

    // Check query parameter
    if let Some(query) = request.uri().query() {
        for param in query.split('&') {
            if let Some(value) = param.strip_prefix("token=") {
                tokens.push((value, TokenSource::QueryParam));
            }
        }
    }

    // Check Authorization: Bearer header
    if let Some(auth_header) = request.headers().get(header::AUTHORIZATION) {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(value) = auth_str.strip_prefix("Bearer ") {
                tokens.push((value.trim(), TokenSource::Bearer));
            }
        }
    }

    tokens
}

/// Extract all WebSocket sub-protocol values from the request.
/// Each must be individually validated since the token could be in any position
/// alongside actual sub-protocol names (e.g., "graphql-ws, <token>").
fn extract_ws_protocols(request: &Request) -> Vec<String> {
    let mut protocols = Vec::new();
    if let Some(header) = request.headers().get("sec-websocket-protocol") {
        if let Ok(proto_str) = header.to_str() {
            for proto in proto_str.split(',') {
                let trimmed = proto.trim();
                if !trimmed.is_empty() {
                    protocols.push(trimmed.to_string());
                }
            }
        }
    }
    protocols
}

/// Strip a possible trailing slash from a path so suffix matches are
/// not bypassed by `/api/sessions/123/cockpit/prompt/` (axum routes
/// both forms to the same handler). Cheap and explicit.
fn normalize_path(path: &str) -> &str {
    path.strip_suffix('/').unwrap_or(path)
}

/// Whether a request path is exempt from the passphrase session +
/// device-binding check. These paths are either the login bootstrap
/// flow itself or static assets that pre-load the SPA shell.
fn is_login_session_exempt(path: &str) -> bool {
    path == "/login"
        || path == "/api/login"
        || path == "/api/login/status"
        || path == "/api/logout"
        || path.starts_with("/assets/")
        || path == "/manifest.json"
        || path == "/sw.js"
        || path.starts_with("/icon-")
        || path.starts_with("/fonts/")
}

/// Whether to append a sliding-window refresh of the `aoe_session`
/// cookie on the response for a session-authenticated request.
/// Login-exempt paths skip the refresh because their own handlers
/// own the `aoe_session` cookie's lifecycle on that response: the
/// `POST /api/logout` handler sets `Max-Age=0` to clear it, and the
/// `POST /api/login` handler mints a fresh session id and sets a
/// new cookie. An unconditional refresh would emit a second
/// `Set-Cookie: aoe_session=<id>; Max-Age=2592000` after the
/// handler's, and browsers process Set-Cookie headers in order
/// (later wins), so logout would leave a stale 30-day cookie
/// pointing at a server session that no longer exists, and login
/// would clobber the new session id with the old one.
fn should_refresh_session_cookie(path: &str) -> bool {
    !is_login_session_exempt(path)
}

/// Whether a request path + method needs an elevated login session
/// (step-up auth, 15-minute passphrase confirmation window).
///
/// Scope is intentionally narrow: only persistent-config writes that
/// can plant code for the owner's next session spawn. Daily-use
/// surfaces (cockpit prompt, terminal attach, session lifecycle,
/// approval resolution) rely on the session cookie + device binding
/// alone, matching the SSH model the user wanted. See discussion on
/// #1137. The protected attack class is the persisted-tamper pattern:
/// an attacker with stolen session and binding plants a malicious
/// Docker image, worktree template, or profile, then waits for the
/// owner to spawn a session that runs it. The writes must be gated
/// even though the spawn itself is not, because the spawn runs with
/// the legitimate owner's elevation, not the attacker's.
///
/// Read-only `GET`/`HEAD` on these resources stay open; this is an
/// allow-list, not a default-deny, so adding a benign read endpoint
/// never accidentally hides behind a passphrase prompt. When adding
/// a new mutating settings/profile surface, add it here AND a case
/// in `requires_elevation_paths`.
fn requires_elevation(method: &axum::http::Method, path: &str) -> bool {
    use axum::http::Method;

    let path = normalize_path(path);

    if method == Method::GET || method == Method::HEAD {
        return false;
    }

    // Settings + profile mutations. These persist the Docker image,
    // environment, volume mounts, and worktree templates the owner's
    // next session spawn uses.
    if path == "/api/settings" && method == Method::PATCH {
        return true;
    }
    if path == "/api/default-profile" && method == Method::PATCH {
        return true;
    }
    if path == "/api/profiles" && method == Method::POST {
        return true;
    }
    if path.starts_with("/api/profiles/") {
        // Per-profile writes: PATCH /api/profiles/{name}/settings,
        // PATCH .../rename, DELETE /api/profiles/{name}. Read GETs
        // were filtered out by the GET/HEAD bypass above.
        return true;
    }

    false
}

/// Strip the leading `<prefix>.` from a subprotocol value when present,
/// returning the suffix. Used to read prefixed values like
/// `aoe-token.<token>` and `aoe-device.<base64url-secret>` from a
/// `Sec-WebSocket-Protocol` header without confusing them with
/// historically-bare token entries.
fn strip_ws_prefix<'a>(proto: &'a str, prefix: &str) -> Option<&'a str> {
    let with_dot = proto.strip_prefix(prefix)?;
    with_dot.strip_prefix('.')
}

/// Extract the device binding secret presented by the client. Returns
/// the decoded 32 raw bytes when present and well-formed; `None`
/// otherwise. For REST the secret rides the `X-Aoe-Device-Binding`
/// header; for WebSocket upgrades it rides as
/// `Sec-WebSocket-Protocol: aoe-device.<base64url>` (never a query
/// param, which would leak into proxy logs). See #1131.
pub(crate) fn extract_device_binding(request: &Request) -> Option<Vec<u8>> {
    if let Some(value) = request.headers().get("x-aoe-device-binding") {
        if let Ok(s) = value.to_str() {
            if let Some(bytes) = super::login::decode_binding_secret(s) {
                return Some(bytes);
            }
        }
    }
    for proto in extract_ws_protocols(request) {
        if let Some(secret) = strip_ws_prefix(&proto, "aoe-device") {
            if let Some(bytes) = super::login::decode_binding_secret(secret) {
                return Some(bytes);
            }
        }
    }
    None
}

#[derive(Debug, PartialEq)]
enum TokenSource {
    Cookie,
    QueryParam,
    WebSocketProtocol,
    Bearer,
}

/// Request extension carrying the SHA-256 hash of the bearer token that
/// authenticated this request. Inserted by `auth_middleware` after a
/// successful token match; absent in no-auth mode. Push handlers read
/// this to filter subscriptions by owner.
#[derive(Clone, Copy, Debug)]
pub struct AuthenticatedTokenHash(pub [u8; 32]);

pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    mut request: Request,
    next: Next,
) -> Response {
    let client_ip = resolve_client_ip(addr, request.headers());

    // Trace cockpit ws specifically so we can see whether the
    // browser ever reached the server when the cockpit live updates
    // get stuck. Other ws paths (terminal) are not as load-bearing
    // for diagnostics today.
    if request.uri().path().contains("/cockpit/ws") {
        let token_sources: Vec<&'static str> = extract_tokens(&request)
            .iter()
            .map(|(_, src)| match src {
                TokenSource::Cookie => "cookie",
                TokenSource::QueryParam => "query",
                TokenSource::Bearer => "bearer",
                TokenSource::WebSocketProtocol => "ws-proto",
            })
            .collect();
        let ws_protocols = extract_ws_protocols(&request);
        tracing::debug!(
            target: "auth",
            ip = %client_ip,
            token_sources = ?token_sources,
            ws_protocol_count = ws_protocols.len(),
            "auth_middleware entered for cockpit ws"
        );
    }

    // No-auth mode: pass everything through. Insert a zeroed
    // AuthenticatedTokenHash so handlers that extract the extension
    // still succeed; all no-auth clients share the same "owner" value.
    if state.token_manager.is_no_auth().await {
        // Once per process: surface that auth is disabled. Helps when a
        // user is confused why their token isn't being checked.
        static NO_AUTH_LOGGED: std::sync::Once = std::sync::Once::new();
        NO_AUTH_LOGGED.call_once(|| {
            tracing::info!(
                target: "auth.token",
                "running in no-auth mode; requests pass through without token check"
            );
        });
        request
            .extensions_mut()
            .insert(AuthenticatedTokenHash([0u8; 32]));
        return next.run(request).await;
    }

    // Rate limit check BEFORE token validation
    if let Some(remaining_secs) = state.rate_limiter.check_locked(client_ip).await {
        tracing::warn!(
            target: "auth.rate_limit",
            ip = %client_ip,
            remaining_secs,
            "rejecting request from locked-out IP"
        );
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [("Retry-After", remaining_secs.to_string())],
            axum::Json(serde_json::json!({
                "error": "rate_limited",
                "message": format!(
                    "Too many failed attempts. Try again in {} seconds.",
                    remaining_secs
                )
            })),
        )
            .into_response();
    }

    // Steady-state path: a bound device authenticates with its
    // passphrase session + device binding alone. The token is a
    // first-device-pairing nonce, NOT a per-request second factor;
    // we only consult it on bootstrap paths below. This is the
    // core simplification from #1167: rotation still happens for
    // URL-leak mitigation of the bootstrap URL, but bound devices
    // never see the rotation because they don't ride on tokens.
    let login_enabled = state.login_manager.is_enabled();
    let presented_session_id = if login_enabled {
        super::login::extract_login_session(&request)
    } else {
        None
    };
    let presented_binding = if login_enabled {
        extract_device_binding(&request)
    } else {
        None
    };
    let session_valid = if login_enabled {
        match (&presented_session_id, &presented_binding) {
            (Some(id), Some(b)) => state.login_manager.validate_session(id, b).await,
            _ => false,
        }
    } else {
        false
    };

    if session_valid {
        let session_id = presented_session_id.expect("session_valid implies session_id exists");
        return handle_session_authenticated(&state, client_ip, request, next, session_id).await;
    }

    // Token-check path: either passphrase login is disabled (token
    // is the sole factor) or this device is bootstrapping its
    // first session. Try every token source; on success route
    // either through the login flow (login enabled, non-bootstrap
    // path) or straight to the handler (login-exempt path or
    // token-only mode).
    let mut matched_source = None;
    let mut needs_upgrade = false;
    let mut matched_token_hash: Option<[u8; 32]> = None;

    for (token_value, source) in extract_tokens(&request) {
        let (valid, upgrade) = state.token_manager.validate(token_value).await;
        if valid {
            matched_source = Some(source);
            needs_upgrade = upgrade;
            matched_token_hash = Some(super::push::sha256_token(token_value));
            break;
        }
    }

    // WebSocket sub-protocol fallback. A client may send multiple
    // protocols (e.g., "graphql-ws, <token>"), so check each.
    // Accept either bare-token (older PWA tabs) or the prefixed
    // `aoe-token.<token>` form. See #1131.
    if matched_source.is_none() {
        for proto in extract_ws_protocols(&request) {
            let candidate = strip_ws_prefix(&proto, "aoe-token").unwrap_or(&proto);
            let (valid, upgrade) = state.token_manager.validate(candidate).await;
            if valid {
                matched_source = Some(TokenSource::WebSocketProtocol);
                needs_upgrade = upgrade;
                matched_token_hash = Some(super::push::sha256_token(candidate));
                break;
            }
        }
    }

    let Some(source) = matched_source else {
        // No valid token and no valid session: true unauthenticated
        // state. For API and WebSocket routes return 401; for
        // anything else serve the SPA shell so the frontend can
        // render its own re-auth UI.
        let path = request.uri().path();
        let is_api_or_ws = path.starts_with("/api/") || path.contains("/ws");
        if !is_api_or_ws {
            return next.run(request).await;
        }
        let locked = state.rate_limiter.record_failure(client_ip).await;
        let reason =
            if extract_tokens(&request).is_empty() && extract_ws_protocols(&request).is_empty() {
                "missing"
            } else {
                "invalid"
            };
        tracing::warn!(
            target: "auth.middleware",
            ip = %client_ip,
            path = %path,
            locked = locked,
            reason = %reason,
            "auth rejected"
        );
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(serde_json::json!({
                "error": "unauthorized",
                "message": "Invalid or missing auth token"
            })),
        )
            .into_response();
    };

    // Token valid: record success, stamp owner, record device.
    state.rate_limiter.record_success(client_ip).await;
    tracing::trace!(
        target: "auth.middleware",
        ip = %client_ip,
        path = %request.uri().path(),
        source = ?source,
        "auth accepted via token (bootstrap)"
    );
    state.touch_web_activity();
    if let Some(hash) = matched_token_hash {
        request
            .extensions_mut()
            .insert(AuthenticatedTokenHash(hash));
    }
    let user_agent = request
        .headers()
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();
    record_device(&state, client_ip, user_agent.as_str()).await;

    let path = request.uri().path().to_string();
    let should_attach_token =
        matches!(source, TokenSource::QueryParam | TokenSource::Bearer) || needs_upgrade;

    // When login is enabled, a valid token alone is not enough for
    // non-bootstrap paths: the user still needs to complete the
    // passphrase flow to mint a session. Return login_required
    // (or redirect to /login for HTML) so the SPA pops the
    // passphrase prompt.
    if login_enabled && !is_login_session_exempt(&path) {
        tracing::warn!(
            target: "auth",
            ip = %client_ip,
            path = %path,
            had_session_cookie = presented_session_id.is_some(),
            had_device_binding = presented_binding.is_some(),
            "valid token but no session on non-login-exempt path; returning login_required"
        );
        if path.starts_with("/api/") || path.contains("/ws") {
            return (
                StatusCode::UNAUTHORIZED,
                axum::Json(serde_json::json!({
                    "error": "login_required",
                    "message": "Passphrase login required"
                })),
            )
                .into_response();
        } else {
            let mut response = axum::response::Redirect::temporary("/login").into_response();
            if should_attach_token {
                attach_token_headers(&mut response, &state).await;
            }
            return response;
        }
    }

    // Bootstrap path (login enabled + /login, /api/login, etc.) or
    // token-only mode. Pass through; attach token cookie for the
    // upcoming login POST when the token came via QueryParam /
    // Bearer / grace upgrade.
    let mut response = next.run(request).await;
    if should_attach_token {
        attach_token_headers(&mut response, &state).await;
    }
    response
}

/// Steady-state handler for a bound device. The session + binding
/// pair is the credential; the token is not consulted. Stamps the
/// owner identity (from the current token hash for push attribution),
/// records the device, enforces step-up elevation for sensitive
/// routes, and refreshes the session cookie's sliding window. NO
/// `aoe_token` cookie or `x-aoe-token` header is attached: bound
/// devices don't need the rotating token propagated to them.
async fn handle_session_authenticated(
    state: &Arc<AppState>,
    client_ip: IpAddr,
    mut request: Request,
    next: Next,
    session_id: String,
) -> Response {
    state.rate_limiter.record_success(client_ip).await;
    tracing::trace!(
        target: "auth.middleware",
        ip = %client_ip,
        path = %request.uri().path(),
        "auth accepted via session+binding"
    );
    state.touch_web_activity();

    let owner_hash = match state.token_manager.current_token().await {
        Some(t) => super::push::sha256_token(&t),
        None => [0u8; 32],
    };
    request
        .extensions_mut()
        .insert(AuthenticatedTokenHash(owner_hash));

    let user_agent = request
        .headers()
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown");
    record_device(state, client_ip, user_agent).await;

    let path = request.uri().path().to_string();
    let method = request.method().clone();

    if requires_elevation(&method, &path) && !state.login_manager.is_elevated(&session_id).await {
        tracing::info!(
            target: "auth.passphrase",
            ip = %client_ip,
            path = %path,
            "sensitive route required elevation; returning 403 elevation_required"
        );
        return (
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({
                "error": "elevation_required",
                "message": "Re-enter the passphrase to continue"
            })),
        )
            .into_response();
    }

    let mut response = next.run(request).await;

    if should_refresh_session_cookie(&path) {
        let login_cookie = super::login::build_login_cookie(&session_id, state.behind_tunnel);
        response.headers_mut().append(
            header::SET_COOKIE,
            login_cookie.parse().expect("cookie format must be valid"),
        );
    }

    response
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_time_eq_matching() {
        assert!(constant_time_eq("abc123", "abc123"));
        assert!(constant_time_eq("", ""));
    }

    #[test]
    fn constant_time_eq_different_content() {
        assert!(!constant_time_eq("abc123", "abc124"));
        assert!(!constant_time_eq("abc123", "xyz789"));
    }

    #[test]
    fn constant_time_eq_different_length() {
        assert!(!constant_time_eq("short", "longer_string"));
        assert!(!constant_time_eq("abc", "ab"));
    }

    #[test]
    fn constant_time_eq_empty_vs_nonempty() {
        assert!(!constant_time_eq("", "x"));
        assert!(!constant_time_eq("x", ""));
    }

    #[test]
    fn resolve_ip_prefers_cf_connecting_ip() {
        let socket: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("cf-connecting-ip", "203.0.113.50".parse().unwrap());
        headers.insert("x-forwarded-for", "10.0.0.1".parse().unwrap());
        let ip = resolve_client_ip(socket, &headers);
        assert_eq!(ip, "203.0.113.50".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn resolve_ip_falls_back_to_xff_last() {
        let socket: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            "spoofed.by.client, 203.0.113.50".parse().unwrap(),
        );
        let ip = resolve_client_ip(socket, &headers);
        assert_eq!(ip, "203.0.113.50".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn resolve_ip_loopback_without_xff() {
        let socket: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        let headers = axum::http::HeaderMap::new();
        let ip = resolve_client_ip(socket, &headers);
        assert!(ip.is_loopback());
    }

    #[test]
    fn resolve_ip_remote_ignores_xff() {
        let socket: SocketAddr = "192.168.1.100:12345".parse().unwrap();
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-forwarded-for", "10.0.0.1".parse().unwrap());
        let ip = resolve_client_ip(socket, &headers);
        assert_eq!(ip, "192.168.1.100".parse::<IpAddr>().unwrap());
    }

    #[test]
    fn resolve_ip_malformed_xff() {
        let socket: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-forwarded-for", "not-an-ip".parse().unwrap());
        let ip = resolve_client_ip(socket, &headers);
        assert!(ip.is_loopback());
    }

    #[test]
    fn build_cookie_without_secure() {
        let cookie = build_cookie("mytoken", false, 14400);
        assert!(cookie.contains("aoe_token=mytoken"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
        assert!(cookie.contains("Max-Age=14400"));
        assert!(!cookie.contains("Secure"));
    }

    fn build_request_with_headers(headers: Vec<(&'static str, &'static str)>) -> Request {
        let mut builder = Request::builder().uri("/api/sessions");
        for (name, value) in headers {
            builder = builder.header(name, value);
        }
        builder.body(axum::body::Body::empty()).unwrap()
    }

    #[test]
    fn extract_tokens_reads_bearer_header() {
        let req = build_request_with_headers(vec![("authorization", "Bearer abc123")]);
        let tokens = extract_tokens(&req);
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].0, "abc123");
        assert_eq!(tokens[0].1, TokenSource::Bearer);
    }

    #[test]
    fn extract_tokens_cookie_wins_over_bearer() {
        let req = build_request_with_headers(vec![
            ("cookie", "aoe_token=cookie_tok"),
            ("authorization", "Bearer bearer_tok"),
        ]);
        let tokens = extract_tokens(&req);
        // Priority order: cookie first, then Bearer. Both are attempted until
        // one validates, so order matters for skipping bad cookies.
        assert_eq!(tokens[0].0, "cookie_tok");
        assert_eq!(tokens[0].1, TokenSource::Cookie);
        assert_eq!(tokens[1].0, "bearer_tok");
        assert_eq!(tokens[1].1, TokenSource::Bearer);
    }

    #[test]
    fn extract_tokens_ignores_non_bearer_authorization() {
        let req = build_request_with_headers(vec![("authorization", "Basic dXNlcjpwYXNz")]);
        let tokens = extract_tokens(&req);
        assert!(tokens.is_empty());
    }

    #[test]
    fn extract_tokens_trims_bearer_value() {
        let req = build_request_with_headers(vec![("authorization", "Bearer   padded  ")]);
        let tokens = extract_tokens(&req);
        assert_eq!(tokens.len(), 1);
        assert_eq!(tokens[0].0, "padded");
    }

    #[test]
    fn build_cookie_with_secure() {
        let cookie = build_cookie("mytoken", true, 14400);
        assert!(cookie.contains("Secure"));
        assert!(cookie.contains("Max-Age=14400"));
    }

    #[test]
    fn strip_ws_prefix_works() {
        assert_eq!(strip_ws_prefix("aoe-token.abc", "aoe-token"), Some("abc"));
        assert_eq!(strip_ws_prefix("aoe-device.xyz", "aoe-device"), Some("xyz"));
        // No leading dot -> not a prefixed value, just a coincidentally
        // matching string. Don't strip.
        assert_eq!(strip_ws_prefix("aoe-tokenabc", "aoe-token"), None);
        // Unrelated subprotocol.
        assert_eq!(strip_ws_prefix("graphql-ws", "aoe-token"), None);
    }

    #[test]
    fn extract_device_binding_from_header() {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        let raw = [0xAB; 32];
        let encoded = URL_SAFE_NO_PAD.encode(raw);
        let req = build_request_with_headers(vec![(
            "x-aoe-device-binding",
            Box::leak(encoded.into_boxed_str()),
        )]);
        let bytes = extract_device_binding(&req).expect("decodes");
        assert_eq!(bytes, raw);
    }

    #[test]
    fn extract_device_binding_from_ws_subprotocol() {
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use base64::Engine;
        let raw = [0xCD; 32];
        let encoded = URL_SAFE_NO_PAD.encode(raw);
        let proto = format!("aoe-token.tok123, aoe-device.{}", encoded);
        let req = build_request_with_headers(vec![(
            "sec-websocket-protocol",
            Box::leak(proto.into_boxed_str()),
        )]);
        let bytes = extract_device_binding(&req).expect("decodes");
        assert_eq!(bytes, raw);
    }

    #[test]
    fn extract_device_binding_missing_returns_none() {
        let req = build_request_with_headers(vec![]);
        assert!(extract_device_binding(&req).is_none());
    }

    #[test]
    fn extract_device_binding_rejects_malformed() {
        let req = build_request_with_headers(vec![(
            "x-aoe-device-binding",
            "not-base64-and-wrong-length",
        )]);
        assert!(extract_device_binding(&req).is_none());
    }

    // Regression test for the token-cookie clobber bug:
    // `write_token_headers` must `.append` the `Set-Cookie` for
    // `aoe_token`, never `.insert`. A handler that already set
    // `aoe_session` on its response (login_handler on success,
    // logout_handler on clear) must keep its cookie when the
    // middleware later adds the `aoe_token` cookie. A `.insert`
    // would replace the handler's `Set-Cookie` (HeaderMap::insert
    // semantics: remove all prior values, set the new one), so
    // browsers would receive only `aoe_token=...` and the
    // session-cookie write would be silently dropped. This test
    // pins the two-cookies-in-the-response invariant.
    #[test]
    fn write_token_headers_preserves_prior_set_cookie() {
        use axum::http::{HeaderMap, HeaderValue};

        let mut headers = HeaderMap::new();
        // Simulate a handler that already set its own Set-Cookie
        // (e.g., `login_handler` setting `aoe_session=...`).
        headers.insert(
            header::SET_COOKIE,
            HeaderValue::from_static(
                "aoe_session=abc; HttpOnly; SameSite=Strict; Path=/; Max-Age=2592000",
            ),
        );

        // Middleware writes the `aoe_token` cookie + companion
        // header on top.
        write_token_headers(&mut headers, "tok123", false, 14400);

        let cookies: Vec<String> = headers
            .get_all(header::SET_COOKIE)
            .iter()
            .filter_map(|v| v.to_str().ok())
            .map(str::to_string)
            .collect();
        assert_eq!(
            cookies.len(),
            2,
            "both Set-Cookie values must survive, got {cookies:?}"
        );
        assert!(
            cookies.iter().any(|c| c.contains("aoe_session=abc")),
            "handler's aoe_session cookie was clobbered: {cookies:?}"
        );
        assert!(
            cookies.iter().any(|c| c.contains("aoe_token=tok123")),
            "middleware's aoe_token cookie missing: {cookies:?}"
        );
        assert_eq!(
            headers.get("x-aoe-token").and_then(|v| v.to_str().ok()),
            Some("tok123")
        );
    }

    // Regression test for the logout-clobber bug: when a request
    // hits a login-exempt path (notably `POST /api/logout` and
    // `POST /api/login`), `handle_session_authenticated` must NOT
    // append a sliding-window `Set-Cookie: aoe_session=<id>` after
    // the handler runs. The handler is the one that owns the cookie
    // on those responses (logout clears with `Max-Age=0`; login
    // mints a fresh id), and an unconditional refresh would emit a
    // second Set-Cookie that browsers process later-wins, so logout
    // would leave a stale 30-day cookie pointing at an invalidated
    // session and login would clobber the new id with the old one.
    #[test]
    fn session_cookie_refresh_skips_login_exempt_paths() {
        // Login-exempt: must not refresh.
        assert!(!should_refresh_session_cookie("/api/logout"));
        assert!(!should_refresh_session_cookie("/api/login"));
        assert!(!should_refresh_session_cookie("/login"));
        assert!(!should_refresh_session_cookie("/api/login/status"));
        assert!(!should_refresh_session_cookie("/assets/index.js"));
        assert!(!should_refresh_session_cookie("/manifest.json"));
        assert!(!should_refresh_session_cookie("/sw.js"));

        // Non-exempt: must refresh (sliding window).
        assert!(should_refresh_session_cookie("/"));
        assert!(should_refresh_session_cookie("/api/sessions"));
        assert!(should_refresh_session_cookie(
            "/api/sessions/abc/cockpit/ws"
        ));
        assert!(should_refresh_session_cookie("/api/settings"));
        // /api/login/elevate is gated by the session check (not
        // exempt), so its response should slide the window.
        assert!(should_refresh_session_cookie("/api/login/elevate"));
    }

    #[test]
    fn login_session_exempt_paths() {
        // Bootstrap + status endpoints: the user might hit these
        // before a session exists (or after it expired) and the
        // middleware must let them through so the SPA can recover.
        assert!(is_login_session_exempt("/login"));
        assert!(is_login_session_exempt("/api/login"));
        assert!(is_login_session_exempt("/api/login/status"));
        assert!(is_login_session_exempt("/api/logout"));

        // Static assets: pre-load the SPA shell before any auth.
        assert!(is_login_session_exempt("/assets/index.css"));
        assert!(is_login_session_exempt("/assets/index-abc123.js"));
        assert!(is_login_session_exempt("/manifest.json"));
        assert!(is_login_session_exempt("/sw.js"));
        assert!(is_login_session_exempt("/icon-192.png"));
        assert!(is_login_session_exempt("/fonts/inter.woff2"));

        // Everything else stays gated.
        assert!(!is_login_session_exempt("/"));
        assert!(!is_login_session_exempt("/api/sessions"));
        assert!(!is_login_session_exempt("/api/login/elevate"));
        assert!(!is_login_session_exempt("/api/settings"));
        assert!(!is_login_session_exempt("/api/sessions/abc/ws"));
        // /api/login/foo is not the same as /api/login exactly.
        assert!(!is_login_session_exempt("/api/login/foo"));
        // /logins is not /login.
        assert!(!is_login_session_exempt("/logins"));
    }

    // Pin the session lifetime to 30 days. Catches a silent
    // regression to the old 24h window: that broke the
    // "rarely-log-out" UX the device-bound design promises (see
    // #1167). The test asserts both the server-side constant and
    // the cookie's advertised Max-Age stay in sync, since a
    // mismatch between the two creates a confusing client/server
    // disagreement (browser thinks the cookie is fresh, server
    // 401s it).
    #[tokio::test]
    async fn login_session_lifetime_is_thirty_days_sliding() {
        use crate::server::login::{LoginManager, SESSION_LIFETIME};
        use std::time::Duration;

        // Direct pin on the server-side TTL. Any future edit that
        // shortens this without updating the test (and the docs in
        // `docs/guides/remote-phone-access.md`) will fail here.
        assert_eq!(
            SESSION_LIFETIME,
            Duration::from_secs(30 * 24 * 60 * 60),
            "SESSION_LIFETIME must be 30 days; see #1167"
        );

        // Cookie's advertised Max-Age must equal the server TTL so
        // the browser and server agree on when the session is gone.
        let mgr = LoginManager::new(Some("test"));
        let binding = vec![0xAB; 32];
        let session_id = mgr.create_session(&binding).await;
        assert!(mgr.validate_session(&session_id, &binding).await);

        let cookie = super::super::login::build_login_cookie(&session_id, false);
        let expected = format!("Max-Age={}", SESSION_LIFETIME.as_secs());
        assert!(
            cookie.contains(&expected),
            "cookie {cookie:?} must advertise {expected} to match server TTL"
        );
    }

    #[test]
    fn requires_elevation_paths() {
        use axum::http::Method;
        // Sensitive: settings + profile writes. These persist the
        // Docker image, env, volume mounts, and worktree templates
        // that the owner's next session spawn will use, so an
        // attacker with stolen session + binding could plant a
        // malicious config and wait for the owner to spawn. See
        // #1137 (settings-only step-up scope).
        assert!(requires_elevation(&Method::PATCH, "/api/settings"));
        assert!(requires_elevation(&Method::PATCH, "/api/default-profile"));
        assert!(requires_elevation(&Method::POST, "/api/profiles"));
        assert!(requires_elevation(
            &Method::PATCH,
            "/api/profiles/work/settings"
        ));
        assert!(requires_elevation(
            &Method::PATCH,
            "/api/profiles/work/rename"
        ));
        assert!(requires_elevation(&Method::DELETE, "/api/profiles/work"));
        // Trailing slash must not bypass the gate.
        assert!(requires_elevation(&Method::PATCH, "/api/settings/"));
        assert!(requires_elevation(
            &Method::PATCH,
            "/api/profiles/work/settings/"
        ));

        // NOT gated: daily-use surfaces. Device binding + session
        // cookie alone authenticate these. See #1137.
        assert!(!requires_elevation(&Method::GET, "/api/sessions/abc/ws"));
        assert!(!requires_elevation(
            &Method::GET,
            "/api/sessions/abc/ws-readonly"
        ));
        assert!(!requires_elevation(
            &Method::GET,
            "/sessions/abc/cockpit/ws"
        ));
        assert!(!requires_elevation(
            &Method::POST,
            "/api/sessions/abc/cockpit/prompt"
        ));
        assert!(!requires_elevation(
            &Method::POST,
            "/api/sessions/abc/cockpit/cancel"
        ));
        assert!(!requires_elevation(
            &Method::POST,
            "/api/sessions/abc/cockpit/approvals/nonce1"
        ));
        assert!(!requires_elevation(&Method::POST, "/api/sessions"));
        assert!(!requires_elevation(&Method::DELETE, "/api/sessions/abc"));
        assert!(!requires_elevation(&Method::POST, "/api/sessions/abc/send"));
        assert!(!requires_elevation(&Method::PATCH, "/api/sessions/abc"));
        assert!(!requires_elevation(
            &Method::POST,
            "/api/sessions/abc/ensure"
        ));
        assert!(!requires_elevation(
            &Method::PATCH,
            "/api/sessions/abc/notifications"
        ));
        assert!(!requires_elevation(&Method::POST, "/api/git/clone"));
        assert!(!requires_elevation(&Method::POST, "/api/projects"));
        assert!(!requires_elevation(&Method::DELETE, "/api/projects/myproj"));
        assert!(!requires_elevation(&Method::POST, "/api/push/subscribe"));
        assert!(!requires_elevation(&Method::POST, "/api/push/unsubscribe"));

        // Read-only GETs are NOT gated even on settings/profile paths.
        assert!(!requires_elevation(&Method::GET, "/api/settings"));
        assert!(!requires_elevation(&Method::GET, "/api/profiles"));
        assert!(!requires_elevation(
            &Method::GET,
            "/api/profiles/work/settings"
        ));
        assert!(!requires_elevation(&Method::GET, "/api/sessions"));
        assert!(!requires_elevation(&Method::GET, "/api/sessions/abc"));

        // Out-of-scope paths never gate.
        assert!(!requires_elevation(&Method::GET, "/api/about"));
        assert!(!requires_elevation(&Method::POST, "/api/login"));
        assert!(!requires_elevation(&Method::POST, "/api/login/elevate"));
    }
}
