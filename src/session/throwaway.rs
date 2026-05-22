//! Throwaway-session directory provisioning and identification.
//!
//! A throwaway session has no associated project path. The session layer
//! provisions a fresh directory under `std::env::temp_dir()` with the basename
//! `aoe-throwaway-<instance-id>` and attaches the session to it. On deletion,
//! the directory is removed; the basename prefix plus the temp-dir ancestry
//! check guard against `remove_dir_all` being aimed at unrelated paths if a
//! session JSON is tampered.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

/// Basename prefix for every throwaway session directory. Used by both the
/// provisioner (to name the directory) and the deletion guard (to reject
/// tampered `project_path` values that point at unrelated dirs under
/// `temp_dir()`).
pub const THROWAWAY_DIR_PREFIX: &str = "aoe-throwaway-";

/// Create a fresh directory for a throwaway session and return its absolute
/// path. Uses `fs::create_dir` (not `create_dir_all`) so a collision with a
/// pre-existing directory surfaces as an error rather than silently reusing
/// the directory's contents, which would violate the freshness contract.
pub fn provision_throwaway_dir(instance_id: &str) -> Result<PathBuf> {
    let path = std::env::temp_dir().join(format!("{THROWAWAY_DIR_PREFIX}{instance_id}"));
    fs::create_dir(&path)
        .with_context(|| format!("Failed to create throwaway directory at {}", path.display()))?;
    Ok(path)
}

/// Return true iff `path` is plausibly a throwaway directory created by this
/// crate: it lives under `std::env::temp_dir()` AND its basename starts with
/// `THROWAWAY_DIR_PREFIX`. Used by `session::deletion::perform_deletion` to
/// guard `fs::remove_dir_all` against accidental or malicious targeting of
/// unrelated paths.
pub fn is_throwaway_path(path: &Path) -> bool {
    if !path.starts_with(std::env::temp_dir()) {
        return false;
    }
    path.file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|n| n.starts_with(THROWAWAY_DIR_PREFIX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provisions_and_returns_temp_path() {
        let id = format!("test-{}", uuid::Uuid::new_v4());
        let path = provision_throwaway_dir(&id).expect("provision must succeed");
        assert!(path.exists());
        assert!(path.is_dir());
        assert!(path.starts_with(std::env::temp_dir()));
        assert!(path
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap()
            .starts_with(THROWAWAY_DIR_PREFIX));
        let _ = fs::remove_dir_all(&path);
    }

    #[test]
    fn provision_collision_errors() {
        let id = format!("collision-{}", uuid::Uuid::new_v4());
        let first = provision_throwaway_dir(&id).expect("first provision must succeed");
        let second = provision_throwaway_dir(&id);
        assert!(
            second.is_err(),
            "provision_throwaway_dir must error on collision rather than reuse contents",
        );
        let _ = fs::remove_dir_all(&first);
    }

    #[test]
    fn is_throwaway_path_accepts_well_formed() {
        let id = format!("accept-{}", uuid::Uuid::new_v4());
        let path = provision_throwaway_dir(&id).expect("provision");
        assert!(is_throwaway_path(&path));
        let _ = fs::remove_dir_all(&path);
    }

    #[test]
    fn is_throwaway_path_rejects_wrong_prefix_under_temp_dir() {
        let path = std::env::temp_dir().join("important-data");
        assert!(
            !is_throwaway_path(&path),
            "guard must reject paths under temp_dir without the aoe-throwaway- prefix",
        );
    }

    #[test]
    fn is_throwaway_path_rejects_outside_temp_dir() {
        let path = PathBuf::from("/aoe-throwaway-anywhere");
        assert!(
            !is_throwaway_path(&path),
            "guard must reject paths outside temp_dir even if basename matches",
        );
    }

    #[test]
    fn is_throwaway_path_rejects_root_with_matching_basename_in_other_root() {
        // Defense against the tampered-JSON case: `throwaway: true` with
        // `project_path: "/etc"` must NOT trip the deletion guard, even
        // though `/etc` is a real directory.
        let path = PathBuf::from("/etc");
        assert!(!is_throwaway_path(&path));
    }
}
