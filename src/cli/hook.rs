//! Hidden `_hook` subcommand invoked by Claude Code lifecycle hooks.
//!
//! Reads hook event JSON from stdin, maps it to a status, and writes
//! an atomic status file that the TUI status poller reads instead of
//! scraping tmux pane content.

use anyhow::Result;
use serde::Deserialize;
use std::io::Read;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Deserialize)]
struct HookInput {
    hook_event_name: String,
    #[serde(default)]
    notification_type: Option<String>,
}

fn map_event_to_status(input: &HookInput) -> Option<&'static str> {
    match input.hook_event_name.as_str() {
        "SessionStart" | "UserPromptSubmit" | "PreToolUse" | "PostToolUse" => Some("running"),
        "Stop" => Some("idle"),
        "Notification" => match input.notification_type.as_deref() {
            Some("permission_prompt") | Some("elicitation_dialog") => Some("waiting"),
            Some("idle_prompt") => Some("idle"),
            _ => None,
        },
        _ => None,
    }
}

/// Write a debug log line when `AOE_HOOK_DEBUG=1`.
///
/// Appends to `{app_dir}/hook_debug.log`. Best-effort: silently ignores
/// any I/O errors since this runs in a hook subprocess where tracing
/// isn't initialized.
fn debug_log(msg: &str) {
    if std::env::var("AOE_HOOK_DEBUG").as_deref() != Ok("1") {
        return;
    }
    let Ok(app_dir) = crate::session::get_app_dir() else {
        return;
    };
    let path = app_dir.join("hook_debug.log");
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let _ = writeln!(f, "[{}] {}", timestamp, msg);
    }
}

/// Extract the 8-char instance ID prefix from a tmux session name.
///
/// Session names follow the format `aoe_{title}_{id_prefix}` where id_prefix
/// is the first 8 chars of the 16-char instance ID. Terminal sessions
/// (`aoe_term_*`, `aoe_cterm_*`) are excluded.
fn extract_id_from_session_name(name: &str) -> Option<String> {
    if !name.starts_with(crate::tmux::SESSION_PREFIX) {
        return None;
    }
    // Exclude terminal sessions (aoe_term_*, aoe_cterm_*)
    if name.starts_with(crate::tmux::TERMINAL_PREFIX)
        || name.starts_with(crate::tmux::CONTAINER_TERMINAL_PREFIX)
    {
        return None;
    }
    // The last segment after '_' is the 8-char ID prefix
    let id_prefix = name.rsplit('_').next()?;
    if id_prefix.len() == 8 && id_prefix.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(id_prefix.to_string())
    } else {
        None
    }
}

/// Detect the instance ID by querying the current tmux session name.
///
/// Uses `TMUX_PANE` with `-t` targeting to resolve the correct session even
/// when multiple aoe sessions are running. Returns `None` if `TMUX_PANE` is
/// unavailable (e.g. Claude Code running outside tmux).
fn detect_instance_id_from_tmux() -> Option<String> {
    let name = get_pane_session_name()?;
    extract_id_from_session_name(&name)
}

/// Get the session name for the pane this process is running in.
///
/// `TMUX_PANE` (e.g. `%36`) is set by tmux for every pane process and
/// inherited by all children. Using it with `-t` avoids the ambiguity
/// of untargeted `display-message`, which resolves to the most recently
/// active client rather than the calling process's pane.
fn get_pane_session_name() -> Option<String> {
    let pane_id = std::env::var("TMUX_PANE").ok()?;
    debug_log(&format!("TMUX_PANE={}", pane_id));
    let output = std::process::Command::new("tmux")
        .args(["display-message", "-t", &pane_id, "-p", "#{session_name}"])
        .output()
        .ok()?;
    if output.status.success() {
        let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !name.is_empty() {
            debug_log(&format!("pane session resolved: {}", name));
            return Some(name);
        }
    }
    debug_log("pane session resolution failed");
    None
}

pub fn run() -> Result<()> {
    debug_log(&format!(
        "hook invoked | TMUX_PANE={:?} | AOE_INSTANCE_ID={:?} | TMUX={:?}",
        std::env::var("TMUX_PANE").ok(),
        std::env::var("AOE_INSTANCE_ID").ok(),
        std::env::var("TMUX").ok(),
    ));
    let instance_id = match std::env::var("AOE_INSTANCE_ID") {
        Ok(id) if !id.is_empty() => {
            debug_log(&format!("AOE_INSTANCE_ID={}", id));
            id
        }
        _ => {
            debug_log("AOE_INSTANCE_ID not set, detecting from tmux");
            match detect_instance_id_from_tmux() {
                Some(id) => {
                    debug_log(&format!("detected instance_id={}", id));
                    id
                }
                None => {
                    debug_log("no instance_id found, exiting");
                    return Ok(());
                }
            }
        }
    };

    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;

    let hook_input: HookInput = serde_json::from_str(&input)?;

    let app_dir = crate::session::get_app_dir()?;
    let hook_dir = app_dir.join("hook_status");
    let status_path = hook_dir.join(&instance_id);

    if hook_input.hook_event_name == "SessionEnd" {
        debug_log(&format!("SessionEnd: removing {}", status_path.display()));
        let _ = std::fs::remove_file(&status_path);
        return Ok(());
    }

    let status = map_event_to_status(&hook_input);
    debug_log(&format!(
        "event={} -> status={:?}",
        hook_input.hook_event_name, status
    ));

    if let Some(status) = status {
        std::fs::create_dir_all(&hook_dir)?;

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let tmp_path = hook_dir.join(format!(".{}.tmp", instance_id));
        std::fs::write(&tmp_path, format!("{} {}", status, timestamp))?;
        std::fs::rename(&tmp_path, &status_path)?;
        debug_log(&format!("wrote {} to {}", status, status_path.display()));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_session_start() {
        let input = HookInput {
            hook_event_name: "SessionStart".to_string(),
            notification_type: None,
        };
        assert_eq!(map_event_to_status(&input), Some("running"));
    }

    #[test]
    fn test_map_user_prompt_submit() {
        let input = HookInput {
            hook_event_name: "UserPromptSubmit".to_string(),
            notification_type: None,
        };
        assert_eq!(map_event_to_status(&input), Some("running"));
    }

    #[test]
    fn test_map_pre_tool_use() {
        let input = HookInput {
            hook_event_name: "PreToolUse".to_string(),
            notification_type: None,
        };
        assert_eq!(map_event_to_status(&input), Some("running"));
    }

    #[test]
    fn test_map_post_tool_use() {
        let input = HookInput {
            hook_event_name: "PostToolUse".to_string(),
            notification_type: None,
        };
        assert_eq!(map_event_to_status(&input), Some("running"));
    }

    #[test]
    fn test_map_stop() {
        let input = HookInput {
            hook_event_name: "Stop".to_string(),
            notification_type: None,
        };
        assert_eq!(map_event_to_status(&input), Some("idle"));
    }

    #[test]
    fn test_map_notification_permission() {
        let input = HookInput {
            hook_event_name: "Notification".to_string(),
            notification_type: Some("permission_prompt".to_string()),
        };
        assert_eq!(map_event_to_status(&input), Some("waiting"));
    }

    #[test]
    fn test_map_notification_elicitation() {
        let input = HookInput {
            hook_event_name: "Notification".to_string(),
            notification_type: Some("elicitation_dialog".to_string()),
        };
        assert_eq!(map_event_to_status(&input), Some("waiting"));
    }

    #[test]
    fn test_map_notification_idle() {
        let input = HookInput {
            hook_event_name: "Notification".to_string(),
            notification_type: Some("idle_prompt".to_string()),
        };
        assert_eq!(map_event_to_status(&input), Some("idle"));
    }

    #[test]
    fn test_map_unknown_notification() {
        let input = HookInput {
            hook_event_name: "Notification".to_string(),
            notification_type: Some("something_else".to_string()),
        };
        assert_eq!(map_event_to_status(&input), None);
    }

    #[test]
    fn test_map_session_end() {
        let input = HookInput {
            hook_event_name: "SessionEnd".to_string(),
            notification_type: None,
        };
        // SessionEnd is handled separately (file deletion), not mapped to a status
        assert_eq!(map_event_to_status(&input), None);
    }

    #[test]
    fn test_map_unknown_event() {
        let input = HookInput {
            hook_event_name: "SomeFutureEvent".to_string(),
            notification_type: None,
        };
        assert_eq!(map_event_to_status(&input), None);
    }

    #[test]
    fn test_extract_id_from_agent_session() {
        assert_eq!(
            extract_id_from_session_name("aoe_my_project_a1b2c3d4"),
            Some("a1b2c3d4".to_string())
        );
    }

    #[test]
    fn test_extract_id_from_session_with_underscores_in_title() {
        assert_eq!(
            extract_id_from_session_name("aoe_my_cool_project_deadbeef"),
            Some("deadbeef".to_string())
        );
    }

    #[test]
    fn test_extract_id_rejects_terminal_session() {
        assert_eq!(extract_id_from_session_name("aoe_term_proj_a1b2c3d4"), None);
    }

    #[test]
    fn test_extract_id_rejects_container_terminal() {
        assert_eq!(
            extract_id_from_session_name("aoe_cterm_proj_a1b2c3d4"),
            None
        );
    }

    #[test]
    fn test_extract_id_rejects_non_aoe_session() {
        assert_eq!(extract_id_from_session_name("my_session"), None);
    }

    #[test]
    fn test_extract_id_rejects_wrong_length_suffix() {
        // 7 chars - too short
        assert_eq!(extract_id_from_session_name("aoe_proj_a1b2c3d"), None);
        // 9 chars - too long
        assert_eq!(extract_id_from_session_name("aoe_proj_a1b2c3d4e"), None);
    }

    #[test]
    fn test_extract_id_rejects_non_hex_suffix() {
        assert_eq!(extract_id_from_session_name("aoe_proj_ghijklmn"), None);
    }
}
