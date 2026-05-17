//! Furkan → AVK ajanı chat endpoint — FUR-4164.
//!
//! `POST /api/avk/furkan-chat` Mac-yerel `agentmemory` MCP HTTP proxy'sine
//! `memory_signal_send` çağrısı atar (from=furkan, to=<slug>, type=chat).
//! Broadcast endpoint'inden (FUR-4121) farkı: tmux pane'lere değil,
//! ajanın memory signal kuyruğuna mesaj düşer — ajan idle iken bekler,
//! kendi loop turn'ünde `memory_signal_read agentId=<slug> unreadOnly=true`
//! ile yakalar.
//!
//! ## Güvenlik
//!
//! - `to` AVK_AGENTS registry'sinde tanımlı olmalı (bilinmeyen → 404)
//! - `from` her zaman `furkan` (override edilemez; UI bu endpoint'i
//!   Furkan dashboard'undan çağırır, başka kimlik yok)
//! - Mesaj 1-8KB cap, boş 400, type sabit `chat`
//!
//! ## Threading
//!
//! `thread_id` opsiyonel; verilmezse agentmemory MCP yeni thread oluşturur.
//! Aynı sohbet devamı için frontend son `thread_id`'yi tutup tekrar gönderir.

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;

use super::AppState;
use crate::avk_agents::find_by_slug;

const MCP_URL: &str = "http://localhost:3111/agentmemory/mcp/call";
const MCP_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_MESSAGE_BYTES: usize = 8192;
const SENDER: &str = "furkan";

#[derive(Deserialize)]
pub struct ChatRequest {
    /// Hedef AVK ajan slug (örn `koord`, `komuta`).
    pub to: String,
    /// Mesaj metni (1-8KB).
    pub message: String,
    /// Mevcut sohbet thread id (opsiyonel — yoksa yeni thread).
    #[serde(default)]
    pub thread_id: Option<String>,
}

#[derive(Serialize)]
pub struct ChatResponse {
    pub signal_id: String,
    pub thread_id: String,
    pub to: String,
    pub from: &'static str,
    pub created_at: String,
}

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

pub async fn post_avk_furkan_chat(
    State(_state): State<Arc<AppState>>,
    Json(req): Json<ChatRequest>,
) -> Response {
    let message = req.message.trim();
    if message.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "message boş olamaz");
    }
    if message.len() > MAX_MESSAGE_BYTES {
        return error_response(
            StatusCode::PAYLOAD_TOO_LARGE,
            &format!("message {} B > cap {} B", message.len(), MAX_MESSAGE_BYTES),
        );
    }

    if find_by_slug(&req.to).is_none() {
        return error_response(
            StatusCode::NOT_FOUND,
            &format!(
                "hedef slug '{}' AVK_AGENTS registry'sinde tanımlı değil",
                req.to
            ),
        );
    }

    match send_signal(&req.to, message, req.thread_id.as_deref()).await {
        Ok(resp) => Json(resp).into_response(),
        Err(e) => error_response(StatusCode::BAD_GATEWAY, &format!("MCP fail: {e}")),
    }
}

async fn send_signal(
    to: &str,
    message: &str,
    thread_id: Option<&str>,
) -> Result<ChatResponse, String> {
    let mut args = serde_json::json!({
        "from": SENDER,
        "to": to,
        "type": "chat",
        "content": message,
    });
    if let Some(tid) = thread_id {
        if !tid.is_empty() {
            args["threadId"] = serde_json::Value::String(tid.to_string());
        }
    }
    let body = serde_json::json!({
        "name": "memory_signal_send",
        "arguments": args,
    });

    let client = reqwest::Client::builder()
        .timeout(MCP_TIMEOUT)
        .build()
        .map_err(|e| format!("reqwest build: {e}"))?;
    let resp = client
        .post(MCP_URL)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("MCP unreachable: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("MCP status {}", resp.status()));
    }

    let outer: Value = resp.json().await.map_err(|e| format!("outer parse: {e}"))?;
    let inner_text = outer
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|first| first.get("text"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| "content[0].text missing".to_string())?;
    let inner: Value = serde_json::from_str(inner_text).map_err(|e| format!("inner parse: {e}"))?;

    let signal = inner
        .get("signal")
        .ok_or_else(|| "signal missing in MCP response".to_string())?;
    let signal_id = signal
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "signal.id missing".to_string())?
        .to_string();
    let thread_id = signal
        .get("threadId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let created_at = signal
        .get("createdAt")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Ok(ChatResponse {
        signal_id,
        thread_id,
        to: to.to_string(),
        from: SENDER,
        created_at,
    })
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
