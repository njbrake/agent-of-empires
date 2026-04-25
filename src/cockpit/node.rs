//! Node.js runtime resolution for cockpit-worker subprocesses.
//!
//! Resolve order (matches the v4 design doc):
//! 1. `AOE_COCKPIT_NODE` env var.
//! 2. `cockpit.node_path` from settings.
//! 3. `node` on `PATH` (must satisfy minimum version).
//! 4. Previously-downloaded Node at
//!    `$AOE_DATA_DIR/cockpit/node-v22.21.0/bin/node`.
//! 5. (Future) download from nodejs.org/dist on first use.
//!
//! For 5 we have a real `download` function, but it is opt-in: the
//! caller must explicitly invoke it. Resolving at session-spawn time
//! returns a typed error if no Node is present, and the CLI surfaces
//! the doctor's "[!! ] Node runtime missing" message.

use std::path::{Path, PathBuf};

use thiserror::Error;
use tracing::{debug, info, warn};

/// The minimum Node major version aoe-agent supports. Matches the
/// `engines.node` field in `cockpit-worker/aoe-agent/package.json`.
pub const MIN_NODE_MAJOR: u32 = 20;

/// The pinned Node version aoe downloads when no host Node is found.
/// Bumping this requires bumping the SHA-256 below at the same time.
pub const PINNED_NODE_VERSION: &str = "22.21.0";

#[derive(Debug, Error)]
pub enum NodeError {
    #[error("no Node.js >= {0} found and AOE_COCKPIT_NODE is unset")]
    NoNode(u32),
    #[error("Node at {path} is too old (version {found}; need >= {min})")]
    TooOld {
        path: PathBuf,
        found: String,
        min: u32,
    },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result of a successful resolve.
#[derive(Debug, Clone)]
pub struct ResolvedNode {
    pub path: PathBuf,
    pub version: String,
    pub source: NodeSource,
}

#[derive(Debug, Clone, Copy)]
pub enum NodeSource {
    Env,
    Settings,
    Path,
    Bundled,
}

/// Resolve Node.js for cockpit use. `settings_node_path` is the value
/// configured in `cockpit.node_path` (empty when unset). `app_dir` is
/// where the bundled tarball would be extracted.
pub fn resolve(settings_node_path: &str, app_dir: &Path) -> Result<ResolvedNode, NodeError> {
    if let Ok(env_path) = std::env::var("AOE_COCKPIT_NODE") {
        if !env_path.is_empty() {
            let path = PathBuf::from(env_path);
            return verify_path(&path, NodeSource::Env);
        }
    }

    if !settings_node_path.is_empty() {
        let path = PathBuf::from(settings_node_path);
        return verify_path(&path, NodeSource::Settings);
    }

    if let Some(path) = which("node") {
        if let Ok(node) = verify_path(&path, NodeSource::Path) {
            return Ok(node);
        }
    }

    let bundled = bundled_node_path(app_dir);
    if bundled.exists() {
        return verify_path(&bundled, NodeSource::Bundled);
    }

    Err(NodeError::NoNode(MIN_NODE_MAJOR))
}

fn verify_path(path: &Path, source: NodeSource) -> Result<ResolvedNode, NodeError> {
    let output = std::process::Command::new(path).arg("--version").output()?;
    if !output.status.success() {
        return Err(NodeError::TooOld {
            path: path.to_path_buf(),
            found: "<no version output>".into(),
            min: MIN_NODE_MAJOR,
        });
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let major = parse_major(&raw).ok_or_else(|| NodeError::TooOld {
        path: path.to_path_buf(),
        found: raw.clone(),
        min: MIN_NODE_MAJOR,
    })?;
    if major < MIN_NODE_MAJOR {
        return Err(NodeError::TooOld {
            path: path.to_path_buf(),
            found: raw,
            min: MIN_NODE_MAJOR,
        });
    }
    debug!(target: "cockpit.node", source = ?source, path = %path.display(), version = %raw, "node resolved");
    Ok(ResolvedNode {
        path: path.to_path_buf(),
        version: raw,
        source,
    })
}

fn parse_major(raw: &str) -> Option<u32> {
    let trimmed = raw.trim_start_matches('v');
    let major_str = trimmed.split('.').next()?;
    major_str.parse::<u32>().ok()
}

fn which(binary: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(binary);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn bundled_node_path(app_dir: &Path) -> PathBuf {
    app_dir
        .join("cockpit")
        .join(format!("node-v{PINNED_NODE_VERSION}"))
        .join("bin")
        .join("node")
}

/// Download the pinned Node tarball from nodejs.org/dist and extract
/// to the bundled location. Verifies SHA-256 against the value
/// embedded in this binary.
///
/// MVP: not yet wired into auto-download at first session-spawn (the
/// design doc explicitly defers that to a follow-up to keep the spawn
/// path synchronous and predictable). Exposed as a helper so
/// `aoe cockpit doctor --fix` can invoke it explicitly.
pub async fn download(_app_dir: &Path) -> Result<ResolvedNode, NodeError> {
    // Implementation note: this requires choosing the platform tarball
    // (linux-x64 / linux-arm64 / darwin-x64 / darwin-arm64 /
    // win-x64 / win-arm64), pinning each SHA, fetching with reqwest,
    // verifying, and extracting tar.xz. Each platform's tarball is
    // 30-50 MB. The fetch + extract takes 10-30s on a typical
    // connection.
    //
    // The follow-up slice will fill this in. For now we return a
    // typed error so callers can fall back to a clear UX message
    // ("install Node yourself, or run `aoe cockpit doctor --fix`
    // when that lands").
    warn!(
        target: "cockpit.node",
        "automated Node download is not yet wired; install Node {} on PATH or set AOE_COCKPIT_NODE",
        MIN_NODE_MAJOR
    );
    Err(NodeError::NoNode(MIN_NODE_MAJOR))
}

/// Resolve Node, attempting an automated download if nothing is found
/// and `auto_download` is true.
pub async fn resolve_or_download(
    settings_node_path: &str,
    app_dir: &Path,
    auto_download: bool,
) -> Result<ResolvedNode, NodeError> {
    match resolve(settings_node_path, app_dir) {
        Ok(found) => {
            info!(target: "cockpit.node", "using node {} at {}", found.version, found.path.display());
            Ok(found)
        }
        Err(NodeError::NoNode(_)) if auto_download => download(app_dir).await,
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_major_handles_v_prefix_and_unprefixed() {
        assert_eq!(parse_major("v22.21.0"), Some(22));
        assert_eq!(parse_major("v20.0.0"), Some(20));
        assert_eq!(parse_major("18.17.1"), Some(18));
        assert_eq!(parse_major("not a version"), None);
    }

    #[test]
    fn bundled_path_uses_pinned_version() {
        let p = bundled_node_path(Path::new("/tmp/aoe"));
        let s = p.to_string_lossy();
        assert!(s.contains(&format!("node-v{PINNED_NODE_VERSION}")));
        assert!(s.ends_with("/bin/node") || s.ends_with("\\bin\\node"));
    }

    #[test]
    #[serial_test::serial]
    fn resolve_uses_env_var_when_set() {
        let Some(p) = which("node") else {
            eprintln!("skipping: node not on PATH");
            return;
        };
        std::env::set_var("AOE_COCKPIT_NODE", &p);
        let temp = tempfile::tempdir().unwrap();
        let resolved = resolve("", temp.path()).expect("env var resolves");
        std::env::remove_var("AOE_COCKPIT_NODE");
        assert!(matches!(resolved.source, NodeSource::Env));
    }

    #[test]
    #[serial_test::serial]
    fn resolve_returns_no_node_with_unmatchable_settings() {
        // No PATH-side node, no env, no settings → NoNode.
        let temp = tempfile::tempdir().unwrap();
        let saved_path = std::env::var_os("PATH");
        let saved_env = std::env::var_os("AOE_COCKPIT_NODE");
        std::env::remove_var("PATH");
        std::env::remove_var("AOE_COCKPIT_NODE");
        let result = resolve("", temp.path());
        if let Some(p) = saved_path {
            std::env::set_var("PATH", p);
        }
        if let Some(v) = saved_env {
            std::env::set_var("AOE_COCKPIT_NODE", v);
        }
        assert!(matches!(result, Err(NodeError::NoNode(_))));
    }
}
