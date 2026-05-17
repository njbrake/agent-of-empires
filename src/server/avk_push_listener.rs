//! AVK signal → Web Push background listener.
//!
//! Tokio background task that polls the local `agentmemory` MCP every
//! [`POLL_INTERVAL`] seconds for new `to=furkan` signals, then re-uses
//! the existing AoE Web Push pipeline (`push_send::send_one`) to deliver
//! a notification to every subscribed PWA.
//!
//! ## Flow
//!
//! 1. Spawn at server startup (after `AppState` ready).
//! 2. Every 30s, POST `memory_signal_read agentId=furkan limit=50` to
//!    `http://localhost:3111/agentmemory/mcp/call`.
//! 3. Diff against last-seen ID set (persisted to disk so daemon restart
//!    does not re-fire historical notifications).
//! 4. For every truly-new signal, snapshot the subscription store and
//!    deliver a `PushPayload { title: "<from> → Furkan", body, tag, ... }`
//!    to each subscriber. Send errors are best-effort (handled by
//!    `send_one` which marks Gone subscribers internally).
//!
//! ## Why no broadcast channel
//!
//! AoE's existing push pipeline subscribes to a `tokio::broadcast`
//! (`status_tx`) carrying `StatusChange` events from the tmux poller.
//! AVK signals originate outside the process (agentmemory MCP) so the
//! cleanest contract is a polling loop here that converts signals into
//! the same `PushPayload` shape `push_send::send_one` already consumes.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;
use tokio::time::sleep;

use super::push_send::{build_client, send_one, PushPayload};
use super::AppState;

const POLL_INTERVAL: Duration = Duration::from_secs(30);
const MCP_URL: &str = "http://localhost:3111/agentmemory/mcp/call";
const FETCH_TIMEOUT: Duration = Duration::from_secs(5);
const SIGNAL_LIMIT: u32 = 50;
const SEEN_MAX_ENTRIES: usize = 500;
const SEEN_FILE_NAME: &str = "avk_signal_seen.txt";
const SIGNAL_AGENT_ID: &str = "furkan";

#[derive(Debug, Deserialize)]
struct InboxSignal {
    id: String,
    from: String,
    #[serde(default)]
    r#type: String,
    content: String,
}

/// Spawn the background listener. Idempotent guard NOT included — caller
/// must invoke at most once during server bootstrap.
pub fn spawn(state: Arc<AppState>) {
    tokio::spawn(async move {
        run(state).await;
    });
}

async fn run(state: Arc<AppState>) {
    let seen_path = seen_file_path();
    let mut seen = load_seen(&seen_path);
    tracing::info!(
        seen_count = seen.len(),
        path = ?seen_path,
        "avk_push: listener started"
    );

    loop {
        sleep(POLL_INTERVAL).await;

        let Some(push) = state.push.as_ref() else {
            // VAPID not configured at startup — listener idles.
            continue;
        };

        let signals = match fetch_furkan_signals().await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "avk_push: signal fetch failed");
                continue;
            }
        };

        let new_signals: Vec<InboxSignal> = signals
            .into_iter()
            .filter(|s| !seen.contains(&s.id))
            .collect();
        if new_signals.is_empty() {
            continue;
        }

        let subs = push.store.snapshot().await;
        if subs.is_empty() {
            // Nothing to push to yet — still mark as seen to avoid blowing
            // a backlog when the first subscription lands.
            for sig in &new_signals {
                seen.insert(sig.id.clone());
            }
            save_seen(&seen_path, &seen);
            continue;
        }

        let client = match build_client() {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(error = %e, "avk_push: reqwest client build failed");
                continue;
            }
        };

        tracing::info!(
            new = new_signals.len(),
            subs = subs.len(),
            "avk_push: dispatching signals"
        );

        for sig in &new_signals {
            let payload = signal_to_payload(sig);
            for sub in &subs {
                let _ = send_one(&client, push.as_ref(), sub, &payload).await;
            }
            seen.insert(sig.id.clone());
        }

        truncate_seen(&mut seen);
        save_seen(&seen_path, &seen);
    }
}

fn signal_to_payload(sig: &InboxSignal) -> PushPayload {
    let type_emoji = match sig.r#type.as_str() {
        "alert" => "🚨",
        "question" => "❓",
        "handoff" => "🔁",
        "report" => "📝",
        "request" => "📨",
        _ => "💬",
    };
    let title = format!("{} {} → Furkan", type_emoji, sig.from);
    let body: String = sig.content.chars().take(220).collect();
    PushPayload {
        title,
        body,
        url: "/".to_string(),
        tag: sig.id.clone(),
        session_id: format!("avk-signal:{}", sig.id),
    }
}

async fn fetch_furkan_signals() -> Result<Vec<InboxSignal>, String> {
    let body = serde_json::json!({
        "name": "memory_signal_read",
        "arguments": {
            "agentId": SIGNAL_AGENT_ID,
            "limit": SIGNAL_LIMIT,
        }
    });

    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .build()
        .map_err(|e| format!("reqwest build: {e}"))?;

    let resp = client
        .post(MCP_URL)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("MCP unreachable: {e}"))?;

    if !resp.status().is_success() {
        return Err(format!("MCP HTTP {}", resp.status()));
    }

    let outer: Value = resp.json().await.map_err(|e| format!("outer parse: {e}"))?;
    let inner_text = outer
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|a| a.first())
        .and_then(|f| f.get("text"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| "no content[0].text".to_string())?;
    let inner: Value = serde_json::from_str(inner_text).map_err(|e| format!("inner parse: {e}"))?;
    let arr = inner
        .get("signals")
        .and_then(|s| s.as_array())
        .ok_or_else(|| "no signals[]".to_string())?;

    Ok(arr
        .iter()
        .filter_map(|s| {
            Some(InboxSignal {
                id: s.get("id")?.as_str()?.to_string(),
                from: s.get("from")?.as_str()?.to_string(),
                r#type: s
                    .get("type")
                    .and_then(|v| v.as_str())
                    .unwrap_or("chat")
                    .to_string(),
                content: s
                    .get("content")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string(),
            })
        })
        .collect())
}

fn seen_file_path() -> PathBuf {
    match crate::session::get_app_dir() {
        Ok(dir) => dir.join(SEEN_FILE_NAME),
        Err(_) => std::env::temp_dir().join(SEEN_FILE_NAME),
    }
}

fn load_seen(path: &PathBuf) -> HashSet<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|raw| {
            raw.lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn save_seen(path: &PathBuf, seen: &HashSet<String>) {
    let serialized = seen.iter().cloned().collect::<Vec<_>>().join("\n");
    if let Err(e) = std::fs::write(path, serialized) {
        tracing::warn!(error = %e, ?path, "avk_push: seen file write failed");
    }
}

fn truncate_seen(seen: &mut HashSet<String>) {
    if seen.len() <= SEEN_MAX_ENTRIES {
        return;
    }
    // HashSet ordering not guaranteed; drop deterministically by id sort
    // — IDs include timestamp prefix so lexical sort keeps the most recent.
    let mut sorted: Vec<String> = seen.iter().cloned().collect();
    sorted.sort();
    let keep_from = sorted.len() - SEEN_MAX_ENTRIES;
    let keep: HashSet<String> = sorted.into_iter().skip(keep_from).collect();
    *seen = keep;
}
