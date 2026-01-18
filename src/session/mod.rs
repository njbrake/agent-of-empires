//! Session management module

pub mod civilizations;
pub mod claude;
pub mod config;
mod groups;
mod instance;
mod storage;

pub use config::{
    get_claude_config_dir, get_update_settings, load_config, save_config, ClaudeConfig, Config,
    SandboxConfig, ThemeConfig, UpdatesConfig, WorktreeConfig,
};
pub use groups::{flatten_tree, Group, GroupTree, Item};
pub use instance::{
    Instance, SandboxInfo, Status, TerminalInfo, WorktreeInfo, YOLO_SUPPORTED_TOOLS,
};
pub use storage::Storage;

use anyhow::Result;
use std::fs;
use std::path::PathBuf;

pub const DEFAULT_PROFILE: &str = "default";

pub fn get_app_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Cannot find home directory"))?;
    let dir = home.join(".agent-of-empires");
    if !dir.exists() {
        fs::create_dir_all(&dir)?;
    }
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

pub fn set_default_profile(name: &str) -> Result<()> {
    let mut config = load_config()?.unwrap_or_default();
    config.default_profile = name.to_string();
    save_config(&config)?;
    Ok(())
}
