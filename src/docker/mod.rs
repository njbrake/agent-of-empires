pub mod container;
pub mod error;

pub use container::{ContainerConfig, DockerContainer, VolumeMount};
pub use error::{DockerError, Result};

use std::process::Command;

pub const CLAUDE_AUTH_VOLUME: &str = "aoe-claude-auth";
pub const OPENCODE_AUTH_VOLUME: &str = "aoe-opencode-auth";
pub const VIBE_AUTH_VOLUME: &str = "aoe-vibe-auth";
pub const CODEX_AUTH_VOLUME: &str = "aoe-codex-auth";
pub const GEMINI_AUTH_VOLUME: &str = "aoe-gemini-auth";

pub fn is_docker_available() -> bool {
    Command::new("docker")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn is_daemon_running() -> bool {
    Command::new("docker")
        .args(["info"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn get_docker_version() -> Result<String> {
    let output = Command::new("docker").arg("--version").output()?;

    if !output.status.success() {
        return Err(DockerError::NotInstalled);
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn image_exists_locally(image: &str) -> bool {
    Command::new("docker")
        .args(["image", "inspect", image])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn pull_image(image: &str) -> Result<()> {
    let output = Command::new("docker").args(["pull", image]).output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(DockerError::ImageNotFound(format!(
            "{}: {}",
            image,
            stderr.trim()
        )));
    }

    Ok(())
}

/// Ensure an image is available locally.
/// If the image exists locally, uses it as-is (supports local-only images).
/// If not, attempts to pull from the registry.
pub fn ensure_image(image: &str) -> Result<()> {
    if image_exists_locally(image) {
        tracing::info!("Using local Docker image '{}'", image);
        return Ok(());
    }

    tracing::info!("Pulling Docker image '{}'", image);
    pull_image(image)
}

pub fn ensure_named_volume(name: &str) -> Result<()> {
    let check = Command::new("docker")
        .args(["volume", "inspect", name])
        .output()?;

    if !check.status.success() {
        let create = Command::new("docker")
            .args(["volume", "create", name])
            .output()?;

        if !create.status.success() {
            let stderr = String::from_utf8_lossy(&create.stderr);
            return Err(DockerError::CommandFailed(format!(
                "Failed to create volume {}: {}",
                name, stderr
            )));
        }
    }

    Ok(())
}

/// The hardcoded fallback sandbox image.
pub fn default_sandbox_image() -> &'static str {
    "ghcr.io/njbrake/aoe-sandbox:latest"
}

/// Returns the effective default sandbox image, checking user config first.
pub fn effective_default_image() -> String {
    crate::session::Config::load()
        .ok()
        .map(|c| c.sandbox.default_image)
        .unwrap_or_else(|| default_sandbox_image().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skip_if_no_docker() -> bool {
        !is_docker_available() || !is_daemon_running()
    }

    #[test]
    fn test_image_exists_locally_with_common_image() {
        if skip_if_no_docker() {
            return;
        }

        // hello-world is a tiny image that's commonly available or quick to pull
        let _ = Command::new("docker")
            .args(["pull", "hello-world"])
            .output();

        assert!(image_exists_locally("hello-world"));
    }

    #[test]
    fn test_image_exists_locally_nonexistent() {
        if skip_if_no_docker() {
            return;
        }

        assert!(!image_exists_locally(
            "nonexistent-image-that-does-not-exist:v999"
        ));
    }

    #[test]
    fn test_ensure_image_uses_local_image() {
        if skip_if_no_docker() {
            return;
        }

        // Ensure hello-world exists locally
        let _ = Command::new("docker")
            .args(["pull", "hello-world"])
            .output();

        // Should succeed without pulling since image exists
        let result = ensure_image("hello-world");
        assert!(result.is_ok());
    }

    #[test]
    fn test_ensure_image_fails_for_nonexistent_remote() {
        if skip_if_no_docker() {
            return;
        }

        // Should fail since image doesn't exist locally or remotely
        let result = ensure_image("nonexistent-image-that-does-not-exist:v999");
        assert!(result.is_err());
    }
}
