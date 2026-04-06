//! Agent hook management for status detection.
//!
//! AoE installs hooks into an agent's settings file that write session
//! status (`running`/`waiting`/`idle`) to a sidecar file. This provides
//! reliable status detection without parsing tmux pane content.
//!
//! Hook events are agent-specific and defined in `AgentHookConfig::events`.

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
        "sh -c '[ -n \"$AOE_INSTANCE_ID\" ] || exit 0; mkdir -p /tmp/aoe-hooks/$AOE_INSTANCE_ID && printf {} > /tmp/aoe-hooks/$AOE_INSTANCE_ID/status'",
        status
    )
}

fn is_aoe_hook_command(cmd: &str) -> bool {
    cmd.contains(AOE_HOOK_MARKER)
}

/// Build the AoE hooks JSON structure from agent-defined events.
///
/// Events with `status: None` (lifecycle-only) are skipped since shell
/// one-liners can only write a status string.
fn build_aoe_hooks(events: &[crate::agents::HookEvent]) -> Value {
    let mut hooks_obj = serde_json::Map::new();
    for event in events {
        let Some(status) = event.status else {
            continue;
        };
        let mut entry = serde_json::Map::new();
        if let Some(m) = event.matcher {
            entry.insert("matcher".to_string(), Value::String(m.to_string()));
        }
        entry.insert(
            "hooks".to_string(),
            Value::Array(vec![serde_json::json!({
                "type": "command",
                "command": hook_command(status)
            })]),
        );
        hooks_obj.insert(
            event.name.to_string(),
            Value::Array(vec![Value::Object(entry)]),
        );
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

/// Install AoE status hooks into an agent's `settings.json` file.
///
/// Merges AoE hook entries into the existing hooks configuration, preserving
/// any user-defined hooks. Existing AoE hooks are replaced (idempotent).
///
/// If the file doesn't exist, it will be created with just the hooks.
pub fn install_hooks(settings_path: &Path, events: &[crate::agents::HookEvent]) -> Result<()> {
    let mut settings: Value = if settings_path.exists() {
        let content = std::fs::read_to_string(settings_path)?;
        serde_json::from_str(&content).unwrap_or_else(|e| {
            tracing::warn!("Failed to parse {}: {}", settings_path.display(), e);
            serde_json::json!({})
        })
    } else {
        serde_json::json!({})
    };

    let aoe_hooks = build_aoe_hooks(events);

    if !settings.get("hooks").is_some_and(|h| h.is_object()) {
        settings
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("Settings file root is not a JSON object"))?
            .insert("hooks".to_string(), serde_json::json!({}));
    }

    let settings_hooks = settings
        .get_mut("hooks")
        .and_then(|h| h.as_object_mut())
        .ok_or_else(|| anyhow::anyhow!("hooks key is not a JSON object"))?;

    let aoe_hooks_obj = aoe_hooks
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("Internal error: built hooks is not a JSON object"))?;
    for (event_name, aoe_matchers) in aoe_hooks_obj {
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

/// Remove all AoE hooks from an agent's `settings.json` file.
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
        if let Some(obj) = settings.as_object_mut() {
            obj.remove("hooks");
        }
    }

    let formatted = serde_json::to_string_pretty(&settings)?;
    std::fs::write(settings_path, formatted)?;

    tracing::info!("Removed AoE hooks from {}", settings_path.display());
    Ok(true)
}

/// settl hook events and the AoE status they map to.
const SETTL_HOOKS: &[(&str, &str)] = &[
    ("TurnStarted", "running"),
    ("WaitingForHuman", "waiting"),
    ("GameWon", "idle"),
];

/// Install AoE status hooks into settl's `~/.settl/config.toml`.
///
/// settl uses TOML config with `[[hooks]]` array entries instead of JSON
/// settings files. This function reads the existing config, removes any
/// previous AoE-managed hooks (identified by the marker), and adds hooks
/// for the three status transitions: TurnStarted->running,
/// WaitingForHuman->waiting, GameWon->idle.
pub fn install_settl_hooks() -> Result<()> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("No home directory"))?;
    let config_path = home.join(".settl").join("config.toml");

    // Parse existing config or start fresh
    let mut config: toml::Value = if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)?;
        toml::from_str(&content).unwrap_or_else(|e| {
            tracing::warn!("Failed to parse {}: {}", config_path.display(), e);
            toml::Value::Table(toml::map::Map::new())
        })
    } else {
        toml::Value::Table(toml::map::Map::new())
    };

    let table = config
        .as_table_mut()
        .ok_or_else(|| anyhow::anyhow!("Config root is not a TOML table"))?;

    // Get or create the hooks array
    let hooks = table
        .entry("hooks")
        .or_insert_with(|| toml::Value::Array(Vec::new()));
    let hooks_arr = hooks
        .as_array_mut()
        .ok_or_else(|| anyhow::anyhow!("hooks key is not a TOML array"))?;

    // Remove existing AoE hooks
    hooks_arr.retain(|hook| {
        !hook
            .get("command")
            .and_then(|c| c.as_str())
            .is_some_and(is_aoe_hook_command)
    });

    // Add one hook per status transition
    for (event, status) in SETTL_HOOKS {
        let mut entry = toml::map::Map::new();
        entry.insert("event".into(), toml::Value::String((*event).into()));
        entry.insert("command".into(), toml::Value::String(hook_command(status)));
        hooks_arr.push(toml::Value::Table(entry));
    }

    // Write back
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let formatted = toml::to_string_pretty(&config)?;
    std::fs::write(&config_path, formatted)?;

    tracing::info!("Installed AoE hooks in {}", config_path.display());
    Ok(())
}

/// Remove AoE hooks from settl's `~/.settl/config.toml`.
pub fn uninstall_settl_hooks() -> Result<bool> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("No home directory"))?;
    let config_path = home.join(".settl").join("config.toml");

    if !config_path.exists() {
        return Ok(false);
    }

    let content = std::fs::read_to_string(&config_path)?;
    let mut config: toml::Value = toml::from_str(&content).unwrap_or_else(|e| {
        tracing::warn!("Failed to parse {}: {}", config_path.display(), e);
        toml::Value::Table(toml::map::Map::new())
    });

    let Some(hooks_arr) = config.get_mut("hooks").and_then(|h| h.as_array_mut()) else {
        return Ok(false);
    };

    let before = hooks_arr.len();
    hooks_arr.retain(|hook| {
        !hook
            .get("command")
            .and_then(|c| c.as_str())
            .is_some_and(is_aoe_hook_command)
    });

    if hooks_arr.len() == before {
        return Ok(false);
    }

    let formatted = toml::to_string_pretty(&config)?;
    std::fs::write(&config_path, formatted)?;
    tracing::info!("Removed AoE hooks from {}", config_path.display());
    Ok(true)
}

/// Remove all AoE hooks from all known agent settings files and clean up
/// the hook status base directory. Called during `aoe uninstall`.
pub fn uninstall_all_hooks() {
    // Remove settl TOML hooks
    match uninstall_settl_hooks() {
        Ok(true) => println!("Removed AoE hooks from ~/.settl/config.toml"),
        Ok(false) => {}
        Err(e) => tracing::warn!("Failed to remove settl hooks: {}", e),
    }

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

    fn claude_events() -> &'static [crate::agents::HookEvent] {
        crate::agents::get_agent("claude")
            .unwrap()
            .hook_config
            .as_ref()
            .unwrap()
            .events
    }

    #[test]
    fn test_install_hooks_creates_new_file() {
        let tmp = TempDir::new().unwrap();
        let settings_path = tmp.path().join(".claude").join("settings.json");

        install_hooks(&settings_path, claude_events()).unwrap();

        let content: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        let hooks = content.get("hooks").unwrap().as_object().unwrap();

        assert!(hooks.contains_key("PreToolUse"));
        assert!(hooks.contains_key("UserPromptSubmit"));
        assert!(hooks.contains_key("Stop"));
        assert!(hooks.contains_key("Notification"));
        assert!(hooks.contains_key("ElicitationResult"));
    }

    #[test]
    fn test_install_hooks_preserves_existing_user_hooks() {
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

        install_hooks(&settings_path, claude_events()).unwrap();

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

        install_hooks(&settings_path, claude_events()).unwrap();
        install_hooks(&settings_path, claude_events()).unwrap();

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

        install_hooks(&settings_path, claude_events()).unwrap();

        let content: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert_eq!(content["apiKey"], "test-key");
        assert_eq!(content["model"], "opus");
    }

    #[test]
    fn test_hook_command_format() {
        let cmd = hook_command("running");
        assert!(cmd.contains(AOE_HOOK_MARKER));
        assert!(cmd.contains("printf running"));
    }

    #[test]
    fn test_hook_command_contains_instance_id_guard() {
        let cmd = hook_command("idle");
        assert!(cmd.contains("AOE_INSTANCE_ID"));
        assert!(cmd.contains("printf idle"));
    }

    #[test]
    fn test_notification_hook_has_matcher() {
        let hooks = build_aoe_hooks(claude_events());
        let notification = hooks["Notification"].as_array().unwrap();
        assert_eq!(notification.len(), 1);
        let matcher = notification[0]["matcher"].as_str().unwrap();
        assert!(matcher.contains("permission_prompt"));
        assert!(matcher.contains("elicitation_dialog"));
        assert!(!matcher.contains("idle_prompt"));
    }

    #[test]
    fn test_stop_hook_writes_idle() {
        let hooks = build_aoe_hooks(claude_events());
        let stop = hooks["Stop"].as_array().unwrap();
        let cmd = stop[0]["hooks"][0]["command"].as_str().unwrap();
        assert!(
            cmd.contains("printf idle"),
            "Stop hook should write idle status: {}",
            cmd
        );
    }

    #[test]
    fn test_elicitation_result_hook_writes_running() {
        let hooks = build_aoe_hooks(claude_events());
        let er = hooks["ElicitationResult"].as_array().unwrap();
        assert_eq!(er.len(), 1);
        let cmd = er[0]["hooks"][0]["command"].as_str().unwrap();
        assert!(
            cmd.contains("printf running"),
            "ElicitationResult hook should write running status: {}",
            cmd
        );
    }

    #[test]
    fn test_hooks_are_synchronous() {
        let hooks = build_aoe_hooks(claude_events());
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

        install_hooks(&settings_path, claude_events()).unwrap();

        let content: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert!(!content
            .get("hooks")
            .unwrap()
            .as_object()
            .unwrap()
            .is_empty());

        let modified = uninstall_hooks(&settings_path).unwrap();
        assert!(modified);

        let content: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        assert!(content.get("hooks").is_none());
    }

    #[test]
    fn test_uninstall_hooks_preserves_user_hooks() {
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

        install_hooks(&settings_path, claude_events()).unwrap();
        let modified = uninstall_hooks(&settings_path).unwrap();
        assert!(modified);

        let content: Value =
            serde_json::from_str(&std::fs::read_to_string(&settings_path).unwrap()).unwrap();
        let pre_tool = content["hooks"]["PreToolUse"].as_array().unwrap();
        assert_eq!(pre_tool.len(), 1);
        assert_eq!(pre_tool[0]["matcher"], "Bash");
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
    fn test_install_replaces_existing_hooks() {
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

        install_hooks(&settings_path, claude_events()).unwrap();

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
}
