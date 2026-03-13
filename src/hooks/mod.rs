//! Claude Code hooks management for status detection.
//!
//! AoE installs hooks into Claude Code's `settings.json` that write session
//! status (`running`/`waiting`/`idle`) to a file. This provides reliable
//! status detection without parsing tmux pane content.

mod status_file;

use std::path::Path;

use anyhow::Result;
use serde_json::Value;

pub use status_file::{
    cleanup_hook_status_dir, hook_status_dir, read_hook_session_id, read_hook_status,
};

/// Base directory for all AoE hook status files.
pub(crate) const HOOK_STATUS_BASE: &str = "/tmp/aoe-hooks";

/// Marker substrings that identify AoE-managed hooks in settings.json.
/// Any hook command containing ANY of these strings is considered ours.
/// The first entry is the legacy shell one-liner marker (path-based).
/// The second entry matches the binary hook-handler command format.
const AOE_HOOK_MARKERS: &[&str] = &["aoe-hooks", "hook-handler"];

/// Resolve the absolute canonicalized path to the running `aoe` binary.
///
/// Uses `std::env::current_exe()` to get the binary path and `std::fs::canonicalize()`
/// to resolve symlinks and relative components. Returns `None` if resolution fails.
pub(crate) fn resolve_aoe_binary_path() -> Option<String> {
    std::env::current_exe()
        .ok()
        .and_then(|path| std::fs::canonicalize(&path).ok())
        .and_then(|path| path.to_str().map(|s| s.to_string()))
}

/// Check if a command string is an AoE-managed hook.
fn is_aoe_hook_command(cmd: &str) -> bool {
    AOE_HOOK_MARKERS.iter().any(|marker| cmd.contains(marker))
}

/// Build the command string that invokes the `aoe hook-handler` binary.
///
/// Resolves the absolute path to the running binary so hooks work regardless
/// of `$PATH`. Falls back to bare `"aoe"` if resolution fails.
fn aoe_hook_command() -> String {
    let binary = resolve_aoe_binary_path().unwrap_or_else(|| "aoe".to_string());
    format!("{binary} hook-handler")
}

/// Build the complete AoE hooks JSON structure.
fn build_aoe_hooks() -> Value {
    let command = aoe_hook_command();

    let events: &[(&str, Option<&str>)] = &[
        ("PreToolUse", None),
        ("UserPromptSubmit", None),
        ("Stop", None),
        ("Notification", Some("permission_prompt|elicitation_dialog")),
        ("SessionStart", None),
        ("SessionEnd", None),
    ];

    let mut hooks_obj = serde_json::Map::new();
    for &(event, matcher) in events {
        let mut entry = serde_json::Map::new();
        if let Some(m) = matcher {
            entry.insert("matcher".to_string(), Value::String(m.to_string()));
        }
        entry.insert(
            "hooks".to_string(),
            Value::Array(vec![serde_json::json!({
                "type": "command",
                "command": command
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
                .is_some_and(is_aoe_hook_command)
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
        serde_json::from_str(&content).unwrap_or_else(|e| {
            tracing::warn!("Failed to parse {}: {}", settings_path.display(), e);
            serde_json::json!({})
        })
    } else {
        serde_json::json!({})
    };

    let aoe_hooks = build_aoe_hooks();

    if !settings.get("hooks").is_some_and(|h| h.is_object()) {
        settings
            .as_object_mut()
            .unwrap()
            .insert("hooks".to_string(), serde_json::json!({}));
    }

    let settings_hooks = settings.get_mut("hooks").unwrap().as_object_mut().unwrap();

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
    let mut settings: Value = serde_json::from_str(&content).unwrap_or_else(|e| {
        tracing::warn!("Failed to parse {}: {}", settings_path.display(), e);
        serde_json::json!({})
    });

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

    let empty_events: Vec<String> = hooks_obj
        .iter()
        .filter(|(_, v)| v.as_array().is_some_and(|a| a.is_empty()))
        .map(|(k, _)| k.clone())
        .collect();
    for key in empty_events {
        hooks_obj.remove(&key);
    }

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
        assert!(hooks.contains_key("SessionStart"));
        assert!(hooks.contains_key("SessionEnd"));
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
        assert!(is_aoe_hook_command(cmd));
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
        let cmd = aoe_hook_command();
        assert!(
            cmd.contains("hook-handler"),
            "Command should reference hook-handler: {}",
            cmd
        );
        assert!(
            cmd.ends_with("hook-handler"),
            "Command should end with hook-handler: {}",
            cmd
        );
    }

    #[test]
    fn test_hook_command_uses_absolute_path_or_fallback() {
        let cmd = aoe_hook_command();
        let binary_part = cmd.strip_suffix(" hook-handler").unwrap();
        let is_absolute = std::path::Path::new(binary_part).is_absolute();
        let is_fallback = binary_part == "aoe";
        assert!(
            is_absolute || is_fallback,
            "Binary path should be absolute or fallback 'aoe', got: {}",
            binary_part
        );
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
    fn test_stop_hook_uses_binary_handler() {
        let hooks = build_aoe_hooks();
        let stop = hooks["Stop"].as_array().unwrap();
        let cmd = stop[0]["hooks"][0]["command"].as_str().unwrap();
        assert!(
            cmd.contains("hook-handler"),
            "Stop hook should use binary handler: {}",
            cmd
        );
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
        assert!(!content
            .get("hooks")
            .unwrap()
            .as_object()
            .unwrap()
            .is_empty());

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

    #[test]
    fn test_install_replaces_old_format_hooks() {
        let tmp = TempDir::new().unwrap();
        let settings_path = tmp.path().join("settings.json");

        let old_hooks = serde_json::json!({
            "hooks": {
                "PreToolUse": [{
                    "hooks": [{
                        "type": "command",
                        "command": "sh -c '[ -n \"$AOE_INSTANCE_ID\" ] || exit 0; mkdir -p /tmp/aoe-hooks/$AOE_INSTANCE_ID && printf running > /tmp/aoe-hooks/$AOE_INSTANCE_ID/status'"
                    }]
                }]
            }
        });
        std::fs::write(
            &settings_path,
            serde_json::to_string_pretty(&old_hooks).unwrap(),
        )
        .unwrap();

        install_hooks(&settings_path).unwrap();

        let content: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        let pre_tool = &content["hooks"]["PreToolUse"];
        let all_cmds: Vec<String> = pre_tool
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|m| m["hooks"].as_array().unwrap())
            .filter_map(|h| h["command"].as_str().map(|s| s.to_string()))
            .collect();
        assert_eq!(
            all_cmds.len(),
            1,
            "Expected exactly 1 hook after reinstall, got: {:?}",
            all_cmds
        );
    }

    #[test]
    fn test_remove_new_format_hooks() {
        let tmp = TempDir::new().unwrap();
        let settings_path = tmp.path().join("settings.json");

        let new_hooks = serde_json::json!({
            "hooks": {
                "PreToolUse": [{
                    "hooks": [{
                        "type": "command",
                        "command": "/usr/local/bin/aoe hook-handler"
                    }]
                }]
            }
        });
        std::fs::write(
            &settings_path,
            serde_json::to_string_pretty(&new_hooks).unwrap(),
        )
        .unwrap();

        uninstall_hooks(&settings_path).unwrap();

        let content: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        let hooks_obj = content.get("hooks").and_then(|h| h.as_object());
        let pre_tool = hooks_obj.and_then(|o| o.get("PreToolUse"));
        assert!(
            pre_tool
                .map(|v| v.as_array().map(|a| a.is_empty()).unwrap_or(true))
                .unwrap_or(true),
            "New-format hook was not removed by uninstall"
        );
    }

    #[test]
    fn test_remove_mixed_format_hooks() {
        let tmp = TempDir::new().unwrap();
        let settings_path = tmp.path().join("settings.json");

        let mixed_hooks = serde_json::json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "hooks": [{
                            "type": "command",
                            "command": "sh -c 'mkdir -p /tmp/aoe-hooks/$AOE_INSTANCE_ID && printf running > /tmp/aoe-hooks/$AOE_INSTANCE_ID/status'"
                        }]
                    },
                    {
                        "hooks": [{
                            "type": "command",
                            "command": "/usr/local/bin/aoe hook-handler"
                        }]
                    },
                    {
                        "hooks": [{
                            "type": "command",
                            "command": "echo user_hook"
                        }]
                    }
                ]
            }
        });
        std::fs::write(
            &settings_path,
            serde_json::to_string_pretty(&mixed_hooks).unwrap(),
        )
        .unwrap();

        uninstall_hooks(&settings_path).unwrap();

        let content: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        let pre_tool = &content["hooks"]["PreToolUse"];
        let cmds: Vec<String> = pre_tool
            .as_array()
            .unwrap_or(&vec![])
            .iter()
            .flat_map(|m| {
                m["hooks"]
                    .as_array()
                    .unwrap_or(&vec![])
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
            })
            .filter_map(|h| h["command"].as_str().map(|s| s.to_string()))
            .collect();
        assert_eq!(
            cmds,
            vec!["echo user_hook"],
            "Expected only user hook to remain, got: {:?}",
            cmds
        );
    }

    #[test]
    fn test_resolve_aoe_binary_path() {
        let path = resolve_aoe_binary_path();

        assert!(path.is_some(), "resolve_aoe_binary_path should return Some");

        let path_str = path.unwrap();

        assert!(
            std::path::Path::new(&path_str).is_absolute(),
            "Path should be absolute: {}",
            path_str
        );

        assert!(
            std::path::Path::new(&path_str).exists(),
            "Binary path should exist: {}",
            path_str
        );
    }
}
