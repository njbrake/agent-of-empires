//! Hook handler for agent hook events.
//!
//! This command is called by agents (e.g. Claude Code) for every hook event.
//! It reads the event JSON from stdin, extracts the session_id and status,
//! and writes them to sidecar files in /tmp/aoe-hooks/{instance_id}/.
//!
//! PERFORMANCE CRITICAL: This runs on every agent tool call. No Storage,
//! no migrations, no profile resolution. Just stdin -> parse -> write files.

use std::io::Read;

use anyhow::Result;

use crate::hooks::hook_status_dir;
use crate::session::capture::validated_session_id;

/// Look up the AoE status for a hook event name across all agents' registered events.
fn event_to_status(event: &str) -> Option<&'static str> {
    crate::agents::AGENTS
        .iter()
        .filter_map(|a| a.hook_config.as_ref())
        .flat_map(|cfg| cfg.events.iter())
        .find(|e| e.name == event)
        .map(|e| e.status)
}

pub fn run() -> Result<()> {
    let instance_id = match std::env::var("AOE_INSTANCE_ID") {
        Ok(id) if !id.is_empty() => id,
        _ => return Ok(()),
    };

    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap_or(0);

    // serde_json::Value (not a strict struct) because Claude Code payloads evolve
    let payload: serde_json::Value = match serde_json::from_str(&input) {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };

    let event = payload
        .get("hook_event_name")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let session_id = payload.get("session_id").and_then(|v| v.as_str());

    let status = match event_to_status(event) {
        Some(s) => s,
        None => return Ok(()),
    };

    let dir = hook_status_dir(&instance_id);
    if std::fs::create_dir_all(&dir).is_err() {
        return Ok(());
    }

    let _ = std::fs::write(dir.join("status"), status);

    if let Some(sid) = session_id {
        let sid = sid.trim().to_string();
        if !sid.is_empty() {
            if let Some(valid_sid) = validated_session_id(sid) {
                let _ = std::fs::write(dir.join("session_id"), valid_sid);
            }
        }
    }

    // SessionEnd clears the session_id since the session is over
    if event == "SessionEnd" {
        let _ = std::fs::remove_file(dir.join("session_id"));
    }

    // CRITICAL: No stdout. Claude Code injects SessionStart hook stdout into its context window.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_to_status_running() {
        assert_eq!(event_to_status("PreToolUse"), Some("running"));
        assert_eq!(event_to_status("UserPromptSubmit"), Some("running"));
        assert_eq!(event_to_status("SessionStart"), Some("running"));
    }

    #[test]
    fn test_event_to_status_idle() {
        assert_eq!(event_to_status("Stop"), Some("idle"));
        assert_eq!(event_to_status("SessionEnd"), Some("idle"));
    }

    #[test]
    fn test_event_to_status_waiting() {
        assert_eq!(event_to_status("Notification"), Some("waiting"));
    }

    #[test]
    fn test_event_to_status_unknown() {
        assert_eq!(event_to_status("SomeNewEvent"), None);
        assert_eq!(event_to_status(""), None);
    }
}
