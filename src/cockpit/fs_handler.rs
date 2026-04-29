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
    pub fn resolve_inside(&self, path: &Path) -> Result<PathBuf, FsError> {
        if !path.is_absolute() {
            return Err(FsError::NotAbsolute(path.to_path_buf()));
        }
        // Don't strictly require the path to exist (writes create new
        // files); resolve the parent dir and re-attach the file name.
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
    let text = std::fs::read_to_string(&resolved)?;
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
    std::fs::write(&resolved, contents)?;
    info!(
        target: "cockpit.fs",
        session = %session_id,
        path = %resolved.display(),
        bytes = contents.len(),
        "fs/write"
    );
    Ok(())
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
}
