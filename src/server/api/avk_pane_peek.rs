//! AVK pane peek endpoint — FUR-4161.
//!
//! `GET /api/avk/pane-peek?slug=<slug>&lines=<N>` belirtilen AVK ajanın
//! tmux pane'inden son N satırı `tmux capture-pane -t <target> -pS -<N>`
//! ile çekip JSON döner. Dashboard'da AvkAgentsGrid kart tıklaması ile
//! inline expand preview gösterilir.
//!
//! ## Güvenlik
//!
//! `slug` AVK_AGENTS registry'sinde tanımlı olmalı — bilinmeyen slug 404.
//! `lines` 1-200 arası clamp (dahili limit; uzun capture stalled pane'lerde
//! gereksiz I/O). tmux target runtime resolver (FUR-4122) varsa onu,
//! yoksa registry sabit `tmux_target` fallback.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use std::process::Command;
use std::sync::Arc;

use super::avk_broadcast::resolve_runtime_target;
use super::AppState;
use crate::avk_agents::find_by_slug;

const DEFAULT_LINES: u32 = 40;
const MAX_LINES: u32 = 200;

#[derive(Deserialize)]
pub struct PanePeekQuery {
    pub slug: String,
    pub lines: Option<u32>,
}

#[derive(Serialize)]
pub struct PanePeekResponse {
    pub slug: String,
    pub target: String,
    pub runtime_resolved: bool,
    pub lines: u32,
    /// Pane'den ham capture (UTF-8). Boş string pane'in boş ya da yeni olduğunu
    /// gösterir; UI buna göre "pane sessiz" mesajı verebilir.
    pub content: String,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

pub async fn get_avk_pane_peek(
    State(_state): State<Arc<AppState>>,
    Query(query): Query<PanePeekQuery>,
) -> Response {
    let agent = match find_by_slug(&query.slug) {
        Some(a) => a,
        None => {
            return error_response(
                StatusCode::NOT_FOUND,
                &format!(
                    "slug '{}' AVK_AGENTS registry'sinde tanımlı değil",
                    query.slug
                ),
            );
        }
    };

    let lines = query.lines.unwrap_or(DEFAULT_LINES).clamp(1, MAX_LINES);

    let (target, runtime_resolved) = match resolve_runtime_target(agent.slug) {
        Some(t) => (t, true),
        None => (agent.tmux_target.to_string(), false),
    };

    match capture_pane(&target, lines) {
        Ok(content) => Json(PanePeekResponse {
            slug: agent.slug.to_string(),
            target,
            runtime_resolved,
            lines,
            content,
        })
        .into_response(),
        Err(e) => error_response(
            StatusCode::BAD_GATEWAY,
            &format!("tmux capture-pane failed: {e}"),
        ),
    }
}

fn capture_pane(target: &str, lines: u32) -> Result<String, String> {
    let start_line = format!("-{lines}");
    let output = Command::new("tmux")
        .args(["capture-pane", "-t", target, "-pS", &start_line])
        .output()
        .map_err(|e| format!("tmux spawn: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "exit code {:?}: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn error_response(status: StatusCode, msg: &str) -> Response {
    (
        status,
        Json(ErrorBody {
            error: msg.to_string(),
        }),
    )
        .into_response()
}
