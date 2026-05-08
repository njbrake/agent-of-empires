//! Handlers for ACP `fs/*` requests delegated by agents.
//!
//! ACP defines `fs/read_text_file` and `fs/write_text_file` as agent → client
//! requests. The agent asks aoe to read or write; aoe enforces sandboxing
//! and worktree isolation before doing the fs op.
//!
//! Important security invariants enforced here:
//! 1. Reads and writes must resolve to a path inside the session's allowed
//!    roots (worktree path + any explicit additional dirs from
//!    `session/new`).
//! 2. Symlinks are followed but the resolved path must still be inside the
//!    allowed roots.
//! 3. Writes outside the allowed roots produce a structured ACP error.
//! 4. All accesses are logged via `tracing::info!` with the session id.
//!
//! Sandbox interaction: when the session runs inside a Docker container,
//! the agent process lives inside the container; this handler runs in
//! aoe-host. The unix socket transport (see design v4) is what makes the
//! request cross the container boundary; once it arrives here, the path is
//! interpreted in the container's mounted-volume layout.

use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;
use tracing::info;

#[derive(Debug, Error)]
pub enum FsError {
    #[error("path is outside session roots: {0}")]
    OutsideRoots(PathBuf),
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("path is not absolute: {0}")]
    NotAbsolute(PathBuf),
    #[error("path contains invalid utf-8")]
    NonUtf8Path,
    #[error("refusing to write through symlink: {0}")]
    SymlinkInPath(PathBuf),
}

/// Per-session allowed-roots policy. The session's worktree path plus any
/// additional dirs declared at `session/new` time.
#[derive(Debug, Clone)]
pub struct FsPolicy {
    pub allowed_roots: Vec<PathBuf>,
}

impl FsPolicy {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self {
            allowed_roots: roots,
        }
    }

    /// Returns the path canonicalized iff it is inside one of the allowed
    /// roots. Symlinks are resolved before the inside-roots check.
    ///
    /// For non-existent paths (writes that create new files) we
    /// canonicalize the parent and reattach the literal `file_name`. We
    /// then `symlink_metadata` the assembled path: if the leaf already
    /// exists as a symlink, we refuse the operation so a write can't
    /// follow the symlink to a target outside the allowed roots. This
    /// closes the TOCTOU between policy check and the actual `write`
    /// (which follows symlinks by default).
    pub fn resolve_inside(&self, path: &Path) -> Result<PathBuf, FsError> {
        if !path.is_absolute() {
            return Err(FsError::NotAbsolute(path.to_path_buf()));
        }
        let canonical = if path.exists() {
            path.canonicalize()?
        } else if let Some(parent) = path.parent() {
            let parent_canonical = parent.canonicalize()?;
            match path.file_name() {
                Some(name) => parent_canonical.join(name),
                None => parent_canonical,
            }
        } else {
            path.to_path_buf()
        };

        // If the leaf exists as a symlink (even when the symlink target
        // doesn't), reject. The agent gets a structured error and can
        // ask the user for a different path. This is the primary
        // sandbox-escape we close: an agent could otherwise place a
        // symlink in the allowed root pointing outside, then ask aoe
        // to write through it.
        if let Ok(meta) = std::fs::symlink_metadata(&canonical) {
            if meta.file_type().is_symlink() {
                return Err(FsError::SymlinkInPath(canonical));
            }
        }

        for root in &self.allowed_roots {
            let root_canonical = root.canonicalize().unwrap_or_else(|_| root.clone());
            if canonical.starts_with(&root_canonical) {
                return Ok(canonical);
            }
        }
        Err(FsError::OutsideRoots(canonical))
    }
}

/// Implementation of ACP `fs/read_text_file`.
pub fn handle_read(policy: &FsPolicy, session_id: &str, path: &Path) -> Result<String, FsError> {
    let resolved = policy.resolve_inside(path)?;
    let text = read_no_follow(&resolved)?;
    info!(target: "cockpit.fs", session = %session_id, path = %resolved.display(), bytes = text.len(), "fs/read");
    Ok(text)
}

/// Implementation of ACP `fs/write_text_file`.
pub fn handle_write(
    policy: &FsPolicy,
    session_id: &str,
    path: &Path,
    contents: &str,
) -> Result<(), FsError> {
    let resolved = policy.resolve_inside(path)?;
    if let Some(parent) = resolved.parent() {
        std::fs::create_dir_all(parent)?;
    }
    write_no_follow(&resolved, contents)?;
    info!(
        target: "cockpit.fs",
        session = %session_id,
        path = %resolved.display(),
        bytes = contents.len(),
        "fs/write"
    );
    Ok(())
}

/// Open with `O_NOFOLLOW` so the kernel itself refuses to follow a
/// symlink at the leaf. Pairs with `FsPolicy::resolve_inside` to close
/// the TOCTOU window between the policy check and the actual I/O.
#[cfg(unix)]
fn read_no_follow(path: &Path) -> io::Result<String> {
    use std::io::Read;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(nix::fcntl::OFlag::O_NOFOLLOW.bits())
        .open(path)?;
    let mut buf = String::new();
    file.read_to_string(&mut buf)?;
    Ok(buf)
}

#[cfg(not(unix))]
fn read_no_follow(path: &Path) -> io::Result<String> {
    std::fs::read_to_string(path)
}

#[cfg(unix)]
fn write_no_follow(path: &Path, contents: &str) -> io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .custom_flags(nix::fcntl::OFlag::O_NOFOLLOW.bits())
        .open(path)?;
    file.write_all(contents.as_bytes())?;
    Ok(())
}

#[cfg(not(unix))]
fn write_no_follow(path: &Path, contents: &str) -> io::Result<()> {
    std::fs::write(path, contents)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn resolve_inside_allowed_root() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        fs::write(root.join("file.txt"), "hello").unwrap();
        let policy = FsPolicy::new(vec![root.clone()]);
        let resolved = policy
            .resolve_inside(&root.join("file.txt"))
            .expect("should resolve inside root");
        assert!(resolved.starts_with(root.canonicalize().unwrap()));
    }

    #[test]
    fn rejects_path_outside_roots() {
        let temp = tempfile::tempdir().unwrap();
        let policy = FsPolicy::new(vec![temp.path().to_path_buf()]);
        let outside = std::env::temp_dir().join("definitely-not-in-temp-dir-of-test");
        let result = policy.resolve_inside(&outside);
        assert!(matches!(result, Err(FsError::OutsideRoots(_))));
    }

    #[test]
    fn rejects_relative_path() {
        let policy = FsPolicy::new(vec![PathBuf::from("/tmp")]);
        let result = policy.resolve_inside(Path::new("relative/file.txt"));
        assert!(matches!(result, Err(FsError::NotAbsolute(_))));
    }

    #[test]
    fn read_and_write_roundtrip() {
        let temp = tempfile::tempdir().unwrap();
        let policy = FsPolicy::new(vec![temp.path().to_path_buf()]);
        let path = temp.path().join("hello.txt");
        handle_write(&policy, "s-1", &path, "hi there").unwrap();
        let read = handle_read(&policy, "s-1", &path).unwrap();
        assert_eq!(read, "hi there");
    }

    /// Symlink whose target exists outside the allowed root: the
    /// canonicalize-and-check path catches it as `OutsideRoots`. The
    /// outside file must remain untouched.
    #[cfg(unix)]
    #[test]
    fn rejects_symlink_leaf_pointing_outside_root() {
        let temp = tempfile::tempdir().unwrap();
        let policy = FsPolicy::new(vec![temp.path().to_path_buf()]);
        let outside = std::env::temp_dir().join("aoe-fs-handler-symlink-target");
        let _ = std::fs::remove_file(&outside);
        std::fs::write(&outside, "secret").unwrap();
        let symlink_in_root = temp.path().join("escape");
        std::os::unix::fs::symlink(&outside, &symlink_in_root).unwrap();

        let read_result = handle_read(&policy, "s-1", &symlink_in_root);
        assert!(matches!(read_result, Err(FsError::OutsideRoots(_))));

        let write_result = handle_write(&policy, "s-1", &symlink_in_root, "owned");
        assert!(matches!(write_result, Err(FsError::OutsideRoots(_))));

        let target_after = std::fs::read_to_string(&outside).unwrap();
        assert_eq!(target_after, "secret", "outside file must remain untouched");
        let _ = std::fs::remove_file(outside);
    }

    /// Dangling symlink (target does not exist) inside the allowed root.
    /// `path.exists()` returns false because it follows symlinks; the
    /// fallback parent-canonicalize branch catches the leaf as a
    /// symlink and rejects with `SymlinkInPath`.
    #[cfg(unix)]
    #[test]
    fn rejects_dangling_symlink_leaf() {
        let temp = tempfile::tempdir().unwrap();
        let policy = FsPolicy::new(vec![temp.path().to_path_buf()]);
        let dangling = temp.path().join("dangling");
        std::os::unix::fs::symlink("/no/such/path", &dangling).unwrap();
        let result = handle_write(&policy, "s-1", &dangling, "x");
        assert!(matches!(result, Err(FsError::SymlinkInPath(_))));
    }

    /// Belt-and-suspenders: even if a symlink races into place between
    /// the policy check and the open, `O_NOFOLLOW` makes the kernel
    /// refuse the open (ELOOP on Linux, EMLINK on some BSDs). This
    /// closes the TOCTOU window between `resolve_inside` and the
    /// actual read/write.
    #[cfg(unix)]
    #[test]
    fn open_with_nofollow_rejects_symlink_leaf() {
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("real");
        std::fs::write(&target, "ok").unwrap();
        let link = temp.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(
            read_no_follow(&link).is_err(),
            "O_NOFOLLOW must refuse a symlinked leaf"
        );
    }
}
