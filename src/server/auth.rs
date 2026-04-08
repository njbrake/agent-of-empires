//! Token-based authentication middleware for the web dashboard.
//!
//! Accepts the auth token via:
//! - Cookie: `aoe_token=<token>`
//! - Query parameter: `?token=<token>` (sets the cookie for future requests)
//! - WebSocket protocol header: `Sec-WebSocket-Protocol: <token>`

use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::{header, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use super::AppState;

/// Constant-time string comparison to prevent timing attacks on token values.
fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.bytes()
        .zip(b.bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

pub async fn auth_middleware(
    State(state): State<Arc<AppState>>,
    request: Request,
    next: Next,
) -> Response {
    // No-auth mode: pass everything through
    let expected_token = match &state.auth_token {
        Some(t) => t,
        None => return next.run(request).await,
    };

    // Check cookie
    if let Some(cookie_header) = request.headers().get(header::COOKIE) {
        if let Ok(cookie_str) = cookie_header.to_str() {
            for cookie in cookie_str.split(';') {
                let cookie = cookie.trim();
                if let Some(value) = cookie.strip_prefix("aoe_token=") {
                    if constant_time_eq(value, expected_token) {
                        return next.run(request).await;
                    }
                }
            }
        }
    }

    // Check query parameter
    if let Some(query) = request.uri().query() {
        for param in query.split('&') {
            if let Some(value) = param.strip_prefix("token=") {
                if constant_time_eq(value, expected_token) {
                    // Set the cookie so future requests don't need the query param
                    let mut response = next.run(request).await;
                    let cookie = format!(
                        "aoe_token={}; HttpOnly; SameSite=Strict; Path=/",
                        expected_token
                    );
                    response.headers_mut().insert(
                        header::SET_COOKIE,
                        cookie
                            .parse()
                            .expect("hardcoded cookie format must be valid"),
                    );
                    return response;
                }
            }
        }
    }

    // Check WebSocket protocol header
    if let Some(protocols) = request.headers().get("sec-websocket-protocol") {
        if let Ok(proto_str) = protocols.to_str() {
            for proto in proto_str.split(',') {
                if constant_time_eq(proto.trim(), expected_token) {
                    return next.run(request).await;
                }
            }
        }
    }

    // Unauthorized
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
}
