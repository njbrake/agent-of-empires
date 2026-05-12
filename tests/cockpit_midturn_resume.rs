//! Mid-turn `aoe serve` reattach: integration coverage for the daemon
//! side of the fix. Stands up a UNIX socket fronted by a byte-proxy to a
//! Node ACP shim (so we exercise the real `AcpClient::attach` →
//! `connect_via_socket` → ACP `initialize` path) and asserts:
//!
//! 1. `attach` with `in_flight_turn = true` synthesizes
//!    `Event::Stopped { reason: "reattach_idle" }` after the configured
//!    grace, since the orphaned upstream `session/prompt` response has
//!    no daemon-side request id to land against.
//!
//! 2. `attach` with `in_flight_turn = false` does NOT synthesize one —
//!    the watchdog must stay disarmed when the session was idle.
//!
//! Skipped automatically if `node` is not on PATH.

#![cfg(feature = "serve")]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use agent_of_empires::cockpit::acp_client::AcpClient;
use agent_of_empires::cockpit::state::{CockpitSessionId, Event};
use tokio::net::UnixListener;
use tokio::process::Command;

fn node_available() -> bool {
    std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn shim_path() -> PathBuf {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    PathBuf::from(manifest)
        .join("cockpit-worker")
        .join("test-shim")
        .join("shim.mjs")
}

/// Spawn the shim and bridge its stdio to a UNIX listener. Mimics what
/// `aoe __cockpit-runner` does in production: byte-proxy, no protocol
/// awareness. Accepts exactly one connection per call so we don't have
/// to coordinate listener lifetime with the test's drain logic.
///
/// Returns the listener path; the bridge task is detached.
async fn spawn_shim_socket_bridge() -> (PathBuf, tempfile::TempDir) {
    let shim = shim_path();
    let temp = tempfile::tempdir().unwrap();
    let socket_path = temp.path().join("runner.sock");

    let mut shim_proc = Command::new("node")
        .arg(&shim)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .expect("spawn shim");
    let shim_stdin = shim_proc.stdin.take().expect("shim stdin");
    let shim_stdout = shim_proc.stdout.take().expect("shim stdout");

    let listener = UnixListener::bind(&socket_path).expect("bind listener");

    tokio::spawn(async move {
        // Single accept — the test only attaches once. After the first
        // connection closes we stop accepting; the shim process is then
        // dropped via kill_on_drop when this task ends.
        let _shim_proc = shim_proc;
        let (stream, _) = match listener.accept().await {
            Ok(pair) => pair,
            Err(_) => return,
        };
        let (mut sock_read, mut sock_write) = stream.into_split();
        let mut shim_in = shim_stdin;
        let mut shim_out = shim_stdout;
        let to_shim = async move { tokio::io::copy(&mut sock_read, &mut shim_in).await.ok() };
        let from_shim = async move { tokio::io::copy(&mut shim_out, &mut sock_write).await.ok() };
        let _ = tokio::join!(to_shim, from_shim);
    });

    (socket_path, temp)
}

async fn drain_for_stopped_reason(client: &mut AcpClient, deadline: Instant) -> Option<String> {
    while Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(200), client.next_event()).await {
            Ok(Some(Event::Stopped { reason })) => return Some(reason),
            Ok(Some(_)) => continue,
            Ok(None) => return None,
            Err(_) => continue,
        }
    }
    None
}

#[tokio::test]
async fn attach_in_flight_synthesizes_reattach_idle_stopped() {
    if !node_available() {
        eprintln!("skipping: node not on PATH");
        return;
    }
    if !shim_path().exists() {
        eprintln!("skipping: shim missing");
        return;
    }

    // Shorten the watchdog grace so the test completes inside ~3s
    // instead of the 10s production default.
    std::env::set_var("AOE_RESUME_IDLE_GRACE_MS", "500");

    let (socket_path, _tmp) = spawn_shim_socket_bridge().await;

    let mut client = AcpClient::attach(
        socket_path,
        std::env::temp_dir(),
        vec![],
        "test-acp-session-id".into(),
        true, // in_flight_turn
        CockpitSessionId("midturn-true".into()),
    )
    .await
    .expect("attach in_flight=true");

    let stopped =
        drain_for_stopped_reason(&mut client, Instant::now() + Duration::from_secs(3)).await;
    let _ = client.shutdown().await;

    assert_eq!(
        stopped.as_deref(),
        Some("reattach_idle"),
        "resume-idle watchdog must synthesize a Stopped event"
    );
}

#[tokio::test]
async fn attach_idle_session_does_not_synthesize_stopped() {
    if !node_available() {
        eprintln!("skipping: node not on PATH");
        return;
    }
    if !shim_path().exists() {
        eprintln!("skipping: shim missing");
        return;
    }

    std::env::set_var("AOE_RESUME_IDLE_GRACE_MS", "500");

    let (socket_path, _tmp) = spawn_shim_socket_bridge().await;

    let mut client = AcpClient::attach(
        socket_path,
        std::env::temp_dir(),
        vec![],
        "test-acp-session-id".into(),
        false, // NOT in flight
        CockpitSessionId("midturn-false".into()),
    )
    .await
    .expect("attach in_flight=false");

    let stopped =
        drain_for_stopped_reason(&mut client, Instant::now() + Duration::from_secs(2)).await;
    let _ = client.shutdown().await;

    assert!(
        stopped.is_none(),
        "watchdog must stay disarmed when in_flight_turn=false; got Stopped reason={stopped:?}"
    );
}
