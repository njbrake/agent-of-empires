//! End-to-end smoke test: Rust ACP client spawns a Node ACP shim agent,
//! sends a prompt, and observes structured events come back.
//!
//! Validates the cockpit's plumbing without any API keys: the shim agent
//! at `cockpit-worker/test-shim/shim.mjs` replays a scripted sequence of
//! `session/update` notifications.
//!
//! Skipped automatically if `node` is not on PATH (cockpit feature
//! requires Node anyway, so on a real cockpit-enabled build environment
//! this test runs).

#![cfg(feature = "cockpit")]

use std::time::Duration;

use agent_of_empires::cockpit::acp_client::{AcpClient, SpawnConfig};
use agent_of_empires::cockpit::agent_registry::AgentSpec;
use agent_of_empires::cockpit::state::{CockpitSessionId, Event};

fn node_available() -> bool {
    std::process::Command::new("node")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn shim_path() -> std::path::PathBuf {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    std::path::PathBuf::from(manifest)
        .join("cockpit-worker")
        .join("test-shim")
        .join("shim.mjs")
}

#[tokio::test]
async fn shim_agent_round_trips_prompt() {
    if !node_available() {
        eprintln!("skipping: node not on PATH");
        return;
    }
    let shim = shim_path();
    if !shim.exists() {
        eprintln!("skipping: shim missing at {}", shim.display());
        return;
    }

    let cwd = std::env::temp_dir();
    let config = SpawnConfig {
        spec: AgentSpec {
            command: "node".into(),
            args: vec![shim.to_string_lossy().to_string()],
            description: "test shim".into(),
            env_allowlist: None,
        },
        cwd,
        additional_dirs: vec![],
        provider_env: vec![],
    };

    let mut client = AcpClient::spawn(config, CockpitSessionId("smoke".into()))
        .await
        .expect("spawn shim agent");

    client
        .send_prompt("hello smoke")
        .await
        .expect("send_prompt");

    // Drain events with a generous timeout. The shim emits 4 session/update
    // notifications + we expect a Stopped event after the prompt completes.
    let mut events: Vec<Event> = Vec::new();
    let drain_deadline = std::time::Instant::now() + Duration::from_secs(15);
    while std::time::Instant::now() < drain_deadline {
        match tokio::time::timeout(Duration::from_millis(500), client.next_event()).await {
            Ok(Some(event)) => {
                let stopped = matches!(event, Event::Stopped { .. });
                events.push(event);
                if stopped {
                    break;
                }
            }
            Ok(None) | Err(_) => continue,
        }
    }

    // We expect at least the 4 RawAgentUpdate events from the shim plus the
    // Stopped event our client emits when the prompt round-trip completes.
    let raw_count = events
        .iter()
        .filter(|e| matches!(e, Event::RawAgentUpdate { .. }))
        .count();
    let stopped_count = events
        .iter()
        .filter(|e| matches!(e, Event::Stopped { .. }))
        .count();

    let _ = client.shutdown().await;

    eprintln!(
        "smoke: collected {} events ({} raw updates, {} stopped)",
        events.len(),
        raw_count,
        stopped_count,
    );
    for (i, event) in events.iter().enumerate() {
        eprintln!("  [{i}] {:?}", event);
    }

    assert!(
        raw_count >= 4,
        "expected >= 4 RawAgentUpdate events, got {raw_count}; events: {events:?}"
    );
    assert!(
        stopped_count >= 1,
        "expected at least 1 Stopped event, got {stopped_count}; events: {events:?}"
    );
}
