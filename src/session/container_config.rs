//! Container configuration building for sandboxed sessions.
//!
//! Standalone functions for computing Docker volume mounts and building
//! `ContainerConfig` structs. Includes sandbox directory sync, agent config
//! mounting, and credential extraction.

use std::path::Path;

use anyhow::Result;

use crate::containers::{ContainerConfig, VolumeMount};
use crate::git::GitWorktree;

use super::environment::{collect_env_keys, collect_env_values};
use super::instance::SandboxInfo;

/// Subdirectory name inside each agent's config dir for the shared sandbox config.
const SANDBOX_SUBDIR: &str = "sandbox";

/// Declarative definition of an agent CLI's config directory for sandbox mounting.
struct AgentConfigMount {
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
        host_rel: ".cursor",
        container_suffix: ".cursor",
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
/// For bare repo worktrees, mounts the entire bare repo and sets working_dir to the worktree.
/// This allows git commands inside the container to access the full repository structure.
///
/// `project_path_str` is the raw project path string (used as the host mount path in the
/// default case where no bare repo is detected).
///
/// Returns (host_mount_path, container_mount_path, working_dir)
pub(crate) fn compute_volume_paths(
    project_path: &Path,
    project_path_str: &str,
) -> Result<(String, String, String)> {
    // Try to find the main repo if this is a git repository
    if let Ok(main_repo) = GitWorktree::find_main_repo(project_path) {
        // Canonicalize paths for reliable comparison (handles symlinks like /tmp -> /private/tmp)
        let main_repo_canonical = main_repo
            .canonicalize()
            .unwrap_or_else(|_| main_repo.clone());
        let project_canonical = project_path
            .canonicalize()
            .unwrap_or_else(|_| project_path.to_path_buf());

        // Check if main repo is a bare repo and project_path is a worktree within it
        if GitWorktree::is_bare_repo(&main_repo) && main_repo_canonical != project_canonical {
            // Bare repo worktree: mount the entire repo, set working_dir to the worktree
            let repo_name = main_repo_canonical
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "workspace".to_string());

            // Calculate relative path from main_repo to project_path (using canonical paths)
            let relative_worktree = project_canonical
                .strip_prefix(&main_repo_canonical)
                .map(|p| p.to_path_buf())
                .unwrap_or_default();

            let container_base = format!("/workspace/{}", repo_name);
            let working_dir = if relative_worktree.as_os_str().is_empty() {
                container_base.clone()
            } else {
                format!("{}/{}", container_base, relative_worktree.display())
            };

            return Ok((
                main_repo_canonical.to_string_lossy().to_string(),
                container_base,
                working_dir,
            ));
        }
    }

    // Default behavior: mount project_path directly
    let dir_name = project_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "workspace".to_string());
    let workspace_path = format!("/workspace/{}", dir_name);

    Ok((
        project_path_str.to_string(),
        workspace_path.clone(),
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
        if let Err(e) = prepare_sandbox_dir(mount, &home) {
            tracing::warn!(
                "Failed to refresh agent config for {}: {}",
                mount.host_rel,
                e
            );
        }
    }
}

/// Build a full `ContainerConfig` for creating a sandboxed container.
pub(crate) fn build_container_config(
    project_path_str: &str,
    sandbox_info: &SandboxInfo,
    tool: &str,
    is_yolo_mode: bool,
) -> Result<ContainerConfig> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;

    let project_path = Path::new(project_path_str);

    // Determine mount path and working directory.
    // For bare repo worktrees, mount the entire bare repo and set working_dir to the worktree.
    // This allows git commands to access the full repository structure.
    let (mount_host_path, container_base_path, workspace_path) =
        compute_volume_paths(project_path, project_path_str)?;

    let mut volumes = vec![VolumeMount {
        host_path: mount_host_path,
        container_path: container_base_path,
        read_only: false,
    }];

    let sandbox_config = match super::config::Config::load() {
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
    };

    const CONTAINER_HOME: &str = "/root";

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

    let opencode_config = home.join(".config").join("opencode");
    if opencode_config.exists() {
        volumes.push(VolumeMount {
            host_path: opencode_config.to_string_lossy().to_string(),
            container_path: format!("{}/.config/opencode", CONTAINER_HOME),
            read_only: true,
        });
    }

    // Sync host agent config into a shared sandbox directory per agent and
    // bind-mount it read-write. All containers share the same directory (1:N),
    // so in-container changes persist.
    // Agent definitions are in AGENT_CONFIG_MOUNTS -- add new agents there, not here.
    for mount in AGENT_CONFIG_MOUNTS {
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

        tracing::debug!(
            "Sandbox dir ready for {}, binding {} -> {}",
            mount.host_rel,
            sandbox_dir.display(),
            container_path
        );
        volumes.push(VolumeMount {
            host_path: sandbox_dir.to_string_lossy().to_string(),
            container_path,
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
    }

    let env_keys = collect_env_keys(&sandbox_config, sandbox_info);

    let mut environment: Vec<(String, String)> = env_keys
        .iter()
        .filter_map(|key| std::env::var(key).ok().map(|val| (key.clone(), val)))
        .collect();

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

    environment.extend(collect_env_values(&sandbox_config, sandbox_info));

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

    Ok(ContainerConfig {
        working_dir: workspace_path,
        volumes,
        anonymous_volumes,
        environment,
        cpu_limit: sandbox_config.cpu_limit,
        memory_limit: sandbox_config.memory_limit,
    })
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

        let (mount_path, container_path, working_dir) =
            compute_volume_paths(&repo_path, project_path_str).unwrap();

        // Regular repo: mount path should be the project path
        assert_eq!(mount_path, repo_path.to_string_lossy().to_string());
        // Container path and working dir should be the same
        assert_eq!(container_path, working_dir);
        // Should be /workspace/{dir_name}
        let dir_name = repo_path.file_name().unwrap().to_string_lossy();
        assert_eq!(container_path, format!("/workspace/{}", dir_name));
    }

    #[test]
    fn test_compute_volume_paths_non_git_directory() {
        let dir = TempDir::new().unwrap();
        let project_path_str = dir.path().to_str().unwrap();

        let (mount_path, container_path, working_dir) =
            compute_volume_paths(dir.path(), project_path_str).unwrap();

        // Non-git: mount path should be the project path
        assert_eq!(mount_path, dir.path().to_string_lossy().to_string());
        // Container path and working dir should be the same
        assert_eq!(container_path, working_dir);
    }

    #[test]
    fn test_compute_volume_paths_bare_repo_worktree() {
        let (_dir, main_repo_path, worktree_path) = setup_bare_repo_with_worktree();

        // Skip if worktree wasn't created (git might not be available)
        if !worktree_path.exists() {
            return;
        }

        let project_path_str = worktree_path.to_str().unwrap();

        let (mount_path, container_path, working_dir) =
            compute_volume_paths(&worktree_path, project_path_str).unwrap();

        // Canonicalize paths for comparison (handles /var -> /private/var on macOS)
        let mount_path_canon = Path::new(&mount_path).canonicalize().unwrap();
        let main_repo_canon = main_repo_path.canonicalize().unwrap();

        // For bare repo worktree: mount the entire repo root
        assert_eq!(
            mount_path_canon, main_repo_canon,
            "Should mount the bare repo root, not just the worktree"
        );

        // Container path should be /workspace/{repo_name}
        let repo_name = main_repo_path.file_name().unwrap().to_string_lossy();
        assert_eq!(
            container_path,
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
    fn test_compute_volume_paths_bare_repo_root() {
        let (_dir, main_repo_path, _worktree_path) = setup_bare_repo_with_worktree();

        let project_path_str = main_repo_path.to_str().unwrap();

        let (mount_path, _container_path, working_dir) =
            compute_volume_paths(&main_repo_path, project_path_str).unwrap();

        // When at repo root, mount path equals project path
        let mount_canon = Path::new(&mount_path).canonicalize().unwrap();
        let main_canon = main_repo_path.canonicalize().unwrap();
        assert_eq!(mount_canon, main_canon);

        // Working dir should be set
        assert!(!working_dir.is_empty());
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
            assert!(!mount.host_rel.is_empty());
            assert!(!mount.container_suffix.is_empty());
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
