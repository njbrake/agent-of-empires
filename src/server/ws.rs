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

use super::AppState;

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

    match session_info {
        Some(tmux_name) => ws
            .on_upgrade(move |socket| handle_terminal_ws(socket, tmux_name, read_only))
            .into_response(),
        None => (axum::http::StatusCode::NOT_FOUND, "Session not found").into_response(),
    }
}

async fn handle_terminal_ws(socket: WebSocket, tmux_name: String, read_only: bool) {
    use futures_util::{SinkExt, StreamExt};

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

    // Task 2: channel -> WebSocket sender
    let send_handle = tokio::spawn(async move {
        while let Some(data) = output_rx.recv().await {
            if ws_sender.send(Message::Binary(data.into())).await.is_err() {
                break; // WebSocket closed
            }
        }
        let _ = ws_sender.send(Message::Close(None)).await;
    });

    // Task 3: WebSocket receiver -> PTY stdin (and resize)
    let writer_for_input = writer.clone();
    let master_for_resize = master.clone();

    let recv_handle = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_receiver.next().await {
            match msg {
                Message::Binary(data) => {
                    // Raw bytes from xterm.js -> PTY stdin (blocked in read-only mode)
                    if read_only {
                        continue;
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
                    // JSON control messages (resize) are always allowed
                    if let Ok(control) = serde_json::from_str::<ControlMessage>(&text) {
                        match control {
                            ControlMessage::Resize { cols, rows } => {
                                let master = master_for_resize.clone();
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
                        }
                    } else if !read_only {
                        // Plain text input -> PTY stdin (blocked in read-only mode)
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

    // Clean up: kill the tmux attach process
    let _ = child.kill();
    let _ = child.wait();
}

#[derive(serde::Deserialize)]
#[serde(tag = "type")]
enum ControlMessage {
    #[serde(rename = "resize")]
    Resize { cols: u16, rows: u16 },
}
