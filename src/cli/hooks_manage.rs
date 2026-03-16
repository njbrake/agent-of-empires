//! `aoe hooks install/uninstall/status` - manage Claude Code lifecycle hooks.
//!
//! Installs hook entries into Claude Code's settings.json so that Claude fires
//! `aoe _hook` on lifecycle events. This enables deterministic status detection
//! without scraping tmux pane content.

use anyhow::{Context, Result};
use clap::Subcommand;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum HooksCommands {
    /// Install Claude Code hooks for status detection
    Install,
    /// Remove aoe hooks from Claude Code settings
    Uninstall,
    /// Show whether hooks are installed and enabled
    Status,
}

pub async fn run(command: HooksCommands) -> Result<()> {
    match command {
        HooksCommands::Install => install().await,
        HooksCommands::Uninstall => uninstall().await,
        HooksCommands::Status => status().await,
    }
}

/// Resolve the absolute path to the current aoe binary.
fn resolve_aoe_path() -> Result<String> {
    let exe = std::env::current_exe().context("Failed to determine aoe binary path")?;
    let canonical = exe
        .canonicalize()
        .context("Failed to canonicalize aoe binary path")?;
    Ok(canonical.display().to_string())
}

/// Get the path to Claude Code's settings.json.
fn get_settings_path() -> Result<PathBuf> {
    // Respect user's configured claude config_dir
    if let Some(config_dir) = crate::session::get_claude_config_dir() {
        return Ok(config_dir.join("settings.json"));
    }

    // Default: ~/.claude/settings.json
    let home = dirs::home_dir().context("Could not determine home directory")?;
    Ok(home.join(".claude").join("settings.json"))
}

/// Check if a hook entry was created by aoe (contains "aoe _hook" in its command).
fn is_aoe_hook(hook: &Value) -> bool {
    hook.get("command")
        .and_then(|c| c.as_str())
        .map(|cmd| cmd.contains("aoe _hook") || cmd.contains("aoe\" _hook"))
        .unwrap_or(false)
}

/// Build the hook entries for a given aoe binary path.
fn build_hook_entries(aoe_path: &str) -> Value {
    let command = format!("{} _hook", aoe_path);
    let async_hook = |matcher: &str| -> Value {
        json!([{
            "matcher": matcher,
            "hooks": [{"type": "command", "command": command, "async": true}]
        }])
    };

    let sync_hook = |matcher: &str| -> Value {
        json!([{
            "matcher": matcher,
            "hooks": [{"type": "command", "command": command}]
        }])
    };

    json!({
        "SessionStart": async_hook(""),
        "UserPromptSubmit": async_hook(""),
        "PreToolUse": async_hook(""),
        "PostToolUse": async_hook(""),
        "Stop": async_hook(""),
        "Notification": [
            {
                "matcher": "permission_prompt",
                "hooks": [{"type": "command", "command": command, "async": true}]
            },
            {
                "matcher": "elicitation_dialog",
                "hooks": [{"type": "command", "command": command, "async": true}]
            },
            {
                "matcher": "idle_prompt",
                "hooks": [{"type": "command", "command": command, "async": true}]
            }
        ],
        "SessionEnd": sync_hook("")
    })
}

/// Remove all aoe hook entries from an existing hooks object, preserving user hooks.
/// Returns true if any entries were removed.
fn remove_aoe_hooks(hooks: &mut Map<String, Value>) -> bool {
    let mut changed = false;
    let events: Vec<String> = hooks.keys().cloned().collect();

    for event in events {
        if let Some(Value::Array(entries)) = hooks.get_mut(&event) {
            let before_len = entries.len();
            entries.retain(|entry| {
                // Each entry has a "hooks" array; remove entries where ALL hooks are aoe hooks
                if let Some(Value::Array(hook_list)) = entry.get("hooks") {
                    !hook_list.iter().all(is_aoe_hook)
                } else {
                    true
                }
            });
            if entries.len() != before_len {
                changed = true;
            }
        }
    }

    // Clean up empty arrays
    let empty_events: Vec<String> = hooks
        .iter()
        .filter(|(_, v)| v.as_array().map(|a| a.is_empty()).unwrap_or(false))
        .map(|(k, _)| k.clone())
        .collect();
    for event in empty_events {
        hooks.remove(&event);
    }

    changed
}

/// Merge aoe hook entries into existing hooks, preserving user hooks.
fn merge_aoe_hooks(hooks: &mut Map<String, Value>, new_hooks: &Map<String, Value>) {
    for (event, new_entries) in new_hooks {
        if let Some(Value::Array(new_arr)) = Some(new_entries) {
            match hooks.get_mut(event) {
                Some(Value::Array(existing)) => {
                    existing.extend(new_arr.iter().cloned());
                }
                _ => {
                    hooks.insert(event.clone(), new_entries.clone());
                }
            }
        }
    }
}

async fn install() -> Result<()> {
    let aoe_path = resolve_aoe_path()?;
    let settings_path = get_settings_path()?;

    // Load or create settings
    let mut settings: Value = if settings_path.exists() {
        let content = std::fs::read_to_string(&settings_path)
            .context("Failed to read Claude Code settings")?;
        serde_json::from_str(&content).context("Failed to parse Claude Code settings")?
    } else {
        json!({})
    };

    let settings_obj = settings
        .as_object_mut()
        .context("Claude Code settings is not a JSON object")?;

    // Get or create the hooks section
    let hooks = settings_obj
        .entry("hooks")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("hooks section is not a JSON object")?;

    // Remove existing aoe hooks first (idempotent)
    remove_aoe_hooks(hooks);

    // Build and merge new hook entries
    let new_hooks = build_hook_entries(&aoe_path);
    let new_hooks_obj = new_hooks.as_object().unwrap();
    merge_aoe_hooks(hooks, new_hooks_obj);

    // Write back
    if let Some(parent) = settings_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(&settings)?;
    std::fs::write(&settings_path, content)?;

    // Create the hook_status directory so hooks don't fail on first write
    let app_dir = crate::session::get_app_dir()?;
    std::fs::create_dir_all(app_dir.join("hook_status"))?;

    // Enable status_hooks in aoe config
    let mut config = crate::session::Config::load().unwrap_or_default();
    config.claude.status_hooks = true;
    crate::session::save_config(&config)?;

    println!("Installed Claude Code hooks for status detection");
    println!("  Settings: {}", settings_path.display());
    println!("  Binary:   {}", aoe_path);
    println!();
    println!("Status detection will now use hooks instead of pane capture for Claude sessions.");

    Ok(())
}

async fn uninstall() -> Result<()> {
    let settings_path = get_settings_path()?;

    if !settings_path.exists() {
        println!(
            "No Claude Code settings found at {}",
            settings_path.display()
        );
        return Ok(());
    }

    let content =
        std::fs::read_to_string(&settings_path).context("Failed to read Claude Code settings")?;
    let mut settings: Value =
        serde_json::from_str(&content).context("Failed to parse Claude Code settings")?;

    let settings_obj = settings
        .as_object_mut()
        .context("Claude Code settings is not a JSON object")?;

    if let Some(Value::Object(hooks)) = settings_obj.get_mut("hooks") {
        let changed = remove_aoe_hooks(hooks);
        if !changed {
            println!("No aoe hooks found in Claude Code settings");
        } else {
            // Clean up empty hooks object
            if hooks.is_empty() {
                settings_obj.remove("hooks");
            }

            let content = serde_json::to_string_pretty(&settings)?;
            std::fs::write(&settings_path, content)?;
            println!("Removed aoe hooks from Claude Code settings");
        }
    } else {
        println!("No hooks section found in Claude Code settings");
    }

    // Disable status_hooks in aoe config
    let mut config = crate::session::Config::load().unwrap_or_default();
    config.claude.status_hooks = false;
    crate::session::save_config(&config)?;

    Ok(())
}

async fn status() -> Result<()> {
    let settings_path = get_settings_path()?;

    // Check aoe config
    let config = crate::session::Config::load().unwrap_or_default();
    let enabled = config.claude.status_hooks;

    // Check if hooks are present in Claude Code settings
    let installed = if settings_path.exists() {
        let content = std::fs::read_to_string(&settings_path).unwrap_or_default();
        if let Ok(settings) = serde_json::from_str::<Value>(&content) {
            if let Some(Value::Object(hooks)) = settings.get("hooks") {
                hooks.values().any(|entries| {
                    if let Value::Array(arr) = entries {
                        arr.iter().any(|entry| {
                            if let Some(Value::Array(hook_list)) = entry.get("hooks") {
                                hook_list.iter().any(is_aoe_hook)
                            } else {
                                false
                            }
                        })
                    } else {
                        false
                    }
                })
            } else {
                false
            }
        } else {
            false
        }
    } else {
        false
    };

    println!("Claude Code Hooks Status:");
    println!(
        "  Hooks in settings.json: {}",
        if installed {
            "installed"
        } else {
            "not installed"
        }
    );
    println!(
        "  Status hooks enabled:   {}",
        if enabled { "yes" } else { "no" }
    );
    println!("  Settings path:          {}", settings_path.display());

    if installed && !enabled {
        println!();
        println!("Note: Hooks are installed but status_hooks is disabled in aoe config.");
        println!("Run `aoe hooks install` to re-enable.");
    } else if !installed && enabled {
        println!();
        println!("Note: status_hooks is enabled but hooks are not installed.");
        println!("Run `aoe hooks install` to install them.");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_aoe_hook() {
        let hook = json!({"type": "command", "command": "/usr/local/bin/aoe _hook", "async": true});
        assert!(is_aoe_hook(&hook));

        let hook = json!({"type": "command", "command": "some-other-tool", "async": true});
        assert!(!is_aoe_hook(&hook));

        let hook = json!({"type": "command", "command": "/path/to/aoe\" _hook", "async": true});
        assert!(is_aoe_hook(&hook));
    }

    #[test]
    fn test_build_hook_entries() {
        let entries = build_hook_entries("/usr/local/bin/aoe");
        let obj = entries.as_object().unwrap();

        // Check all expected events are present
        assert!(obj.contains_key("SessionStart"));
        assert!(obj.contains_key("UserPromptSubmit"));
        assert!(obj.contains_key("PreToolUse"));
        assert!(obj.contains_key("PostToolUse"));
        assert!(obj.contains_key("Stop"));
        assert!(obj.contains_key("Notification"));
        assert!(obj.contains_key("SessionEnd"));

        // SessionEnd should be synchronous (no async field)
        let session_end = obj.get("SessionEnd").unwrap();
        let hooks = session_end[0]["hooks"][0].as_object().unwrap();
        assert!(!hooks.contains_key("async"));

        // Notification should have 3 matchers
        let notifications = obj.get("Notification").unwrap().as_array().unwrap();
        assert_eq!(notifications.len(), 3);
    }

    #[test]
    fn test_remove_aoe_hooks_preserves_user_hooks() {
        let mut hooks = Map::new();

        // Add a mix of aoe and user hooks
        hooks.insert(
            "PreToolUse".to_string(),
            json!([
                {
                    "matcher": "",
                    "hooks": [{"type": "command", "command": "/usr/bin/aoe _hook", "async": true}]
                },
                {
                    "matcher": "Bash",
                    "hooks": [{"type": "command", "command": "user-linter check", "async": false}]
                }
            ]),
        );

        let changed = remove_aoe_hooks(&mut hooks);
        assert!(changed);

        // User hook should remain
        let pre_tool = hooks.get("PreToolUse").unwrap().as_array().unwrap();
        assert_eq!(pre_tool.len(), 1);
        assert_eq!(
            pre_tool[0]["hooks"][0]["command"].as_str().unwrap(),
            "user-linter check"
        );
    }

    #[test]
    fn test_remove_aoe_hooks_cleans_empty_arrays() {
        let mut hooks = Map::new();
        hooks.insert(
            "Stop".to_string(),
            json!([
                {
                    "matcher": "",
                    "hooks": [{"type": "command", "command": "aoe _hook", "async": true}]
                }
            ]),
        );

        remove_aoe_hooks(&mut hooks);
        // Empty array should be removed entirely
        assert!(!hooks.contains_key("Stop"));
    }

    #[test]
    fn test_merge_aoe_hooks() {
        let mut hooks = Map::new();
        hooks.insert(
            "PreToolUse".to_string(),
            json!([
                {
                    "matcher": "Bash",
                    "hooks": [{"type": "command", "command": "user-hook"}]
                }
            ]),
        );

        let new_hooks = serde_json::from_value::<Map<String, Value>>(json!({
            "PreToolUse": [{
                "matcher": "",
                "hooks": [{"type": "command", "command": "aoe _hook", "async": true}]
            }],
            "Stop": [{
                "matcher": "",
                "hooks": [{"type": "command", "command": "aoe _hook", "async": true}]
            }]
        }))
        .unwrap();

        merge_aoe_hooks(&mut hooks, &new_hooks);

        // PreToolUse should have both entries
        let pre_tool = hooks.get("PreToolUse").unwrap().as_array().unwrap();
        assert_eq!(pre_tool.len(), 2);

        // Stop should be added
        assert!(hooks.contains_key("Stop"));
    }
}
