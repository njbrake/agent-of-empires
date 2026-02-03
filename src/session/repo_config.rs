//! Repository-level configuration (`.aoe/config.toml`)
//!
//! Allows repos to define hooks and override session/sandbox/worktree settings.
//! Settings that are personal/global (theme, updates, tmux, claude config_dir) are
//! intentionally not overridable at the repo level.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

/// Progress messages streamed from hook execution.
#[derive(Debug, Clone)]
pub enum HookProgress {
    /// A new hook command is starting.
    Started(String),
    /// A line of stdout/stderr output from the running hook.
    Output(String),
}

use super::config::Config;
use super::profile_config::{
    HooksConfigOverride, ProfileConfig, SandboxConfigOverride, SessionConfigOverride,
    TmuxConfigOverride, UpdatesConfigOverride, WorktreeConfigOverride,
};

/// Repository-level configuration loaded from `.aoe/config.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RepoConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hooks: Option<HooksConfig>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session: Option<SessionConfigOverride>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<SandboxConfigOverride>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree: Option<WorktreeConfigOverride>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updates: Option<UpdatesConfigOverride>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tmux: Option<TmuxConfigOverride>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sound: Option<crate::sound::SoundConfigOverride>,
}

/// Hook commands to run at various lifecycle points.
///
/// Failure semantics differ by hook type:
/// - `on_create`: failures abort session creation (hard failure).
/// - `on_launch`: failures are logged as warnings but do not prevent the session
///   from starting, since blocking an existing session on a transient hook failure
///   would be disruptive.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HooksConfig {
    /// Commands run once when a session is first created.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on_create: Vec<String>,

    /// Commands run every time a session starts (failures are non-fatal).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on_launch: Vec<String>,
}

impl HooksConfig {
    pub fn is_empty(&self) -> bool {
        self.on_create.is_empty() && self.on_launch.is_empty()
    }
}

/// Path to the repo config file relative to the project root.
const REPO_CONFIG_PATH: &str = ".aoe/config.toml";

/// Load repo config from `<project_path>/.aoe/config.toml`.
/// Returns `None` if the file doesn't exist.
pub fn load_repo_config(project_path: &Path) -> Result<Option<RepoConfig>> {
    let config_path = project_path.join(REPO_CONFIG_PATH);
    if !config_path.exists() {
        return Ok(None);
    }

    let content = fs::read_to_string(&config_path)
        .with_context(|| format!("Failed to read {}", config_path.display()))?;

    if content.trim().is_empty() {
        return Ok(None);
    }

    let config: RepoConfig = toml::from_str(&content)
        .with_context(|| format!("Failed to parse {}", config_path.display()))?;

    Ok(Some(config))
}

/// Save repo config to `<project_path>/.aoe/config.toml`.
/// Creates the `.aoe/` directory if it does not exist.
pub fn save_repo_config(project_path: &Path, config: &RepoConfig) -> Result<()> {
    let aoe_dir = project_path.join(".aoe");
    if !aoe_dir.exists() {
        fs::create_dir_all(&aoe_dir)
            .with_context(|| format!("Failed to create {}", aoe_dir.display()))?;
    }

    let config_path = project_path.join(REPO_CONFIG_PATH);
    let content = toml::to_string_pretty(config)
        .with_context(|| "Failed to serialize repo config".to_string())?;

    fs::write(&config_path, content)
        .with_context(|| format!("Failed to write {}", config_path.display()))?;

    Ok(())
}

/// Merge repo config overrides into an already-resolved config (global + profile).
pub fn merge_repo_config(mut config: Config, repo: &RepoConfig) -> Config {
    use super::profile_config::{
        apply_sandbox_overrides, apply_session_overrides, apply_tmux_overrides,
        apply_worktree_overrides,
    };

    if let Some(ref session_override) = repo.session {
        apply_session_overrides(&mut config.session, session_override);
    }

    if let Some(ref sandbox_override) = repo.sandbox {
        apply_sandbox_overrides(&mut config.sandbox, sandbox_override);
    }

    if let Some(ref worktree_override) = repo.worktree {
        apply_worktree_overrides(&mut config.worktree, worktree_override);
    }

    if let Some(ref hooks) = repo.hooks {
        if !hooks.on_create.is_empty() {
            config.hooks.on_create = hooks.on_create.clone();
        }
        if !hooks.on_launch.is_empty() {
            config.hooks.on_launch = hooks.on_launch.clone();
        }
    }

    if let Some(ref updates_override) = repo.updates {
        if let Some(check_enabled) = updates_override.check_enabled {
            config.updates.check_enabled = check_enabled;
        }
        if let Some(auto_update) = updates_override.auto_update {
            config.updates.auto_update = auto_update;
        }
        if let Some(check_interval_hours) = updates_override.check_interval_hours {
            config.updates.check_interval_hours = check_interval_hours;
        }
        if let Some(notify_in_cli) = updates_override.notify_in_cli {
            config.updates.notify_in_cli = notify_in_cli;
        }
    }

    if let Some(ref tmux_override) = repo.tmux {
        apply_tmux_overrides(&mut config.tmux, tmux_override);
    }

    if let Some(ref sound_override) = repo.sound {
        crate::sound::apply_sound_overrides(&mut config.sound, sound_override);
    }

    config
}

/// Convert a RepoConfig into a ProfileConfig for TUI editing.
/// This allows the settings TUI to reuse the same field infrastructure
/// for all three scopes (Global, Profile, Repo).
pub fn repo_config_to_profile(repo: &RepoConfig) -> ProfileConfig {
    ProfileConfig {
        updates: repo.updates.clone(),
        worktree: repo.worktree.clone(),
        sandbox: repo.sandbox.clone(),
        tmux: repo.tmux.clone(),
        session: repo.session.clone(),
        sound: repo.sound.clone(),
        hooks: repo.hooks.as_ref().map(|h| HooksConfigOverride {
            on_create: if h.on_create.is_empty() {
                None
            } else {
                Some(h.on_create.clone())
            },
            on_launch: if h.on_launch.is_empty() {
                None
            } else {
                Some(h.on_launch.clone())
            },
        }),
        ..Default::default()
    }
}

/// Convert a ProfileConfig back into a RepoConfig after TUI editing.
pub fn profile_to_repo_config(profile: &ProfileConfig) -> RepoConfig {
    RepoConfig {
        hooks: profile.hooks.as_ref().map(|h| HooksConfig {
            on_create: h.on_create.clone().unwrap_or_default(),
            on_launch: h.on_launch.clone().unwrap_or_default(),
        }),
        session: profile.session.clone(),
        sandbox: profile.sandbox.clone(),
        worktree: profile.worktree.clone(),
        updates: profile.updates.clone(),
        tmux: profile.tmux.clone(),
        sound: profile.sound.clone(),
    }
}

/// Resolve config with repo overrides: global -> profile -> repo.
pub fn resolve_config_with_repo(profile: &str, project_path: &Path) -> Result<Config> {
    let config = super::profile_config::resolve_config(profile)?;

    match load_repo_config(project_path)? {
        Some(repo_config) => Ok(merge_repo_config(config, &repo_config)),
        None => Ok(config),
    }
}

// ---------------------------------------------------------------------------
// Hook trust system
// ---------------------------------------------------------------------------

/// A single trusted repo entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TrustedRepo {
    path: String,
    hooks_hash: String,
    trusted_at: String,
}

/// Top-level structure for `trusted_repos.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct TrustedRepos {
    #[serde(default)]
    repos: Vec<TrustedRepo>,
}

/// Compute a SHA-256 hash of the hook commands for change detection.
pub fn compute_hooks_hash(hooks: &HooksConfig) -> String {
    let mut hasher = Sha256::new();
    for cmd in &hooks.on_create {
        hasher.update(b"on_create:");
        hasher.update(cmd.as_bytes());
        hasher.update(b"\n");
    }
    for cmd in &hooks.on_launch {
        hasher.update(b"on_launch:");
        hasher.update(cmd.as_bytes());
        hasher.update(b"\n");
    }
    format!("{:x}", hasher.finalize())
}

/// Path to the global trust store. Trust decisions are shared across all
/// profiles so that a repo trusted in one profile doesn't require re-approval
/// in another.
fn trusted_repos_path() -> Result<PathBuf> {
    Ok(super::get_app_dir()?.join("trusted_repos.toml"))
}

fn load_trusted_repos() -> Result<TrustedRepos> {
    let path = trusted_repos_path()?;
    if !path.exists() {
        return Ok(TrustedRepos::default());
    }
    let content = fs::read_to_string(&path)?;
    if content.trim().is_empty() {
        return Ok(TrustedRepos::default());
    }
    Ok(toml::from_str(&content)?)
}

/// Normalize a path by canonicalizing it, with fallback to the original string.
fn normalize_path(path: &Path) -> String {
    std::fs::canonicalize(path)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string_lossy().to_string())
}

/// Check if a repo's hooks are trusted (hash matches stored trust entry).
/// Normalizes `project_path` before lookup.
pub fn is_repo_trusted(project_path: &Path, hooks_hash: &str) -> Result<bool> {
    let normalized = normalize_path(project_path);
    is_repo_trusted_normalized(&normalized, hooks_hash)
}

/// Like `is_repo_trusted` but expects an already-normalized path.
fn is_repo_trusted_normalized(normalized_path: &str, hooks_hash: &str) -> Result<bool> {
    let trusted = load_trusted_repos()?;
    Ok(trusted
        .repos
        .iter()
        .any(|r| r.path == normalized_path && r.hooks_hash == hooks_hash))
}

/// Mark a repo's hooks as trusted.
///
/// Uses file locking to prevent concurrent writes from clobbering each other
/// (e.g. multiple sessions being created simultaneously). Writes through the
/// locked file handle to ensure the lock is effective.
pub fn trust_repo(project_path: &Path, hooks_hash: &str) -> Result<()> {
    use fs2::FileExt;
    use std::io::{Read, Seek, SeekFrom, Write};

    let normalized = normalize_path(project_path);
    let path = trusted_repos_path()?;

    // Ensure the file exists so we can lock it
    if !path.exists() {
        fs::write(&path, "")?;
    }

    let mut lock_file = fs::OpenOptions::new().read(true).write(true).open(&path)?;
    lock_file
        .lock_exclusive()
        .context("Failed to acquire lock on trusted_repos.toml")?;

    // Read through the locked handle to avoid a separate file descriptor race
    let mut content = String::new();
    lock_file.read_to_string(&mut content)?;

    let mut trusted: TrustedRepos = if content.trim().is_empty() {
        TrustedRepos::default()
    } else {
        toml::from_str(&content).context("Failed to parse trusted_repos.toml")?
    };

    trusted.repos.retain(|r| r.path != normalized);

    trusted.repos.push(TrustedRepo {
        path: normalized,
        hooks_hash: hooks_hash.to_string(),
        trusted_at: chrono::Utc::now().to_rfc3339(),
    });

    let new_content = toml::to_string_pretty(&trusted)?;
    lock_file.seek(SeekFrom::Start(0))?;
    lock_file.set_len(0)?;
    lock_file.write_all(new_content.as_bytes())?;

    Ok(())
}

/// Result of checking hook trust for a project.
pub enum HookTrustStatus {
    /// No hooks defined, nothing to trust.
    NoHooks,
    /// Hooks are trusted (hash matches).
    Trusted(HooksConfig),
    /// Hooks need user approval before execution.
    NeedsTrust {
        hooks: HooksConfig,
        hooks_hash: String,
    },
}

/// Check hook trust status for a project path.
/// Loads the repo config, checks for hooks, and validates trust.
pub fn check_hook_trust(project_path: &Path) -> Result<HookTrustStatus> {
    let normalized = normalize_path(project_path);
    let repo_config = match load_repo_config(Path::new(&normalized))? {
        Some(rc) => rc,
        None => return Ok(HookTrustStatus::NoHooks),
    };

    let hooks = match repo_config.hooks {
        Some(h) if !h.is_empty() => h,
        _ => return Ok(HookTrustStatus::NoHooks),
    };

    let hooks_hash = compute_hooks_hash(&hooks);

    // Pass already-normalized path to avoid double canonicalization
    if is_repo_trusted_normalized(&normalized, &hooks_hash)? {
        Ok(HookTrustStatus::Trusted(hooks))
    } else {
        Ok(HookTrustStatus::NeedsTrust { hooks, hooks_hash })
    }
}

// ---------------------------------------------------------------------------
// Hook execution
// ---------------------------------------------------------------------------

/// Where to run a hook command.
enum HookTarget<'a> {
    /// Run locally in the given project directory.
    Local { project_path: &'a Path },
    /// Run inside a Docker container.
    Container {
        container_name: &'a str,
        workdir: &'a str,
    },
}

/// Build a `Command` for running a hook via `bash -c`.
fn build_hook_command(cmd: &str, target: &HookTarget, merge_stderr: bool) -> std::process::Command {
    let shell_cmd = if merge_stderr {
        format!("{} 2>&1", cmd)
    } else {
        cmd.to_string()
    };

    match target {
        HookTarget::Local { project_path } => {
            let mut command = std::process::Command::new("bash");
            command.arg("-c").arg(shell_cmd).current_dir(project_path);
            command
        }
        HookTarget::Container {
            container_name,
            workdir,
        } => {
            let mut command = std::process::Command::new("docker");
            command.args([
                "exec",
                "--workdir",
                workdir,
                container_name,
                "bash",
                "-c",
                &shell_cmd,
            ]);
            command
        }
    }
}

/// Format a hook failure error message from captured output.
fn format_hook_error(
    cmd: &str,
    exit_code: Option<i32>,
    stderr: &str,
    stdout: &str,
    in_container: bool,
) -> String {
    let prefix = if in_container {
        "Hook command failed in container"
    } else {
        "Hook command failed"
    };
    let mut detail = format!(
        "{} with exit code {}: {}",
        prefix,
        exit_code.unwrap_or(-1),
        cmd
    );
    if !stderr.is_empty() {
        detail.push_str(&format!("\nstderr:\n{}", stderr.trim_end()));
    }
    if !stdout.is_empty() {
        detail.push_str(&format!("\nstdout:\n{}", stdout.trim_end()));
    }
    detail
}

/// Run hook commands with captured output (non-streamed).
fn run_hooks_captured(commands: &[String], target: &HookTarget) -> Result<()> {
    let in_container = matches!(target, HookTarget::Container { .. });

    for cmd in commands {
        tracing::info!("Running hook: {}", cmd);
        let mut command = build_hook_command(cmd, target, false);
        let output = command
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .with_context(|| format!("Failed to execute hook: {}", cmd))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            anyhow::bail!(format_hook_error(
                cmd,
                output.status.code(),
                &stderr,
                &stdout,
                in_container
            ));
        }

        tracing::debug!(
            "Hook completed: {} (stdout: {} bytes, stderr: {} bytes)",
            cmd,
            output.stdout.len(),
            output.stderr.len()
        );
    }
    Ok(())
}

/// Run hook commands with streamed output sent through a progress channel.
fn run_hooks_streamed(
    commands: &[String],
    target: &HookTarget,
    progress_tx: &mpsc::Sender<HookProgress>,
) -> Result<()> {
    use std::io::BufRead;

    let in_container = matches!(target, HookTarget::Container { .. });

    for cmd in commands {
        tracing::info!("Running hook (streamed): {}", cmd);
        let _ = progress_tx.send(HookProgress::Started(cmd.clone()));

        let mut command = build_hook_command(cmd, target, true);
        let mut child = command
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .with_context(|| format!("Failed to execute hook: {}", cmd))?;

        if let Some(stdout) = child.stdout.take() {
            let reader = std::io::BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                let _ = progress_tx.send(HookProgress::Output(line));
            }
        }

        let status = child.wait()?;
        if !status.success() {
            let detail = format_hook_error(cmd, status.code(), "", "", in_container);
            let _ = progress_tx.send(HookProgress::Output(detail.clone()));
            anyhow::bail!(detail);
        }

        tracing::debug!("Hook completed (streamed): {}", cmd);
    }
    Ok(())
}

/// Execute a list of hook commands in the given directory.
pub fn execute_hooks(commands: &[String], project_path: &Path) -> Result<()> {
    run_hooks_captured(commands, &HookTarget::Local { project_path })
}

/// Execute hooks inside a Docker container.
pub fn execute_hooks_in_container(
    commands: &[String],
    container_name: &str,
    workdir: &str,
) -> Result<()> {
    run_hooks_captured(
        commands,
        &HookTarget::Container {
            container_name,
            workdir,
        },
    )
}

/// Execute a list of hook commands with streamed output.
pub fn execute_hooks_streamed(
    commands: &[String],
    project_path: &Path,
    progress_tx: &mpsc::Sender<HookProgress>,
) -> Result<()> {
    run_hooks_streamed(commands, &HookTarget::Local { project_path }, progress_tx)
}

/// Execute hooks inside a Docker container with streamed output.
pub fn execute_hooks_in_container_streamed(
    commands: &[String],
    container_name: &str,
    workdir: &str,
    progress_tx: &mpsc::Sender<HookProgress>,
) -> Result<()> {
    run_hooks_streamed(
        commands,
        &HookTarget::Container {
            container_name,
            workdir,
        },
        progress_tx,
    )
}

/// Template content for `aoe init`.
pub const INIT_TEMPLATE: &str = r#"# Agent of Empires - Repository Configuration
# This file configures aoe behavior for this repository.
# See: https://github.com/njbrake/agent-of-empires

# [hooks]
# Commands run once when a session is first created
# on_create = ["npm install", "cp .env.example .env"]
# Commands run every time a session starts
# on_launch = ["npm install"]

# [session]
# default_tool = "claude"

# [sandbox]
# enabled_by_default = true
# default_image = "ghcr.io/njbrake/aoe-dev-sandbox:0.10"
# List fields below replace (not append to) global settings when set:
# environment = ["NODE_ENV", "DATABASE_URL"]
# volume_ignores = ["node_modules", ".next"]

# [worktree]
# enabled = true

# [updates]
# check_enabled = false

# [tmux]
# status_bar = "auto"
# mouse = "auto"

# [sound]
# enabled = false
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hooks_config_empty() {
        let hooks = HooksConfig::default();
        assert!(hooks.is_empty());
    }

    #[test]
    fn test_hooks_config_not_empty() {
        let hooks = HooksConfig {
            on_create: vec!["npm install".to_string()],
            on_launch: vec![],
        };
        assert!(!hooks.is_empty());
    }

    #[test]
    fn test_compute_hooks_hash_deterministic() {
        let hooks = HooksConfig {
            on_create: vec!["npm install".to_string()],
            on_launch: vec!["echo hello".to_string()],
        };
        let hash1 = compute_hooks_hash(&hooks);
        let hash2 = compute_hooks_hash(&hooks);
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_compute_hooks_hash_differs_on_change() {
        let hooks1 = HooksConfig {
            on_create: vec!["npm install".to_string()],
            on_launch: vec![],
        };
        let hooks2 = HooksConfig {
            on_create: vec!["yarn install".to_string()],
            on_launch: vec![],
        };
        assert_ne!(compute_hooks_hash(&hooks1), compute_hooks_hash(&hooks2));
    }

    #[test]
    fn test_compute_hooks_hash_distinguishes_hook_types() {
        let hooks1 = HooksConfig {
            on_create: vec!["echo hello".to_string()],
            on_launch: vec![],
        };
        let hooks2 = HooksConfig {
            on_create: vec![],
            on_launch: vec!["echo hello".to_string()],
        };
        assert_ne!(compute_hooks_hash(&hooks1), compute_hooks_hash(&hooks2));
    }

    #[test]
    fn test_repo_config_deserialization() {
        let toml = r#"
            [hooks]
            on_create = ["npm install"]
            on_launch = ["echo start"]

            [session]
            default_tool = "opencode"

            [sandbox]
            enabled_by_default = true
            volume_ignores = ["node_modules"]

            [worktree]
            enabled = true
        "#;

        let config: RepoConfig = toml::from_str(toml).unwrap();
        let hooks = config.hooks.unwrap();
        assert_eq!(hooks.on_create, vec!["npm install"]);
        assert_eq!(hooks.on_launch, vec!["echo start"]);
        assert_eq!(
            config.session.unwrap().default_tool,
            Some("opencode".to_string())
        );
        assert_eq!(config.sandbox.unwrap().enabled_by_default, Some(true));
        assert_eq!(config.worktree.unwrap().enabled, Some(true));
    }

    #[test]
    fn test_repo_config_empty_deserialization() {
        let config: RepoConfig = toml::from_str("").unwrap();
        assert!(config.hooks.is_none());
        assert!(config.session.is_none());
        assert!(config.sandbox.is_none());
        assert!(config.worktree.is_none());
    }

    #[test]
    fn test_merge_repo_config_session() {
        let config = Config::default();
        let repo = RepoConfig {
            session: Some(SessionConfigOverride {
                default_tool: Some("opencode".to_string()),
            }),
            ..Default::default()
        };
        let merged = merge_repo_config(config, &repo);
        assert_eq!(merged.session.default_tool, Some("opencode".to_string()));
    }

    #[test]
    fn test_merge_repo_config_sandbox() {
        let config = Config::default();
        let repo = RepoConfig {
            sandbox: Some(SandboxConfigOverride {
                enabled_by_default: Some(true),
                volume_ignores: Some(vec!["node_modules".to_string()]),
                ..Default::default()
            }),
            ..Default::default()
        };
        let merged = merge_repo_config(config, &repo);
        assert!(merged.sandbox.enabled_by_default);
        assert_eq!(merged.sandbox.volume_ignores, vec!["node_modules"]);
    }

    #[test]
    fn test_merge_repo_config_worktree() {
        let config = Config::default();
        let repo = RepoConfig {
            worktree: Some(WorktreeConfigOverride {
                enabled: Some(true),
                path_template: Some("../wt/{branch}".to_string()),
                ..Default::default()
            }),
            ..Default::default()
        };
        let merged = merge_repo_config(config, &repo);
        assert!(merged.worktree.enabled);
        assert_eq!(merged.worktree.path_template, "../wt/{branch}");
    }

    #[test]
    fn test_merge_repo_config_no_overrides() {
        let config = Config::default();
        let repo = RepoConfig::default();
        let merged = merge_repo_config(config.clone(), &repo);
        assert_eq!(merged.worktree.enabled, config.worktree.enabled);
        assert_eq!(
            merged.sandbox.enabled_by_default,
            config.sandbox.enabled_by_default
        );
    }

    #[test]
    fn test_load_repo_config_nonexistent() {
        let result = load_repo_config(Path::new("/nonexistent/path")).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_init_template_is_valid_toml_when_uncommented() {
        // Verify that uncommenting the TOML sections produces valid TOML.
        // Skip pure comment lines (those that don't look like TOML key/section syntax).
        let uncommented: String = INIT_TEMPLATE
            .lines()
            .filter_map(|line| {
                if let Some(stripped) = line.strip_prefix("# ") {
                    // Only uncomment lines that look like TOML (start with [ or key =)
                    let trimmed = stripped.trim();
                    if trimmed.starts_with('[') || trimmed.contains(" = ") || trimmed.contains("= ")
                    {
                        Some(stripped.to_string())
                    } else {
                        None
                    }
                } else {
                    Some(line.to_string())
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let _config: RepoConfig = toml::from_str(&uncommented).unwrap();
    }

    #[test]
    fn test_trusted_repos_serialization() {
        let trusted = TrustedRepos {
            repos: vec![TrustedRepo {
                path: "/home/user/project".to_string(),
                hooks_hash: "abc123".to_string(),
                trusted_at: "2026-01-31T00:00:00Z".to_string(),
            }],
        };
        let serialized = toml::to_string_pretty(&trusted).unwrap();
        assert!(serialized.contains("path = \"/home/user/project\""));
        assert!(serialized.contains("hooks_hash = \"abc123\""));

        let deserialized: TrustedRepos = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.repos.len(), 1);
        assert_eq!(deserialized.repos[0].path, "/home/user/project");
    }

    #[test]
    fn test_normalize_path_nonexistent_falls_back() {
        let path = Path::new("/nonexistent/path/that/does/not/exist");
        assert_eq!(normalize_path(path), path.to_string_lossy());
    }

    #[test]
    fn test_normalize_path_real_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let normalized = normalize_path(tmp.path());
        assert_eq!(
            std::fs::canonicalize(tmp.path()).unwrap().to_string_lossy(),
            normalized
        );
    }

    #[test]
    fn test_normalize_path_symlink() {
        let tmp = tempfile::tempdir().unwrap();
        let real_dir = tmp.path().join("real");
        std::fs::create_dir(&real_dir).unwrap();
        let link_dir = tmp.path().join("link");
        std::os::unix::fs::symlink(&real_dir, &link_dir).unwrap();

        let normalized_real = normalize_path(&real_dir);
        let normalized_link = normalize_path(&link_dir);
        assert_eq!(normalized_real, normalized_link);
    }

    #[test]
    fn test_execute_hooks_in_container_fails_gracefully() {
        let result = execute_hooks_in_container(
            &["echo test".to_string()],
            "nonexistent_container",
            "/workspace/myproject",
        );
        // Should fail because docker/container doesn't exist, but should not panic
        assert!(result.is_err());
    }

    #[test]
    fn test_merge_repo_config_preserves_unset_fields() {
        let mut config = Config::default();
        config.sandbox.enabled_by_default = true;
        config.sandbox.auto_cleanup = true;
        config.worktree.enabled = true;
        config.worktree.auto_cleanup = true;

        // Only override one field per section
        let repo = RepoConfig {
            sandbox: Some(SandboxConfigOverride {
                enabled_by_default: Some(false),
                ..Default::default()
            }),
            worktree: Some(WorktreeConfigOverride {
                enabled: Some(false),
                ..Default::default()
            }),
            ..Default::default()
        };

        let merged = merge_repo_config(config, &repo);
        // Overridden fields should change
        assert!(!merged.sandbox.enabled_by_default);
        assert!(!merged.worktree.enabled);
        // Non-overridden fields should be preserved
        assert!(merged.sandbox.auto_cleanup);
        assert!(merged.worktree.auto_cleanup);
    }
}
