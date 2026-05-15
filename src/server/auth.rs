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
    let cookie = build_cookie(&current, state.behind_tunnel, max_age);
    response.headers_mut().insert(
        header::SET_COOKIE,
        cookie.parse().expect("cookie format must be valid"),
    );
    if let Ok(value) = current.parse() {
        response.headers_mut().insert("x-aoe-token", value);
    }
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

/// Whether a request path + method needs an elevated login session
/// (step-up auth, 15-minute passphrase confirmation window). Covers
/// surfaces that map to SSH-equivalent capabilities or that could
/// redirect notifications / write to disk:
///
/// - Terminal attach + cockpit live WS upgrades.
/// - Session lifecycle (create, delete, rename, send keystrokes,
///   spawn terminal/cockpit, set notifications).
/// - Cockpit mutating endpoints (prompt, cancel, force end turn,
///   approval resolution, spawn/restart/stop/kill, mode, enable,
///   disable).
/// - Filesystem-mutating endpoints outside the session tree
///   (`/api/git/clone`, project create/delete).
/// - Push subscription mutation (an attacker could redirect
///   notifications to their own endpoint).
///
/// Read-only `GET`/`HEAD` on the same resources stay open; this is
/// an allow-list, not a default-deny, so adding a benign read
/// endpoint never accidentally hides behind a passphrase prompt.
/// When adding a new mutating REST surface in the future, add it
/// here AND add a corresponding test in `requires_elevation_paths`.
/// See #1131.
fn requires_elevation(method: &axum::http::Method, path: &str) -> bool {
    use axum::http::Method;

    let path = normalize_path(path);

    // WebSocket upgrades grant live shell / live cockpit streaming.
    // Match specific suffixes rather than `.contains("/ws")` so a
    // future path like `/api/debug/ws-status` does not get gated by
    // accident.
    if path.ends_with("/ws") || path.ends_with("/ws-readonly") || path.ends_with("/cockpit/ws") {
        return true;
    }

    if method == Method::GET || method == Method::HEAD {
        return false;
    }

    // Session lifecycle on the collection endpoint.
    if path == "/api/sessions" && method == Method::POST {
        return true;
    }

    // Per-session mutations. Includes DELETE /api/sessions/{id} and
    // DELETE /api/sessions/{id}/cockpit (cockpit shutdown).
    if path.starts_with("/api/sessions/") {
        if method == Method::DELETE {
            return true;
        }
        let cockpit_prompt = path.ends_with("/cockpit/prompt");
        let cockpit_cancel = path.ends_with("/cockpit/cancel");
        let cockpit_force = path.ends_with("/cockpit/force_end_turn");
        let cockpit_approval = path.contains("/cockpit/approvals/");
        let cockpit_spawn = path.ends_with("/cockpit/spawn");
        let cockpit_restart = path.ends_with("/cockpit/restart");
        let cockpit_stop = path.ends_with("/cockpit/stop");
        let cockpit_kill = path.ends_with("/cockpit/kill");
        let cockpit_mode = path.ends_with("/cockpit/mode");
        let cockpit_enable = path.ends_with("/cockpit/enable");
        let cockpit_disable = path.ends_with("/cockpit/disable");
        let session_send = path.ends_with("/send");
        let session_terminal = path.ends_with("/terminal");
        let container_terminal = path.ends_with("/container-terminal");
        let session_rename_or_patch = method == Method::PATCH;
        let session_ensure = path.ends_with("/ensure") && method == Method::POST;
        let session_notifications = path.ends_with("/notifications") && method == Method::PATCH;

        if cockpit_prompt
            || cockpit_cancel
            || cockpit_force
            || cockpit_approval
            || cockpit_spawn
            || cockpit_restart
            || cockpit_stop
            || cockpit_kill
            || cockpit_mode
            || cockpit_enable
            || cockpit_disable
            || session_send
            || session_terminal
            || container_terminal
            || session_rename_or_patch
            || session_ensure
            || session_notifications
        {
            return true;
        }
        return false;
    }

    // Filesystem mutations outside `/api/sessions/`. `git/clone`
    // writes a fresh worktree to disk; project create/delete touch
    // the on-disk project registry. Push subscribe / unsubscribe is
    // gated because an attacker could redirect approval / new-login
    // notifications away from the legitimate owner.
    if path == "/api/git/clone" && method == Method::POST {
        return true;
    }
    if path == "/api/projects" && method == Method::POST {
        return true;
    }
    if path.starts_with("/api/projects/") && method == Method::DELETE {
        return true;
    }
    if path == "/api/push/subscribe" && method == Method::POST {
        return true;
    }
    if path == "/api/push/unsubscribe" && method == Method::POST {
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

    // Try each token source in order: cookie, then query param.
    // A stale cookie must not block a valid query param token.
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

    // If cookie/query didn't match, try each WebSocket sub-protocol.
    // A client may send multiple protocols (e.g., "graphql-ws, <token>"),
    // so we must check each one, not just the first. Accept either the
    // legacy bare-token form (used by older PWA tabs) or the prefixed
    // `aoe-token.<token>` form introduced alongside device binding so
    // that subprotocol values are self-describing instead of relying on
    // "try every entry as a token" coincidence. See #1131.
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

    if let Some(source) = matched_source {
        // Record success
        state.rate_limiter.record_success(client_ip).await;
        tracing::trace!(
            target: "auth.middleware",
            ip = %client_ip,
            path = %request.uri().path(),
            source = ?source,
            "auth accepted"
        );

        // Stamp web activity so the push consumer can suppress
        // notifications when the dashboard is actively in use.
        state.touch_web_activity();

        // Propagate the matched token's SHA-256 hash as a request extension
        // so downstream handlers (especially /api/push/*) can filter and
        // attribute subscriptions by owner without re-extracting the token.
        if let Some(hash) = matched_token_hash {
            request
                .extensions_mut()
                .insert(AuthenticatedTokenHash(hash));
        }

        let user_agent = request
            .headers()
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown");
        record_device(&state, client_ip, user_agent).await;

        // Login session check (second factor). Validates both the
        // session cookie and the per-device binding secret introduced
        // in #1131. The IP is no longer consulted, mobile rotation is
        // a normal pattern. Sensitive routes additionally require a
        // step-up elevation timestamp (15-minute window) set via
        // `POST /api/login/elevate`.
        if state.login_manager.is_enabled() {
            let path = request.uri().path();

            // Allow login-related paths and static assets through without a session.
            // `/api/login/elevate` IS exempt from the elevation requirement (it
            // is the path that grants elevation) but NOT from the session +
            // binding requirement (the elevate handler enforces both internally
            // via the same middleware path).
            let is_login_exempt = path == "/login"
                || path == "/api/login"
                || path == "/api/login/status"
                || path == "/api/logout"
                || path.starts_with("/assets/")
                || path == "/manifest.json"
                || path == "/sw.js"
                || path.starts_with("/icon-")
                || path.starts_with("/fonts/");

            if !is_login_exempt {
                let session_id = super::login::extract_login_session(&request);
                let presented_binding = extract_device_binding(&request);

                let has_valid_session = match (&session_id, &presented_binding) {
                    (Some(id), Some(binding)) => {
                        state.login_manager.validate_session(id, binding).await
                    }
                    _ => false,
                };

                if !has_valid_session {
                    // For API routes, return JSON 401. For HTML routes, redirect.
                    if path.starts_with("/api/") || path.contains("/ws") {
                        tracing::warn!(
                            target: "auth",
                            ip = %client_ip,
                            path = %path,
                            had_session_cookie = session_id.is_some(),
                            had_device_binding = presented_binding.is_some(),
                            "login session check failed; rejecting api/ws with 401"
                        );
                        return (
                            StatusCode::UNAUTHORIZED,
                            axum::Json(serde_json::json!({
                                "error": "login_required",
                                "message": "Passphrase login required"
                            })),
                        )
                            .into_response();
                    } else {
                        let mut response =
                            axum::response::Redirect::temporary("/login").into_response();

                        // Set token cookie/header on the redirect so the browser
                        // has the current token when it follows the redirect.
                        if source == TokenSource::QueryParam
                            || source == TokenSource::Bearer
                            || needs_upgrade
                        {
                            attach_token_headers(&mut response, &state).await;
                        }

                        return response;
                    }
                }

                // Session + binding are valid.
                let session_id = session_id.expect("valid session implies session_id exists");

                // Step-up gate for sensitive routes (terminal attach, cockpit
                // command execution, file writes). Returning 403 with a typed
                // body lets the frontend pop the inline passphrase prompt
                // (POST /api/login/elevate) and retry. See #1131.
                if requires_elevation(request.method(), path)
                    && !state.login_manager.is_elevated(&session_id).await
                {
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

                // Set token cookie/header if needed (including Bearer to
                // rehydrate cookie from localStorage)
                if source == TokenSource::QueryParam
                    || source == TokenSource::Bearer
                    || needs_upgrade
                {
                    attach_token_headers(&mut response, &state).await;
                }

                // Refresh login session cookie (sliding window)
                let login_cookie =
                    super::login::build_login_cookie(&session_id, state.behind_tunnel);
                response.headers_mut().append(
                    header::SET_COOKIE,
                    login_cookie.parse().expect("cookie format must be valid"),
                );

                return response;
            }
        }

        let mut response = next.run(request).await;

        // Refresh the auth cookie when the token was provided via query param,
        // Bearer header, or when the token needs upgrade (grace period).
        // Including Bearer here is important: when the cookie expires but the
        // SPA still has the token in localStorage, API calls authenticate via
        // Bearer header. Setting the cookie on those responses "rehydrates" it
        // so the next browser navigation (HTML page load) works without
        // re-pasting the ?token= URL.
        let should_refresh =
            source == TokenSource::QueryParam || source == TokenSource::Bearer || needs_upgrade;

        if should_refresh {
            attach_token_headers(&mut response, &state).await;
        }

        return response;
    }

    // Auth failed. For API and WebSocket routes, return 401. For everything
    // else (the SPA shell, static assets), serve the page anyway so the
    // frontend can attempt auth via localStorage + Bearer header. Without
    // this, an expired cookie means the SPA never loads and localStorage
    // never gets a chance to re-authenticate.
    let path = request.uri().path();
    let is_api_or_ws = path.starts_with("/api/") || path.contains("/ws");

    if !is_api_or_ws {
        return next.run(request).await;
    }

    let locked = state.rate_limiter.record_failure(client_ip).await;
    let reason = if extract_tokens(&request).is_empty() && extract_ws_protocols(&request).is_empty()
    {
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

    (
        StatusCode::UNAUTHORIZED,
        axum::Json(serde_json::json!({
            "error": "unauthorized",
            "message": "Invalid or missing auth token"
        })),
    )
        .into_response()
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

    #[test]
    fn requires_elevation_paths() {
        use axum::http::Method;
        // Sensitive: terminal WS attach.
        assert!(requires_elevation(&Method::GET, "/api/sessions/abc/ws"));
        assert!(requires_elevation(
            &Method::GET,
            "/api/sessions/abc/ws-readonly"
        ));
        // Sensitive: cockpit WS upgrade.
        assert!(requires_elevation(&Method::GET, "/sessions/abc/cockpit/ws"));
        // Sensitive: cockpit mutating endpoints.
        assert!(requires_elevation(
            &Method::POST,
            "/api/sessions/abc/cockpit/prompt"
        ));
        assert!(requires_elevation(
            &Method::POST,
            "/api/sessions/abc/cockpit/cancel"
        ));
        assert!(requires_elevation(
            &Method::POST,
            "/api/sessions/abc/cockpit/approvals/nonce1"
        ));
        // Sensitive: session lifecycle.
        assert!(requires_elevation(&Method::POST, "/api/sessions"));
        assert!(requires_elevation(&Method::DELETE, "/api/sessions/abc"));
        assert!(requires_elevation(&Method::POST, "/api/sessions/abc/send"));
        assert!(requires_elevation(&Method::PATCH, "/api/sessions/abc"));
        assert!(requires_elevation(
            &Method::POST,
            "/api/sessions/abc/ensure"
        ));
        assert!(requires_elevation(
            &Method::PATCH,
            "/api/sessions/abc/notifications"
        ));
        // Sensitive: filesystem / project mutations outside sessions.
        assert!(requires_elevation(&Method::POST, "/api/git/clone"));
        assert!(requires_elevation(&Method::POST, "/api/projects"));
        assert!(requires_elevation(&Method::DELETE, "/api/projects/myproj"));
        // Sensitive: push subscription mutations.
        assert!(requires_elevation(&Method::POST, "/api/push/subscribe"));
        assert!(requires_elevation(&Method::POST, "/api/push/unsubscribe"));
        // Trailing slash must not bypass the gate.
        assert!(requires_elevation(
            &Method::POST,
            "/api/sessions/abc/cockpit/prompt/"
        ));
        // Read-only GETs are NOT gated even on per-session paths.
        assert!(!requires_elevation(&Method::GET, "/api/sessions"));
        assert!(!requires_elevation(&Method::GET, "/api/sessions/abc"));
        assert!(!requires_elevation(
            &Method::GET,
            "/api/sessions/abc/cockpit/replay"
        ));
        assert!(!requires_elevation(
            &Method::GET,
            "/api/sessions/abc/diff/files"
        ));
        assert!(!requires_elevation(&Method::GET, "/api/push/status"));
        // Coincidental substring must not match the WS suffix.
        assert!(!requires_elevation(&Method::GET, "/api/debug/ws-status"));
        // Out-of-scope paths never gate.
        assert!(!requires_elevation(&Method::GET, "/api/about"));
        assert!(!requires_elevation(&Method::POST, "/api/login"));
        assert!(!requires_elevation(&Method::POST, "/api/login/elevate"));
        // Settings / profiles / themes were intentionally left ungated
        // for v1 (their writes are local config, not host-affecting).
        // Document the choice in tests so a future maintainer thinks
        // twice before adding sensitive operations there.
        assert!(!requires_elevation(&Method::PATCH, "/api/settings"));
    }
}
