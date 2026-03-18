//! Migration v005: Backfill `launched_with_wrapper` for existing sessions
//!
//! Sessions created before the `launched_with_wrapper` field was added
//! deserialize with `false` (via `#[serde(default)]`). For sessions that
//! were actually started with a `command_wrapper`, this causes the
//! shell-detection heuristic to misfire and kill them on the next attach.
//!
//! This migration reads each `sessions.json`, resolves the config for every
//! session, and sets `launched_with_wrapper: true` where the effective config
//! has a non-empty `command_wrapper`.

use anyhow::Result;
use std::fs;
use std::path::Path;
use tracing::{debug, info};

pub fn run() -> Result<()> {
    let app_dir = crate::session::get_app_dir()?;
    let profiles_dir = app_dir.join("profiles");

    if !profiles_dir.exists() {
        debug!("No profiles directory, nothing to migrate");
        return Ok(());
    }

    for entry in fs::read_dir(&profiles_dir)? {
        let entry = entry?;
        if entry.path().is_dir() {
            let profile_name = entry.file_name().to_string_lossy().to_string();
            let sessions_path = entry.path().join("sessions.json");
            if let Err(e) = migrate_sessions_file(&sessions_path, &profile_name) {
                debug!("Skipping migration for profile '{}': {}", profile_name, e);
            }
        }
    }

    Ok(())
}

fn migrate_sessions_file(path: &Path, profile: &str) -> Result<()> {
    if !path.exists() {
        debug!("{} does not exist, skipping", path.display());
        return Ok(());
    }

    let content = fs::read_to_string(path)?;
    if content.trim().is_empty() {
        return Ok(());
    }

    let mut sessions: Vec<serde_json::Value> = serde_json::from_str(&content)?;
    let mut modified = false;

    for session in &mut sessions {
        let obj = match session.as_object_mut() {
            Some(o) => o,
            None => continue,
        };

        // Skip if already set to true
        if obj
            .get("launched_with_wrapper")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            continue;
        }

        let source_profile = obj
            .get("source_profile")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let effective_profile = if source_profile.is_empty() {
            obj.get("profile")
                .and_then(|v| v.as_str())
                .unwrap_or(profile)
        } else {
            source_profile
        };

        let project_path = match obj.get("project_path").and_then(|v| v.as_str()) {
            Some(p) => p.to_string(),
            None => continue,
        };

        let has_wrapper = match crate::session::repo_config::resolve_config_with_repo(
            effective_profile,
            Path::new(&project_path),
        ) {
            Ok(c) => c
                .sandbox
                .command_wrapper
                .is_some_and(|w| !w.trim().is_empty()),
            Err(_) => false,
        };

        if has_wrapper {
            info!(
                "Backfilling launched_with_wrapper=true for session '{}' in profile '{}'",
                obj.get("title").and_then(|v| v.as_str()).unwrap_or("?"),
                profile
            );
            obj.insert(
                "launched_with_wrapper".to_string(),
                serde_json::Value::Bool(true),
            );
            modified = true;
        }
    }

    if modified {
        let new_content = serde_json::to_string_pretty(&sessions)?;
        fs::write(path, new_content)?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_migrate_sessions_file_nonexistent() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("sessions.json");
        // Should succeed without error for missing file
        migrate_sessions_file(&path, "default").unwrap();
    }

    #[test]
    fn test_migrate_sessions_file_empty() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("sessions.json");
        fs::write(&path, "").unwrap();
        migrate_sessions_file(&path, "default").unwrap();
    }

    #[test]
    fn test_migrate_sessions_file_already_set() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("sessions.json");
        let sessions = serde_json::json!([{
            "id": "test-1",
            "title": "test",
            "project_path": "/nonexistent",
            "profile": "default",
            "launched_with_wrapper": true
        }]);
        fs::write(&path, serde_json::to_string(&sessions).unwrap()).unwrap();

        migrate_sessions_file(&path, "default").unwrap();

        let result: Vec<serde_json::Value> =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert!(result[0]["launched_with_wrapper"].as_bool().unwrap());
    }

    #[test]
    fn test_migrate_sessions_file_no_wrapper_config() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("sessions.json");
        let sessions = serde_json::json!([{
            "id": "test-1",
            "title": "test",
            "project_path": "/nonexistent",
            "profile": "default"
        }]);
        fs::write(&path, serde_json::to_string(&sessions).unwrap()).unwrap();

        migrate_sessions_file(&path, "default").unwrap();

        let result: Vec<serde_json::Value> =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        // Should not have added the flag since config resolution would fail/have no wrapper
        assert!(!result[0]
            .get("launched_with_wrapper")
            .and_then(|v| v.as_bool())
            .unwrap_or(false));
    }
}
