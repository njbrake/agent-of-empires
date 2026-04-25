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
use agent_of_empires::cockpit::approvals::ApprovalDecision;
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

    // The shim emits 4 ACP session/update notifications. With the typed
    // mapping in place these now arrive as: 2x AgentMessageChunk, 1x
    // ToolCallStarted, 1x ToolCallCompleted. Plus our Stopped marker
    // when the prompt round-trip completes.
    let agent_msg_count = events
        .iter()
        .filter(|e| matches!(e, Event::AgentMessageChunk { .. }))
        .count();
    let tool_started = events
        .iter()
        .filter(|e| matches!(e, Event::ToolCallStarted { .. }))
        .count();
    let tool_completed = events
        .iter()
        .filter(|e| matches!(e, Event::ToolCallCompleted { .. }))
        .count();
    let stopped_count = events
        .iter()
        .filter(|e| matches!(e, Event::Stopped { .. }))
        .count();

    let _ = client.shutdown().await;

    eprintln!(
        "smoke: collected {} events (agent_msg={}, tool_started={}, tool_completed={}, stopped={})",
        events.len(),
        agent_msg_count,
        tool_started,
        tool_completed,
        stopped_count,
    );
    for (i, event) in events.iter().enumerate() {
        eprintln!("  [{i}] {:?}", event);
    }

    assert!(
        agent_msg_count >= 2,
        "expected >= 2 AgentMessageChunk events, got {agent_msg_count}"
    );
    assert!(
        tool_started >= 1,
        "expected >= 1 ToolCallStarted event, got {tool_started}"
    );
    assert!(
        tool_completed >= 1,
        "expected >= 1 ToolCallCompleted event, got {tool_completed}"
    );
    assert!(
        stopped_count >= 1,
        "expected at least 1 Stopped event, got {stopped_count}"
    );

    // Verify the tool call name carries through the typed mapping.
    let tool_call_titles: Vec<&str> = events
        .iter()
        .filter_map(|e| match e {
            Event::ToolCallStarted { tool_call } => Some(tool_call.name.as_str()),
            _ => None,
        })
        .collect();
    assert!(
        tool_call_titles.iter().any(|t| t.contains("shim file")),
        "tool call title should be preserved through the mapping; got {tool_call_titles:?}"
    );
}

/// Permission round-trip: shim asks for permission, cockpit resolves
/// allow, agent observes the selected option_id and reports back.
#[tokio::test]
async fn shim_agent_round_trips_approval_allow() {
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

    let mut client = AcpClient::spawn(config, CockpitSessionId("approve".into()))
        .await
        .expect("spawn shim agent");

    client
        .send_prompt("REQUEST_PERMISSION please")
        .await
        .expect("send_prompt");

    // Auto-resolve the approval as soon as we observe the
    // ApprovalRequested event. Drain until Stopped.
    let mut events: Vec<Event> = Vec::new();
    let drain_deadline = std::time::Instant::now() + Duration::from_secs(15);
    while std::time::Instant::now() < drain_deadline {
        match tokio::time::timeout(Duration::from_millis(500), client.next_event()).await {
            Ok(Some(event)) => {
                if let Event::ApprovalRequested { approval } = &event {
                    let nonce = approval.nonce.clone();
                    let resolve_client = &client;
                    resolve_client
                        .resolve_permission(nonce, ApprovalDecision::Allow)
                        .await
                        .expect("resolve_permission");
                }
                let stopped = matches!(event, Event::Stopped { .. });
                events.push(event);
                if stopped {
                    break;
                }
            }
            Ok(None) | Err(_) => continue,
        }
    }

    let _ = client.shutdown().await;

    let saw_request = events
        .iter()
        .any(|e| matches!(e, Event::ApprovalRequested { .. }));
    let saw_resolved = events
        .iter()
        .any(|e| matches!(e, Event::ApprovalResolved { decision: ApprovalDecision::Allow, .. }));
    let saw_yes_outcome = events.iter().any(|e| match e {
        Event::AgentMessageChunk { text } => text.contains("permission_outcome=yes"),
        _ => false,
    });

    assert!(saw_request, "expected ApprovalRequested in events; got {events:?}");
    assert!(saw_resolved, "expected ApprovalResolved Allow in events; got {events:?}");
    assert!(
        saw_yes_outcome,
        "shim should have echoed permission_outcome=yes; got {events:?}"
    );
}
