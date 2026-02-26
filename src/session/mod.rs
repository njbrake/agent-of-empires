//! Session management module

pub mod builder;
pub mod civilizations;
pub mod config;
mod container_config;
mod environment;
mod groups;
mod instance;
pub mod profile_config;
pub mod repo_config;
mod storage;

pub use crate::sound::{SoundConfig, SoundConfigOverride};
pub use config::{
    get_claude_config_dir, get_update_settings, load_config, save_config, ClaudeConfig, Config,
    ContainerRuntimeName, DefaultTerminalMode, SandboxConfig, SessionConfig, ThemeConfig,
    TmuxMouseMode, TmuxStatusBarMode, UpdatesConfig, WorktreeConfig,
};
pub use groups::{flatten_tree, Group, GroupTree, Item};
pub use instance::{Instance, SandboxInfo, Status, TerminalInfo, WorktreeInfo};
pub use profile_config::{
    load_profile_config, merge_configs, resolve_config, save_profile_config,
    validate_check_interval, validate_memory_limit, validate_path_exists, validate_volume_format,
    ClaudeConfigOverride, HooksConfigOverride, ProfileConfig, SandboxConfigOverride,
    SessionConfigOverride, ThemeConfigOverride, TmuxConfigOverride, UpdatesConfigOverride,
    WorktreeConfigOverride,
};
pub use repo_config::{
    check_hook_trust, execute_hooks, execute_hooks_in_container, load_repo_config,
    merge_repo_config, profile_to_repo_config, repo_config_to_profile, resolve_config_with_repo,
    save_repo_config, trust_repo, HookTrustStatus, HooksConfig, RepoConfig,
};
pub use storage::Storage;

use anyhow::Result;
use std::fs;
use std::path::PathBuf;

pub const DEFAULT_PROFILE: &str = "default";

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
        .join("agent-of-empires");

    #[cfg(not(target_os = "linux"))]
    let dir = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?
        .join(".agent-of-empires");

    Ok(dir)
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
