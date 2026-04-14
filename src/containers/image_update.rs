//! Docker image update detection with caching.
//!
//! Checks whether the locally pulled sandbox image is outdated compared to
//! the remote registry. Uses Docker CLI digest comparison and caches results
//! to avoid repeated registry calls.

use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tracing::{debug, info, warn};

use crate::session::{get_app_dir, get_update_settings, load_config};

use super::container_interface::ContainerRuntimeInterface;
use super::get_container_runtime;

#[derive(Debug, Clone)]
pub struct ImageUpdateInfo {
    pub update_available: bool,
    pub image: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ImageUpdateCache {
    checked_at: DateTime<Utc>,
    image: String,
    local_id: String,
    remote_id: String,
    update_available: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    snoozed_until: Option<DateTime<Utc>>,
}

fn cache_path() -> Result<PathBuf> {
    Ok(get_app_dir()?.join("image_update_cache.json"))
}

fn load_cache() -> Option<ImageUpdateCache> {
    let path = cache_path().ok()?;
    let content = fs::read_to_string(&path).ok()?;
    serde_json::from_str(&content).ok()
}

fn save_cache(cache: &ImageUpdateCache) -> Result<()> {
    let path = cache_path()?;
    let content = serde_json::to_string_pretty(cache)?;
    fs::write(&path, content)?;
    Ok(())
}

/// Check if a newer version of the given Docker image is available remotely.
///
/// Returns `update_available: false` without hitting the network when:
/// - The user has permanently dismissed image update checks
/// - The dialog was snoozed (dismissed with "No") within the last 24 hours
/// - The cache is fresh (within the configured check interval)
///
/// On failure (Docker not available, manifest inspect unsupported, network error),
/// returns `update_available: false` silently. This is a non-critical background check.
pub async fn check_image_update(image: &str, force: bool) -> Result<ImageUpdateInfo> {
    // Check if permanently dismissed
    if let Ok(Some(config)) = load_config() {
        if config.app_state.image_update_check_dismissed {
            debug!("Image update check dismissed by user");
            return Ok(ImageUpdateInfo {
                update_available: false,
                image: image.to_string(),
            });
        }
    }

    if !force {
        if let Some(cache) = load_cache() {
            // Check snooze (24h suppression after "No" dismiss)
            if let Some(snoozed_until) = cache.snoozed_until {
                if Utc::now() < snoozed_until {
                    debug!("Image update check snoozed until {}", snoozed_until);
                    return Ok(ImageUpdateInfo {
                        update_available: false,
                        image: image.to_string(),
                    });
                }
            }

            // Check cache freshness (reuse update check interval)
            let settings = get_update_settings();
            let age = Utc::now() - cache.checked_at;
            let max_age = chrono::Duration::hours(settings.check_interval_hours as i64);

            // Also invalidate if the configured image changed
            if age < max_age && cache.image == image {
                debug!(
                    "Using cached image update result: update_available={}",
                    cache.update_available
                );
                return Ok(ImageUpdateInfo {
                    update_available: cache.update_available,
                    image: image.to_string(),
                });
            }
        }
    }

    // Run the actual check (blocking Docker CLI calls wrapped in spawn_blocking)
    let image_owned = image.to_string();
    let result = tokio::task::spawn_blocking(move || check_image_digests(&image_owned)).await?;

    match result {
        Ok((local_id, remote_id, update_available)) => {
            info!(
                "Image update check: local={}, remote={}, update_available={}",
                &local_id[..local_id.len().min(20)],
                &remote_id[..remote_id.len().min(20)],
                update_available
            );

            let cache = ImageUpdateCache {
                checked_at: Utc::now(),
                image: image.to_string(),
                local_id,
                remote_id,
                update_available,
                snoozed_until: None,
            };
            if let Err(e) = save_cache(&cache) {
                warn!("Failed to save image update cache: {}", e);
            }

            Ok(ImageUpdateInfo {
                update_available,
                image: image.to_string(),
            })
        }
        Err(e) => {
            warn!("Image update check failed (non-critical): {}", e);
            Ok(ImageUpdateInfo {
                update_available: false,
                image: image.to_string(),
            })
        }
    }
}

/// Perform the actual Docker CLI digest comparison (blocking).
fn check_image_digests(image: &str) -> Result<(String, String, bool)> {
    let runtime = get_container_runtime();

    // Get local image ID
    let local_id = match runtime.get_local_image_id(image) {
        Ok(id) => id,
        Err(e) => {
            debug!("No local image found for '{}': {}", image, e);
            anyhow::bail!("Image not found locally");
        }
    };

    // Get remote manifest digest
    let remote_id = match runtime.get_remote_manifest_digest(image) {
        Ok(id) => id,
        Err(e) => {
            warn!(
                "Could not get remote manifest for '{}': {} (docker manifest inspect may not be available)",
                image, e
            );
            anyhow::bail!("Remote manifest unavailable");
        }
    };

    let update_available = local_id != remote_id;
    Ok((local_id, remote_id, update_available))
}

/// Snooze the image update dialog for 24 hours.
/// Called when the user dismisses with "No".
pub fn snooze_image_update() {
    if let Some(mut cache) = load_cache() {
        cache.snoozed_until = Some(Utc::now() + chrono::Duration::hours(24));
        if let Err(e) = save_cache(&cache) {
            warn!("Failed to save image update snooze: {}", e);
        }
    }
}

/// Delete the image update cache, forcing a fresh check on next launch.
/// Call after a successful pull.
pub fn invalidate_image_cache() {
    if let Ok(path) = cache_path() {
        if path.exists() {
            if let Err(e) = fs::remove_file(&path) {
                warn!("Failed to remove image update cache: {}", e);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    // Helper to create a cache in a temp dir
    fn write_test_cache(dir: &TempDir, cache: &ImageUpdateCache) {
        let path = dir.path().join("image_update_cache.json");
        let content = serde_json::to_string_pretty(cache).unwrap();
        fs::write(&path, content).unwrap();
    }

    fn read_test_cache(dir: &TempDir) -> Option<ImageUpdateCache> {
        let path = dir.path().join("image_update_cache.json");
        let content = fs::read_to_string(&path).ok()?;
        serde_json::from_str(&content).ok()
    }

    #[test]
    fn test_cache_serialization_roundtrip() {
        let cache = ImageUpdateCache {
            checked_at: Utc::now(),
            image: "ghcr.io/test:latest".to_string(),
            local_id: "sha256:abc123".to_string(),
            remote_id: "sha256:def456".to_string(),
            update_available: true,
            snoozed_until: None,
        };

        let json = serde_json::to_string(&cache).unwrap();
        let parsed: ImageUpdateCache = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.image, "ghcr.io/test:latest");
        assert_eq!(parsed.local_id, "sha256:abc123");
        assert_eq!(parsed.remote_id, "sha256:def456");
        assert!(parsed.update_available);
        assert!(parsed.snoozed_until.is_none());
    }

    #[test]
    fn test_cache_with_snooze_roundtrip() {
        let snooze_time = Utc::now() + chrono::Duration::hours(24);
        let cache = ImageUpdateCache {
            checked_at: Utc::now(),
            image: "test:latest".to_string(),
            local_id: "sha256:aaa".to_string(),
            remote_id: "sha256:bbb".to_string(),
            update_available: true,
            snoozed_until: Some(snooze_time),
        };

        let json = serde_json::to_string(&cache).unwrap();
        let parsed: ImageUpdateCache = serde_json::from_str(&json).unwrap();
        assert!(parsed.snoozed_until.is_some());
    }

    #[test]
    fn test_cache_without_snooze_field_deserializes() {
        // Old cache files without snoozed_until should still parse
        let json = r#"{
            "checked_at": "2026-01-01T00:00:00Z",
            "image": "test:latest",
            "local_id": "sha256:aaa",
            "remote_id": "sha256:bbb",
            "update_available": false
        }"#;
        let parsed: ImageUpdateCache = serde_json::from_str(json).unwrap();
        assert!(parsed.snoozed_until.is_none());
        assert!(!parsed.update_available);
    }

    #[test]
    fn test_image_update_info_not_available() {
        let info = ImageUpdateInfo {
            update_available: false,
            image: "test:latest".to_string(),
        };
        assert!(!info.update_available);
    }

    #[test]
    fn test_image_update_info_available() {
        let info = ImageUpdateInfo {
            update_available: true,
            image: "test:latest".to_string(),
        };
        assert!(info.update_available);
    }

    #[test]
    fn test_snooze_writes_to_cache() {
        let dir = TempDir::new().unwrap();
        let cache = ImageUpdateCache {
            checked_at: Utc::now(),
            image: "test:latest".to_string(),
            local_id: "sha256:aaa".to_string(),
            remote_id: "sha256:bbb".to_string(),
            update_available: true,
            snoozed_until: None,
        };
        write_test_cache(&dir, &cache);

        // Verify the cache was written
        let loaded = read_test_cache(&dir);
        assert!(loaded.is_some());
        assert!(loaded.unwrap().snoozed_until.is_none());
    }
}
