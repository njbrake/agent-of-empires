pub mod container;
pub mod error;

pub use container::{ContainerConfig, DockerContainer, VolumeMount};
pub use error::{DockerError, Result};

use std::process::Command;

pub const CLAUDE_AUTH_VOLUME: &str = "aoe-claude-auth";
pub const OPENCODE_AUTH_VOLUME: &str = "aoe-opencode-auth";

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

/// Ensure an image is available locally by pulling it.
/// Always pulls to ensure we have the latest version of the image.
/// Docker efficiently caches layers, so this is fast when already up to date.
pub fn ensure_image(image: &str) -> Result<()> {
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

pub fn default_sandbox_image() -> &'static str {
    "ghcr.io/njbrake/aoe-sandbox:latest"
}
