//! Migration v005: Delete empty `.sandbox-gitconfig` so the new seed is written.
//!
//! v004 and earlier seeded an empty `.sandbox-gitconfig` into `~/.claude/sandbox/`.
//! The seed is now a scoped credential helper that lets `git push` to github.com
//! authenticate via `GH_TOKEN` when the host forwards it. The seed logic is
//! write-once, so existing zero-byte files would shadow the new content forever.
//!
//! This migration removes zero-byte `.sandbox-gitconfig` files in the shared
//! sandbox dir; non-empty files (user-customized) are left alone. The next session
//! launch re-seeds the file with the new content.

use anyhow::Result;
use std::fs;
use std::path::Path;
use tracing::{debug, info};

pub fn run() -> Result<()> {
    let Some(home) = dirs::home_dir() else {
        debug!("No home directory, skipping");
        return Ok(());
    };
    run_for_home(&home)
}

fn run_for_home(home: &Path) -> Result<()> {
    let path = home
        .join(".claude")
        .join("sandbox")
        .join(".sandbox-gitconfig");
    if !path.exists() {
        debug!("{} does not exist, skipping", path.display());
        return Ok(());
    }

    let metadata = fs::metadata(&path)?;
    if metadata.len() == 0 {
        info!(
            "Removing empty {} so the new credential-helper seed is written",
            path.display()
        );
        fs::remove_file(&path)?;
    } else {
        debug!(
            "{} is non-empty ({} bytes), leaving user customizations alone",
            path.display(),
            metadata.len()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn empty_file_is_removed() {
        let tmp = TempDir::new().unwrap();
        let sandbox_dir = tmp.path().join(".claude").join("sandbox");
        fs::create_dir_all(&sandbox_dir).unwrap();
        let path = sandbox_dir.join(".sandbox-gitconfig");
        fs::write(&path, "").unwrap();

        run_for_home(tmp.path()).unwrap();

        assert!(!path.exists(), "empty file should be deleted");
    }

    #[test]
    fn non_empty_file_is_preserved() {
        let tmp = TempDir::new().unwrap();
        let sandbox_dir = tmp.path().join(".claude").join("sandbox");
        fs::create_dir_all(&sandbox_dir).unwrap();
        let path = sandbox_dir.join(".sandbox-gitconfig");
        fs::write(&path, "[user]\n\temail = me@example.com\n").unwrap();

        run_for_home(tmp.path()).unwrap();

        assert!(path.exists(), "user-customized file must be kept");
        assert!(fs::read_to_string(&path)
            .unwrap()
            .contains("me@example.com"));
    }

    #[test]
    fn missing_file_is_ok() {
        let tmp = TempDir::new().unwrap();
        run_for_home(tmp.path()).unwrap();
    }
}
