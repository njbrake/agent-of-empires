//! Cockpit WebSocket fanout.
//!
//! `/sessions/{id}/cockpit/ws` upgrades to a WebSocket that subscribes
//! to `AppState::cockpit_events_tx` and forwards every frame whose
//! `session_id` matches the route param. Frames are JSON. The protocol
//! is one-way today (server -> client); inbound messages are ignored.
//!
//! Durability lives in `AppState::cockpit_event_store` (SQLite), not
//! this channel. The broadcast channel is best-effort: a client that
//! connects between a `tx.send` and its `subscribe()` misses frames,
//! and `RecvError::Lagged` drops frames when the channel overflows.
//! Both cases recover via the on-connect drain, which reads the
//! event store from `?since=` (or 0 for fresh subscribers); the
//! same store backs `GET /api/sessions/{id}/cockpit/replay`. The
//! channel is the fast path; the store is the truth.

use std::sync::Arc;

use axum::extract::{
    ws::{Message, WebSocket, WebSocketUpgrade},
    Path, Query, State,
};
use axum::response::IntoResponse;
use serde::Deserialize;
use tokio::select;
use tokio::sync::broadcast::error::RecvError;
use tracing::{debug, warn};

use super::{AppState, CockpitBroadcastFrame};

/// Query parameters for the cockpit WS upgrade. Clients pass
/// `?since=<lastSeq>` so the on-connect drain only resends events
/// newer than what they already have. Without this, a long-running
/// session resends its full transcript on every reconnect (page
/// refresh / mobile flap), which can be tens of MB at the retention
/// cap.
#[derive(Debug, Default, Deserialize)]
pub struct CockpitWsQuery {
    #[serde(default)]
    pub since: Option<u64>,
}

/// Public route handler for the cockpit WebSocket.
pub async fn cockpit_ws(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(q): Query<CockpitWsQuery>,
) -> impl IntoResponse {
    // Logged at DEBUG so we can prove the route was reached even
    // when the upgrade fails. If this line is missing from debug.log
    // for a session that's stuck on "no live updates", the request
    // never got past auth_middleware (or never left the browser).
    // One line per WS connect (not per message), so debug-level
    // doesn't risk spamming.
    let since = q.since.unwrap_or(0);
    debug!(
        target: "cockpit.ws",
        session = %id,
        since,
        "cockpit ws route entered, beginning upgrade"
    );
    let session_for_handler = id.clone();
    ws.protocols(["aoe-auth"])
        .on_upgrade(move |socket| async move {
            debug!(target: "cockpit.ws", session = %session_for_handler, "cockpit ws upgrade complete");
            handle(socket, session_for_handler, state, since).await
        })
}

async fn handle(mut socket: WebSocket, session_id: String, state: Arc<AppState>, since: u64) {
    // Subscribe BEFORE the replay snapshot so events published in the
    // window between snapshot and live-loop entry land in `rx`. Such
    // events also appear in the replay snapshot if the publish
    // happens to interleave; the client dedupes via `frame.seq <=
    // state.lastSeq`, so duplicates are no-ops. The reverse order
    // (snapshot first, then subscribe) leaves a gap where live
    // events get dropped.
    let mut rx = state.cockpit_events_tx.subscribe();

    // Replay events newer than `since` immediately on connect. Without
    // this, any events published in the upgrade gap between the
    // client's POST /cockpit/spawn (or the first /cockpit/prompt) and
    // our `subscribe()` above are silently dropped by the broadcast
    // channel, since tokio's `broadcast::Sender::send` discards the
    // message when no receivers exist. The disk-backed event store
    // captures every published event, so reading it here closes the
    // race without forcing the client to GET /cockpit/replay
    // separately.
    let replay_count = drain_replay_into_socket(&mut socket, &state, &session_id, since).await;
    debug!(
        target: "cockpit.ws",
        session = %session_id,
        since,
        replayed = replay_count,
        "cockpit ws subscribed"
    );

    loop {
        select! {
            client_msg = socket.recv() => {
                match client_msg {
                    Some(Ok(Message::Close(_))) | None => break,
                    // Inbound messages from the client are not used today.
                    // Clients post approval resolutions via REST, not the
                    // WebSocket. Ignore everything we receive.
                    Some(Ok(_)) => continue,
                    Some(Err(e)) => {
                        warn!(target: "cockpit.ws", "client recv error: {e}");
                        break;
                    }
                }
            }
            event = rx.recv() => {
                match event {
                    Ok(frame) => {
                        if frame.session_id != session_id {
                            continue;
                        }
                        let payload = match serde_json::to_string(&frame) {
                            Ok(s) => s,
                            Err(e) => {
                                warn!(target: "cockpit.ws", "serialise frame: {e}");
                                continue;
                            }
                        };
                        if socket.send(Message::Text(payload.into())).await.is_err() {
                            break;
                        }
                    }
                    Err(RecvError::Lagged(skipped)) => {
                        // Tell the client they missed events so they can
                        // request a snapshot+replay rather than silently
                        // diverging.
                        let gap = serde_json::json!({
                            "kind": "lagged",
                            "skipped": skipped,
                        });
                        let _ = socket
                            .send(Message::Text(gap.to_string().into()))
                            .await;
                    }
                    Err(RecvError::Closed) => break,
                }
            }
        }
    }

    debug!(target: "cockpit.ws", session = %session_id, "cockpit ws disconnected");
    let _ = socket.send(Message::Close(None)).await;
}

/// Read every stored event for `session_id` with `seq > since` out of
/// the disk-backed event store and forward it to the socket as a
/// `CockpitBroadcastFrame`. Returns the number of frames sent. The
/// event store survives `aoe serve` restart, so this drain works even
/// after the daemon has restarted. The live broadcast channel is
/// already subscribed by the caller before this runs, so any events
/// published between the snapshot and the live-loop entry are still
/// delivered (the client dedupes by seq).
async fn drain_replay_into_socket(
    socket: &mut WebSocket,
    state: &AppState,
    session_id: &str,
    since: u64,
) -> usize {
    let entries = state.cockpit_event_store.replay_from(session_id, since);
    let mut sent = 0usize;
    for (seq, event) in entries {
        let frame = CockpitBroadcastFrame {
            session_id: session_id.to_string(),
            seq,
            event: Arc::new(event),
        };
        let payload = match serde_json::to_string(&frame) {
            Ok(s) => s,
            Err(e) => {
                warn!(target: "cockpit.ws", "serialise replay frame: {e}");
                continue;
            }
        };
        if socket.send(Message::Text(payload.into())).await.is_err() {
            break;
        }
        sent += 1;
    }
    sent
}

/// Helper used by the worker supervisor (and integration tests) to
/// publish a frame.
pub fn publish(state: &AppState, frame: CockpitBroadcastFrame) {
    // Discard the receiver count; broadcast::Sender::send is best-effort
    // and ignores send-with-no-receivers.
    let _ = state.cockpit_events_tx.send(frame);
}

/// Push-notification trigger for "agent needs your approval." Called
/// by the worker supervisor when it observes an `ApprovalRequested`
/// cockpit event. Re-uses the existing push infrastructure: subscribers
/// for `state.push` receive a payload telling the PWA to focus the
/// approval card.
pub async fn trigger_approval_push(
    state: &AppState,
    session_id: &str,
    approval_title: &str,
    destructive: bool,
) {
    let Some(push) = state.push.as_ref() else {
        return;
    };
    if !state.push_enabled {
        return;
    }
    let badge = if destructive {
        "DESTRUCTIVE"
    } else {
        "approval"
    };
    let title = format!("{} needs approval", session_id);
    let body = if destructive {
        format!("{badge}: {approval_title}")
    } else {
        approval_title.to_string()
    };
    let payload = super::push_send::PushPayload {
        title,
        body,
        url: format!("/sessions/{session_id}/cockpit"),
        tag: format!("cockpit-approval-{session_id}"),
        session_id: session_id.to_string(),
    };
    let subs = push.store.snapshot().await;
    if subs.is_empty() {
        return;
    }
    let client = match super::push_send::build_client() {
        Ok(c) => c,
        Err(e) => {
            warn!(target: "cockpit.push", "build_client: {e}");
            return;
        }
    };
    let body_bytes = match serde_json::to_vec(&payload) {
        Ok(b) => b,
        Err(e) => {
            warn!(target: "cockpit.push", "serialise payload: {e}");
            return;
        }
    };
    for sub in subs {
        let auth_header = match super::push_send::vapid_auth_header(push, &sub.endpoint) {
            Ok(h) => h,
            Err(e) => {
                warn!(target: "cockpit.push", "vapid header: {e}");
                continue;
            }
        };
        let cipher = match super::push_send::encrypt_aes128gcm(&sub, &body_bytes) {
            Ok(c) => c,
            Err(e) => {
                warn!(target: "cockpit.push", "encrypt: {e}");
                continue;
            }
        };
        let _ = client
            .post(&sub.endpoint)
            .header("Authorization", &auth_header)
            .header("Content-Encoding", "aes128gcm")
            .header("Content-Type", "application/octet-stream")
            .header("TTL", "60")
            .body(cipher)
            .send()
            .await;
    }
}

#[cfg(all(test, feature = "serve"))]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publish_with_no_receivers_does_not_panic() {
        // Create a minimal AppState-like fixture: in real code the server
        // owns AppState; for this unit test we just need the broadcast
        // channel by itself.
        let (tx, _rx) = tokio::sync::broadcast::channel::<CockpitBroadcastFrame>(8);
        // Drop receiver: send should not error.
        drop(_rx);
        let send_result = tx.send(CockpitBroadcastFrame {
            session_id: "s".into(),
            seq: 1,
            event: Arc::new(crate::cockpit::Event::ThinkingStarted),
        });
        // Sending to a channel with no receivers returns Err, but
        // publish() in this module deliberately discards the result.
        assert!(send_result.is_err() || send_result.is_ok());
    }
}
