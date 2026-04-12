//! Token-based authentication middleware for the web dashboard.
//!
//! Accepts the auth token via:
//! - Cookie: `aoe_token=<token>`
//! - Query parameter: `?token=<token>` (sets the cookie for future requests)
//! - WebSocket protocol header: `Sec-WebSocket-Protocol: <token>`
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
fn resolve_client_ip(socket_addr: SocketAddr, headers: &axum::http::HeaderMap) -> IpAddr {
    let socket_ip = socket_addr.ip();
    if socket_ip.is_loopback() {
        if let Some(xff) = headers.get("x-forwarded-for") {
            if let Ok(xff_str) = xff.to_str() {
                if let Some(first) = xff_str.split(',').next() {
                    if let Ok(ip) = first.trim().parse::<IpAddr>() {
                        return ip;
                    }
                }
            }
        }
    }
    socket_ip
}

/// Build a Set-Cookie header value with optional Secure flag for HTTPS tunnels.
fn build_cookie(token: &str, secure: bool) -> String {
    let mut cookie = format!("aoe_token={}; HttpOnly; SameSite=Strict; Path=/", token);
    if secure {
        cookie.push_str("; Secure");
    }
    cookie
}

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
        devices.push(super::DeviceInfo {
            ip: ip_str,
            user_agent: ua,
            first_seen: chrono::Utc::now(),
            last_seen: chrono::Utc::now(),
            request_count: 1,
        });
    }
}

/// Extract a token from the request (cookie, query param, or WS protocol header).
/// Returns `Some((token_value, source))` or `None`.
fn extract_token(request: &Request) -> Option<(&str, TokenSource)> {
    // Check cookie
    if let Some(cookie_header) = request.headers().get(header::COOKIE) {
        if let Ok(cookie_str) = cookie_header.to_str() {
            for cookie in cookie_str.split(';') {
                let cookie = cookie.trim();
                if let Some(value) = cookie.strip_prefix("aoe_token=") {
                    return Some((value, TokenSource::Cookie));
                }
            }
        }
    }

    // Check query parameter
    if let Some(query) = request.uri().query() {
        for param in query.split('&') {
            if let Some(value) = param.strip_prefix("token=") {
                return Some((value, TokenSource::QueryParam));
            }
        }
    }

    // Check WebSocket protocol header
    if let Some(protocols) = request.headers().get("sec-websocket-protocol") {
        if let Ok(proto_str) = protocols.to_str() {
            for proto in proto_str.split(',') {
                let trimmed = proto.trim();
                if !trimmed.is_empty() {
                    return Some((trimmed, TokenSource::WebSocketProtocol));
                }
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

    // Extract and validate token
    if let Some((token_value, source)) = extract_token(&request) {
        let (valid, needs_upgrade) = state.token_manager.validate(token_value).await;

        if valid {
            // Record success
            state.rate_limiter.record_success(client_ip).await;

            let user_agent = request
                .headers()
                .get(header::USER_AGENT)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("unknown");
            record_device(&state, client_ip, user_agent).await;

            let mut response = next.run(request).await;

            // Set cookie if authenticated via query param or if token needs upgrade
            let should_set_cookie = source == TokenSource::QueryParam || needs_upgrade;

            if should_set_cookie {
                if let Some(current) = state.token_manager.current_token().await {
                    let cookie = build_cookie(&current, state.behind_tunnel);
                    response.headers_mut().insert(
                        header::SET_COOKIE,
                        cookie.parse().expect("cookie format must be valid"),
                    );
                }
            }

            return response;
        }
    }

    // Auth failed: record failure
    state.rate_limiter.record_failure(client_ip).await;

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
    fn resolve_ip_loopback_with_xff() {
        let socket: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("x-forwarded-for", "203.0.113.50".parse().unwrap());
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
        let cookie = build_cookie("mytoken", false);
        assert!(cookie.contains("aoe_token=mytoken"));
        assert!(cookie.contains("HttpOnly"));
        assert!(cookie.contains("SameSite=Strict"));
        assert!(!cookie.contains("Secure"));
    }

    #[test]
    fn build_cookie_with_secure() {
        let cookie = build_cookie("mytoken", true);
        assert!(cookie.contains("Secure"));
    }
}
