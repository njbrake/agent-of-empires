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

#[derive(Debug, PartialEq)]
enum TokenSource {
    Cookie,
    QueryParam,
    WebSocketProtocol,
    Bearer,
}

pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    request: Request,
    next: Next,
) -> Response {
    let client_ip = resolve_client_ip(addr, request.headers());

    // No-auth mode: pass everything through
    if state.token_manager.is_no_auth().await {
        return next.run(request).await;
    }

    // Rate limit check BEFORE token validation
    if let Some(remaining_secs) = state.rate_limiter.check_locked(client_ip).await {
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

    for (token_value, source) in extract_tokens(&request) {
        let (valid, upgrade) = state.token_manager.validate(token_value).await;
        if valid {
            matched_source = Some(source);
            needs_upgrade = upgrade;
            break;
        }
    }

    // If cookie/query didn't match, try each WebSocket sub-protocol.
    // A client may send multiple protocols (e.g., "graphql-ws, <token>"),
    // so we must check each one, not just the first.
    if matched_source.is_none() {
        for proto in extract_ws_protocols(&request) {
            let (valid, upgrade) = state.token_manager.validate(&proto).await;
            if valid {
                matched_source = Some(TokenSource::WebSocketProtocol);
                needs_upgrade = upgrade;
                break;
            }
        }
    }

    if let Some(source) = matched_source {
        // Record success
        state.rate_limiter.record_success(client_ip).await;

        let user_agent = request
            .headers()
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("unknown");
        record_device(&state, client_ip, user_agent).await;

        // Login session check (second factor)
        if state.login_manager.is_enabled() {
            let path = request.uri().path();

            // Allow login-related paths and static assets through without a session
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
                let has_valid_session = match session_id {
                    Some(ref id) => state.login_manager.validate_session(id, client_ip).await,
                    None => false,
                };

                if !has_valid_session {
                    // For API routes, return JSON 401. For HTML routes, redirect.
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

                // Session is valid. Refresh the sliding window cookie.
                let session_id = session_id.expect("valid session implies session_id exists");
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
    tracing::warn!(
        ip = %client_ip,
        path = %path,
        locked = locked,
        "Authentication failed"
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
}
