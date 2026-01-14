//! Update check functionality

use anyhow::Result;
use serde::Deserialize;
use std::fs;
use std::path::PathBuf;
use tracing::warn;

use crate::session::{get_app_dir, get_update_settings};

const GITHUB_API_URL: &str =
    "https://api.github.com/repos/njbrake/agent-of-empires/releases/latest";

#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub available: bool,
    pub current_version: String,
    pub latest_version: String,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
}

#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct UpdateCache {
    checked_at: chrono::DateTime<chrono::Utc>,
    latest_version: String,
}

fn cache_path() -> Result<PathBuf> {
    Ok(get_app_dir()?.join("update_cache.json"))
}

fn load_cache() -> Option<UpdateCache> {
    let path = cache_path().ok()?;
    let content = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

fn save_cache(cache: &UpdateCache) -> Result<()> {
    let path = cache_path()?;
    let content = serde_json::to_string_pretty(cache)?;
    fs::write(&path, content)?;
    Ok(())
}

pub async fn check_for_update(current_version: &str, force: bool) -> Result<UpdateInfo> {
    let settings = get_update_settings();

    if !force {
        if let Some(cache) = load_cache() {
            let age = chrono::Utc::now() - cache.checked_at;
            let max_age = chrono::Duration::hours(settings.check_interval_hours as i64);

            if age < max_age {
                let available = is_newer_version(&cache.latest_version, current_version);
                return Ok(UpdateInfo {
                    available,
                    current_version: current_version.to_string(),
                    latest_version: cache.latest_version,
                });
            }
        }
    }

    let client = reqwest::Client::builder()
        .user_agent("agent-of-empires")
        .build()?;

    let response = client.get(GITHUB_API_URL).send().await?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to check for updates: HTTP {}", response.status());
    }

    let release: GitHubRelease = response.json().await?;
    let latest_version = release.tag_name.trim_start_matches('v').to_string();

    let cache = UpdateCache {
        checked_at: chrono::Utc::now(),
        latest_version: latest_version.clone(),
    };
    if let Err(e) = save_cache(&cache) {
        warn!("Failed to save update cache: {}", e);
    }

    let available = is_newer_version(&latest_version, current_version);

    Ok(UpdateInfo {
        available,
        current_version: current_version.to_string(),
        latest_version,
    })
}

fn is_newer_version(latest: &str, current: &str) -> bool {
    let parse_version =
        |v: &str| -> Vec<u32> { v.split('.').filter_map(|s| s.parse().ok()).collect() };

    let latest_parts = parse_version(latest);
    let current_parts = parse_version(current);

    for i in 0..latest_parts.len().max(current_parts.len()) {
        let l = latest_parts.get(i).copied().unwrap_or(0);
        let c = current_parts.get(i).copied().unwrap_or(0);
        if l > c {
            return true;
        }
        if l < c {
            return false;
        }
    }
    false
}

pub async fn print_update_notice() {
    let settings = get_update_settings();
    if !settings.check_enabled || !settings.notify_in_cli {
        return;
    }

    let version = env!("CARGO_PKG_VERSION");

    if let Ok(info) = check_for_update(version, false).await {
        if info.available {
            eprintln!(
                "\n💡 Update available: v{} → v{} (run: brew update && brew upgrade aoe)",
                info.current_version, info.latest_version
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_comparison() {
        assert!(is_newer_version("1.0.1", "1.0.0"));
        assert!(is_newer_version("1.1.0", "1.0.9"));
        assert!(is_newer_version("2.0.0", "1.9.9"));
        assert!(!is_newer_version("1.0.0", "1.0.0"));
        assert!(!is_newer_version("1.0.0", "1.0.1"));
    }
}
