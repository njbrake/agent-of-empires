//! Container configuration building for sandboxed sessions.
//!
//! Standalone functions for computing Docker volume mounts and building
//! `ContainerConfig` structs. Includes sandbox directory sync, agent config
//! mounting, and credential extraction.

use std::path::{Path, PathBuf};

use anyhow::Result;

use crate::containers::{ContainerConfig, VolumeMount};
use crate::git::GitWorktree;

use super::environment::collect_environment;
use super::instance::SandboxInfo;

/// Container path where the hook relay script is installed.
const HOOK_RELAY_PATH: &str = "/root/.claude/aoe_hook_relay.sh";

/// Home directory inside the container (root user).
const CONTAINER_HOME: &str = "/root";

/// POSIX shell script that replicates the event-to-status mapping from
/// `src/cli/hook.rs`. Runs inside the container as a Claude Code hook command,
/// writing status files to a host-mounted directory so the TUI can read them.
const HOOK_RELAY_SCRIPT: &str = r#"#!/bin/sh
# aoe hook relay - container-side status writer
# Mirrors the logic in src/cli/hook.rs for use inside Docker sandboxes.
# Writes status files to a mounted volume visible to the host TUI.

STATUS_DIR="/tmp/aoe_hook_status"
INSTANCE_ID="${AOE_INSTANCE_ID:-}"

if [ -z "$INSTANCE_ID" ]; then
    exit 0
fi

INPUT=$(cat)

EVENT=$(printf '%s' "$INPUT" | grep -o '"hook_event_name"[[:space:]]*:[[:space:]]*"[^"]*"' | sed 's/.*:[[:space:]]*"\([^"]*\)".*/\1/')

if [ -z "$EVENT" ]; then
    exit 0
fi

if [ "$EVENT" = "SessionEnd" ]; then
    rm -f "${STATUS_DIR}/${INSTANCE_ID}"
    exit 0
fi

STATUS=""
case "$EVENT" in
    SessionStart|UserPromptSubmit|PreToolUse|PostToolUse)
        STATUS="running"
        ;;
    Stop)
        STATUS="idle"
        ;;
    Notification)
        NTYPE=$(printf '%s' "$INPUT" | grep -o '"notification_type"[[:space:]]*:[[:space:]]*"[^"]*"' | sed 's/.*:[[:space:]]*"\([^"]*\)".*/\1/')
        case "$NTYPE" in
            permission_prompt|elicitation_dialog) STATUS="waiting" ;;
            idle_prompt) STATUS="idle" ;;
        esac
        ;;
esac

if [ -z "$STATUS" ]; then
    exit 0
fi

mkdir -p "$STATUS_DIR"
TIMESTAMP=$(date +%s)
printf '%s %s' "$STATUS" "$TIMESTAMP" > "${STATUS_DIR}/.${INSTANCE_ID}.tmp"
mv "${STATUS_DIR}/.${INSTANCE_ID}.tmp" "${STATUS_DIR}/${INSTANCE_ID}"
"#;

/// Subdirectory name inside each agent's config dir for the shared sandbox config.
const SANDBOX_SUBDIR: &str = "sandbox";

/// Declarative definition of an agent CLI's config directory for sandbox mounting.
struct AgentConfigMount {
    /// Canonical agent name from the agent registry (e.g. "claude", "opencode").
    /// Used to filter mounts so only the active tool's config is mounted.
    tool_name: &'static str,
    /// Path relative to home (e.g. ".claude").
    host_rel: &'static str,
    /// Path suffix relative to container home (e.g. ".claude").
    container_suffix: &'static str,
    /// Top-level entry names to skip when copying (large/recursive/unnecessary).
    skip_entries: &'static [&'static str],
    /// Files to seed into the sandbox dir with static content (write-once: only written
    /// if the file doesn't already exist, so container changes are preserved).
    seed_files: &'static [(&'static str, &'static str)],
    /// Directories to recursively copy into the sandbox dir (e.g. plugins, skills).
    copy_dirs: &'static [&'static str],
    /// macOS Keychain service name and target filename. If set, credentials are extracted
    /// from the Keychain and written to the sandbox dir as the specified file.
    keychain_credential: Option<(&'static str, &'static str)>,
    /// Files to seed at the container home directory level (outside the config dir).
    /// Each (filename, content) pair is written to the sandbox dir root and mounted as
    /// a separate file at CONTAINER_HOME/filename (write-once).
    home_seed_files: &'static [(&'static str, &'static str)],
    /// Files that should only be copied from the host if they don't already exist in the
    /// sandbox. Protects credentials placed by the v002 migration or by in-container
    /// authentication from being overwritten by stale host copies.
    preserve_files: &'static [&'static str],
}

/// Agent config definitions. Each entry describes one agent CLI's config directory.
/// To add a new agent, add an entry here -- no code changes needed.
const AGENT_CONFIG_MOUNTS: &[AgentConfigMount] = &[
    AgentConfigMount {
        tool_name: "claude",
        host_rel: ".claude",
        container_suffix: ".claude",
        skip_entries: &["sandbox", "projects"],
        seed_files: &[],
        copy_dirs: &["plugins", "skills"],
        // On macOS, OAuth tokens live in the Keychain. Extract and write as .credentials.json
        // so the container can authenticate without re-login.
        keychain_credential: Some(("Claude Code-credentials", ".credentials.json")),
        // Claude Code reads ~/.claude.json (home level, NOT inside ~/.claude/) for onboarding
        // state. Seeding hasCompletedOnboarding skips the first-run wizard.
        // Claude Code sets GIT_CONFIG_GLOBAL=/root/.sandbox-gitconfig when IS_SANDBOX=1;
        // the file must exist or all git commands fail.
        home_seed_files: &[
            (".claude.json", r#"{"hasCompletedOnboarding":true}"#),
            (".sandbox-gitconfig", ""),
        ],
        preserve_files: &[".credentials.json", "history.jsonl"],
    },
    AgentConfigMount {
        tool_name: "opencode",
        host_rel: ".local/share/opencode",
        container_suffix: ".local/share/opencode",
        skip_entries: &["sandbox"],
        seed_files: &[],
        copy_dirs: &[],
        keychain_credential: None,
        home_seed_files: &[],
        preserve_files: &[],
    },
    AgentConfigMount {
        tool_name: "codex",
        host_rel: ".codex",
        container_suffix: ".codex",
        skip_entries: &["sandbox"],
        seed_files: &[],
        copy_dirs: &[],
        keychain_credential: None,
        home_seed_files: &[],
        preserve_files: &[],
    },
    AgentConfigMount {
        tool_name: "gemini",
        host_rel: ".gemini",
        container_suffix: ".gemini",
        skip_entries: &["sandbox"],
        seed_files: &[],
        copy_dirs: &[],
        keychain_credential: None,
        home_seed_files: &[],
        preserve_files: &[],
    },
    AgentConfigMount {
        tool_name: "vibe",
        host_rel: ".vibe",
        container_suffix: ".vibe",
        skip_entries: &["sandbox"],
        seed_files: &[],
        copy_dirs: &[],
        keychain_credential: None,
        home_seed_files: &[],
        preserve_files: &[],
    },
    AgentConfigMount {
        tool_name: "cursor",
        host_rel: ".cursor",
        container_suffix: ".cursor",
        skip_entries: &["sandbox"],
        seed_files: &[],
        copy_dirs: &[],
        keychain_credential: None,
        home_seed_files: &[],
        preserve_files: &[],
    },
    AgentConfigMount {
        tool_name: "copilot",
        host_rel: ".copilot",
        container_suffix: ".copilot",
        skip_entries: &["sandbox"],
        seed_files: &[],
        copy_dirs: &[],
        keychain_credential: None,
        home_seed_files: &[],
        preserve_files: &[],
    },
    AgentConfigMount {
        tool_name: "pi",
        host_rel: ".pi",
        container_suffix: ".pi",
        skip_entries: &["sandbox"],
        seed_files: &[],
        copy_dirs: &[],
        keychain_credential: None,
        home_seed_files: &[],
        preserve_files: &[],
    },
];

/// Sync host agent config into the shared sandbox directory. Copies top-level files
/// and `copy_dirs` from the host (always overwritten on refresh). Seed files are
/// write-once: only created if they don't already exist, so container-accumulated
/// changes (e.g. permission approvals) are preserved across sessions.
fn sync_agent_config(
    host_dir: &Path,
    sandbox_dir: &Path,
    skip_entries: &[&str],
    seed_files: &[(&str, &str)],
    copy_dirs: &[&str],
    preserve_files: &[&str],
) -> Result<()> {
    std::fs::create_dir_all(sandbox_dir)?;

    // Write-once: only seed files that don't already exist.
    for &(name, content) in seed_files {
        let path = sandbox_dir.join(name);
        if !path.exists() {
            std::fs::write(path, content)?;
        }
    }

    // If the sandbox already has a "projects/" subdirectory, a prior container
    // session ran and created state we must not overwrite (e.g. settings.json,
    // statsig/, session metadata). Only seed files, copy_dirs, and keychain
    // credentials are still synced; the general top-level file copy is skipped.
    //
    // Why "projects/"? Claude Code creates this directory on first run to store
    // per-project session data. Its presence reliably indicates the container
    // has been used before. If this sentinel changes upstream, container restarts
    // would fall back to the old behavior of re-copying all host files (safe,
    // just potentially overwriting container-side customizations).
    let has_prior_data = sandbox_dir.join("projects").exists();
    if has_prior_data {
        tracing::info!(
            "sync_agent_config: sandbox={} has prior session data, skipping general file copy",
            sandbox_dir.display()
        );
    }

    for entry in std::fs::read_dir(host_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if skip_entries.iter().any(|&s| s == name_str.as_ref()) {
            continue;
        }

        // Follow symlinks so symlinked dirs are treated as dirs.
        let metadata = match std::fs::metadata(entry.path()) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("Skipping {}: {}", entry.path().display(), e);
                continue;
            }
        };

        if metadata.is_dir() {
            if copy_dirs.iter().any(|&d| d == name_str.as_ref()) {
                let dest = sandbox_dir.join(&name);
                if let Err(e) = copy_dir_recursive(&entry.path(), &dest) {
                    tracing::warn!("Failed to copy dir {}: {}", name_str, e);
                }
            }
            continue;
        }

        // Skip general top-level file copies on restart to preserve
        // container-created files (settings.json, statsig/, etc.).
        if has_prior_data {
            continue;
        }

        let dest = sandbox_dir.join(&name);

        // Preserved files are only seeded from the host when they don't already exist
        // in the sandbox. This protects credentials placed by migration or in-container
        // authentication from being overwritten by stale host copies.
        if preserve_files.iter().any(|&p| p == name_str.as_ref()) && dest.exists() {
            continue;
        }

        if let Err(e) = std::fs::copy(entry.path(), &dest) {
            tracing::warn!("Failed to copy {}: {}", name_str, e);
        }
    }

    Ok(())
}

/// Recursively copy a directory tree, following symlinks.
fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    std::fs::create_dir_all(dest)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let target = dest.join(entry.file_name());
        // Follow symlinks so symlinked dirs/files are handled correctly.
        let metadata = std::fs::metadata(entry.path())?;
        if metadata.is_dir() {
            copy_dir_recursive(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Write the hook relay shell script into the sandbox directory.
///
/// Always overwrites (not write-once like seed_files) so the script stays
/// in sync with any future status mapping changes.
fn seed_hook_relay_script(sandbox_dir: &Path) -> Result<()> {
    let script_path = sandbox_dir.join("aoe_hook_relay.sh");
    std::fs::write(&script_path, HOOK_RELAY_SCRIPT)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755))?;
    }

    Ok(())
}

/// Rewrite `aoe _hook` commands in the sandbox's settings.json to use the
/// container-local relay script path. Preserves all non-aoe user hooks.
fn rewrite_hooks_for_container(sandbox_dir: &Path) -> Result<()> {
    let settings_path = sandbox_dir.join("settings.json");
    if !settings_path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(&settings_path)?;
    let mut settings: serde_json::Value = serde_json::from_str(&content)?;

    let mut changed = false;

    if let Some(hooks) = settings.get_mut("hooks").and_then(|h| h.as_object_mut()) {
        for entries in hooks.values_mut() {
            if let Some(entries_arr) = entries.as_array_mut() {
                for entry in entries_arr.iter_mut() {
                    if let Some(hook_list) = entry.get_mut("hooks").and_then(|h| h.as_array_mut()) {
                        for hook in hook_list.iter_mut() {
                            let is_aoe = hook
                                .get("command")
                                .and_then(|c| c.as_str())
                                .map(|cmd| cmd.contains("aoe _hook") || cmd.contains("aoe\" _hook"))
                                .unwrap_or(false);
                            if is_aoe {
                                if let Some(obj) = hook.as_object_mut() {
                                    obj.insert(
                                        "command".to_string(),
                                        serde_json::Value::String(HOOK_RELAY_PATH.to_string()),
                                    );
                                    changed = true;
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    if changed {
        let new_content = serde_json::to_string_pretty(&settings)?;
        std::fs::write(&settings_path, new_content)?;
    }

    Ok(())
}

/// Rewrite host-absolute `installLocation` paths in the sandbox's
/// `plugins/known_marketplaces.json` to use the container home directory.
/// Without this, marketplace plugins fail to load because the container
/// filesystem doesn't have the host user's home path.
fn rewrite_marketplaces_for_container(sandbox_dir: &Path) -> Result<()> {
    let mp_path = sandbox_dir.join("plugins/known_marketplaces.json");
    if !mp_path.exists() {
        return Ok(());
    }

    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    let host_prefix = format!("{}/.claude/", home.to_string_lossy());
    let container_prefix = format!("{CONTAINER_HOME}/.claude/");

    let content = std::fs::read_to_string(&mp_path)?;
    let mut root: serde_json::Value = serde_json::from_str(&content)?;

    let mut changed = false;

    if let Some(obj) = root.as_object_mut() {
        for entry in obj.values_mut() {
            if let Some(loc) = entry.get("installLocation").and_then(|v| v.as_str()) {
                if loc.starts_with(&host_prefix) {
                    let rewritten = format!("{}{}", container_prefix, &loc[host_prefix.len()..]);
                    entry.as_object_mut().unwrap().insert(
                        "installLocation".to_string(),
                        serde_json::Value::String(rewritten),
                    );
                    changed = true;
                }
            }
        }
    }

    if changed {
        let new_content = serde_json::to_string_pretty(&root)?;
        std::fs::write(&mp_path, new_content)?;
    }

    Ok(())
}

/// Rewrite `installed_plugins.json` from the sandbox with container-local paths.
///
/// Each container needs its own manifest because the sandbox is shared 1:N and
/// different projects have different plugins. The rewritten manifest is stored at
/// `~/.agent-of-empires/plugin_manifests/<container_name>.json` and should be
/// file-mounted over `/root/.claude/plugins/installed_plugins.json` so Docker
/// shadows the shared copy.
///
/// Path transformations:
/// - `installPath`: host `~/.claude/` prefix -> `/root/.claude/`
/// - `projectPath`: host project path -> container workspace path
fn rewrite_plugins_for_container(
    sandbox_dir: &Path,
    container_name: &str,
    host_project_path: &str,
    workspace_path: &str,
    home: &Path,
) -> Result<Option<std::path::PathBuf>> {
    let plugins_json = sandbox_dir.join("plugins").join("installed_plugins.json");
    if !plugins_json.exists() {
        return Ok(None);
    }
    let app_dir = crate::session::get_app_dir()?;
    rewrite_plugins_for_container_with_app_dir(
        sandbox_dir,
        container_name,
        host_project_path,
        workspace_path,
        home,
        &app_dir,
    )
}

fn rewrite_plugins_for_container_with_app_dir(
    sandbox_dir: &Path,
    container_name: &str,
    host_project_path: &str,
    workspace_path: &str,
    home: &Path,
    app_dir: &Path,
) -> Result<Option<std::path::PathBuf>> {
    let plugins_json = sandbox_dir.join("plugins").join("installed_plugins.json");
    if !plugins_json.exists() {
        return Ok(None);
    }

    let content = std::fs::read_to_string(&plugins_json)?;
    let mut manifest: serde_json::Value = serde_json::from_str(&content)?;

    let host_claude_prefix = home.join(".claude");
    let host_claude_str = format!("{}/", host_claude_prefix.display());
    let container_claude_prefix = format!("{CONTAINER_HOME}/.claude/");

    // Build a set of host paths that should match for projectPath rewriting.
    // This includes the literal project path, plus the main repo path if the
    // project is a worktree (plugins are often registered against the main repo).
    let mut matching_project_paths: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    matching_project_paths.insert(host_project_path.to_string());

    let project_path = std::path::Path::new(host_project_path);
    if let Ok(main_repo) = GitWorktree::find_main_repo(project_path) {
        let main_canonical = main_repo.canonicalize().unwrap_or(main_repo.clone());
        let project_canonical = project_path
            .canonicalize()
            .unwrap_or_else(|_| project_path.to_path_buf());
        if main_canonical != project_canonical {
            matching_project_paths.insert(main_canonical.to_string_lossy().to_string());
        }
    }

    if let Some(plugins) = manifest.get_mut("plugins").and_then(|p| p.as_object_mut()) {
        for entries in plugins.values_mut() {
            if let Some(entries_arr) = entries.as_array_mut() {
                for entry in entries_arr.iter_mut() {
                    if let Some(obj) = entry.as_object_mut() {
                        // Rewrite installPath: ~/.claude/... -> CONTAINER_HOME/.claude/...
                        if let Some(relative) = obj
                            .get("installPath")
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.strip_prefix(&host_claude_str))
                        {
                            obj.insert(
                                "installPath".to_string(),
                                serde_json::Value::String(format!(
                                    "{}{}",
                                    container_claude_prefix, relative
                                )),
                            );
                        }

                        // Rewrite projectPath if it matches the host project
                        if let Some(project_path_val) = obj
                            .get("projectPath")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                        {
                            if matching_project_paths.contains(&project_path_val) {
                                obj.insert(
                                    "projectPath".to_string(),
                                    serde_json::Value::String(workspace_path.to_string()),
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    let manifests_dir = app_dir.join("plugin_manifests");
    std::fs::create_dir_all(&manifests_dir)?;

    let safe_name = container_name.replace('/', "_");
    let manifest_path = manifests_dir.join(format!("{}.json", safe_name));
    let new_content = serde_json::to_string_pretty(&manifest)?;
    std::fs::write(&manifest_path, new_content)?;

    tracing::info!(
        "Wrote per-instance plugin manifest for {} -> {}",
        container_name,
        manifest_path.display()
    );

    Ok(Some(manifest_path))
}

/// Remove the per-instance plugin manifest file created for a container.
pub(crate) fn cleanup_plugin_manifest(container_name: &str) {
    if let Ok(app_dir) = crate::session::get_app_dir() {
        cleanup_plugin_manifest_in_dir(container_name, &app_dir);
    }
}

fn cleanup_plugin_manifest_in_dir(container_name: &str, app_dir: &Path) {
    let safe_name = container_name.replace('/', "_");
    let manifest_path = app_dir
        .join("plugin_manifests")
        .join(format!("{}.json", safe_name));
    if manifest_path.exists() {
        if let Err(e) = std::fs::remove_file(&manifest_path) {
            tracing::warn!(
                "Failed to remove plugin manifest for {}: {}",
                container_name,
                e
            );
        }
    }
}

/// Parse the `expiresAt` timestamp from a Claude Code credential JSON string.
/// Returns `None` if the JSON is malformed or the field is missing/wrong type.
#[cfg(any(target_os = "macos", test))]
fn parse_credential_expires_at(content: &str) -> Option<u64> {
    let value: serde_json::Value = serde_json::from_str(content).ok()?;
    value.get("claudeAiOauth")?.get("expiresAt")?.as_u64()
}

/// Decide whether an incoming credential should overwrite the existing one,
/// based on `expiresAt` timestamps. Returns `true` if the incoming credential
/// should be written.
#[cfg(any(target_os = "macos", test))]
fn should_overwrite_credential(existing_content: &str, incoming_content: &str) -> bool {
    let existing_exp = parse_credential_expires_at(existing_content);
    let incoming_exp = parse_credential_expires_at(incoming_content);

    match (existing_exp, incoming_exp) {
        (Some(existing), Some(incoming)) => incoming > existing,
        (Some(_), None) => false,
        _ => true,
    }
}

/// Extract credentials from the macOS Keychain and write to a file.
/// Returns Ok(true) if credentials were written, Ok(false) if not available.
#[cfg(target_os = "macos")]
fn extract_keychain_credential(service: &str, dest: &Path) -> Result<bool> {
    use std::process::Command;

    let user = std::env::var("USER").unwrap_or_default();
    let output = Command::new("security")
        .args(["find-generic-password", "-a"])
        .arg(&user)
        .args(["-w", "-s", service])
        .output()?;

    if !output.status.success() {
        let code = output.status.code().unwrap_or(-1);
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Exit code 36 = errSecInteractionNotAllowed (keychain locked or ACL denied)
        // Exit code 44 = errSecItemNotFound
        if code == 36 {
            tracing::warn!(
                "Keychain access denied for service '{}' (exit code 36). \
                 The keychain may be locked. Run 'security unlock-keychain' and restart. \
                 Stderr: {}",
                service,
                stderr.trim()
            );
        } else if code == 44 {
            tracing::debug!(
                "No keychain entry found for service '{}' (account '{}')",
                service,
                user
            );
        } else {
            tracing::warn!(
                "Failed to extract keychain credential for service '{}' \
                 (account '{}', exit code {}): {}",
                service,
                user,
                code,
                stderr.trim()
            );
        }
        return Ok(false);
    }

    let content = String::from_utf8_lossy(&output.stdout);
    let trimmed = content.trim();
    if trimmed.is_empty() {
        tracing::warn!(
            "Keychain entry for service '{}' exists but has empty content",
            service
        );
        return Ok(false);
    }

    // Only overwrite if the keychain credential is fresher than what the sandbox already has.
    if dest.exists() {
        if let Ok(existing_content) = std::fs::read_to_string(dest) {
            if !should_overwrite_credential(&existing_content, trimmed) {
                tracing::debug!(
                    "Keychain credential for '{}' is not fresher than sandbox, keeping sandbox",
                    service,
                );
                return Ok(false);
            }
        }
    }

    std::fs::write(dest, trimmed)?;
    tracing::debug!(
        "Extracted keychain credential for '{}' -> {}",
        service,
        dest.display()
    );
    Ok(true)
}

#[cfg(not(target_os = "macos"))]
fn extract_keychain_credential(_service: &str, _dest: &Path) -> Result<bool> {
    Ok(false)
}

/// Sync a single agent's host config into its shared sandbox directory.
/// Handles config file sync, keychain credential extraction, and home-level seed files.
fn prepare_sandbox_dir(mount: &AgentConfigMount, home: &Path) -> Result<std::path::PathBuf> {
    let host_dir = home.join(mount.host_rel);
    let sandbox_dir = home.join(mount.host_rel).join(SANDBOX_SUBDIR);

    if host_dir.exists() {
        sync_agent_config(
            &host_dir,
            &sandbox_dir,
            mount.skip_entries,
            mount.seed_files,
            mount.copy_dirs,
            mount.preserve_files,
        )?;

        if let Some((service, filename)) = mount.keychain_credential {
            if let Err(e) = extract_keychain_credential(service, &sandbox_dir.join(filename)) {
                tracing::warn!(
                    "Failed to extract keychain credential for {}: {}",
                    mount.host_rel,
                    e
                );
            }
        }
    } else {
        std::fs::create_dir_all(&sandbox_dir)?;
    }

    for &(filename, content) in mount.home_seed_files {
        let path = sandbox_dir.join(filename);
        if !path.exists() {
            std::fs::write(&path, content)?;
        }
    }

    Ok(sandbox_dir)
}

/// Compute volume mount paths for Docker container.
///
/// For bare repo worktrees (worktree inside the repo), mounts the main repo.
/// For sibling worktrees (non-bare layout), mounts the main repo and worktree
/// as separate volumes at paths preserving their relative structure.
/// For non-git paths, mounts the project path directly.
///
/// `project_path_str` is the raw project path string (used as the host mount path in the
/// default case where no worktree is detected).
///
/// Returns (host_mount_path, container_mount_path, working_dir)
pub(crate) fn compute_volume_paths(
    project_path: &Path,
    project_path_str: &str,
) -> Result<(Vec<VolumeMount>, String)> {
    // Only look for a main repo if the project path itself has a .git entry (file or
    // directory). This prevents git2::Repository::discover from walking up the directory
    // tree and finding an unrelated ancestor repo (e.g., a dotfile-managed home directory),
    // which would cause aoe to mount that ancestor -- potentially the user's entire $HOME --
    // into the container.
    //
    // Legitimate git repos have a .git directory; worktrees have a .git file containing a
    // gitdir pointer. Both cases are covered by this check.
    if project_path.join(".git").exists() {
        if let Ok(main_repo) = GitWorktree::find_main_repo(project_path) {
            // Canonicalize paths for reliable comparison (handles symlinks like /tmp -> /private/tmp)
            let main_repo_canonical = main_repo
                .canonicalize()
                .unwrap_or_else(|_| main_repo.clone());
            let project_canonical = project_path
                .canonicalize()
                .unwrap_or_else(|_| project_path.to_path_buf());

            // Check if project_path is a worktree (different from the main repo root).
            // Mount enough of the filesystem so the worktree's relative gitdir reference
            // resolves correctly inside the container.
            if main_repo_canonical != project_canonical {
                if project_canonical.starts_with(&main_repo_canonical) {
                    // Worktree is inside the main repo (bare repo layout) --
                    // mounting the main repo is sufficient.
                    let name = main_repo_canonical
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| "workspace".to_string());
                    let container_base = format!("/workspace/{}", name);
                    let relative_worktree = project_canonical
                        .strip_prefix(&main_repo_canonical)
                        .map(|p| p.to_path_buf())
                        .unwrap_or_default();
                    let working_dir = if relative_worktree.as_os_str().is_empty() {
                        container_base.clone()
                    } else {
                        format!("{}/{}", container_base, relative_worktree.display())
                    };

                    return Ok((
                        vec![VolumeMount {
                            host_path: main_repo_canonical.to_string_lossy().to_string(),
                            container_path: container_base,
                            read_only: false,
                        }],
                        working_dir,
                    ));
                } else {
                    // Worktree is a sibling of the main repo (non-bare layout).
                    // Mount each separately under /workspace/, preserving their
                    // relative path structure from their common ancestor. This
                    // ensures the worktree's .git file (which contains a relative
                    // gitdir path) resolves correctly inside the container.
                    let common = common_ancestor(&main_repo_canonical, &project_canonical);
                    let repo_rel = main_repo_canonical
                        .strip_prefix(&common)
                        .unwrap_or(&main_repo_canonical);
                    let wt_rel = project_canonical
                        .strip_prefix(&common)
                        .unwrap_or(&project_canonical);

                    let repo_container = format!("/workspace/{}", repo_rel.display());
                    let wt_container = format!("/workspace/{}", wt_rel.display());

                    return Ok((
                        vec![
                            VolumeMount {
                                host_path: main_repo_canonical.to_string_lossy().to_string(),
                                container_path: repo_container,
                                read_only: false,
                            },
                            VolumeMount {
                                host_path: project_canonical.to_string_lossy().to_string(),
                                container_path: wt_container.clone(),
                                read_only: false,
                            },
                        ],
                        wt_container,
                    ));
                }
            }
        }
    }

    // Default behavior: mount project_path directly
    let dir_name = project_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "workspace".to_string());
    let workspace_path = format!("/workspace/{}", dir_name);

    Ok((
        vec![VolumeMount {
            host_path: project_path_str.to_string(),
            container_path: workspace_path.clone(),
            read_only: false,
        }],
        workspace_path,
    ))
}

/// Re-sync shared sandbox directories from the host so the container picks up
/// any credential changes (e.g. re-auth) since it was created.
pub(crate) fn refresh_agent_configs() {
    let Some(home) = dirs::home_dir() else {
        return;
    };

    for mount in AGENT_CONFIG_MOUNTS {
        let sandbox_dir = match prepare_sandbox_dir(mount, &home) {
            Ok(dir) => dir,
            Err(e) => {
                tracing::warn!(
                    "Failed to refresh agent config for {}: {}",
                    mount.host_rel,
                    e
                );
                continue;
            }
        };

        if mount.host_rel == ".claude" {
            if let Err(e) = seed_hook_relay_script(&sandbox_dir) {
                tracing::warn!("Failed to seed hook relay script: {}", e);
            }
            if let Err(e) = rewrite_hooks_for_container(&sandbox_dir) {
                tracing::warn!("Failed to rewrite hooks for container: {}", e);
            }
            if let Err(e) = rewrite_marketplaces_for_container(&sandbox_dir) {
                tracing::warn!("Failed to rewrite marketplaces for container: {}", e);
            }
        }
    }
}

/// Build a full `ContainerConfig` for creating a sandboxed container.
pub(crate) fn build_container_config(
    project_path_str: &str,
    sandbox_info: &SandboxInfo,
    tool: &str,
    is_yolo_mode: bool,
    instance_id: &str,
) -> Result<ContainerConfig> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;

    let project_path = Path::new(project_path_str);

    // Determine mount path(s) and working directory.
    // For bare repo worktrees, mount the entire bare repo and set working_dir to the worktree.
    // For sibling worktrees, mount the main repo and worktree as separate volumes.
    let (project_volumes, workspace_path) = compute_volume_paths(project_path, project_path_str)?;

    let mut volumes = project_volumes;

    let sandbox_config = {
        let profile = super::config::resolve_default_profile();
        match super::profile_config::resolve_config(&profile) {
            Ok(c) => {
                tracing::debug!(
                    "Loaded sandbox config: extra_volumes={:?}, mount_ssh={}, volume_ignores={:?}",
                    c.sandbox.extra_volumes,
                    c.sandbox.mount_ssh,
                    c.sandbox.volume_ignores
                );
                c.sandbox
            }
            Err(e) => {
                tracing::warn!("Failed to load config, using defaults: {}", e);
                Default::default()
            }
        }
    };

    let gitconfig = home.join(".gitconfig");
    if gitconfig.exists() {
        volumes.push(VolumeMount {
            host_path: gitconfig.to_string_lossy().to_string(),
            container_path: format!("{}/.gitconfig", CONTAINER_HOME),
            read_only: true,
        });
    }

    if sandbox_config.mount_ssh {
        let ssh_dir = home.join(".ssh");
        if ssh_dir.exists() {
            volumes.push(VolumeMount {
                host_path: ssh_dir.to_string_lossy().to_string(),
                container_path: format!("{}/.ssh", CONTAINER_HOME),
                read_only: true,
            });
        }
    }

    if tool == "opencode" {
        let opencode_config = home.join(".config").join("opencode");
        if opencode_config.exists() {
            volumes.push(VolumeMount {
                host_path: opencode_config.to_string_lossy().to_string(),
                container_path: format!("{}/.config/opencode", CONTAINER_HOME),
                read_only: true,
            });
        }
    }

    // Sync host agent config into a shared sandbox directory per agent and
    // bind-mount it read-write. Only mount the config for the active tool.
    // Agent definitions are in AGENT_CONFIG_MOUNTS -- add new agents there, not here.
    for mount in AGENT_CONFIG_MOUNTS.iter().filter(|m| m.tool_name == tool) {
        let container_path = format!("{}/{}", CONTAINER_HOME, mount.container_suffix);

        let sandbox_dir = match prepare_sandbox_dir(mount, &home) {
            Ok(dir) => dir,
            Err(e) => {
                tracing::warn!(
                    "Failed to prepare sandbox dir for {}, skipping: {}",
                    mount.host_rel,
                    e
                );
                continue;
            }
        };

        // For .claude config: seed the hook relay script, rewrite hook commands,
        // and generate a per-instance plugin manifest so that Claude Code hooks
        // and plugins work inside the container.
        let mut plugin_manifest_path = None;
        if mount.host_rel == ".claude" {
            if let Err(e) = seed_hook_relay_script(&sandbox_dir) {
                tracing::warn!("Failed to seed hook relay script: {}", e);
            }
            if let Err(e) = rewrite_hooks_for_container(&sandbox_dir) {
                tracing::warn!("Failed to rewrite hooks for container: {}", e);
            }
            if let Err(e) = rewrite_marketplaces_for_container(&sandbox_dir) {
                tracing::warn!("Failed to rewrite marketplaces for container: {}", e);
            }
            match rewrite_plugins_for_container(
                &sandbox_dir,
                &sandbox_info.container_name,
                project_path_str,
                &workspace_path,
                &home,
            ) {
                Ok(path) => plugin_manifest_path = path,
                Err(e) => {
                    tracing::warn!("Failed to rewrite plugins for container: {}", e);
                }
            }
        }

        tracing::debug!(
            "Sandbox dir ready for {}, binding {} -> {}",
            mount.host_rel,
            sandbox_dir.display(),
            container_path
        );
        volumes.push(VolumeMount {
            host_path: sandbox_dir.to_string_lossy().to_string(),
            container_path: container_path.clone(),
            read_only: false,
        });

        // Home-level seed files are mounted as individual files at the container
        // home directory (already written by prepare_sandbox_dir).
        for &(filename, _) in mount.home_seed_files {
            let file_path = sandbox_dir.join(filename);
            if file_path.exists() {
                volumes.push(VolumeMount {
                    host_path: file_path.to_string_lossy().to_string(),
                    container_path: format!("{}/{}", CONTAINER_HOME, filename),
                    read_only: false,
                });
            }
        }

        // Per-instance plugin manifest: file-mounted AFTER the directory mount
        // so Docker shadows the shared installed_plugins.json with the
        // container-specific version.
        if let Some(manifest_path) = plugin_manifest_path {
            volumes.push(VolumeMount {
                host_path: manifest_path.to_string_lossy().to_string(),
                container_path: format!("{}/plugins/installed_plugins.json", container_path),
                read_only: false,
            });
        }
    }

    if let Some(agent) = crate::agents::get_agent(tool) {
        if let Some(hook_cfg) = &agent.hook_config {
            let hook_dir = crate::hooks::hook_status_dir(instance_id);
            if let Err(e) = std::fs::create_dir_all(&hook_dir) {
                tracing::warn!(
                    "Failed to create hook directory {}: {}",
                    hook_dir.display(),
                    e
                );
            }
            volumes.push(VolumeMount {
                host_path: hook_dir.to_string_lossy().to_string(),
                container_path: hook_dir.to_string_lossy().to_string(),
                read_only: false,
            });

            // Install hooks into sandbox settings.json for the containerized agent.
            // Shell one-liners work inside containers since they only use sh/mkdir/printf.
            let config_dir_name = std::path::Path::new(hook_cfg.settings_rel_path)
                .parent()
                .unwrap_or(std::path::Path::new("."));
            // Find the matching agent config mount to locate the sandbox dir
            for mount in AGENT_CONFIG_MOUNTS {
                if mount.host_rel == config_dir_name.to_string_lossy() {
                    let sandbox_dir = home.join(mount.host_rel).join(SANDBOX_SUBDIR);
                    let settings_file = sandbox_dir.join("settings.json");
                    if let Err(e) = crate::hooks::install_hooks(&settings_file, hook_cfg.events) {
                        tracing::warn!("Failed to install hooks in sandbox settings: {}", e);
                    }
                    break;
                }
            }
        }
    }

    let mut environment: Vec<(String, String)> = collect_environment(&sandbox_config, sandbox_info);

    if let Some(agent) = crate::agents::get_agent(tool) {
        for &(key, value) in agent.container_env {
            environment.push((key.to_string(), value.to_string()));
        }
        if is_yolo_mode {
            if let Some(crate::agents::YoloMode::EnvVar(key, value)) = &agent.yolo {
                environment.push((key.to_string(), value.to_string()));
            }
        }
    }

    // Add extra_volumes from config (host:container format)
    // Also collect container paths to filter conflicting volume_ignores later
    tracing::debug!(
        "extra_volumes from config: {:?}",
        sandbox_config.extra_volumes
    );
    let mut extra_volume_container_paths: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for entry in &sandbox_config.extra_volumes {
        let parts: Vec<&str> = entry.splitn(3, ':').collect();
        if parts.len() >= 2 {
            tracing::info!(
                "Mounting extra volume: {} -> {} (ro: {})",
                parts[0],
                parts[1],
                parts.get(2) == Some(&"ro")
            );
            extra_volume_container_paths.insert(parts[1].to_string());
            volumes.push(VolumeMount {
                host_path: parts[0].to_string(),
                container_path: parts[1].to_string(),
                read_only: parts.get(2) == Some(&"ro"),
            });
        } else {
            tracing::warn!("Ignoring malformed extra_volume entry: {}", entry);
        }
    }

    // Filter anonymous_volumes to exclude paths that conflict with extra_volumes
    // (extra_volumes should take precedence over volume_ignores)
    // Conflicts include:
    //   - Exact match: both point to same path
    //   - Anonymous volume is parent of extra_volume (would shadow the mount)
    //   - Anonymous volume is inside extra_volume (redundant/conflicting)
    let anonymous_volumes: Vec<String> = sandbox_config
        .volume_ignores
        .iter()
        .map(|ignore| format!("{}/{}", workspace_path, ignore))
        .filter(|anon_path| {
            !extra_volume_container_paths.iter().any(|extra_path| {
                anon_path == extra_path
                    || extra_path.starts_with(&format!("{}/", anon_path))
                    || anon_path.starts_with(&format!("{}/", extra_path))
            })
        })
        .collect();

    // Deduplicate volumes by container_path (last writer wins, so extra_volumes
    // from user config override automatic mounts).
    let mut seen = std::collections::HashSet::new();
    let mut deduped = Vec::with_capacity(volumes.len());
    for vol in volumes.into_iter().rev() {
        if seen.insert(vol.container_path.clone()) {
            deduped.push(vol);
        } else {
            tracing::debug!("Dropping duplicate mount for {}", vol.container_path);
        }
    }
    deduped.reverse();

    Ok(ContainerConfig {
        working_dir: workspace_path,
        volumes: deduped,
        anonymous_volumes,
        environment,
        cpu_limit: sandbox_config.cpu_limit,
        memory_limit: sandbox_config.memory_limit,
        port_mappings: sandbox_config.port_mappings.clone(),
    })
}

/// Find the longest common ancestor path of two absolute paths.
fn common_ancestor(a: &Path, b: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    let mut a_components = a.components();
    let mut b_components = b.components();
    loop {
        match (a_components.next(), b_components.next()) {
            (Some(ac), Some(bc)) if ac == bc => result.push(ac),
            _ => break,
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // --- compute_volume_paths tests ---

    fn setup_regular_repo() -> (TempDir, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let repo = git2::Repository::init(dir.path()).unwrap();

        // Create initial commit so HEAD is valid
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let tree_id = repo.index().unwrap().write_tree().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial", &tree, &[])
            .unwrap();

        let repo_path = dir.path().to_path_buf();
        (dir, repo_path)
    }

    fn setup_bare_repo_with_worktree() -> (TempDir, std::path::PathBuf, std::path::PathBuf) {
        let dir = TempDir::new().unwrap();
        let bare_path = dir.path().join(".bare");

        // Create bare repository
        let repo = git2::Repository::init_bare(&bare_path).unwrap();

        // Create initial commit
        let sig = git2::Signature::now("Test", "test@example.com").unwrap();
        let tree_id = repo.treebuilder(None).unwrap().write().unwrap();
        let tree = repo.find_tree(tree_id).unwrap();
        repo.commit(Some("HEAD"), &sig, &sig, "Initial", &tree, &[])
            .unwrap();

        // Create .git file pointing to bare repo
        std::fs::write(dir.path().join(".git"), "gitdir: ./.bare\n").unwrap();

        // Create worktree
        let worktree_path = dir.path().join("main");
        let _ = std::process::Command::new("git")
            .args(["worktree", "add", worktree_path.to_str().unwrap(), "HEAD"])
            .current_dir(&bare_path)
            .output();

        let main_repo_path = dir.path().to_path_buf();
        (dir, main_repo_path, worktree_path)
    }

    #[test]
    fn test_compute_volume_paths_regular_repo() {
        let (_dir, repo_path) = setup_regular_repo();
        let project_path_str = repo_path.to_str().unwrap();

        let (volumes, working_dir) = compute_volume_paths(&repo_path, project_path_str).unwrap();

        assert_eq!(volumes.len(), 1);
        // Regular repo: mount path should be the project path
        assert_eq!(
            volumes[0].host_path,
            repo_path.to_string_lossy().to_string()
        );
        // Container path and working dir should be the same
        assert_eq!(volumes[0].container_path, working_dir);
        // Should be /workspace/{dir_name}
        let dir_name = repo_path.file_name().unwrap().to_string_lossy();
        assert_eq!(
            volumes[0].container_path,
            format!("/workspace/{}", dir_name)
        );
    }

    #[test]
    fn test_compute_volume_paths_non_git_directory() {
        let dir = TempDir::new().unwrap();
        let project_path_str = dir.path().to_str().unwrap();

        let (volumes, working_dir) = compute_volume_paths(dir.path(), project_path_str).unwrap();

        assert_eq!(volumes.len(), 1);
        // Non-git: mount path should be the project path
        assert_eq!(
            volumes[0].host_path,
            dir.path().to_string_lossy().to_string()
        );
        // Container path and working dir should be the same
        assert_eq!(volumes[0].container_path, working_dir);
    }

    #[test]
    fn test_compute_volume_paths_bare_repo_worktree() {
        let (_dir, main_repo_path, worktree_path) = setup_bare_repo_with_worktree();

        // Skip if worktree wasn't created (git might not be available)
        if !worktree_path.exists() {
            return;
        }

        let project_path_str = worktree_path.to_str().unwrap();

        let (volumes, working_dir) =
            compute_volume_paths(&worktree_path, project_path_str).unwrap();

        // Bare repo worktree: single mount of the repo root
        assert_eq!(volumes.len(), 1);

        // Canonicalize paths for comparison (handles /var -> /private/var on macOS)
        let mount_path_canon = Path::new(&volumes[0].host_path).canonicalize().unwrap();
        let main_repo_canon = main_repo_path.canonicalize().unwrap();

        // For bare repo worktree: mount the entire repo root
        assert_eq!(
            mount_path_canon, main_repo_canon,
            "Should mount the bare repo root, not just the worktree"
        );

        // Container path should be /workspace/{repo_name}
        let repo_name = main_repo_path.file_name().unwrap().to_string_lossy();
        assert_eq!(
            volumes[0].container_path,
            format!("/workspace/{}", repo_name),
            "Container mount path should be /workspace/{{repo_name}}"
        );

        // Working dir should point to the worktree within the mount
        assert!(
            working_dir.starts_with(&format!("/workspace/{}", repo_name)),
            "Working dir should be under /workspace/{{repo_name}}"
        );
        assert!(
            working_dir.ends_with("/main"),
            "Working dir should end with worktree name 'main', got: {}",
            working_dir
        );
    }

    #[test]
    fn test_compute_volume_paths_non_bare_repo_worktree() {
        let (_dir, repo_path) = setup_regular_repo();

        // Create a worktree from the regular (non-bare) repo
        let worktree_path = repo_path.parent().unwrap().join("my-worktree");
        let head = git2::Repository::open(&repo_path)
            .unwrap()
            .head()
            .unwrap()
            .peel_to_commit()
            .unwrap()
            .id();
        let repo = git2::Repository::open(&repo_path).unwrap();
        repo.branch("wt-branch", &repo.find_commit(head).unwrap(), false)
            .unwrap();
        drop(repo);

        let output = std::process::Command::new("git")
            .args([
                "worktree",
                "add",
                worktree_path.to_str().unwrap(),
                "wt-branch",
            ])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        if !output.status.success() {
            // git not available, skip
            return;
        }

        let project_path_str = worktree_path.to_str().unwrap();

        let (volumes, working_dir) =
            compute_volume_paths(&worktree_path, project_path_str).unwrap();

        // For non-bare sibling worktrees: mount the main repo and worktree separately
        // as flat siblings under /workspace/.
        assert_eq!(
            volumes.len(),
            2,
            "Should have two volumes: main repo and worktree"
        );

        // First volume: the main repo
        let repo_canon = repo_path.canonicalize().unwrap();
        let mount0_canon = Path::new(&volumes[0].host_path).canonicalize().unwrap();
        assert_eq!(
            mount0_canon, repo_canon,
            "First volume should mount the main repo"
        );
        let repo_name = repo_canon.file_name().unwrap().to_string_lossy();
        assert_eq!(
            volumes[0].container_path,
            format!("/workspace/{}", repo_name),
        );

        // Second volume: the worktree
        let wt_canon = worktree_path.canonicalize().unwrap();
        let mount1_canon = Path::new(&volumes[1].host_path).canonicalize().unwrap();
        assert_eq!(
            mount1_canon, wt_canon,
            "Second volume should mount the worktree"
        );
        assert_eq!(volumes[1].container_path, "/workspace/my-worktree");

        // Working dir should point to the worktree
        assert_eq!(
            working_dir, "/workspace/my-worktree",
            "Working dir should be the worktree container path"
        );
    }

    #[test]
    fn test_compute_volume_paths_bare_repo_root() {
        let (_dir, main_repo_path, _worktree_path) = setup_bare_repo_with_worktree();

        let project_path_str = main_repo_path.to_str().unwrap();

        let (volumes, working_dir) =
            compute_volume_paths(&main_repo_path, project_path_str).unwrap();

        assert_eq!(volumes.len(), 1);

        // When at repo root, mount path equals project path
        let mount_canon = Path::new(&volumes[0].host_path).canonicalize().unwrap();
        let main_canon = main_repo_path.canonicalize().unwrap();
        assert_eq!(mount_canon, main_canon);

        // Working dir should be set
        assert!(!working_dir.is_empty());
    }

    #[test]
    fn test_compute_volume_paths_subdir_of_ancestor_repo_not_mounted() {
        // Simulates the scenario from GitHub issue #375: a user has a git repo at
        // their home directory (e.g., for dotfile management) and sets their project
        // path to a non-git subdirectory like ~/playground. Without the guard,
        // git2::Repository::discover walks up and finds the ancestor repo, causing
        // the entire parent (home directory) to be mounted into the container.
        let dir = TempDir::new().unwrap();

        // Create a git repo at the "parent" (simulating ~/  with dotfile management)
        let _repo = git2::Repository::init(dir.path()).unwrap();

        // Create a subdirectory that is NOT its own git repo (simulating ~/playground)
        let subdir = dir.path().join("playground");
        fs::create_dir_all(&subdir).unwrap();

        let project_path_str = subdir.to_str().unwrap();

        let (volumes, working_dir) = compute_volume_paths(&subdir, project_path_str).unwrap();

        assert_eq!(volumes.len(), 1);
        // The subdirectory should be mounted directly, NOT the parent repo
        assert_eq!(
            volumes[0].host_path,
            subdir.to_string_lossy().to_string(),
            "Should mount the subdirectory itself, not the ancestor git repo"
        );
        assert_eq!(volumes[0].container_path, working_dir);
        assert_eq!(volumes[0].container_path, "/workspace/playground");
    }

    #[test]
    fn test_common_ancestor() {
        assert_eq!(
            common_ancestor(Path::new("/a/b/c"), Path::new("/a/b/d")),
            PathBuf::from("/a/b")
        );
        assert_eq!(
            common_ancestor(Path::new("/a/b"), Path::new("/a/b")),
            PathBuf::from("/a/b")
        );
        assert_eq!(
            common_ancestor(Path::new("/a/b/c"), Path::new("/x/y/z")),
            PathBuf::from("/")
        );
    }

    #[test]
    fn test_compute_volume_paths_non_bare_worktree_nested_layout() {
        // Simulates a host layout where the worktree is nested deeper than the
        // main repo relative to their common ancestor (e.g., repo at
        // /scm/my-repo and worktree at /scm/worktrees/my-repo/1).
        let dir = TempDir::new().unwrap();
        let repo_path = dir.path().join("my-repo");
        fs::create_dir_all(&repo_path).unwrap();
        let repo = git2::Repository::init(&repo_path).unwrap();
        {
            let mut index = repo.index().unwrap();
            let oid = index.write_tree().unwrap();
            let sig = git2::Signature::now("test", "test@test.com").unwrap();
            let tree = repo.find_tree(oid).unwrap();
            repo.commit(Some("HEAD"), &sig, &sig, "init", &tree, &[])
                .unwrap();
        }

        let worktrees_dir = dir.path().join("worktrees").join("my-repo");
        fs::create_dir_all(&worktrees_dir).unwrap();
        let worktree_path = worktrees_dir.join("1");

        let head = repo.head().unwrap().peel_to_commit().unwrap().id();
        repo.branch("wt-branch", &repo.find_commit(head).unwrap(), false)
            .unwrap();
        drop(repo);

        let output = std::process::Command::new("git")
            .args([
                "worktree",
                "add",
                worktree_path.to_str().unwrap(),
                "wt-branch",
            ])
            .current_dir(&repo_path)
            .output()
            .unwrap();

        if !output.status.success() {
            return;
        }

        // AoE's create_worktree converts .git to relative paths via
        // convert_git_file_to_relative. Replicate that here since we
        // called git directly.
        let git_file = worktree_path.join(".git");
        let content = fs::read_to_string(&git_file).unwrap();
        let abs_path = content
            .lines()
            .find_map(|l| l.strip_prefix("gitdir:").map(str::trim))
            .unwrap();
        if Path::new(abs_path).is_absolute() {
            let wt_canon = worktree_path.canonicalize().unwrap();
            let gitdir_canon = Path::new(abs_path).canonicalize().unwrap();
            if let Some(rel) = crate::git::GitWorktree::diff_paths(&gitdir_canon, &wt_canon) {
                fs::write(&git_file, format!("gitdir: {}\n", rel.display())).unwrap();
            }
        }

        let project_path_str = worktree_path.to_str().unwrap();
        let (volumes, working_dir) =
            compute_volume_paths(&worktree_path, project_path_str).unwrap();

        assert_eq!(volumes.len(), 2);

        // The container paths must preserve relative depth so the .git file's
        // relative gitdir path resolves correctly.
        let repo_canon = repo_path.canonicalize().unwrap();
        let wt_canon = worktree_path.canonicalize().unwrap();
        let common = common_ancestor(&repo_canon, &wt_canon);
        let expected_repo = format!(
            "/workspace/{}",
            repo_canon.strip_prefix(&common).unwrap().display()
        );
        let expected_wt = format!(
            "/workspace/{}",
            wt_canon.strip_prefix(&common).unwrap().display()
        );

        assert_eq!(volumes[0].container_path, expected_repo);
        assert_eq!(volumes[1].container_path, expected_wt);
        assert_eq!(working_dir, expected_wt);

        // Verify the .git file's relative path resolves correctly in the
        // container layout.
        let content = fs::read_to_string(&git_file).unwrap();
        let gitdir_rel = content
            .lines()
            .find_map(|l| l.strip_prefix("gitdir:").map(str::trim))
            .unwrap();

        let resolved = PathBuf::from(&working_dir).join(gitdir_rel);

        // Normalize the path (resolve .. components)
        let mut normalized = Vec::new();
        for component in resolved.components() {
            match component {
                std::path::Component::ParentDir => {
                    normalized.pop();
                }
                c => normalized.push(c.as_os_str().to_owned()),
            }
        }
        let normalized: PathBuf = normalized.iter().collect();

        // Should land inside the main repo's .git/worktrees/ directory
        assert!(
            normalized
                .to_string_lossy()
                .starts_with(&volumes[0].container_path),
            "Resolved gitdir path '{}' should start with main repo container path '{}'",
            normalized.display(),
            volumes[0].container_path
        );
    }

    // --- sandbox config tests ---

    fn setup_host_dir(dir: &TempDir) -> std::path::PathBuf {
        let host = dir.path().join("host");
        fs::create_dir_all(&host).unwrap();
        fs::write(host.join("auth.json"), r#"{"token":"abc"}"#).unwrap();
        fs::write(host.join("settings.json"), "{}").unwrap();
        fs::create_dir_all(host.join("subdir")).unwrap();
        fs::write(host.join("subdir").join("nested.txt"), "nested").unwrap();
        host
    }

    #[test]
    fn test_copies_top_level_files_only() {
        let dir = TempDir::new().unwrap();
        let host = setup_host_dir(&dir);
        let sandbox = dir.path().join("sandbox");

        sync_agent_config(&host, &sandbox, &[], &[], &[], &[]).unwrap();

        assert!(sandbox.join("auth.json").exists());
        assert!(sandbox.join("settings.json").exists());
        assert!(!sandbox.join("subdir").exists());
    }

    #[test]
    fn test_skips_entries_in_skip_list() {
        let dir = TempDir::new().unwrap();
        let host = setup_host_dir(&dir);
        let sandbox = dir.path().join("sandbox");

        sync_agent_config(&host, &sandbox, &["auth.json"], &[], &[], &[]).unwrap();

        assert!(!sandbox.join("auth.json").exists());
        assert!(sandbox.join("settings.json").exists());
    }

    #[test]
    fn test_writes_seed_files_when_missing() {
        let dir = TempDir::new().unwrap();
        let host = setup_host_dir(&dir);
        let sandbox = dir.path().join("sandbox");

        let seeds = [("seed.json", r#"{"seeded":true}"#)];
        sync_agent_config(&host, &sandbox, &[], &seeds, &[], &[]).unwrap();

        let content = fs::read_to_string(sandbox.join("seed.json")).unwrap();
        assert_eq!(content, r#"{"seeded":true}"#);
    }

    #[test]
    fn test_seed_files_not_overwritten_if_exist() {
        let dir = TempDir::new().unwrap();
        let host = setup_host_dir(&dir);
        let sandbox = dir.path().join("sandbox");

        // First sync writes the seed.
        let seeds = [("seed.json", r#"{"seeded":true}"#)];
        sync_agent_config(&host, &sandbox, &[], &seeds, &[], &[]).unwrap();
        assert_eq!(
            fs::read_to_string(sandbox.join("seed.json")).unwrap(),
            r#"{"seeded":true}"#
        );

        // Container modifies the seed file.
        fs::write(sandbox.join("seed.json"), r#"{"modified":true}"#).unwrap();

        // Re-sync should NOT overwrite the container's changes.
        sync_agent_config(&host, &sandbox, &[], &seeds, &[], &[]).unwrap();
        assert_eq!(
            fs::read_to_string(sandbox.join("seed.json")).unwrap(),
            r#"{"modified":true}"#
        );
    }

    #[test]
    fn test_host_files_overwrite_seeds() {
        let dir = TempDir::new().unwrap();
        let host = setup_host_dir(&dir);
        let sandbox = dir.path().join("sandbox");

        // Seed has the same name as a host file -- host copy wins.
        let seeds = [("auth.json", "seed-content")];
        sync_agent_config(&host, &sandbox, &[], &seeds, &[], &[]).unwrap();

        let content = fs::read_to_string(sandbox.join("auth.json")).unwrap();
        assert_eq!(content, r#"{"token":"abc"}"#);
    }

    #[test]
    fn test_seed_survives_when_no_host_equivalent() {
        let dir = TempDir::new().unwrap();
        let host = setup_host_dir(&dir);
        let sandbox = dir.path().join("sandbox");

        let seeds = [(".claude.json", r#"{"hasCompletedOnboarding":true}"#)];
        sync_agent_config(&host, &sandbox, &[], &seeds, &[], &[]).unwrap();

        let content = fs::read_to_string(sandbox.join(".claude.json")).unwrap();
        assert_eq!(content, r#"{"hasCompletedOnboarding":true}"#);
    }

    #[test]
    fn test_creates_sandbox_dir_if_missing() {
        let dir = TempDir::new().unwrap();
        let host = setup_host_dir(&dir);
        let sandbox = dir.path().join("deep").join("nested").join("sandbox");

        sync_agent_config(&host, &sandbox, &[], &[], &[], &[]).unwrap();

        assert!(sandbox.exists());
        assert!(sandbox.join("auth.json").exists());
    }

    #[test]
    fn test_agent_config_mounts_have_valid_entries() {
        for mount in AGENT_CONFIG_MOUNTS {
            assert!(!mount.tool_name.is_empty());
            assert!(!mount.host_rel.is_empty());
            assert!(!mount.container_suffix.is_empty());
        }
    }

    #[test]
    fn test_agent_config_mounts_each_tool_has_exactly_one() {
        let tool_names: Vec<&str> = AGENT_CONFIG_MOUNTS.iter().map(|m| m.tool_name).collect();
        // Each tool name should appear exactly once
        for name in &tool_names {
            let count = tool_names.iter().filter(|n| *n == name).count();
            assert_eq!(count, 1, "tool_name '{}' appears {} times", name, count);
        }
    }

    #[test]
    fn test_agent_config_mounts_filter_by_tool() {
        let claude_mounts: Vec<_> = AGENT_CONFIG_MOUNTS
            .iter()
            .filter(|m| m.tool_name == "claude")
            .collect();
        assert_eq!(claude_mounts.len(), 1);
        assert_eq!(claude_mounts[0].host_rel, ".claude");

        let cursor_mounts: Vec<_> = AGENT_CONFIG_MOUNTS
            .iter()
            .filter(|m| m.tool_name == "cursor")
            .collect();
        assert_eq!(cursor_mounts.len(), 1);
        assert_eq!(cursor_mounts[0].host_rel, ".cursor");

        // Unknown tool should match nothing
        let unknown_mounts: Vec<_> = AGENT_CONFIG_MOUNTS
            .iter()
            .filter(|m| m.tool_name == "unknown")
            .collect();
        assert_eq!(unknown_mounts.len(), 0);
    }

    #[test]
    fn test_agent_config_mounts_match_agent_registry() {
        // Every mount should correspond to a registered agent
        for mount in AGENT_CONFIG_MOUNTS {
            assert!(
                crate::agents::get_agent(mount.tool_name).is_some(),
                "AGENT_CONFIG_MOUNTS entry '{}' has no matching agent in the registry",
                mount.tool_name
            );
        }
    }

    #[test]
    fn test_home_seed_files_written_to_sandbox_root() {
        let dir = TempDir::new().unwrap();
        let sandbox_base = dir.path().join("sandbox-root");
        fs::create_dir_all(&sandbox_base).unwrap();

        let home_seeds: &[(&str, &str)] = &[(".claude.json", r#"{"hasCompletedOnboarding":true}"#)];

        for &(filename, content) in home_seeds {
            let path = sandbox_base.join(filename);
            if !path.exists() {
                fs::write(path, content).unwrap();
            }
        }

        let written = fs::read_to_string(sandbox_base.join(".claude.json")).unwrap();
        assert_eq!(written, r#"{"hasCompletedOnboarding":true}"#);

        // Verify it's NOT inside an agent config subdirectory.
        assert!(!sandbox_base.join(".claude").join(".claude.json").exists());
    }

    #[test]
    fn test_home_seed_files_not_overwritten_if_exist() {
        let dir = TempDir::new().unwrap();
        let sandbox_base = dir.path().join("sandbox-root");
        fs::create_dir_all(&sandbox_base).unwrap();

        // First write.
        let path = sandbox_base.join(".claude.json");
        fs::write(&path, r#"{"hasCompletedOnboarding":true}"#).unwrap();

        // Container modifies it.
        fs::write(&path, r#"{"hasCompletedOnboarding":true,"extra":"data"}"#).unwrap();

        // Write-once logic should not overwrite.
        if !path.exists() {
            fs::write(&path, r#"{"hasCompletedOnboarding":true}"#).unwrap();
        }

        let content = fs::read_to_string(&path).unwrap();
        assert_eq!(content, r#"{"hasCompletedOnboarding":true,"extra":"data"}"#);
    }

    #[test]
    fn test_refresh_updates_changed_host_files() {
        let dir = TempDir::new().unwrap();
        let host = setup_host_dir(&dir);
        let sandbox = dir.path().join("sandbox");

        sync_agent_config(&host, &sandbox, &[], &[], &[], &[]).unwrap();
        assert_eq!(
            fs::read_to_string(sandbox.join("auth.json")).unwrap(),
            r#"{"token":"abc"}"#
        );

        // Host file changes between sessions.
        fs::write(host.join("auth.json"), r#"{"token":"refreshed"}"#).unwrap();

        sync_agent_config(&host, &sandbox, &[], &[], &[], &[]).unwrap();
        assert_eq!(
            fs::read_to_string(sandbox.join("auth.json")).unwrap(),
            r#"{"token":"refreshed"}"#
        );
    }

    #[test]
    fn test_refresh_picks_up_new_host_files() {
        let dir = TempDir::new().unwrap();
        let host = setup_host_dir(&dir);
        let sandbox = dir.path().join("sandbox");

        sync_agent_config(&host, &sandbox, &[], &[], &[], &[]).unwrap();
        assert!(!sandbox.join("new_cred.json").exists());

        // New credential file appears on host.
        fs::write(host.join("new_cred.json"), "new").unwrap();

        sync_agent_config(&host, &sandbox, &[], &[], &[], &[]).unwrap();
        assert_eq!(
            fs::read_to_string(sandbox.join("new_cred.json")).unwrap(),
            "new"
        );
    }

    #[test]
    fn test_refresh_preserves_container_written_files() {
        let dir = TempDir::new().unwrap();
        let host = setup_host_dir(&dir);
        let sandbox = dir.path().join("sandbox");

        sync_agent_config(&host, &sandbox, &[], &[], &[], &[]).unwrap();

        // Container writes a runtime file into the sandbox dir.
        fs::write(sandbox.join("runtime.log"), "container-state").unwrap();

        // Refresh from host.
        sync_agent_config(&host, &sandbox, &[], &[], &[], &[]).unwrap();

        // Container-written file survives (host has no file with that name).
        assert_eq!(
            fs::read_to_string(sandbox.join("runtime.log")).unwrap(),
            "container-state"
        );
    }

    #[test]
    fn test_copies_listed_dirs_recursively() {
        let dir = TempDir::new().unwrap();
        let host = setup_host_dir(&dir);

        // Create a "plugins" dir with nested content.
        let plugins = host.join("plugins");
        fs::create_dir_all(plugins.join("lsp")).unwrap();
        fs::write(plugins.join("config.json"), "{}").unwrap();
        fs::write(plugins.join("lsp").join("gopls.wasm"), "binary").unwrap();

        let sandbox = dir.path().join("sandbox");
        sync_agent_config(&host, &sandbox, &[], &[], &["plugins"], &[]).unwrap();

        assert!(sandbox.join("plugins").join("config.json").exists());
        assert!(sandbox
            .join("plugins")
            .join("lsp")
            .join("gopls.wasm")
            .exists());
        // "subdir" is NOT in copy_dirs, so still skipped.
        assert!(!sandbox.join("subdir").exists());
    }

    #[test]
    fn test_unlisted_dirs_still_skipped() {
        let dir = TempDir::new().unwrap();
        let host = setup_host_dir(&dir);

        // "subdir" exists from setup_host_dir but is not in copy_dirs.
        let sandbox = dir.path().join("sandbox");
        sync_agent_config(&host, &sandbox, &[], &[], &["nonexistent"], &[]).unwrap();

        assert!(!sandbox.join("subdir").exists());
        assert!(sandbox.join("auth.json").exists());
    }

    #[test]
    fn test_copy_dir_recursive() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src");
        fs::create_dir_all(src.join("a").join("b")).unwrap();
        fs::write(src.join("root.txt"), "root").unwrap();
        fs::write(src.join("a").join("mid.txt"), "mid").unwrap();
        fs::write(src.join("a").join("b").join("deep.txt"), "deep").unwrap();

        let dest = dir.path().join("dest");
        copy_dir_recursive(&src, &dest).unwrap();

        assert_eq!(fs::read_to_string(dest.join("root.txt")).unwrap(), "root");
        assert_eq!(
            fs::read_to_string(dest.join("a").join("mid.txt")).unwrap(),
            "mid"
        );
        assert_eq!(
            fs::read_to_string(dest.join("a").join("b").join("deep.txt")).unwrap(),
            "deep"
        );
    }

    #[test]
    fn test_symlinked_dirs_are_followed() {
        let dir = TempDir::new().unwrap();
        let host = dir.path().join("host");
        fs::create_dir_all(&host).unwrap();
        fs::write(host.join("config.json"), "{}").unwrap();

        // Create a real dir with content, then symlink to it from copy_dirs.
        let real_dir = dir.path().join("real-skills");
        fs::create_dir_all(&real_dir).unwrap();
        fs::write(real_dir.join("skill.md"), "# Skill").unwrap();

        #[cfg(unix)]
        std::os::unix::fs::symlink(&real_dir, host.join("skills")).unwrap();

        let sandbox = dir.path().join("sandbox");
        sync_agent_config(&host, &sandbox, &[], &[], &["skills"], &[]).unwrap();

        assert!(sandbox.join("config.json").exists());
        #[cfg(unix)]
        {
            assert!(sandbox.join("skills").exists());
            assert_eq!(
                fs::read_to_string(sandbox.join("skills").join("skill.md")).unwrap(),
                "# Skill"
            );
        }
    }

    #[test]
    fn test_bad_entry_does_not_fail_sync() {
        let dir = TempDir::new().unwrap();
        let host = dir.path().join("host");
        fs::create_dir_all(&host).unwrap();
        fs::write(host.join("good.json"), "ok").unwrap();

        // Create a symlink pointing to a nonexistent target.
        #[cfg(unix)]
        std::os::unix::fs::symlink("/nonexistent/path", host.join("broken-link")).unwrap();

        let sandbox = dir.path().join("sandbox");
        // Should succeed despite the broken symlink.
        sync_agent_config(&host, &sandbox, &[], &[], &[], &[]).unwrap();

        assert_eq!(fs::read_to_string(sandbox.join("good.json")).unwrap(), "ok");
        // Broken symlink is skipped, not copied.
        assert!(!sandbox.join("broken-link").exists());
    }

    #[test]
    fn test_preserve_files_not_overwritten() {
        let dir = TempDir::new().unwrap();
        let host = setup_host_dir(&dir);
        let sandbox = dir.path().join("sandbox");

        // First sync seeds the preserved file from host.
        sync_agent_config(&host, &sandbox, &[], &[], &[], &["auth.json"]).unwrap();
        assert_eq!(
            fs::read_to_string(sandbox.join("auth.json")).unwrap(),
            r#"{"token":"abc"}"#
        );

        // Simulate migration or in-container auth writing a different credential.
        fs::write(sandbox.join("auth.json"), r#"{"token":"container"}"#).unwrap();

        // Host file changes.
        fs::write(host.join("auth.json"), r#"{"token":"refreshed"}"#).unwrap();

        // Re-sync should NOT overwrite the preserved file.
        sync_agent_config(&host, &sandbox, &[], &[], &[], &["auth.json"]).unwrap();
        assert_eq!(
            fs::read_to_string(sandbox.join("auth.json")).unwrap(),
            r#"{"token":"container"}"#
        );

        // Non-preserved files are still overwritten.
        fs::write(host.join("settings.json"), "updated").unwrap();
        sync_agent_config(&host, &sandbox, &[], &[], &[], &["auth.json"]).unwrap();
        assert_eq!(
            fs::read_to_string(sandbox.join("settings.json")).unwrap(),
            "updated"
        );
    }

    #[test]
    fn test_history_preserved_across_resync() {
        let dir = TempDir::new().unwrap();
        let host = setup_host_dir(&dir);
        let sandbox = dir.path().join("sandbox");

        // Host has a history file with host-only entries.
        fs::write(host.join("history.jsonl"), "host-entry\n").unwrap();

        // First sync copies it in.
        sync_agent_config(&host, &sandbox, &[], &[], &[], &["history.jsonl"]).unwrap();
        assert_eq!(
            fs::read_to_string(sandbox.join("history.jsonl")).unwrap(),
            "host-entry\n"
        );

        // Container session appends entries.
        fs::write(
            sandbox.join("history.jsonl"),
            "host-entry\ncontainer-session-1\ncontainer-session-2\n",
        )
        .unwrap();

        // Re-sync (container restart) should NOT clobber the container's history.
        sync_agent_config(&host, &sandbox, &[], &[], &[], &["history.jsonl"]).unwrap();
        let content = fs::read_to_string(sandbox.join("history.jsonl")).unwrap();
        assert!(
            content.contains("container-session-1"),
            "container history entries must survive re-sync"
        );
    }

    #[test]
    fn test_has_prior_data_skips_general_file_copy() {
        let dir = TempDir::new().unwrap();
        let host = setup_host_dir(&dir);
        let sandbox = dir.path().join("sandbox");

        // First sync copies everything in.
        sync_agent_config(&host, &sandbox, &[], &[], &[], &[]).unwrap();
        assert_eq!(
            fs::read_to_string(sandbox.join("settings.json")).unwrap(),
            "{}"
        );

        // Simulate a prior container session by creating the "projects/" sentinel.
        fs::create_dir_all(sandbox.join("projects")).unwrap();

        // Container modifies settings.json during its session.
        fs::write(sandbox.join("settings.json"), r#"{"theme":"dark"}"#).unwrap();

        // Host updates settings.json independently.
        fs::write(host.join("settings.json"), r#"{"theme":"light"}"#).unwrap();

        // Re-sync should skip general file copies because projects/ exists,
        // preserving the container's settings.json.
        sync_agent_config(&host, &sandbox, &[], &[], &[], &[]).unwrap();
        assert_eq!(
            fs::read_to_string(sandbox.join("settings.json")).unwrap(),
            r#"{"theme":"dark"}"#,
            "container-side settings must not be overwritten when projects/ sentinel exists"
        );
    }

    #[test]
    fn test_preserve_files_seeded_when_missing() {
        let dir = TempDir::new().unwrap();
        let host = setup_host_dir(&dir);
        let sandbox = dir.path().join("sandbox");

        // Preserved file is copied when sandbox doesn't have it yet.
        sync_agent_config(&host, &sandbox, &[], &[], &[], &["auth.json"]).unwrap();
        assert_eq!(
            fs::read_to_string(sandbox.join("auth.json")).unwrap(),
            r#"{"token":"abc"}"#
        );
    }

    // --- hook relay and rewrite tests ---

    #[test]
    fn test_seed_hook_relay_script_creates_executable() {
        let dir = TempDir::new().unwrap();
        seed_hook_relay_script(dir.path()).unwrap();

        let script_path = dir.path().join("aoe_hook_relay.sh");
        assert!(script_path.exists());

        let content = fs::read_to_string(&script_path).unwrap();
        assert!(content.contains("STATUS_DIR="));
        assert!(content.contains("AOE_INSTANCE_ID"));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&script_path).unwrap().permissions().mode();
            assert!(mode & 0o111 != 0, "Script should be executable");
        }
    }

    #[test]
    fn test_seed_hook_relay_script_always_overwrites() {
        let dir = TempDir::new().unwrap();
        let script_path = dir.path().join("aoe_hook_relay.sh");

        fs::write(&script_path, "old-content").unwrap();

        seed_hook_relay_script(dir.path()).unwrap();

        let content = fs::read_to_string(&script_path).unwrap();
        assert_ne!(content, "old-content");
        assert!(content.contains("STATUS_DIR="));
    }

    #[test]
    fn test_rewrite_hooks_for_container_mixed() {
        let dir = TempDir::new().unwrap();
        let settings = serde_json::json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "",
                        "hooks": [{"type": "command", "command": "/usr/local/bin/aoe _hook", "async": true}]
                    },
                    {
                        "matcher": "Bash",
                        "hooks": [{"type": "command", "command": "user-linter check"}]
                    }
                ],
                "Stop": [
                    {
                        "matcher": "",
                        "hooks": [{"type": "command", "command": "/path/to/aoe _hook", "async": true}]
                    }
                ]
            }
        });
        fs::write(
            dir.path().join("settings.json"),
            serde_json::to_string_pretty(&settings).unwrap(),
        )
        .unwrap();

        rewrite_hooks_for_container(dir.path()).unwrap();

        let content = fs::read_to_string(dir.path().join("settings.json")).unwrap();
        let result: serde_json::Value = serde_json::from_str(&content).unwrap();

        // aoe hooks should be rewritten to relay path
        assert_eq!(
            result["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
            HOOK_RELAY_PATH
        );
        assert_eq!(
            result["hooks"]["Stop"][0]["hooks"][0]["command"],
            HOOK_RELAY_PATH
        );

        // User hook should be preserved unchanged
        assert_eq!(
            result["hooks"]["PreToolUse"][1]["hooks"][0]["command"],
            "user-linter check"
        );
    }

    #[test]
    fn test_rewrite_hooks_for_container_no_hooks_section() {
        let dir = TempDir::new().unwrap();
        let settings = serde_json::json!({"env": {"something": "value"}});
        fs::write(
            dir.path().join("settings.json"),
            serde_json::to_string_pretty(&settings).unwrap(),
        )
        .unwrap();

        rewrite_hooks_for_container(dir.path()).unwrap();

        // File should be unchanged (no "hooks" key to rewrite)
        let content = fs::read_to_string(dir.path().join("settings.json")).unwrap();
        let result: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert!(!result.as_object().unwrap().contains_key("hooks"));
        assert_eq!(result["env"]["something"], "value");
    }

    #[test]
    fn test_rewrite_hooks_for_container_no_settings_file() {
        let dir = TempDir::new().unwrap();
        // No settings.json exists -- should be a no-op
        rewrite_hooks_for_container(dir.path()).unwrap();
    }

    #[test]
    fn test_rewrite_hooks_for_container_quoted_aoe_path() {
        let dir = TempDir::new().unwrap();
        let settings = serde_json::json!({
            "hooks": {
                "SessionStart": [
                    {
                        "matcher": "",
                        "hooks": [{"type": "command", "command": "/path/to/aoe\" _hook", "async": true}]
                    }
                ]
            }
        });
        fs::write(
            dir.path().join("settings.json"),
            serde_json::to_string_pretty(&settings).unwrap(),
        )
        .unwrap();

        rewrite_hooks_for_container(dir.path()).unwrap();

        let content = fs::read_to_string(dir.path().join("settings.json")).unwrap();
        let result: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(
            result["hooks"]["SessionStart"][0]["hooks"][0]["command"],
            HOOK_RELAY_PATH
        );
    }

    // --- rewrite_plugins_for_container tests ---

    fn make_plugins_json(plugins: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "version": 2,
            "plugins": plugins
        })
    }

    #[test]
    fn test_rewrite_plugins_for_container_basic() {
        let dir = TempDir::new().unwrap();
        let home = TempDir::new().unwrap();
        let app_dir = TempDir::new().unwrap();
        let plugins_dir = dir.path().join("plugins");
        fs::create_dir_all(&plugins_dir).unwrap();

        let manifest = make_plugins_json(serde_json::json!({
            "linear@official": [{
                "scope": "project",
                "projectPath": "/Users/testuser/dev/myproject",
                "installPath": format!("{}/.claude/plugins/cache/official/linear/v1", home.path().display()),
                "version": "v1"
            }]
        }));
        fs::write(
            plugins_dir.join("installed_plugins.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let result = rewrite_plugins_for_container_with_app_dir(
            dir.path(),
            "test-container",
            "/Users/testuser/dev/myproject",
            "/workspace/myproject",
            home.path(),
            app_dir.path(),
        )
        .unwrap();

        assert!(result.is_some());
        let manifest_path = result.unwrap();
        let content = fs::read_to_string(&manifest_path).unwrap();
        let rewritten: serde_json::Value = serde_json::from_str(&content).unwrap();

        let entry = &rewritten["plugins"]["linear@official"][0];
        assert_eq!(entry["projectPath"], "/workspace/myproject");
        assert_eq!(
            entry["installPath"],
            "/root/.claude/plugins/cache/official/linear/v1"
        );
    }

    #[test]
    fn test_rewrite_plugins_for_container_user_scope() {
        let dir = TempDir::new().unwrap();
        let home = TempDir::new().unwrap();
        let app_dir = TempDir::new().unwrap();
        let plugins_dir = dir.path().join("plugins");
        fs::create_dir_all(&plugins_dir).unwrap();

        let manifest = make_plugins_json(serde_json::json!({
            "rust-analyzer@official": [{
                "scope": "user",
                "installPath": format!("{}/.claude/plugins/cache/official/rust-analyzer/1.0.0", home.path().display()),
                "version": "1.0.0"
            }]
        }));
        fs::write(
            plugins_dir.join("installed_plugins.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let result = rewrite_plugins_for_container_with_app_dir(
            dir.path(),
            "test-container-user",
            "/Users/testuser/dev/someproject",
            "/workspace/someproject",
            home.path(),
            app_dir.path(),
        )
        .unwrap();

        assert!(result.is_some());
        let manifest_path = result.unwrap();
        let content = fs::read_to_string(&manifest_path).unwrap();
        let rewritten: serde_json::Value = serde_json::from_str(&content).unwrap();

        let entry = &rewritten["plugins"]["rust-analyzer@official"][0];
        // User-scoped: no projectPath to rewrite, only installPath
        assert!(entry.get("projectPath").is_none());
        assert_eq!(
            entry["installPath"],
            "/root/.claude/plugins/cache/official/rust-analyzer/1.0.0"
        );
    }

    #[test]
    fn test_rewrite_plugins_for_container_mixed_projects() {
        let dir = TempDir::new().unwrap();
        let home = TempDir::new().unwrap();
        let app_dir = TempDir::new().unwrap();
        let plugins_dir = dir.path().join("plugins");
        fs::create_dir_all(&plugins_dir).unwrap();

        let manifest = make_plugins_json(serde_json::json!({
            "plugin-a@repo": [{
                "scope": "project",
                "projectPath": "/Users/testuser/dev/myproject",
                "installPath": format!("{}/.claude/plugins/cache/repo/plugin-a/v1", home.path().display()),
                "version": "v1"
            }],
            "plugin-b@repo": [{
                "scope": "project",
                "projectPath": "/Users/testuser/dev/other-project",
                "installPath": format!("{}/.claude/plugins/cache/repo/plugin-b/v1", home.path().display()),
                "version": "v1"
            }]
        }));
        fs::write(
            plugins_dir.join("installed_plugins.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let result = rewrite_plugins_for_container_with_app_dir(
            dir.path(),
            "test-container-mixed",
            "/Users/testuser/dev/myproject",
            "/workspace/myproject",
            home.path(),
            app_dir.path(),
        )
        .unwrap();

        assert!(result.is_some());
        let manifest_path = result.unwrap();
        let content = fs::read_to_string(&manifest_path).unwrap();
        let rewritten: serde_json::Value = serde_json::from_str(&content).unwrap();

        // plugin-a: matches host project, should be rewritten
        assert_eq!(
            rewritten["plugins"]["plugin-a@repo"][0]["projectPath"],
            "/workspace/myproject"
        );

        // plugin-b: different project, projectPath should be unchanged
        assert_eq!(
            rewritten["plugins"]["plugin-b@repo"][0]["projectPath"],
            "/Users/testuser/dev/other-project"
        );

        // Both should have installPath rewritten (install paths are independent of project)
        assert!(rewritten["plugins"]["plugin-a@repo"][0]["installPath"]
            .as_str()
            .unwrap()
            .starts_with("/root/.claude/"));
        assert!(rewritten["plugins"]["plugin-b@repo"][0]["installPath"]
            .as_str()
            .unwrap()
            .starts_with("/root/.claude/"));
    }

    #[test]
    fn test_rewrite_plugins_for_container_no_file() {
        let dir = TempDir::new().unwrap();
        let home = TempDir::new().unwrap();

        let result = rewrite_plugins_for_container(
            dir.path(),
            "test-container-nofile",
            "/Users/testuser/dev/myproject",
            "/workspace/myproject",
            home.path(),
        )
        .unwrap();

        assert!(result.is_none());
    }

    #[test]
    fn test_rewrite_plugins_for_container_worktree() {
        // Set up a git repo with a worktree so find_main_repo works
        let (repo_dir, repo_path) = setup_regular_repo();
        let git_wt = crate::git::GitWorktree::new(repo_path.clone()).unwrap();
        let wt_path = repo_dir.path().join("feature-worktree");
        git_wt
            .create_worktree("feature-branch", &wt_path, true)
            .unwrap();

        let home = TempDir::new().unwrap();
        let app_dir = TempDir::new().unwrap();
        let dir = TempDir::new().unwrap();
        let plugins_dir = dir.path().join("plugins");
        fs::create_dir_all(&plugins_dir).unwrap();

        // Plugin registered against the main repo path (not the worktree)
        let main_repo_canonical = repo_path.canonicalize().unwrap();
        let manifest = make_plugins_json(serde_json::json!({
            "plugin-a@repo": [{
                "scope": "project",
                "projectPath": main_repo_canonical.to_string_lossy().to_string(),
                "installPath": format!("{}/.claude/plugins/cache/repo/plugin-a/v1", home.path().display()),
                "version": "v1"
            }]
        }));
        fs::write(
            plugins_dir.join("installed_plugins.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        // We're creating a container for the worktree, but plugins are registered to main repo
        let wt_path_str = wt_path.to_string_lossy().to_string();
        let result = rewrite_plugins_for_container_with_app_dir(
            dir.path(),
            "test-container-wt",
            &wt_path_str,
            "/workspace/feature-branch",
            home.path(),
            app_dir.path(),
        )
        .unwrap();

        assert!(result.is_some());
        let manifest_path = result.unwrap();
        let content = fs::read_to_string(&manifest_path).unwrap();
        let rewritten: serde_json::Value = serde_json::from_str(&content).unwrap();

        // Plugin's projectPath (main repo) should be rewritten to the workspace path
        assert_eq!(
            rewritten["plugins"]["plugin-a@repo"][0]["projectPath"],
            "/workspace/feature-branch"
        );

        drop(repo_dir);
    }

    #[test]
    fn test_rewrite_plugins_for_container_local_scope() {
        let dir = TempDir::new().unwrap();
        let home = TempDir::new().unwrap();
        let app_dir = TempDir::new().unwrap();
        let plugins_dir = dir.path().join("plugins");
        fs::create_dir_all(&plugins_dir).unwrap();

        let manifest = make_plugins_json(serde_json::json!({
            "custom-tool@local": [{
                "scope": "local",
                "projectPath": "/Users/testuser/dev/myproject",
                "installPath": format!("{}/.claude/plugins/cache/local/custom-tool/v1", home.path().display()),
                "version": "v1"
            }]
        }));
        fs::write(
            plugins_dir.join("installed_plugins.json"),
            serde_json::to_string_pretty(&manifest).unwrap(),
        )
        .unwrap();

        let result = rewrite_plugins_for_container_with_app_dir(
            dir.path(),
            "test-container-local",
            "/Users/testuser/dev/myproject",
            "/workspace/myproject",
            home.path(),
            app_dir.path(),
        )
        .unwrap();

        assert!(result.is_some());
        let manifest_path = result.unwrap();
        let content = fs::read_to_string(&manifest_path).unwrap();
        let rewritten: serde_json::Value = serde_json::from_str(&content).unwrap();

        let entry = &rewritten["plugins"]["custom-tool@local"][0];
        // Local-scoped: both paths should be rewritten (same as project scope)
        assert_eq!(entry["projectPath"], "/workspace/myproject");
        assert_eq!(
            entry["installPath"],
            "/root/.claude/plugins/cache/local/custom-tool/v1"
        );
    }

    #[test]
    fn test_cleanup_plugin_manifest() {
        let app_dir = TempDir::new().unwrap();
        let manifests_dir = app_dir.path().join("plugin_manifests");
        fs::create_dir_all(&manifests_dir).unwrap();

        let manifest_path = manifests_dir.join("test-cleanup-container.json");
        fs::write(&manifest_path, "{}").unwrap();
        assert!(manifest_path.exists());

        cleanup_plugin_manifest_in_dir("test-cleanup-container", app_dir.path());
        assert!(!manifest_path.exists());
    }

    // --- rewrite_marketplaces_for_container tests ---

    #[test]
    fn test_rewrite_marketplaces_for_container() {
        let dir = TempDir::new().unwrap();
        let plugins_dir = dir.path().join("plugins");
        fs::create_dir_all(&plugins_dir).unwrap();

        let home = dirs::home_dir().unwrap();
        let host_prefix = format!("{}/.claude/", home.display());

        let marketplaces = serde_json::json!({
            "@anthropic/claude-code-base-tools": {
                "name": "Base Tools",
                "installLocation": format!("{}plugins/marketplaces/@anthropic/claude-code-base-tools/v1", host_prefix)
            },
            "@acme/custom-tool": {
                "name": "Custom Tool",
                "installLocation": format!("{}plugins/marketplaces/@acme/custom-tool/v2", host_prefix)
            }
        });
        fs::write(
            plugins_dir.join("known_marketplaces.json"),
            serde_json::to_string_pretty(&marketplaces).unwrap(),
        )
        .unwrap();

        rewrite_marketplaces_for_container(dir.path()).unwrap();

        let content = fs::read_to_string(plugins_dir.join("known_marketplaces.json")).unwrap();
        let result: serde_json::Value = serde_json::from_str(&content).unwrap();

        assert_eq!(
            result["@anthropic/claude-code-base-tools"]["installLocation"],
            "/root/.claude/plugins/marketplaces/@anthropic/claude-code-base-tools/v1"
        );
        assert_eq!(
            result["@acme/custom-tool"]["installLocation"],
            "/root/.claude/plugins/marketplaces/@acme/custom-tool/v2"
        );

        // Idempotency: calling again should produce the same result
        rewrite_marketplaces_for_container(dir.path()).unwrap();
        let content2 = fs::read_to_string(plugins_dir.join("known_marketplaces.json")).unwrap();
        assert_eq!(content, content2);
    }

    #[test]
    fn test_rewrite_marketplaces_preserves_non_install_fields() {
        let dir = TempDir::new().unwrap();
        let plugins_dir = dir.path().join("plugins");
        fs::create_dir_all(&plugins_dir).unwrap();

        let home = dirs::home_dir().unwrap();
        let host_prefix = format!("{}/.claude/", home.display());

        let marketplaces = serde_json::json!({
            "@anthropic/claude-code-base-tools": {
                "name": "Base Tools",
                "installLocation": format!("{}plugins/marketplaces/@anthropic/claude-code-base-tools/v1", host_prefix),
                "lastUpdated": "2025-06-15T10:30:00Z",
                "source": "marketplace",
                "version": "1.2.3"
            }
        });
        fs::write(
            plugins_dir.join("known_marketplaces.json"),
            serde_json::to_string_pretty(&marketplaces).unwrap(),
        )
        .unwrap();

        rewrite_marketplaces_for_container(dir.path()).unwrap();

        let content = fs::read_to_string(plugins_dir.join("known_marketplaces.json")).unwrap();
        let result: serde_json::Value = serde_json::from_str(&content).unwrap();

        let entry = &result["@anthropic/claude-code-base-tools"];
        // installLocation should be rewritten
        assert_eq!(
            entry["installLocation"],
            "/root/.claude/plugins/marketplaces/@anthropic/claude-code-base-tools/v1"
        );
        // All other fields must survive unchanged
        assert_eq!(entry["name"], "Base Tools");
        assert_eq!(entry["lastUpdated"], "2025-06-15T10:30:00Z");
        assert_eq!(entry["source"], "marketplace");
        assert_eq!(entry["version"], "1.2.3");
    }

    #[test]
    fn test_rewrite_marketplaces_for_container_no_file() {
        let dir = TempDir::new().unwrap();
        // No plugins directory at all -- should succeed silently
        rewrite_marketplaces_for_container(dir.path()).unwrap();
    }

    #[test]
    fn test_rewrite_marketplaces_for_container_foreign_paths_untouched() {
        let dir = TempDir::new().unwrap();
        let plugins_dir = dir.path().join("plugins");
        fs::create_dir_all(&plugins_dir).unwrap();

        let marketplaces = serde_json::json!({
            "@other/tool": {
                "name": "Other Tool",
                "installLocation": "/some/other/path/tool"
            }
        });
        fs::write(
            plugins_dir.join("known_marketplaces.json"),
            serde_json::to_string_pretty(&marketplaces).unwrap(),
        )
        .unwrap();

        let before = fs::read_to_string(plugins_dir.join("known_marketplaces.json")).unwrap();
        rewrite_marketplaces_for_container(dir.path()).unwrap();
        let after = fs::read_to_string(plugins_dir.join("known_marketplaces.json")).unwrap();

        // File should not have been rewritten (no changes = no write)
        assert_eq!(before, after);
    }

    // --- credential freshness tests ---

    #[test]
    fn test_parse_credential_expires_at_valid() {
        let json = r#"{"claudeAiOauth":{"expiresAt":1700000000}}"#;
        assert_eq!(parse_credential_expires_at(json), Some(1700000000));
    }

    #[test]
    fn test_parse_credential_expires_at_missing_key() {
        // Missing claudeAiOauth entirely.
        assert_eq!(parse_credential_expires_at(r#"{"other":"data"}"#), None);
        // Missing expiresAt inside claudeAiOauth.
        assert_eq!(
            parse_credential_expires_at(r#"{"claudeAiOauth":{"token":"abc"}}"#),
            None
        );
    }

    #[test]
    fn test_parse_credential_expires_at_invalid_json() {
        assert_eq!(parse_credential_expires_at("not json at all"), None);
        assert_eq!(parse_credential_expires_at(""), None);
    }

    #[test]
    fn test_parse_credential_expires_at_wrong_type() {
        // expiresAt is a string instead of a number.
        let json = r#"{"claudeAiOauth":{"expiresAt":"1700000000"}}"#;
        assert_eq!(parse_credential_expires_at(json), None);
    }

    #[test]
    fn test_should_not_overwrite_with_stale_keychain() {
        let sandbox = r#"{"claudeAiOauth":{"expiresAt":2000}}"#;
        let keychain = r#"{"claudeAiOauth":{"expiresAt":1000}}"#;
        assert!(!should_overwrite_credential(sandbox, keychain));
    }

    #[test]
    fn test_should_overwrite_with_fresh_keychain() {
        let sandbox = r#"{"claudeAiOauth":{"expiresAt":1000}}"#;
        let keychain = r#"{"claudeAiOauth":{"expiresAt":2000}}"#;
        assert!(should_overwrite_credential(sandbox, keychain));
    }

    #[test]
    fn test_should_not_overwrite_equal_timestamps() {
        let cred = r#"{"claudeAiOauth":{"expiresAt":1000}}"#;
        assert!(!should_overwrite_credential(cred, cred));
    }

    #[test]
    fn test_should_not_overwrite_when_keychain_unparseable() {
        let sandbox = r#"{"claudeAiOauth":{"expiresAt":1000}}"#;
        assert!(!should_overwrite_credential(sandbox, "not-json"));
    }

    #[test]
    fn test_should_overwrite_when_both_unparseable() {
        assert!(should_overwrite_credential("bad", "also-bad"));
    }

    #[test]
    fn test_should_overwrite_when_only_keychain_parseable() {
        let keychain = r#"{"claudeAiOauth":{"expiresAt":1000}}"#;
        assert!(should_overwrite_credential("not-json", keychain));
    }
}
