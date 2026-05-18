//! Browser-side log relay.
//!
//! POST /api/client-log accepts a batch of structured entries and
//! re-emits them through `tracing`. The entry's `target` is whitelisted
//! to one of the known `web.client.*` sub-targets so the backend filter
//! can dial each frontend area independently. Unknown targets fall back
//! to the `web.client` root.
//!
//! GET /api/client-log/policy returns the derived frontend logging policy
//! computed from the current FilterController directive. The frontend
//! uses this to drop trace-level events client-side rather than saturating
//! the rate-limited relay.
//!
//! Caps and truncation are enforced server-side because the frontend
//! throttle is best-effort: a broken or malicious client can POST
//! directly. We also reject the batch outright if it's too large.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::{Deserialize, Serialize};

use super::AppState;

const MAX_ENTRIES: usize = 50;
const MAX_MESSAGE: usize = 4096;
const MAX_STACK: usize = 16384;
const MAX_PATH: usize = 512;
const MAX_USER_AGENT: usize = 512;

/// Closed set of frontend sub-targets we route to. Anything else is
/// rewritten to `web.client` (forge protection: a hostile client cannot
/// emit events under `auth.*`, `cockpit.*`, etc.).
const ALLOWED_TARGETS: &[&str] = &[
    "web.client",
    "web.client.error",
    "web.client.api",
    "web.client.nav",
    "web.client.input",
    "web.client.settings",
    "web.client.ws",
    "web.client.terminal",
    "web.client.cockpit",
    "web.client.pwa",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientLogEntry {
    pub level: String,
    pub message: String,
    pub stack: Option<String>,
    #[serde(rename = "componentStack")]
    pub component_stack: Option<String>,
    pub target: Option<String>,
    #[serde(rename = "sessionId")]
    pub session_id: Option<String>,
    #[serde(rename = "requestId")]
    pub request_id: Option<String>,
    pub rid: Option<String>,
    pub fields: Option<serde_json::Value>,
    pub path: String,
    #[serde(rename = "userAgent")]
    pub user_agent: String,
    pub ts: i64,
    pub dropped: Option<u64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientLogBatch {
    pub entries: Vec<ClientLogEntry>,
}

#[tracing::instrument(target = "http.api.client_log", skip_all, fields(entries = batch.entries.len()))]
pub async fn post_client_log(
    State(_state): State<Arc<AppState>>,
    Json(batch): Json<ClientLogBatch>,
) -> Result<StatusCode, (StatusCode, String)> {
    if batch.entries.len() > MAX_ENTRIES {
        tracing::warn!(
            target: "http.api.client_log",
            entries = batch.entries.len(),
            limit = MAX_ENTRIES,
            "rejected oversized batch"
        );
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("max {MAX_ENTRIES} entries per batch"),
        ));
    }
    for entry in batch.entries {
        emit_event(entry);
    }
    Ok(StatusCode::NO_CONTENT)
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let mut end = max;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        let mut out = s[..end].to_string();
        out.push('…');
        out
    }
}

/// Coerce a client-supplied target string to one of `ALLOWED_TARGETS`. The
/// returned `&'static str` lets us pass it to `tracing::event!`, whose
/// `target:` argument must be a string literal at compile time.
fn allowed_target(s: Option<&str>) -> &'static str {
    let raw = s.unwrap_or("web.client");
    for t in ALLOWED_TARGETS {
        if raw == *t {
            return t;
        }
    }
    "web.client"
}

/// Render the `fields` JSON value to a small flat string for inclusion in
/// the tracing event. Truncated for safety; complex nested structures
/// are JSON-stringified rather than expanded into individual fields.
fn fields_summary(fields: &Option<serde_json::Value>) -> Option<String> {
    let v = fields.as_ref()?;
    let s = match v {
        serde_json::Value::Object(map) if map.is_empty() => return None,
        _ => serde_json::to_string(v).ok()?,
    };
    Some(truncate(&s, 1024))
}

fn emit_event(e: ClientLogEntry) {
    let target = allowed_target(e.target.as_deref());
    let message = truncate(&e.message, MAX_MESSAGE);
    let stack = e.stack.as_deref().map(|s| truncate(s, MAX_STACK));
    let component_stack = e.component_stack.as_deref().map(|s| truncate(s, MAX_STACK));
    let path = truncate(&e.path, MAX_PATH);
    let ua = truncate(&e.user_agent, MAX_USER_AGENT);
    let session = e.session_id.as_deref().map(|s| truncate(s, 128));
    let request_id = e.request_id.as_deref().map(|s| truncate(s, 128));
    let rid = e.rid.as_deref().map(|s| truncate(s, 128));
    let fields_str = fields_summary(&e.fields);
    let dropped = e.dropped;
    let ts = e.ts;

    // tracing's `target:` argument must be a string literal, so dispatch
    // the (level, target) combinations explicitly.
    dispatch(
        e.level.as_str(),
        target,
        &message,
        path.as_str(),
        ua.as_str(),
        session.as_deref(),
        request_id.as_deref(),
        rid.as_deref(),
        fields_str.as_deref(),
        stack.as_deref(),
        component_stack.as_deref(),
        ts,
        dropped,
    );
}

#[allow(clippy::too_many_arguments)]
fn dispatch(
    level: &str,
    target: &'static str,
    message: &str,
    path: &str,
    ua: &str,
    session: Option<&str>,
    request_id: Option<&str>,
    rid: Option<&str>,
    fields: Option<&str>,
    stack: Option<&str>,
    component_stack: Option<&str>,
    ts: i64,
    dropped: Option<u64>,
) {
    macro_rules! emit_one {
        ($t:literal) => {
            match level {
                "trace" => tracing::trace!(
                    target: $t,
                    path, ua, session, request_id, rid,
                    fields, ts, dropped,
                    "{message}"
                ),
                "debug" => tracing::debug!(
                    target: $t,
                    path, ua, session, request_id, rid,
                    fields, ts, dropped,
                    "{message}"
                ),
                "info" => tracing::info!(
                    target: $t,
                    path, ua, session, request_id, rid,
                    fields, ts, dropped,
                    "{message}"
                ),
                "warn" => tracing::warn!(
                    target: $t,
                    path, ua, session, request_id, rid,
                    fields, stack, component_stack, ts, dropped,
                    "{message}"
                ),
                "error" => tracing::error!(
                    target: $t,
                    path, ua, session, request_id, rid,
                    fields, stack, component_stack, ts, dropped,
                    "{message}"
                ),
                _ => tracing::info!(
                    target: $t,
                    path, ua, session, request_id, rid,
                    fields, ts, dropped,
                    "{message}"
                ),
            }
        };
    }
    match target {
        "web.client.error" => emit_one!("web.client.error"),
        "web.client.api" => emit_one!("web.client.api"),
        "web.client.nav" => emit_one!("web.client.nav"),
        "web.client.input" => emit_one!("web.client.input"),
        "web.client.settings" => emit_one!("web.client.settings"),
        "web.client.ws" => emit_one!("web.client.ws"),
        "web.client.terminal" => emit_one!("web.client.terminal"),
        "web.client.cockpit" => emit_one!("web.client.cockpit"),
        "web.client.pwa" => emit_one!("web.client.pwa"),
        _ => emit_one!("web.client"),
    }
}

/// Frontend logging policy derived from the current FilterController.
#[derive(Serialize)]
pub struct ClientLogPolicy {
    pub version: u64,
    pub default_level: String,
    pub targets: BTreeMap<String, String>,
}

/// Parse the controller's filter directive and project the `web.client.*`
/// entries onto a level map the frontend can consume.
///
/// The directive uses EnvFilter syntax (`root=level,root.sub=level,...`).
/// We support the subset: bare `target=level`, last-wins. Unknown levels
/// and complex span/field directives are ignored — the frontend gates
/// conservatively when in doubt.
pub fn build_policy_from_filter(directive: &str) -> ClientLogPolicy {
    let mut default_level: String = "info".to_string();
    let mut targets: BTreeMap<String, String> = BTreeMap::new();
    for part in directive
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        // Skip anything that looks like a span directive (has `[`).
        if part.contains('[') {
            continue;
        }
        let Some((t, lvl)) = part.split_once('=') else {
            // Bare level (no target). Use as default.
            if matches!(part, "trace" | "debug" | "info" | "warn" | "error") {
                default_level = part.to_string();
            }
            continue;
        };
        let t = t.trim();
        let lvl = lvl.trim().to_ascii_lowercase();
        if !matches!(lvl.as_str(), "trace" | "debug" | "info" | "warn" | "error") {
            continue;
        }
        if t == "web" {
            default_level = lvl;
            continue;
        }
        if t.starts_with("web.client") {
            targets.insert(t.to_string(), lvl);
        }
    }
    // Use a per-call counter so the frontend can detect changes (version
    // increments monotonically each refresh; the actual content is what
    // matters for policy decisions).
    static VERSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let version = VERSION.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    ClientLogPolicy {
        version,
        default_level,
        targets,
    }
}

#[tracing::instrument(target = "http.api.client_log", skip_all)]
pub async fn get_client_log_policy(State(_state): State<Arc<AppState>>) -> Json<ClientLogPolicy> {
    let directive = crate::logging::current_filter().unwrap_or_else(|| "info".to_string());
    Json(build_policy_from_filter(&directive))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_short_string_unchanged() {
        assert_eq!(truncate("hello", 100), "hello");
    }

    #[test]
    fn truncate_long_string_adds_ellipsis() {
        let s = "a".repeat(50);
        let t = truncate(&s, 10);
        assert!(t.starts_with("aaaaaaaaaa"));
        assert!(t.ends_with("…"));
    }

    #[test]
    fn allowed_target_known_passes_through() {
        assert_eq!(allowed_target(Some("web.client.api")), "web.client.api");
        assert_eq!(allowed_target(Some("web.client")), "web.client");
    }

    #[test]
    fn allowed_target_unknown_collapses() {
        assert_eq!(allowed_target(Some("auth.token")), "web.client");
        assert_eq!(allowed_target(Some("web.client.evil")), "web.client");
        assert_eq!(allowed_target(None), "web.client");
    }

    #[test]
    fn policy_from_filter_picks_web_client_overrides() {
        let p = build_policy_from_filter(
            "agent_of_empires=info,web.client=debug,web.client.api=trace,auth=warn,info",
        );
        assert_eq!(p.default_level, "info");
        assert_eq!(
            p.targets.get("web.client").map(String::as_str),
            Some("debug")
        );
        assert_eq!(
            p.targets.get("web.client.api").map(String::as_str),
            Some("trace")
        );
        assert!(p.targets.get("auth").is_none());
    }
}
