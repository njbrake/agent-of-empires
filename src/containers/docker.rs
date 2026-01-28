use super::container_interface::{ContainerConfig, ContainerRuntimeInterface};
use super::error::{DockerError, Result};
use serde_json::Value;
use std::process::Command;

#[derive(Default)]
pub struct Docker;

impl ContainerRuntimeInterface for Docker {
    fn is_docker_available(&self) -> bool {
        Command::new("docker")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn is_daemon_running(&self) -> bool {
        Command::new("docker")
            .args(["info"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn get_docker_version(&self) -> Result<String> {
        let output = Command::new("docker").arg("--version").output()?;

        if !output.status.success() {
            return Err(DockerError::NotInstalled);
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn image_exists_locally(&self, image: &str) -> bool {
        Command::new("docker")
            .args(["image", "inspect", image])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn pull_image(&self, image: &str) -> Result<()> {
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
    fn ensure_image(&self, image: &str) -> Result<()> {
        if self.image_exists_locally(image) {
            tracing::info!("Using local Docker image '{}'", image);
            return Ok(());
        }

        tracing::info!("Pulling Docker image '{}'", image);
        self.pull_image(image)
    }

    fn ensure_named_volume(&self, name: &str) -> Result<()> {
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
    fn default_sandbox_image(&self) -> &'static str {
        "ghcr.io/njbrake/aoe-sandbox:latest"
    }

    /// Returns the effective default sandbox image, checking user config first.
    fn effective_default_image(&self) -> String {
        crate::session::Config::load()
            .ok()
            .map(|c| c.sandbox.default_image)
            .unwrap_or_else(|| self.default_sandbox_image().to_string())
    }

    fn does_container_exist(&self, name: &str) -> Result<bool> {
        // container inspect returns success(0) for non-existent container
        let output = Command::new("docker").args(["logs", name]).output()?;

        Ok(output.status.success())
    }

    fn is_container_running(&self, name: &str) -> Result<bool> {
        let output = Command::new("docker").args(["inspect", name]).output()?;

        if !output.status.success() {
            return Ok(false);
        }

        let out_json: Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| DockerError::CommandFailed(e.to_string()))?;

        if let Some(status) = out_json.pointer("/0/status") {
            Ok(status == "running")
        } else {
            Ok(false)
        }
    }

    fn create_container(
        &self,
        name: &str,
        image: &str,
        config: &ContainerConfig,
    ) -> Result<String> {
        if self.does_container_exist(name)? {
            return Err(DockerError::ContainerAlreadyExists(name.to_string()));
        }

        let mut args = vec![
            "run".to_string(),
            "-d".to_string(),
            "--name".to_string(),
            name.to_string(),
            "-w".to_string(),
            config.working_dir.clone(),
        ];

        for vol in &config.volumes {
            let mount = format!("{}:{}", vol.host_path, vol.container_path);
            args.push("-v".to_string());
            args.push(mount);
        }

        for (vol_name, container_path) in &config.named_volumes {
            args.push("-v".to_string());
            args.push(format!("{}:{}", vol_name, container_path));
        }

        for (key, value) in &config.environment {
            args.push("-e".to_string());
            args.push(format!("{}={}", key, value));
        }

        if let Some(cpu) = &config.cpu_limit {
            args.push("--cpus".to_string());
            args.push(cpu.clone());
        }

        if let Some(mem) = &config.memory_limit {
            args.push("-m".to_string());
            args.push(mem.clone());
        }

        args.push(image.to_string());
        args.push("sleep".to_string());
        args.push("infinity".to_string());

        let output = Command::new("docker").args(&args).output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            tracing::debug!("stderr: {}", stderr);
            if stderr.contains("permission denied") {
                return Err(DockerError::PermissionDenied);
            }
            if stderr.contains("Cannot connect to the Docker daemon") {
                return Err(DockerError::DaemonNotRunning);
            }
            if stderr.contains("No such image") || stderr.contains("Unable to find image") {
                return Err(DockerError::ImageNotFound(image.to_string()));
            }
            return Err(DockerError::CreateFailed(stderr.to_string()));
        }

        let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(container_id)
    }

    fn start_container(&self, name: &str) -> Result<()> {
        let output = Command::new("docker").args(["start", name]).output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(DockerError::StartFailed(stderr.to_string()));
        }

        Ok(())
    }

    fn stop_container(&self, name: &str) -> Result<()> {
        let output = Command::new("docker").args(["stop", name]).output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("No such container") {
                return Err(DockerError::ContainerNotFound(name.to_string()));
            }
            return Err(DockerError::StopFailed(stderr.to_string()));
        }

        Ok(())
    }

    fn remove(&self, name: &str, force: bool) -> Result<()> {
        let mut args = vec!["rm".to_string()];
        if force {
            args.push("-f".to_string());
        }
        args.push(name.to_string());

        let output = Command::new("docker").args(&args).output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("No such container") {
                return Err(DockerError::ContainerNotFound(name.to_string()));
            }
            return Err(DockerError::RemoveFailed(stderr.to_string()));
        }

        Ok(())
    }

    fn exec_command(&self, name: &str) -> Vec<String> {
        vec![
            "docker".to_string(),
            "exec".to_string(),
            "-it".to_string(),
            name.to_string(),
        ]
    }

    fn exec(&self, name: &str, cmd: &[&str]) -> Result<std::process::Output> {
        let mut args = vec!["exec", name];
        args.extend(cmd);

        let output = Command::new("docker").args(&args).output()?;

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get_docker_runtime_if_available() -> Option<Docker> {
        let docker = Docker;
        if !docker.is_docker_available() || !docker.is_daemon_running() {
            None
        } else {
            Some(docker)
        }
    }

    #[test]
    fn test_docker_image_exists_locally_with_common_image() {
        if let Some(docker) = get_docker_runtime_if_available() {
            // hello-world is a tiny image that's commonly available or quick to pull
            docker.pull_image("hello-world").unwrap();

            assert!(docker.image_exists_locally("hello-world"));
        }
    }

    #[test]
    fn test_docker_image_exists_locally_nonexistent() {
        if let Some(docker) = get_docker_runtime_if_available() {
            assert!(!docker.image_exists_locally("nonexistent-image-that-does-not-exist:v999"));
        }
    }

    #[test]
    fn test_docker_ensure_image_uses_local_image() {
        if let Some(docker) = get_docker_runtime_if_available() {
            // Ensure hello-world exists locally
            docker.pull_image("hello-world").unwrap();

            // Should succeed without pulling since image exists
            let result = docker.ensure_image("hello-world");
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_docker_ensure_image_fails_for_nonexistent_remote() {
        if let Some(docker) = get_docker_runtime_if_available() {
            // Should fail since image doesn't exist locally or remotely
            let result = docker.ensure_image("nonexistent-image-that-does-not-exist:v999");
            assert!(result.is_err());
        }
    }
}
