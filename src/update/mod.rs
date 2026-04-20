//! Update check functionality

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tracing::warn;

use crate::session::{get_app_dir, get_update_settings};

const GITHUB_API_LATEST: &str =
    "https://api.github.com/repos/njbrake/agent-of-empires/releases/latest";
const GITHUB_API_RELEASES: &str =
    "https://api.github.com/repos/njbrake/agent-of-empires/releases?per_page=20";

#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub available: bool,
    pub current_version: String,
    pub latest_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseInfo {
    pub version: String,
    pub body: String,
    pub published_at: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    published_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct UpdateCache {
    checked_at: chrono::DateTime<chrono::Utc>,
    latest_version: String,
    #[serde(default)]
    releases: Vec<ReleaseInfo>,
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

            // Invalidate cache if current version is newer than cached latest
            // (user upgraded and cache is stale)
            let current_is_newer = is_newer_version(current_version, &cache.latest_version);

            if age < max_age && !current_is_newer {
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
        .timeout(std::time::Duration::from_secs(5))
        .build()?;

    // Fetch all releases (includes body/release notes)
    let releases = fetch_releases(&client).await.unwrap_or_default();

    let latest_version = releases
        .first()
        .map(|r| r.version.clone())
        .unwrap_or_default();

    if latest_version.is_empty() {
        // Fall back to latest endpoint if releases fetch failed
        let response = client.get(GITHUB_API_LATEST).send().await?;
        if !response.status().is_success() {
            anyhow::bail!("Failed to check for updates: HTTP {}", response.status());
        }
        let release: GitHubRelease = response.json().await?;
        let version = release.tag_name.trim_start_matches('v').to_string();

        let release_info = ReleaseInfo {
            version: version.clone(),
            body: release.body.unwrap_or_default(),
            published_at: release.published_at,
        };

        let cache = UpdateCache {
            checked_at: chrono::Utc::now(),
            latest_version: version.clone(),
            releases: vec![release_info],
        };
        if let Err(e) = save_cache(&cache) {
            warn!("Failed to save update cache: {}", e);
        }

        return Ok(UpdateInfo {
            available: is_newer_version(&version, current_version),
            current_version: current_version.to_string(),
            latest_version: version,
        });
    }

    let cache = UpdateCache {
        checked_at: chrono::Utc::now(),
        latest_version: latest_version.clone(),
        releases,
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

async fn fetch_releases(client: &reqwest::Client) -> Result<Vec<ReleaseInfo>> {
    let response = client.get(GITHUB_API_RELEASES).send().await?;

    if !response.status().is_success() {
        anyhow::bail!("Failed to fetch releases: HTTP {}", response.status());
    }

    let github_releases: Vec<GitHubRelease> = response.json().await?;

    let releases = github_releases
        .into_iter()
        .map(|r| ReleaseInfo {
            version: r.tag_name.trim_start_matches('v').to_string(),
            body: r.body.unwrap_or_default(),
            published_at: r.published_at,
        })
        .collect();

    Ok(releases)
}

/// Get cached release notes, filtered to show only releases newer than from_version.
/// Returns releases in newest-first order.
pub fn get_cached_releases(from_version: Option<&str>) -> Vec<ReleaseInfo> {
    let cache = match load_cache() {
        Some(c) => c,
        None => return vec![],
    };

    filter_releases(cache.releases, from_version)
}

fn filter_releases(releases: Vec<ReleaseInfo>, from_version: Option<&str>) -> Vec<ReleaseInfo> {
    match from_version {
        Some(from) => releases
            .into_iter()
            .take_while(|r| r.version != from)
            .collect(),
        None => releases,
    }
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

    #[test]
    fn test_cache_should_invalidate_when_current_newer_than_cached() {
        // When user upgrades to a version newer than cached latest,
        // the cache should be invalidated to fetch fresh release notes.
        // This test documents the version comparison used for cache invalidation.
        let cached_latest = "0.4.5";
        let current_version = "0.5.0";

        // current > cached means cache is stale
        let current_is_newer = is_newer_version(current_version, cached_latest);
        assert!(current_is_newer, "0.5.0 should be newer than 0.4.5");

        // Same version means cache is valid
        let same_version = is_newer_version("0.4.5", "0.4.5");
        assert!(
            !same_version,
            "same version should not trigger invalidation"
        );

        // Older current version (downgrade) should not invalidate
        let downgrade = is_newer_version("0.4.0", "0.4.5");
        assert!(!downgrade, "downgrade should not trigger invalidation");
    }

    fn make_release(version: &str) -> ReleaseInfo {
        ReleaseInfo {
            version: version.to_string(),
            body: format!("Release notes for {}", version),
            published_at: None,
        }
    }

    #[test]
    fn test_filter_releases_returns_all_when_no_filter() {
        let releases = vec![
            make_release("0.5.0"),
            make_release("0.4.3"),
            make_release("0.4.2"),
        ];

        let filtered = filter_releases(releases.clone(), None);

        assert_eq!(filtered.len(), 3);
        assert_eq!(filtered[0].version, "0.5.0");
        assert_eq!(filtered[1].version, "0.4.3");
        assert_eq!(filtered[2].version, "0.4.2");
    }

    #[test]
    fn test_filter_releases_stops_at_from_version() {
        let releases = vec![
            make_release("0.5.0"),
            make_release("0.4.3"),
            make_release("0.4.2"),
            make_release("0.4.1"),
        ];

        let filtered = filter_releases(releases, Some("0.4.3"));

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].version, "0.5.0");
    }

    #[test]
    fn test_filter_releases_returns_empty_when_from_version_is_latest() {
        let releases = vec![make_release("0.5.0"), make_release("0.4.3")];

        let filtered = filter_releases(releases, Some("0.5.0"));

        assert!(filtered.is_empty());
    }

    #[test]
    fn test_filter_releases_returns_all_when_from_version_not_found() {
        let releases = vec![make_release("0.5.0"), make_release("0.4.3")];

        let filtered = filter_releases(releases.clone(), Some("0.3.0"));

        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_filter_releases_handles_empty_list() {
        let releases: Vec<ReleaseInfo> = vec![];

        let filtered = filter_releases(releases.clone(), Some("0.4.3"));
        assert!(filtered.is_empty());

        let filtered = filter_releases(releases, None);
        assert!(filtered.is_empty());
    }
}
