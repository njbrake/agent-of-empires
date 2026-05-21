//! Daemon-side coverage for the silent-orphan watchdog (#1240). Stands
//! up the existing Node test shim over a UNIX socket bridge, sends a
//! `SILENT_ORPHAN` prompt that streams a chunk + cost-populated
//! `usage_update` then parks (the upstream
//! `agentclientprotocol/claude-agent-acp#688` failure mode), and
//! asserts the daemon synthesizes `Stopped { reason: "prompt_orphaned" }`
//! within the test grace window.
//!
//! Three cases:
//!   1. positive: cost-populated usage_update + silence → orphan fires.
//!   2. negative (tool open): a long-running tool keeps
//!      `tool_calls_in_flight` non-empty → orphan must NOT fire.
//!   3. disabled (grace = 0): watchdog skipped entirely → no orphan.
//!
//! Skipped automatically if `node` is missing.
//!
//! Note: the parent `main.rs` only compiles this module under
//! `cfg(all(feature = "serve", debug_assertions))`. Debug-only because
//! the watchdog grace is tunable via `AOE_SILENT_ORPHAN_GRACE_MS` /
//! `AOE_SILENT_ORPHAN_FAST_GRACE_MS` only under `cfg(debug_assertions)`;
//! release builds would wait the full 60s production default.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use agent_of_empires::cockpit::acp_client::AcpClient;
use agent_of_empires::cockpit::state::{CockpitSessionId, Event};
use serial_test::serial;
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

async fn spawn_shim_socket_bridge_with_preseed(
    preseed_session_id: &str,
) -> (PathBuf, tempfile::TempDir) {
    let shim = shim_path();
    let temp = tempfile::tempdir().unwrap();
    let socket_path = temp.path().join("runner.sock");

    let mut cmd = Command::new("node");
    cmd.arg(&shim);
    cmd.env("SHIM_PRESEED_SESSION_ID", preseed_session_id);
    let mut shim_proc = cmd
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
#[serial]
async fn silent_orphan_fires_on_cost_then_silence() {
    if !node_available() || !shim_path().exists() {
        eprintln!("skipping: node or shim missing");
        return;
    }

    // Tight grace so the test completes inside a couple of seconds.
    // The fast path is the one that should fire because the shim emits
    // a cost-populated usage_update before parking. Polling cadence
    // dropped to 50ms so the watchdog evaluation tracks the configured
    // grace closely instead of waiting up to the default 5s tick.
    std::env::set_var("AOE_SILENT_ORPHAN_GRACE_MS", "5000");
    std::env::set_var("AOE_SILENT_ORPHAN_FAST_GRACE_MS", "300");
    std::env::set_var("AOE_SILENT_ORPHAN_CHECK_INTERVAL_MS", "50");

    let preseed = "silent-orphan-positive";
    let (socket_path, _tmp) = spawn_shim_socket_bridge_with_preseed(preseed).await;

    let client = AcpClient::attach(
        socket_path,
        std::env::temp_dir(),
        vec![],
        preseed.to_string(),
        false,
        CockpitSessionId("silent-orphan-positive".into()),
        None,
        "claude".into(),
        None,
    )
    .await
    .expect("attach for silent-orphan positive test");

    let mut client = client;
    client
        .send_prompt("SILENT_ORPHAN trigger")
        .await
        .expect("send prompt");

    let stopped =
        drain_for_stopped_reason(&mut client, Instant::now() + Duration::from_secs(5)).await;
    let _ = client.shutdown().await;

    assert_eq!(
        stopped.as_deref(),
        Some("prompt_orphaned"),
        "silent-orphan watchdog must synthesize Stopped {{ reason: prompt_orphaned }} when the adapter parks after cost-populated UsageUpdate"
    );
}

#[tokio::test]
#[serial]
async fn silent_orphan_suppressed_during_normal_turn() {
    if !node_available() || !shim_path().exists() {
        eprintln!("skipping: node or shim missing");
        return;
    }

    // Generous enough grace that the shim's healthy tool round-trip
    // completes long before the watchdog could fire; we then assert
    // the only Stopped we see is prompt_complete, not prompt_orphaned.
    // Tight polling cadence so a regressed grace would fire within the
    // assertion window instead of waiting for the default 5s tick.
    std::env::set_var("AOE_SILENT_ORPHAN_GRACE_MS", "10000");
    std::env::set_var("AOE_SILENT_ORPHAN_FAST_GRACE_MS", "10000");
    std::env::set_var("AOE_SILENT_ORPHAN_CHECK_INTERVAL_MS", "50");

    let preseed = "silent-orphan-negative";
    let (socket_path, _tmp) = spawn_shim_socket_bridge_with_preseed(preseed).await;

    let client = AcpClient::attach(
        socket_path,
        std::env::temp_dir(),
        vec![],
        preseed.to_string(),
        false,
        CockpitSessionId("silent-orphan-negative".into()),
        None,
        "claude".into(),
        None,
    )
    .await
    .expect("attach for silent-orphan negative test");

    let mut client = client;
    // No SILENT_ORPHAN keyword: the shim's default prompt() runs the
    // healthy chunk + tool_call + tool_call_update + chunk sequence
    // and returns stopReason=end_turn. The watchdog must stay silent
    // and the natural prompt_complete must win.
    client
        .send_prompt("normal turn")
        .await
        .expect("send prompt");

    let stopped =
        drain_for_stopped_reason(&mut client, Instant::now() + Duration::from_secs(5)).await;
    let _ = client.shutdown().await;

    assert_eq!(
        stopped.as_deref(),
        Some("prompt_complete"),
        "silent-orphan watchdog must stay disarmed on a normal turn; saw {stopped:?}"
    );
}

#[tokio::test]
#[serial]
async fn silent_orphan_disabled_by_zero_grace() {
    if !node_available() || !shim_path().exists() {
        eprintln!("skipping: node or shim missing");
        return;
    }

    // `0` disables the watchdog entirely. With the shim parked on
    // SILENT_ORPHAN we'd otherwise see prompt_orphaned within a few
    // hundred milliseconds; instead we should see no Stopped frame at
    // all within the deadline, because nothing else fires.
    //
    // Override the polling cadence too: the default 5s tick would let
    // a regressed "disabled" knob slip past a 2s deadline simply
    // because the watchdog hadn't ticked yet. Forcing a 50ms cadence
    // means a wrongly-armed watchdog WOULD fire within the deadline,
    // turning a silent assertion into a real one.
    std::env::set_var("AOE_SILENT_ORPHAN_GRACE_MS", "0");
    std::env::set_var("AOE_SILENT_ORPHAN_FAST_GRACE_MS", "200");
    std::env::set_var("AOE_SILENT_ORPHAN_CHECK_INTERVAL_MS", "50");

    let preseed = "silent-orphan-disabled";
    let (socket_path, _tmp) = spawn_shim_socket_bridge_with_preseed(preseed).await;

    let client = AcpClient::attach(
        socket_path,
        std::env::temp_dir(),
        vec![],
        preseed.to_string(),
        false,
        CockpitSessionId("silent-orphan-disabled".into()),
        None,
        "claude".into(),
        None,
    )
    .await
    .expect("attach for silent-orphan disabled test");

    let mut client = client;
    client
        .send_prompt("SILENT_ORPHAN trigger")
        .await
        .expect("send prompt");

    let stopped =
        drain_for_stopped_reason(&mut client, Instant::now() + Duration::from_secs(2)).await;
    let _ = client.shutdown().await;

    assert!(
        stopped.is_none(),
        "silent-orphan watchdog must stay fully disarmed when grace = 0; saw Stopped reason={stopped:?}"
    );
}
