//! Hook handler for agent hook events.
//!
//! This command is called by agents (e.g. Claude Code, Gemini CLI) for every hook event.
//! It reads the event JSON from stdin, extracts the status,
//! and writes it to a sidecar file in /tmp/aoe-hooks/{instance_id}/.
//!
//! PERFORMANCE CRITICAL: This runs on every agent tool call. No Storage,
//! no migrations, no profile resolution. Just stdin -> parse -> write files.

use std::io::Read;
use std::path::Path;

use anyhow::Result;

use crate::hooks::hook_status_dir;

fn atomic_write(path: &Path, contents: &str) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("tmp.{}", std::process::id()));
    let result = std::fs::write(&tmp, contents).and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Look up the registered hook event across all agents.
/// Returns `Some(Some(status))` for status-changing events,
/// `Some(None)` for lifecycle-only events, and `None` for unknown events.
fn find_event(event: &str) -> Option<Option<&'static str>> {
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

    let maybe_status = match find_event(event) {
        Some(s) => s,
        None => {
            tracing::debug!("Ignoring unrecognised hook event: {:?}", event);
            return Ok(());
        }
    };

    let dir = hook_status_dir(&instance_id);
    if std::fs::create_dir_all(&dir).is_err() {
        return Ok(());
    }

    if let Some(status) = maybe_status {
        let _ = atomic_write(&dir.join("status"), status);
    }

    // CRITICAL: No stdout. Claude Code injects SessionStart hook stdout into its context window.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_events_running() {
        assert_eq!(find_event("PreToolUse"), Some(Some("running")));
        assert_eq!(find_event("UserPromptSubmit"), Some(Some("running")));
    }

    #[test]
    fn test_status_events_idle() {
        assert_eq!(find_event("Stop"), Some(Some("idle")));
    }

    #[test]
    fn test_status_events_waiting() {
        assert_eq!(find_event("Notification"), Some(Some("waiting")));
    }

    #[test]
    fn test_gemini_status_events() {
        assert_eq!(find_event("BeforeTool"), Some(Some("running")));
        assert_eq!(find_event("BeforeAgent"), Some(Some("running")));
        assert_eq!(find_event("AfterAgent"), Some(Some("idle")));
    }

    #[test]
    fn test_unknown_event() {
        assert_eq!(find_event("SomeNewEvent"), None);
        assert_eq!(find_event(""), None);
    }
}
