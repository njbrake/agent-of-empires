//! WebSocket handler for live terminal streaming via PTY relay.
//!
//! Instead of polling `capture-pane`, each WebSocket connection spawns
//! `tmux attach-session` inside a PTY and relays the raw byte stream
//! bidirectionally. This gives the browser a real terminal experience:
//! zero input lag, all key sequences work, real-time output.

use std::io::{Read, Write};
use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket},
        Path, State, WebSocketUpgrade,
    },
    response::IntoResponse,
};
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use tokio::sync::RwLock;

use super::AppState;

/// WebSocket for the paired host terminal (TerminalSession tmux session)
pub async fn paired_terminal_ws(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let instances = state.instances.read().await;
    let session_info = instances
        .iter()
        .find(|i| i.id == id)
        .map(|inst| crate::tmux::TerminalSession::generate_name(&inst.id, &inst.title));
    drop(instances);

    let read_only = state.read_only;
    let primaries = Arc::clone(&state.session_primaries);
    let pause_counts = Arc::clone(&state.session_pause_counts);

    match session_info {
        // Accept the "aoe-auth" subprotocol so the browser's handshake
        // completes. The client offers `["aoe-auth", <token>]`; the auth
        // middleware validates the token from the same header, and the
        // server echoes back "aoe-auth" to satisfy the WS spec. The token
        // itself is not echoed, only the marker.
        Some(tmux_name) => ws
            .protocols(["aoe-auth"])
            .on_upgrade(move |socket| {
                handle_terminal_ws(socket, tmux_name, read_only, primaries, pause_counts)
            })
            .into_response(),
        None => (axum::http::StatusCode::NOT_FOUND, "Session not found").into_response(),
    }
}

/// WebSocket for the paired container terminal (ContainerTerminalSession tmux session)
pub async fn container_terminal_ws(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let instances = state.instances.read().await;
    let session_info = instances
        .iter()
        .find(|i| i.id == id)
        .map(|inst| crate::tmux::ContainerTerminalSession::generate_name(&inst.id, &inst.title));
    drop(instances);

    let read_only = state.read_only;
    let primaries = Arc::clone(&state.session_primaries);
    let pause_counts = Arc::clone(&state.session_pause_counts);

    match session_info {
        // Accept the "aoe-auth" subprotocol so the browser's handshake
        // completes. The client offers `["aoe-auth", <token>]`; the auth
        // middleware validates the token from the same header, and the
        // server echoes back "aoe-auth" to satisfy the WS spec. The token
        // itself is not echoed, only the marker.
        Some(tmux_name) => ws
            .protocols(["aoe-auth"])
            .on_upgrade(move |socket| {
                handle_terminal_ws(socket, tmux_name, read_only, primaries, pause_counts)
            })
            .into_response(),
        None => (axum::http::StatusCode::NOT_FOUND, "Session not found").into_response(),
    }
}

/// WebSocket for the agent's main tmux session
pub async fn terminal_ws(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    // Verify session exists before upgrading
    let instances = state.instances.read().await;
    let session_info = instances
        .iter()
        .find(|i| i.id == id)
        .map(|inst| crate::tmux::Session::generate_name(&inst.id, &inst.title));
    drop(instances);

    let read_only = state.read_only;
    let primaries = Arc::clone(&state.session_primaries);
    let pause_counts = Arc::clone(&state.session_pause_counts);

    match session_info {
        // Accept the "aoe-auth" subprotocol so the browser's handshake
        // completes. The client offers `["aoe-auth", <token>]`; the auth
        // middleware validates the token from the same header, and the
        // server echoes back "aoe-auth" to satisfy the WS spec. The token
        // itself is not echoed, only the marker.
        Some(tmux_name) => ws
            .protocols(["aoe-auth"])
            .on_upgrade(move |socket| {
                handle_terminal_ws(socket, tmux_name, read_only, primaries, pause_counts)
            })
            .into_response(),
        None => (axum::http::StatusCode::NOT_FOUND, "Session not found").into_response(),
    }
}

/// Unique client ID counter for primary-client tracking.
static CLIENT_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Per-tmux-session map tracking which WebSocket client "owns" resizing.
///
/// Only the primary client's resize messages are applied to its PTY. This
/// prevents multiple browser viewports (phone, desktop, tablet) from
/// fighting over the tmux window size. The primary is whichever client
/// most recently sent keyboard input, since aoe is single-user: if
/// you're typing on your phone you want phone-sized output.
///
/// When no client is primary (all have disconnected, or nobody has typed
/// yet), any client's resize is applied so the initial connection still
/// sets up the PTY dimensions from the default 80x24.
type SessionPrimaries = Arc<RwLock<std::collections::HashMap<String, String>>>;

/// Per-session refcount of clients requesting SIGSTOP of the pane's
/// process tree. Only transitions 0↔1 actually signal the process, so
/// concurrent readers can scroll independently without one's
/// `resume_output` un-pausing the other's scrollback.
type SessionPauseCounts = Arc<tokio::sync::Mutex<std::collections::HashMap<String, u32>>>;

async fn handle_terminal_ws(
    socket: WebSocket,
    tmux_name: String,
    read_only: bool,
    primaries: SessionPrimaries,
    pause_counts: SessionPauseCounts,
) {
    use futures_util::{SinkExt, StreamExt};

    let client_id = format!(
        "ws-{}",
        CLIENT_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );

    // Spawn tmux attach inside a PTY
    let pty_system = NativePtySystem::default();
    let pair = match pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(pair) => pair,
        Err(e) => {
            tracing::error!("Failed to open PTY: {}", e);
            return;
        }
    };

    let mut cmd = CommandBuilder::new("tmux");
    cmd.args(["attach-session", "-t", &tmux_name]);
    cmd.env("TERM", "xterm-256color");
    // Allow nesting: unset TMUX so the attach works when aoe serve runs inside tmux
    cmd.env_remove("TMUX");

    let mut child = match pair.slave.spawn_command(cmd) {
        Ok(child) => child,
        Err(e) => {
            tracing::error!("Failed to spawn tmux attach: {}", e);
            return;
        }
    };

    // We're done with the slave side
    drop(pair.slave);

    let master = pair.master;

    // Get reader and writer from the PTY master
    let mut reader = match master.try_clone_reader() {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("Failed to clone PTY reader: {}", e);
            return;
        }
    };

    let writer = match master.take_writer() {
        Ok(w) => w,
        Err(e) => {
            tracing::error!("Failed to take PTY writer: {}", e);
            return;
        }
    };

    let (mut ws_sender, mut ws_receiver) = socket.split();

    // Share the writer behind a mutex for the input task
    let writer = Arc::new(std::sync::Mutex::new(writer));
    // Share master for resize operations
    let master = Arc::new(std::sync::Mutex::new(master));

    // Use tokio channels to bridge sync PTY I/O with async WebSocket
    let (output_tx, mut output_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(64);
    // Control channel: server -> client JSON messages (primary status, etc.)
    let (ctrl_tx, mut ctrl_rx) = tokio::sync::mpsc::channel::<String>(8);

    // Task 1: PTY stdout -> channel (blocking read in dedicated thread)
    tokio::task::spawn_blocking(move || {
        let mut buf = [0u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if output_tx.blocking_send(buf[..n].to_vec()).is_err() {
                        break; // receiver dropped (WebSocket closed)
                    }
                }
                Err(_) => break,
            }
        }
    });

    // Task 2: PTY output + control messages -> WebSocket sender
    let send_handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                data = output_rx.recv() => {
                    match data {
                        Some(data) => {
                            if ws_sender.send(Message::Binary(data.into())).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    }
                }
                msg = ctrl_rx.recv() => {
                    match msg {
                        Some(text) => {
                            if ws_sender.send(Message::Text(text.into())).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    }
                }
            }
        }
        let _ = ws_sender.send(Message::Close(None)).await;
    });

    // Task 3: WebSocket receiver -> PTY stdin (and resize)
    let writer_for_input = writer.clone();
    let master_for_resize = master.clone();
    let primaries_for_recv = primaries.clone();
    let client_id_for_recv = client_id.clone();
    let tmux_name_for_recv = tmux_name.clone();
    let ctrl_tx_for_recv = ctrl_tx.clone();
    let pause_counts_for_recv = pause_counts.clone();
    // Track whether THIS ws currently contributes to the pause refcount,
    // shared between the receive loop and the post-loop cleanup.
    let this_ws_paused = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let this_ws_paused_for_recv = this_ws_paused.clone();

    let recv_handle = tokio::spawn(async move {
        // Track the last resize the browser requested so we can apply it
        // when this client becomes primary.
        let mut pending_size: Option<(u16, u16)> = None;

        while let Some(Ok(msg)) = ws_receiver.next().await {
            match msg {
                Message::Binary(data) => {
                    // Raw bytes from xterm.js -> PTY stdin (blocked in read-only mode)
                    if read_only {
                        continue;
                    }

                    // Claim primary on input. If we just became primary,
                    // apply our pending resize so the PTY matches our viewport.
                    let became_primary = claim_primary(
                        &primaries_for_recv,
                        &tmux_name_for_recv,
                        &client_id_for_recv,
                    )
                    .await;
                    if became_primary {
                        if let Some((cols, rows)) = pending_size {
                            resize_pty(&master_for_resize, cols, rows).await;
                        }
                        let _ = ctrl_tx_for_recv
                            .send(r#"{"type":"primary_status","is_primary":true}"#.into())
                            .await;
                    }

                    let writer = writer_for_input.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        if let Ok(mut w) = writer.lock() {
                            let _ = w.write_all(&data);
                            let _ = w.flush();
                        }
                    })
                    .await;
                }
                Message::Text(text) => {
                    // JSON control messages (resize, activate) are always allowed
                    if let Ok(control) = serde_json::from_str::<ControlMessage>(&text) {
                        match control {
                            ControlMessage::Resize { cols, rows } if cols > 0 && rows > 0 => {
                                pending_size = Some((cols, rows));

                                let dominated = is_primary_or_vacant(
                                    &primaries_for_recv,
                                    &tmux_name_for_recv,
                                    &client_id_for_recv,
                                )
                                .await;
                                if dominated {
                                    resize_pty(&master_for_resize, cols, rows).await;
                                } else {
                                    let _ = ctrl_tx_for_recv
                                        .send(
                                            r#"{"type":"primary_status","is_primary":false}"#
                                                .into(),
                                        )
                                        .await;
                                }
                            }
                            // Ignore zero-dimension resize (buggy client)
                            ControlMessage::Resize { .. } => {}
                            ControlMessage::Activate => {
                                // In read-only mode, don't claim primary. All
                                // viewers get independent resize (vacant state).
                                if read_only {
                                    continue;
                                }
                                let became_primary = claim_primary(
                                    &primaries_for_recv,
                                    &tmux_name_for_recv,
                                    &client_id_for_recv,
                                )
                                .await;
                                if became_primary {
                                    if let Some((cols, rows)) = pending_size {
                                        resize_pty(&master_for_resize, cols, rows).await;
                                    }
                                    let _ = ctrl_tx_for_recv
                                        .send(
                                            r#"{"type":"primary_status","is_primary":true}"#.into(),
                                        )
                                        .await;
                                }
                            }
                            ControlMessage::PauseOutput => {
                                // Client entered tmux scrollback. SIGSTOP the
                                // pane's process tree so it stops emitting
                                // bytes that would shift scrollback under the
                                // reader. Skipped in read-only mode.
                                //
                                // Refcounted across clients: only the first
                                // pause actually SIGSTOPs; duplicate pauses
                                // from the same ws are idempotent so a client
                                // that sends two pause_outputs doesn't
                                // over-contribute to the count.
                                if read_only {
                                    continue;
                                }
                                pause_enter(
                                    &pause_counts_for_recv,
                                    &tmux_name_for_recv,
                                    &this_ws_paused_for_recv,
                                )
                                .await;
                            }
                            ControlMessage::ResumeOutput => {
                                // Decrement this ws's contribution; only
                                // SIGCONT when the refcount reaches 0. Safe
                                // to call even if this ws wasn't paused.
                                pause_exit(
                                    &pause_counts_for_recv,
                                    &tmux_name_for_recv,
                                    &this_ws_paused_for_recv,
                                )
                                .await;
                            }
                        }
                    } else if !read_only {
                        // Plain text input -> PTY stdin (blocked in read-only mode).
                        // Also claims primary, same as binary input.
                        let became_primary = claim_primary(
                            &primaries_for_recv,
                            &tmux_name_for_recv,
                            &client_id_for_recv,
                        )
                        .await;
                        if became_primary {
                            if let Some((cols, rows)) = pending_size {
                                resize_pty(&master_for_resize, cols, rows).await;
                            }
                            let _ = ctrl_tx_for_recv
                                .send(r#"{"type":"primary_status","is_primary":true}"#.into())
                                .await;
                        }

                        let writer = writer_for_input.clone();
                        let bytes: Vec<u8> = text.as_bytes().to_vec();
                        let _ = tokio::task::spawn_blocking(move || {
                            if let Ok(mut w) = writer.lock() {
                                let _ = w.write_all(&bytes);
                                let _ = w.flush();
                            }
                        })
                        .await;
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    });

    // Wait for either direction to finish
    tokio::select! {
        _ = send_handle => {},
        _ = recv_handle => {},
    }

    // Release primary if this client held it, so the next client can take over.
    release_primary(&primaries, &tmux_name, &client_id).await;

    // Safety net: if this client paused the pane but the WebSocket
    // disconnected before it sent ResumeOutput (WiFi blip, app close,
    // etc.), decrement our contribution to the refcount. If this was
    // the last pauser on the session, SIGCONT the pane's process tree.
    pause_exit(&pause_counts, &tmux_name, &this_ws_paused).await;

    // Clean up: kill the tmux attach process
    let _ = child.kill();
    let _ = child.wait();
}

/// Claim primary for this client. Returns `true` if the client was NOT
/// already primary (i.e. it just became primary and should apply its
/// pending resize).
async fn claim_primary(primaries: &SessionPrimaries, session: &str, client_id: &str) -> bool {
    let mut map = primaries.write().await;
    let current = map.get(session);
    if current.is_some_and(|id| id == client_id) {
        return false; // already primary
    }
    map.insert(session.to_string(), client_id.to_string());
    true
}

/// Check whether this client is primary, or no client is primary for
/// this session (vacant). In either case the caller is allowed to resize.
async fn is_primary_or_vacant(
    primaries: &SessionPrimaries,
    session: &str,
    client_id: &str,
) -> bool {
    let map = primaries.read().await;
    match map.get(session) {
        None => true,
        Some(id) => id == client_id,
    }
}

/// Register that THIS WebSocket is asking the pane to be paused.
/// Idempotent per-ws (a duplicate pause_output from the same client
/// does not double-count). SIGSTOPs the pane's process tree on the
/// refcount 0→1 transition. Fire-and-forget: does not await the
/// spawn_blocking so the ws receive loop is not delayed by a slow
/// `tmux display-message`.
async fn pause_enter(
    pause_counts: &SessionPauseCounts,
    session: &str,
    this_ws_paused: &std::sync::atomic::AtomicBool,
) {
    if this_ws_paused.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return; // already contributing; idempotent
    }
    let should_stop = {
        let mut map = pause_counts.lock().await;
        let count = map.entry(session.to_string()).or_insert(0);
        *count += 1;
        *count == 1
    };
    if should_stop {
        let session = session.to_string();
        tokio::task::spawn_blocking(move || {
            if let Some(pid) = crate::process::get_pane_pid(&session) {
                crate::process::stop_process_tree(pid);
            }
        });
    }
}

/// Decrement THIS WebSocket's contribution to the pause refcount.
/// SIGCONTs the pane's process tree on the N→0 transition. Safe to
/// call even if this ws wasn't paused (no-op).
async fn pause_exit(
    pause_counts: &SessionPauseCounts,
    session: &str,
    this_ws_paused: &std::sync::atomic::AtomicBool,
) {
    if !this_ws_paused.swap(false, std::sync::atomic::Ordering::Relaxed) {
        return; // not paused by us
    }
    let should_cont = {
        let mut map = pause_counts.lock().await;
        if let Some(count) = map.get_mut(session) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                map.remove(session);
                true
            } else {
                false
            }
        } else {
            // Refcount underflow shouldn't happen; SIGCONT as safety.
            true
        }
    };
    if should_cont {
        let session = session.to_string();
        tokio::task::spawn_blocking(move || {
            if let Some(pid) = crate::process::get_pane_pid(&session) {
                crate::process::continue_process_tree(pid);
            }
        });
    }
}

/// Release primary if this client currently holds it.
async fn release_primary(primaries: &SessionPrimaries, session: &str, client_id: &str) {
    let mut map = primaries.write().await;
    if map.get(session).is_some_and(|id| id == client_id) {
        map.remove(session);
    }
}

/// Resize the PTY master to the given dimensions.
async fn resize_pty(
    master: &Arc<std::sync::Mutex<Box<dyn portable_pty::MasterPty + Send>>>,
    cols: u16,
    rows: u16,
) {
    let master = master.clone();
    let _ = tokio::task::spawn_blocking(move || {
        if let Ok(m) = master.lock() {
            let _ = m.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
    })
    .await;
}

#[derive(serde::Deserialize)]
#[serde(tag = "type")]
enum ControlMessage {
    #[serde(rename = "resize")]
    Resize { cols: u16, rows: u16 },
    /// Sent by the browser when the terminal tab/window gains focus.
    /// Claims primary and applies the pending resize so the pane
    /// snaps to this client's viewport without requiring a keystroke.
    #[serde(rename = "activate")]
    Activate,
    /// Pause the pane's foreground process (SIGSTOP). Sent by mobile
    /// web clients when the user enters tmux scrollback — without
    /// pausing, claude's continued output keeps shifting scrollback
    /// under the reader. Paired with `resume_output` on exit. The
    /// server also auto-resumes on WebSocket close to prevent a
    /// dropped connection from leaving the process permanently
    /// suspended.
    #[serde(rename = "pause_output")]
    PauseOutput,
    /// Resume the pane's foreground process (SIGCONT). Inverse of
    /// `pause_output`. SIGCONT to a non-stopped process is a no-op.
    #[serde(rename = "resume_output")]
    ResumeOutput,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_primaries() -> SessionPrimaries {
        Arc::new(RwLock::new(std::collections::HashMap::new()))
    }

    #[tokio::test]
    async fn claim_primary_vacant_returns_true() {
        let p = make_primaries();
        assert!(claim_primary(&p, "session-1", "ws-0").await);
        assert_eq!(p.read().await.get("session-1").unwrap(), "ws-0");
    }

    #[tokio::test]
    async fn claim_primary_already_primary_returns_false() {
        let p = make_primaries();
        assert!(claim_primary(&p, "session-1", "ws-0").await);
        assert!(!claim_primary(&p, "session-1", "ws-0").await);
    }

    #[tokio::test]
    async fn claim_primary_steals_from_other_client() {
        let p = make_primaries();
        assert!(claim_primary(&p, "session-1", "ws-0").await);
        assert!(claim_primary(&p, "session-1", "ws-1").await);
        assert_eq!(p.read().await.get("session-1").unwrap(), "ws-1");
    }

    #[tokio::test]
    async fn is_primary_or_vacant_when_vacant() {
        let p = make_primaries();
        assert!(is_primary_or_vacant(&p, "session-1", "ws-0").await);
    }

    #[tokio::test]
    async fn is_primary_or_vacant_when_primary() {
        let p = make_primaries();
        claim_primary(&p, "session-1", "ws-0").await;
        assert!(is_primary_or_vacant(&p, "session-1", "ws-0").await);
    }

    #[tokio::test]
    async fn is_primary_or_vacant_when_not_primary() {
        let p = make_primaries();
        claim_primary(&p, "session-1", "ws-0").await;
        assert!(!is_primary_or_vacant(&p, "session-1", "ws-1").await);
    }

    #[tokio::test]
    async fn release_primary_clears_entry() {
        let p = make_primaries();
        claim_primary(&p, "session-1", "ws-0").await;
        release_primary(&p, "session-1", "ws-0").await;
        assert!(p.read().await.get("session-1").is_none());
    }

    #[tokio::test]
    async fn release_primary_noop_for_different_client() {
        let p = make_primaries();
        claim_primary(&p, "session-1", "ws-0").await;
        release_primary(&p, "session-1", "ws-1").await;
        assert_eq!(p.read().await.get("session-1").unwrap(), "ws-0");
    }

    #[tokio::test]
    async fn release_primary_noop_on_empty_map() {
        let p = make_primaries();
        release_primary(&p, "session-1", "ws-0").await;
        assert!(p.read().await.is_empty());
    }

    #[tokio::test]
    async fn independent_sessions_dont_interfere() {
        let p = make_primaries();
        claim_primary(&p, "session-1", "ws-0").await;
        claim_primary(&p, "session-2", "ws-1").await;
        assert_eq!(p.read().await.get("session-1").unwrap(), "ws-0");
        assert_eq!(p.read().await.get("session-2").unwrap(), "ws-1");
    }
}
