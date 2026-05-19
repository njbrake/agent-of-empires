//! Session management module

pub mod builder;
pub(crate) mod capture;
pub mod civilizations;
pub mod config;
pub(crate) mod container_config;
pub mod deletion;
pub(crate) mod environment;
mod groups;
mod instance;
pub mod poller;
pub mod profile_config;
pub mod projects;
pub mod repo_config;
pub(crate) mod serde_helpers;
mod storage;

pub use crate::sound::{SoundConfig, SoundConfigOverride};
pub(crate) use capture::is_valid_session_id;
pub use config::{
    get_update_settings, load_config, save_config, Config, ContainerRuntimeName,
    DefaultTerminalMode, GroupByMode, SandboxConfig, SessionConfig, ThemeConfig, TmuxClipboardMode,
    TmuxMouseMode, TmuxStatusBarMode, UpdatesConfig, WorktreeConfig,
};
pub(crate) use environment::user_shell;
pub use environment::{validate_env_entries, validate_env_entry};
pub use groups::{flatten_tree, flatten_tree_all_profiles, Group, GroupTree, Item};
pub use instance::{
    EnsureReadyError, EnsureReadyOutcome, Instance, SandboxInfo, StartOutcome, Status,
    TerminalInfo, WorkspaceInfo, WorkspaceRepo, WorktreeInfo,
};
pub use profile_config::{
    load_profile_config, merge_configs, resolve_config, resolve_config_or_warn,
    save_profile_config, validate_check_interval, validate_memory_limit, validate_volume_format,
    CockpitConfigOverride, HooksConfigOverride, ProfileConfig, SandboxConfigOverride,
    SessionConfigOverride, ThemeConfigOverride, TmuxConfigOverride, UpdatesConfigOverride,
    WorktreeConfigOverride,
};
pub use projects::{Project, ProjectScope};
pub use repo_config::{
    check_hook_trust, execute_hooks, execute_hooks_in_container, load_repo_config,
    merge_repo_config, profile_to_repo_config, repo_config_to_profile, resolve_config_with_repo,
    resolve_config_with_repo_or_warn, save_repo_config, trust_repo, HookTrustStatus, HooksConfig,
    RepoConfig,
};
pub(crate) use storage::atomic_write;
pub use storage::{load_workspace_ordering, update_workspace_ordering, Storage, WorkspaceOrdering};

use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const DEFAULT_PROFILE: &str = "default";

/// Linux app dir name (under `$XDG_CONFIG_HOME`). Debug builds use a `-dev`
/// suffix so a `cargo run` instance shares no state with an installed
/// release binary.
pub const APP_DIR_NAME_LINUX: &str = if cfg!(debug_assertions) {
    "agent-of-empires-dev"
} else {
    "agent-of-empires"
};

/// macOS/Windows app dir name (under `$HOME`). Debug builds use a `-dev`
/// suffix; see `APP_DIR_NAME_LINUX`.
pub const APP_DIR_NAME_OTHER: &str = if cfg!(debug_assertions) {
    ".agent-of-empires-dev"
} else {
    ".agent-of-empires"
};

pub fn get_app_dir() -> Result<PathBuf> {
    let dir = get_app_dir_path()?;
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

fn get_app_dir_path() -> Result<PathBuf> {
    #[cfg(target_os = "linux")]
    let dir = dirs::config_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot find config directory"))?
        .join(APP_DIR_NAME_LINUX);

    #[cfg(not(target_os = "linux"))]
    let dir = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?
        .join(APP_DIR_NAME_OTHER);

    Ok(dir)
}

/// Detect the first-launch case where a debug build is being run on a
/// machine that has populated release-build state in `~/.agent-of-empires`
/// but no dev-build state yet. Returns the (release_dir, dev_dir) pair so
/// callers can surface the paths in a one-time warning.
///
/// Self-extinguishing by design: once `get_app_dir` creates the dev dir on
/// any subsequent call, this returns `None` and the warning stops. No flag
/// file, no config state, no dismissal logic — the directory topology IS
/// the state.
///
/// Returns `None` on release builds (the dev/release split doesn't apply),
/// when the release dir is absent or empty (user has no prior state to
/// "lose visibility of"), or when the dev dir already exists.
pub fn debug_namespace_drift() -> Option<(PathBuf, PathBuf)> {
    if !cfg!(debug_assertions) {
        return None;
    }

    #[cfg(target_os = "linux")]
    let release_dir = dirs::config_dir()?.join("agent-of-empires");
    #[cfg(not(target_os = "linux"))]
    let release_dir = dirs::home_dir()?.join(".agent-of-empires");

    #[cfg(target_os = "linux")]
    let dev_dir = dirs::config_dir()?.join(APP_DIR_NAME_LINUX);
    #[cfg(not(target_os = "linux"))]
    let dev_dir = dirs::home_dir()?.join(APP_DIR_NAME_OTHER);

    let release_populated = fs::read_dir(&release_dir)
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false);

    if release_populated && !dev_dir.exists() {
        Some((release_dir, dev_dir))
    } else {
        None
    }
}

/// Format the user-facing warning shown when `debug_namespace_drift()`
/// fires. Shared between the CLI stderr print and the TUI startup popup so
/// both surfaces say exactly the same thing.
pub fn format_debug_namespace_warning(release: &Path, dev: &Path) -> String {
    format!(
        "Debug builds now use an isolated app dir:\n  \
         {}\n\n\
         Your existing state in\n  \
         {}\n\
         is not visible to this build.\n\n\
         To migrate it, run:\n  \
         cp -r {} {}\n\n\
         Otherwise, do nothing — this notice will not repeat once the dev dir exists.\n\
         See docs/development.md for details.",
        dev.display(),
        release.display(),
        release.display(),
        dev.display(),
    )
}

pub fn get_profile_dir(profile: &str) -> Result<PathBuf> {
    let base = get_app_dir()?;
    let profile_name = if profile.is_empty() {
        DEFAULT_PROFILE
    } else {
        profile
    };
    let dir = base.join("profiles").join(profile_name);
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
    Ok(dir)
}

pub fn list_profiles() -> Result<Vec<String>> {
    let base = get_app_dir()?;
    let profiles_dir = base.join("profiles");

    if !profiles_dir.exists() {
        return Ok(vec![]);
    }

    let mut profiles = Vec::new();
    for entry in fs::read_dir(&profiles_dir)? {
        let entry = entry?;
        if entry.path().is_dir() {
            if let Some(name) = entry.file_name().to_str() {
                profiles.push(name.to_string());
            }
        }
    }
    profiles.sort();
    Ok(profiles)
}

pub fn create_profile(name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("Profile name cannot be empty");
    }
    if name.contains('/') || name.contains('\\') {
        anyhow::bail!("Profile name cannot contain path separators");
    }
    if name.eq_ignore_ascii_case("all") {
        anyhow::bail!("Profile name 'all' is reserved");
    }

    let profiles = list_profiles()?;
    if profiles.contains(&name.to_string()) {
        anyhow::bail!("Profile '{}' already exists", name);
    }

    get_profile_dir(name)?;
    Ok(())
}

pub fn delete_profile(name: &str) -> Result<()> {
    if name == DEFAULT_PROFILE {
        anyhow::bail!("Cannot delete the default profile");
    }

    let base = get_app_dir()?;
    let profile_dir = base.join("profiles").join(name);

    if !profile_dir.exists() {
        anyhow::bail!("Profile '{}' does not exist", name);
    }

    fs::remove_dir_all(&profile_dir)?;
    Ok(())
}

pub fn rename_profile(old_name: &str, new_name: &str) -> Result<()> {
    if new_name.is_empty() {
        anyhow::bail!("New profile name cannot be empty");
    }
    if new_name.contains('/') || new_name.contains('\\') {
        anyhow::bail!("Profile name cannot contain path separators");
    }

    let base = get_app_dir()?;
    let old_dir = base.join("profiles").join(old_name);
    let new_dir = base.join("profiles").join(new_name);

    if !old_dir.exists() {
        anyhow::bail!("Profile '{}' does not exist", old_name);
    }
    if new_dir.exists() {
        anyhow::bail!("Profile '{}' already exists", new_name);
    }

    fs::rename(&old_dir, &new_dir)?;

    // Update default profile if the renamed profile was the default
    if let Some(config) = load_config()? {
        if config.default_profile == old_name {
            set_default_profile(new_name)?;
        }
    }

    Ok(())
}

pub fn set_default_profile(name: &str) -> Result<()> {
    let mut config = load_config()?.unwrap_or_default();
    config.default_profile = name.to_string();
    save_config(&config)?;
    Ok(())
}

/// Probe the global config and the active profile's config at startup so the
/// TUI can show a single user-visible warning when either fails to parse.
/// `tracing::warn!` calls inside the `_or_warn` helpers are silently dropped
/// in default TUI mode (no subscriber), so this gives users a chance to see
/// that their settings have been ignored without needing `AGENT_OF_EMPIRES_DEBUG=1`.
pub fn collect_startup_config_warnings(profile: &str) -> Option<String> {
    let mut messages: Vec<String> = Vec::new();

    let global_path_display = config::config_path()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "config.toml".to_string());

    let global_ok = match Config::load() {
        Ok(_) => true,
        Err(e) => {
            messages.push(format!(
                "Failed to load global config ({global_path_display}); using defaults.\n{e}"
            ));
            false
        }
    };

    let effective = if profile.is_empty() {
        if global_ok {
            config::resolve_default_profile()
        } else {
            DEFAULT_PROFILE.to_string()
        }
    } else {
        profile.to_string()
    };

    let profile_path_display = profile_config::get_profile_config_path(&effective)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| format!("profiles/{effective}/config.toml"));

    if let Err(e) = profile_config::load_profile_config(&effective) {
        messages.push(format!(
            "Failed to load profile config '{effective}' ({profile_path_display}); using defaults.\n{e}"
        ));
    }

    if messages.is_empty() {
        None
    } else {
        Some(messages.join("\n\n"))
    }
}

// ── TUI heartbeat ──────────────────────────────────────────────────────────

const TUI_HEARTBEAT_FILE: &str = "tui.active";

/// Write (or touch) the TUI heartbeat file so the push consumer knows the
/// TUI is currently running. Called periodically from the TUI event loop.
pub fn write_tui_heartbeat() {
    if let Ok(dir) = get_app_dir() {
        let _ = fs::write(dir.join(TUI_HEARTBEAT_FILE), b"");
    }
}

/// Remove the heartbeat file on TUI exit.
pub fn clear_tui_heartbeat() {
    if let Ok(dir) = get_app_dir() {
        let _ = fs::remove_file(dir.join(TUI_HEARTBEAT_FILE));
    }
}

/// Returns true if the TUI heartbeat file was modified within `threshold`.
/// Used by the push consumer to suppress notifications when the user is
/// actively watching the TUI.
pub fn is_tui_active(threshold: Duration) -> bool {
    let dir = match get_app_dir() {
        Ok(d) => d,
        Err(_) => return false,
    };
    let meta = match fs::metadata(dir.join(TUI_HEARTBEAT_FILE)) {
        Ok(m) => m,
        Err(_) => return false,
    };
    let modified = match meta.modified() {
        Ok(t) => t,
        Err(_) => return false,
    };
    modified.elapsed().unwrap_or(Duration::MAX) < threshold
}

#[cfg(test)]
mod tests {
    use super::*;

    fn isolate_app_dir() -> tempfile::TempDir {
        let temp_home = tempfile::TempDir::new().unwrap();
        std::env::set_var("HOME", temp_home.path());
        #[cfg(target_os = "linux")]
        std::env::set_var("XDG_CONFIG_HOME", temp_home.path().join(".config"));
        temp_home
    }

    fn app_dir(temp_home: &tempfile::TempDir) -> PathBuf {
        #[cfg(target_os = "linux")]
        let dir = temp_home.path().join(".config").join(APP_DIR_NAME_LINUX);
        #[cfg(not(target_os = "linux"))]
        let dir = temp_home.path().join(APP_DIR_NAME_OTHER);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    #[serial_test::serial]
    fn test_collect_startup_config_warnings_clean() {
        let _temp = isolate_app_dir();
        // No config files written = defaults everywhere = no warning.
        assert!(collect_startup_config_warnings("").is_none());
    }

    #[test]
    #[serial_test::serial]
    fn test_collect_startup_config_warnings_bad_global() {
        let temp = isolate_app_dir();
        let dir = app_dir(&temp);
        fs::write(
            dir.join("config.toml"),
            "[sandbox]\nenabled_by_default = \"not-a-bool\"\n",
        )
        .unwrap();

        let warning = collect_startup_config_warnings("").expect("expected a warning");
        assert!(warning.contains("Failed to load global config"));
        assert!(warning.contains("config.toml"));
    }

    #[test]
    #[serial_test::serial]
    fn test_collect_startup_config_warnings_bad_profile() {
        let temp = isolate_app_dir();
        let dir = app_dir(&temp);
        let profile_dir = dir.join("profiles").join("default");
        fs::create_dir_all(&profile_dir).unwrap();
        fs::write(
            profile_dir.join("config.toml"),
            "[worktree]\nenabled = \"not-a-bool\"\n",
        )
        .unwrap();

        let warning = collect_startup_config_warnings("default").expect("expected a warning");
        assert!(warning.contains("Failed to load profile config 'default'"));
    }

    fn release_dir_in(temp: &tempfile::TempDir) -> PathBuf {
        #[cfg(target_os = "linux")]
        let d = temp.path().join(".config").join("agent-of-empires");
        #[cfg(not(target_os = "linux"))]
        let d = temp.path().join(".agent-of-empires");
        d
    }

    #[test]
    #[serial_test::serial]
    fn test_drift_none_when_no_release_dir() {
        let _temp = isolate_app_dir();
        // Neither dir exists → no drift to flag.
        assert!(debug_namespace_drift().is_none());
    }

    #[test]
    #[serial_test::serial]
    fn test_drift_none_when_release_empty() {
        let temp = isolate_app_dir();
        let release = release_dir_in(&temp);
        fs::create_dir_all(&release).unwrap();
        // Release dir exists but has no content — user has no prior state
        // to lose visibility of, so don't nag them.
        assert!(debug_namespace_drift().is_none());
    }

    #[test]
    #[serial_test::serial]
    fn test_drift_fires_when_release_populated_and_dev_absent() {
        let temp = isolate_app_dir();
        let release = release_dir_in(&temp);
        fs::create_dir_all(release.join("profiles")).unwrap();

        let drift = debug_namespace_drift();
        // Only assert presence on debug builds — release builds compile this
        // function to `None`.
        if cfg!(debug_assertions) {
            let (r, d) = drift.expect("expected drift on debug build");
            assert_eq!(r, release);
            assert!(d.to_string_lossy().contains("-dev"));
        } else {
            assert!(drift.is_none());
        }
    }

    #[test]
    #[serial_test::serial]
    fn test_drift_silent_once_dev_dir_exists() {
        let temp = isolate_app_dir();
        let release = release_dir_in(&temp);
        fs::create_dir_all(release.join("profiles")).unwrap();
        // Simulate "user has already run aoe once after the namespace
        // change" by creating the dev dir.
        let _dev = app_dir(&temp);
        assert!(debug_namespace_drift().is_none());
    }

    #[test]
    fn test_format_warning_mentions_both_paths_and_migration_command() {
        let release = PathBuf::from("/home/u/.config/agent-of-empires");
        let dev = PathBuf::from("/home/u/.config/agent-of-empires-dev");
        let msg = format_debug_namespace_warning(&release, &dev);
        assert!(msg.contains("/home/u/.config/agent-of-empires"));
        assert!(msg.contains("/home/u/.config/agent-of-empires-dev"));
        assert!(msg.contains("cp -r"));
        assert!(msg.contains("not repeat"));
    }
}
