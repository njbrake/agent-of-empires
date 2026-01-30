use super::container_interface::{ContainerConfig, ContainerRuntimeInterface};
use super::error::{DockerError, Result};
use serde_json::Value;
use std::process::Command;

#[derive(Default)]
pub struct AppleContainer;

impl ContainerRuntimeInterface for AppleContainer {
    fn is_docker_available(&self) -> bool {
        Command::new("container")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn is_daemon_running(&self) -> bool {
        Command::new("container")
            .args(["system", "status"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn get_docker_version(&self) -> Result<String> {
        let output = Command::new("container").arg("--version").output()?;

        if !output.status.success() {
            return Err(DockerError::NotInstalled);
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    fn pull_image(&self, image: &str) -> Result<()> {
        let output = Command::new("container")
            .args(["image", "pull", image])
            .output()?;

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

    fn ensure_image(&self, image: &str) -> Result<()> {
        let output = Command::new("container")
            .args(["image", "inspect", image])
            .output()?;

        if !output.status.success() {
            self.pull_image(image)?;
        }

        Ok(())
    }

    fn ensure_named_volume(&self, name: &str) -> Result<()> {
        let check = Command::new("container")
            .args(["volume", "inspect", name])
            .output()?;

        if !check.status.success() {
            let create = Command::new("container")
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

    fn default_sandbox_image(&self) -> &'static str {
        "ghcr.io/njbrake/aoe-sandbox:latest"
    }

    fn effective_default_image(&self) -> String {
        crate::session::Config::load()
            .ok()
            .map(|c| c.sandbox.default_image)
            .unwrap_or_else(|| self.default_sandbox_image().to_string())
    }

    fn image_exists_locally(&self, image: &str) -> bool {
        Command::new("container")
            .args(["image", "inspect", image])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn does_container_exist(&self, name: &str) -> Result<bool> {
        // container inspect returns success(0) for non-existent container
        let output = Command::new("container").args(["logs", name]).output()?;

        Ok(output.status.success())
    }

    fn is_container_running(&self, name: &str) -> Result<bool> {
        let output = Command::new("container").args(["inspect", name]).output()?;

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
            if vol.read_only {
                tracing::warn!(
                    "apple container does not support read-only volume, will mount read-write."
                );
            }
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

        tracing::debug!("args to container command: {}", args.join(" "));
        let output = Command::new("container").args(&args).output()?;

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
        let output = Command::new("container").args(["start", name]).output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(DockerError::StartFailed(stderr.to_string()));
        }

        Ok(())
    }

    fn stop_container(&self, name: &str) -> Result<()> {
        let output = Command::new("container").args(["stop", name]).output()?;

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
        let mut args = vec!["delete".to_string()];
        if force {
            args.push("-f".to_string());
        }
        args.push(name.to_string());

        let output = Command::new("container").args(&args).output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("No such container") {
                return Err(DockerError::ContainerNotFound(name.to_string()));
            }
            return Err(DockerError::RemoveFailed(stderr.to_string()));
        }

        Ok(())
    }

    fn exec_command(&self, name: &str, options: Option<&str>) -> String {
        if let Some(opt_str) = options {
            ["container", "exec", "-it", opt_str, name, "sh", "-c"].join(" ")
        } else {
            ["container", "exec", "-it", name, "sh", "-c"].join(" ")
        }
    }

    fn exec(&self, name: &str, cmd: &[&str]) -> Result<std::process::Output> {
        let mut args = vec!["exec", name];
        args.extend(cmd);

        let output = Command::new("container").args(&args).output()?;

        Ok(output)
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    fn get_apple_container_runtime_if_available() -> Option<AppleContainer> {
        let apple_container = AppleContainer;
        if !apple_container.is_docker_available() || !apple_container.is_daemon_running() {
            None
        } else {
            Some(apple_container)
        }
    }

    #[test]
    fn test_apple_container_image_exists_locally_with_common_image() {
        if let Some(apple_container) = get_apple_container_runtime_if_available() {
            // hello-world is a tiny image that's commonly available or quick to pull
            apple_container.pull_image("hello-world").unwrap();

            assert!(apple_container.image_exists_locally("hello-world"));
        }
    }

    #[test]
    fn test_apple_container_image_exists_locally_nonexistent() {
        if let Some(apple_container) = get_apple_container_runtime_if_available() {
            assert!(
                !apple_container.image_exists_locally("nonexistent-image-that-does-not-exist:v999")
            );
        }
    }

    #[test]
    fn test_apple_container_ensure_image_uses_local_image() {
        if let Some(apple_container) = get_apple_container_runtime_if_available() {
            // Ensure hello-world exists locally
            apple_container.pull_image("hello-world").unwrap();

            // Should succeed without pulling since image exists
            let result = apple_container.ensure_image("hello-world");
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_apple_container_ensure_image_fails_for_nonexistent_remote() {
        if let Some(apple_container) = get_apple_container_runtime_if_available() {
            // Should fail since image doesn't exist locally or remotely
            let result = apple_container.ensure_image("nonexistent-image-that-does-not-exist:v999");
            assert!(result.is_err());
        }
    }
}
