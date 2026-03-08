//! Claude Code hooks management for status detection.
//!
//! AoE installs hooks into Claude Code's `settings.json` that write session
//! status (`running`/`waiting`/`idle`) to a file. This provides reliable
//! status detection without parsing tmux pane content.

mod status_file;

use std::path::Path;

use anyhow::Result;
use serde_json::Value;

pub use status_file::{cleanup_hook_status_dir, hook_status_dir, read_hook_status};

/// Base directory for all AoE hook status files.
pub(crate) const HOOK_STATUS_BASE: &str = "/tmp/aoe-hooks";

/// Marker substring used to identify AoE-managed hooks in settings.json.
/// Any hook command containing this string is considered ours.
const AOE_HOOK_MARKER: &str = "aoe-hooks";

/// Build the shell command for a hook that writes a status value.
fn hook_command(status: &str) -> String {
    format!(
        "sh -c '[ -n \"$AOE_INSTANCE_ID\" ] && mkdir -p /tmp/aoe-hooks/$AOE_INSTANCE_ID && printf {} > /tmp/aoe-hooks/$AOE_INSTANCE_ID/status'",
        status
    )
}

/// Build the complete AoE hooks JSON structure.
fn build_aoe_hooks() -> Value {
    // Event -> (status, matcher)
    // "idle" = Claude finished, at prompt. "waiting" = needs user action (approval, question).
    let events: &[(&str, &str, Option<&str>)] = &[
        ("PreToolUse", "running", None),
        ("UserPromptSubmit", "running", None),
        ("Stop", "idle", None),
        (
            "Notification",
            "waiting",
            Some("permission_prompt|elicitation_dialog"),
        ),
    ];

    let mut hooks_obj = serde_json::Map::new();
    for &(event, status, matcher) in events {
        let mut entry = serde_json::Map::new();
        if let Some(m) = matcher {
            entry.insert("matcher".to_string(), Value::String(m.to_string()));
        }
        entry.insert(
            "hooks".to_string(),
            Value::Array(vec![serde_json::json!({
                "type": "command",
                "command": hook_command(status)
            })]),
        );
        hooks_obj.insert(event.to_string(), Value::Array(vec![Value::Object(entry)]));
    }

    Value::Object(hooks_obj)
}

/// Remove any existing AoE hooks from an event's matcher array.
fn remove_aoe_entries(matchers: &mut Vec<Value>) {
    matchers.retain(|matcher| {
        let Some(hooks_arr) = matcher.get("hooks").and_then(|h| h.as_array()) else {
            return true;
        };
        // Keep the matcher group only if it has at least one non-AoE hook
        !hooks_arr.iter().all(|hook| {
            hook.get("command")
                .and_then(|c| c.as_str())
                .is_some_and(|cmd| cmd.contains(AOE_HOOK_MARKER))
        })
    });
}

/// Install AoE status hooks into a Claude Code `settings.json` file.
///
/// Merges AoE hook entries into the existing hooks configuration, preserving
/// any user-defined hooks. Existing AoE hooks are replaced (idempotent).
///
/// If the file doesn't exist, it will be created with just the hooks.
pub fn install_hooks(settings_path: &Path) -> Result<()> {
    let mut settings: Value = if settings_path.exists() {
        let content = std::fs::read_to_string(settings_path)?;
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    let aoe_hooks = build_aoe_hooks();

    // Ensure settings.hooks exists as an object
    if !settings.get("hooks").is_some_and(|h| h.is_object()) {
        settings
            .as_object_mut()
            .unwrap()
            .insert("hooks".to_string(), serde_json::json!({}));
    }

    let settings_hooks = settings.get_mut("hooks").unwrap().as_object_mut().unwrap();

    // Merge each event
    for (event_name, aoe_matchers) in aoe_hooks.as_object().unwrap() {
        if let Some(existing) = settings_hooks.get_mut(event_name) {
            if let Some(arr) = existing.as_array_mut() {
                // Remove old AoE entries, then append new ones
                remove_aoe_entries(arr);
                if let Some(new_arr) = aoe_matchers.as_array() {
                    arr.extend(new_arr.iter().cloned());
                }
            }
        } else {
            settings_hooks.insert(event_name.clone(), aoe_matchers.clone());
        }
    }

    // Write back, creating parent dirs if needed
    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let formatted = serde_json::to_string_pretty(&settings)?;
    std::fs::write(settings_path, formatted)?;

    tracing::info!("Installed AoE hooks in {}", settings_path.display());
    Ok(())
}

/// Remove all AoE hooks from a Claude Code `settings.json` file.
///
/// Strips AoE hook entries while preserving user-defined hooks. If an event
/// ends up with no matchers after removal, the event key is removed entirely.
/// If the hooks object becomes empty, the `hooks` key is removed from settings.
///
/// Returns `Ok(true)` if the file was modified, `Ok(false)` if no AoE hooks were found.
pub fn uninstall_hooks(settings_path: &Path) -> Result<bool> {
    if !settings_path.exists() {
        return Ok(false);
    }

    let content = std::fs::read_to_string(settings_path)?;
    let mut settings: Value =
        serde_json::from_str(&content).unwrap_or_else(|_| serde_json::json!({}));

    let Some(hooks_obj) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) else {
        return Ok(false);
    };

    let mut modified = false;
    let event_names: Vec<String> = hooks_obj.keys().cloned().collect();

    for event_name in event_names {
        if let Some(matchers) = hooks_obj
            .get_mut(&event_name)
            .and_then(|v| v.as_array_mut())
        {
            let before = matchers.len();
            remove_aoe_entries(matchers);
            if matchers.len() != before {
                modified = true;
            }
        }
    }

    if !modified {
        return Ok(false);
    }

    // Remove empty event arrays
    let empty_events: Vec<String> = hooks_obj
        .iter()
        .filter(|(_, v)| v.as_array().is_some_and(|a| a.is_empty()))
        .map(|(k, _)| k.clone())
        .collect();
    for key in empty_events {
        hooks_obj.remove(&key);
    }

    // Remove empty hooks object
    if hooks_obj.is_empty() {
        settings.as_object_mut().unwrap().remove("hooks");
    }

    let formatted = serde_json::to_string_pretty(&settings)?;
    std::fs::write(settings_path, formatted)?;

    tracing::info!("Removed AoE hooks from {}", settings_path.display());
    Ok(true)
}

/// Remove all AoE hooks from all known agent settings files and clean up
/// the hook status base directory. Called during `aoe uninstall`.
pub fn uninstall_all_hooks() {
    if let Some(home) = dirs::home_dir() {
        for agent in crate::agents::AGENTS {
            if let Some(hook_cfg) = &agent.hook_config {
                let settings_path = home.join(hook_cfg.settings_rel_path);
                match uninstall_hooks(&settings_path) {
                    Ok(true) => println!("Removed AoE hooks from {}", settings_path.display()),
                    Ok(false) => {}
                    Err(e) => {
                        tracing::warn!(
                            "Failed to remove hooks from {}: {}",
                            settings_path.display(),
                            e
                        );
                    }
                }
            }
        }
    }

    // Clean up the entire hook status base directory
    let base = std::path::Path::new(HOOK_STATUS_BASE);
    if base.exists() {
        if let Err(e) = std::fs::remove_dir_all(base) {
            tracing::warn!("Failed to remove {}: {}", base.display(), e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_install_hooks_creates_new_file() {
        let tmp = TempDir::new().unwrap();
        let settings_path = tmp.path().join(".claude").join("settings.json");

        install_hooks(&settings_path).unwrap();

        let content: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        let hooks = content.get("hooks").unwrap().as_object().unwrap();

        assert!(hooks.contains_key("PreToolUse"));
        assert!(hooks.contains_key("UserPromptSubmit"));
        assert!(hooks.contains_key("Stop"));
        assert!(hooks.contains_key("Notification"));
    }

    #[test]
    fn test_install_hooks_preserves_existing_user_hooks() {
        let tmp = TempDir::new().unwrap();
        let settings_path = tmp.path().join("settings.json");

        // Write existing settings with user hooks
        let existing = serde_json::json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [{"type": "command", "command": "echo user-hook"}]
                    }
                ]
            }
        });
        std::fs::write(
            &settings_path,
            serde_json::to_string_pretty(&existing).unwrap(),
        )
        .unwrap();

        install_hooks(&settings_path).unwrap();

        let content: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        let pre_tool = content["hooks"]["PreToolUse"].as_array().unwrap();

        // Should have both user hook and AoE hook
        assert_eq!(pre_tool.len(), 2);

        // User hook preserved
        let user_hook = &pre_tool[0];
        assert_eq!(user_hook["matcher"], "Bash");

        // AoE hook added
        let aoe_hook = &pre_tool[1];
        let cmd = aoe_hook["hooks"][0]["command"].as_str().unwrap();
        assert!(cmd.contains(AOE_HOOK_MARKER));
    }

    #[test]
    fn test_install_hooks_idempotent() {
        let tmp = TempDir::new().unwrap();
        let settings_path = tmp.path().join("settings.json");

        install_hooks(&settings_path).unwrap();
        install_hooks(&settings_path).unwrap();

        let content: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        let pre_tool = content["hooks"]["PreToolUse"].as_array().unwrap();

        // Should have exactly one AoE entry, not duplicates
        assert_eq!(pre_tool.len(), 1);
    }

    #[test]
    fn test_install_hooks_preserves_non_hook_settings() {
        let tmp = TempDir::new().unwrap();
        let settings_path = tmp.path().join("settings.json");

        let existing = serde_json::json!({
            "apiKey": "test-key",
            "model": "opus",
            "hooks": {}
        });
        std::fs::write(
            &settings_path,
            serde_json::to_string_pretty(&existing).unwrap(),
        )
        .unwrap();

        install_hooks(&settings_path).unwrap();

        let content: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(content["apiKey"], "test-key");
        assert_eq!(content["model"], "opus");
    }

    #[test]
    fn test_hook_command_format() {
        let cmd = hook_command("running");
        assert!(cmd.contains("aoe-hooks"));
        assert!(cmd.contains("AOE_INSTANCE_ID"));
        assert!(cmd.contains("running"));
    }

    #[test]
    fn test_notification_hook_has_matcher() {
        let hooks = build_aoe_hooks();
        let notification = hooks["Notification"].as_array().unwrap();
        assert_eq!(notification.len(), 1);
        let matcher = notification[0]["matcher"].as_str().unwrap();
        assert!(matcher.contains("permission_prompt"));
        assert!(matcher.contains("elicitation_dialog"));
        assert!(!matcher.contains("idle_prompt"));
    }

    #[test]
    fn test_stop_hook_writes_idle() {
        let hooks = build_aoe_hooks();
        let stop = hooks["Stop"].as_array().unwrap();
        let cmd = stop[0]["hooks"][0]["command"].as_str().unwrap();
        assert!(cmd.contains("idle"));
        assert!(!cmd.contains("waiting"));
    }

    #[test]
    fn test_hooks_are_synchronous() {
        let hooks = build_aoe_hooks();
        for (_, matchers) in hooks.as_object().unwrap() {
            for matcher in matchers.as_array().unwrap() {
                for hook in matcher["hooks"].as_array().unwrap() {
                    assert!(
                        hook.get("async").is_none(),
                        "Hooks should be synchronous (no async field): {:?}",
                        hook
                    );
                }
            }
        }
    }

    #[test]
    fn test_uninstall_hooks_removes_aoe_entries() {
        let tmp = TempDir::new().unwrap();
        let settings_path = tmp.path().join("settings.json");

        // Install hooks first
        install_hooks(&settings_path).unwrap();

        // Verify they're there
        let content: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert!(content.get("hooks").unwrap().as_object().unwrap().len() > 0);

        // Uninstall
        let modified = uninstall_hooks(&settings_path).unwrap();
        assert!(modified);

        // Hooks key should be removed (no user hooks remain)
        let content: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert!(content.get("hooks").is_none());
    }

    #[test]
    fn test_uninstall_hooks_preserves_user_hooks() {
        let tmp = TempDir::new().unwrap();
        let settings_path = tmp.path().join("settings.json");

        // Write settings with both user hooks and AoE hooks
        let existing = serde_json::json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [{"type": "command", "command": "echo user-hook"}]
                    }
                ]
            }
        });
        std::fs::write(
            &settings_path,
            serde_json::to_string_pretty(&existing).unwrap(),
        )
        .unwrap();

        install_hooks(&settings_path).unwrap();
        let modified = uninstall_hooks(&settings_path).unwrap();
        assert!(modified);

        let content: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        // PreToolUse should still exist with just the user hook
        let pre_tool = content["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre_tool.len(), 1);
        assert_eq!(pre_tool[0]["matcher"], "Bash");
        // Events that only had AoE hooks should be removed
        assert!(content["hooks"].get("Stop").is_none());
    }

    #[test]
    fn test_uninstall_hooks_nonexistent_file() {
        let tmp = TempDir::new().unwrap();
        let settings_path = tmp.path().join("nonexistent.json");
        let modified = uninstall_hooks(&settings_path).unwrap();
        assert!(!modified);
    }

    #[test]
    fn test_uninstall_hooks_no_aoe_hooks() {
        let tmp = TempDir::new().unwrap();
        let settings_path = tmp.path().join("settings.json");

        let existing = serde_json::json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [{"type": "command", "command": "echo user-hook"}]
                    }
                ]
            }
        });
        std::fs::write(
            &settings_path,
            serde_json::to_string_pretty(&existing).unwrap(),
        )
        .unwrap();

        let modified = uninstall_hooks(&settings_path).unwrap();
        assert!(!modified);
    }

    #[test]
    fn test_remove_aoe_entries_keeps_user_hooks() {
        let mut matchers = vec![
            serde_json::json!({
                "matcher": "Bash",
                "hooks": [{"type": "command", "command": "echo user"}]
            }),
            serde_json::json!({
                "hooks": [{"type": "command", "command": "sh -c 'aoe-hooks stuff'"}]
            }),
        ];

        remove_aoe_entries(&mut matchers);
        assert_eq!(matchers.len(), 1);
        assert_eq!(matchers[0]["matcher"], "Bash");
    }
}
