//! Lazy migration: Fix Docker volume ownership after switch to root user.
//!
//! In commit bb5105135a6152c005c1d840bcb86e5bad12f41a, the sandbox Dockerfile
//! switched from a `sandbox` user (UID 1000) to `root`. Existing auth volumes
//! have files owned by the old UID, causing Claude Code to fail authentication.
//!
//! This migration runs lazily (when a sandbox starts) rather than at app startup
//! because Docker may not be available at startup time.
//!
//! REMOVAL: This migration can be removed once all beta users have upgraded past
//! this point (roughly early 2026). To remove:
//! 1. Delete this file
//! 2. Remove `mod v002_docker_volume_ownership` from mod.rs
//! 3. Remove the `run_lazy_docker_migrations()` call from session/instance.rs

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::{debug, info};

use crate::containers::{
    self, ContainerRuntimeInterface, CLAUDE_AUTH_VOLUME, CODEX_AUTH_VOLUME, GEMINI_AUTH_VOLUME,
    OPENCODE_AUTH_VOLUME, VIBE_AUTH_VOLUME,
};

static HAS_RUN: AtomicBool = AtomicBool::new(false);

/// Fix ownership of Docker auth volumes to root.
/// Safe to call multiple times - only runs once per process.
/// Uses the sandbox image (which is already pulled) to avoid extra image downloads.
pub fn run_lazy() {
    if HAS_RUN.swap(true, Ordering::SeqCst) {
        return;
    }

    let image = containers::default_container_runtime().default_sandbox_image();
    for volume in [
        CLAUDE_AUTH_VOLUME,
        OPENCODE_AUTH_VOLUME,
        VIBE_AUTH_VOLUME,
        CODEX_AUTH_VOLUME,
        GEMINI_AUTH_VOLUME,
    ] {
        if let Err(e) = fix_volume_ownership(volume, image) {
            debug!("Could not fix ownership for volume {}: {}", volume, e);
        }
    }
}

fn fix_volume_ownership(volume_name: &str, image: &str) -> anyhow::Result<()> {
    let check = Command::new("docker")
        .args(["volume", "inspect", volume_name])
        .output()?;

    if !check.status.success() {
        debug!(
            "Volume {} does not exist, skipping ownership fix",
            volume_name
        );
        return Ok(());
    }

    info!(
        "Fixing ownership for volume {} (one-time migration)",
        volume_name
    );

    let output = Command::new("docker")
        .args([
            "run",
            "--rm",
            "-v",
            &format!("{}:/data", volume_name),
            image,
            "chown",
            "-R",
            "root:root",
            "/data",
        ])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("chown failed: {}", stderr.trim());
    }

    Ok(())
}
