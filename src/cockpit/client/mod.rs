//! Cockpit daemon client.
//!
//! HTTP + WebSocket client for talking to an `aoe serve` daemon. Used
//! by:
//!
//! - The `aoe cockpit *` CLI verbs (history, status, prompt, approve,
//!   cancel, tail, attach).
//! - The TUI cockpit view (`src/tui/cockpit_view/`).
//!
//! All three layers share `DaemonEndpoint` discovery, the typed
//! `HttpClient`, and the typed `WsHandle` so a change to the wire
//! shape breaks every consumer at compile time, not at runtime.
//!
//! Discovery resolution order:
//!
//! 1. `AOE_DAEMON_URL` (+ optional `AOE_DAEMON_TOKEN`).
//! 2. Local `<app_dir>/serve.url` paired with a live `serve.pid`.
//!
//! [`daemon_manager::ensure_daemon`] adds an auto-spawn fallback that
//! starts a fresh loopback daemon when neither resolves; the spawned
//! daemon is long-lived and survives the spawning process so the
//! maintainer's "create in TUI, drive from road via web" flow works.

pub mod daemon_manager;
pub mod discovery;
pub mod http;
pub mod ws;

pub use daemon_manager::{ensure_daemon, ManagerError};
pub use discovery::{discover, DaemonEndpoint, DiscoveryError, Source};
pub use http::{HttpClient, HttpError};
pub use ws::{connect as ws_connect, WsError, WsHandle, WsMessage};
