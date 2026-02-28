use super::container_interface::ContainerConfig;
use super::error::{DockerError, Result};
use std::process::Command;

/// Shared implementation for container runtimes.
///
/// Captures the behavioral differences between runtimes (Docker, Apple Container, etc.)
/// as configuration, then provides a single implementation of all the shared logic.
/// Runtime-specific methods (like container existence checks or running state detection)
/// remain in the individual runtime impls.
pub(crate) struct RuntimeBase {
    /// CLI binary name (e.g., "docker", "container")
    pub binary: &'static str,
    /// Human-readable name for log messages (e.g., "Docker", "Apple Container")
    pub name: &'static str,
    /// Args to check if daemon is running (e.g., ["info"] or ["system", "status"])
    pub daemon_check_args: &'static [&'static str],
    /// Args preceding the image name when pulling (e.g., ["pull"] or ["image", "pull"])
    pub pull_prefix: &'static [&'static str],
    /// Subcommand for removing containers (e.g., "rm" or "delete")
    pub remove_subcommand: &'static str,
    /// Whether this runtime supports the `:ro` read-only volume flag
    pub supports_read_only_volumes: bool,
}

impl RuntimeBase {
    pub const DOCKER: Self = Self {
        binary: "docker",
        name: "Docker",
        daemon_check_args: &["info"],
        pull_prefix: &["pull"],
        remove_subcommand: "rm",
        supports_read_only_volumes: true,
    };

    pub const APPLE_CONTAINER: Self = Self {
        binary: "container",
        name: "Apple Container",
        daemon_check_args: &["system", "status"],
        pull_prefix: &["image", "pull"],
        remove_subcommand: "delete",
        supports_read_only_volumes: false,
    };

    pub fn command(&self) -> Command {
        Command::new(self.binary)
    }

    pub fn is_available(&self) -> bool {
        self.command()
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn is_daemon_running(&self) -> bool {
        self.command()
            .args(self.daemon_check_args)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn get_version(&self) -> Result<String> {
        let output = self.command().arg("--version").output()?;

        if !output.status.success() {
            return Err(DockerError::NotInstalled);
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    pub fn image_exists_locally(&self, image: &str) -> bool {
        self.command()
            .args(["image", "inspect", image])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn pull_image(&self, image: &str) -> Result<()> {
        let mut cmd = self.command();
        cmd.args(self.pull_prefix);
        cmd.arg(image);
        let output = cmd.output()?;

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

    pub fn ensure_image(&self, image: &str) -> Result<()> {
        if self.image_exists_locally(image) {
            tracing::info!("Using local {} image '{}'", self.name, image);
            return Ok(());
        }

        tracing::info!("Pulling {} image '{}'", self.name, image);
        self.pull_image(image)
    }

    pub fn default_sandbox_image(&self) -> &'static str {
        "ghcr.io/njbrake/aoe-sandbox:latest"
    }

    pub fn effective_default_image(&self) -> String {
        crate::session::Config::load()
            .ok()
            .map(|c| c.sandbox.default_image)
            .unwrap_or_else(|| self.default_sandbox_image().to_string())
    }

    pub fn build_create_args(
        &self,
        name: &str,
        image: &str,
        config: &ContainerConfig,
    ) -> Vec<String> {
        let mut args = vec![
            "run".to_string(),
            "-d".to_string(),
            "--name".to_string(),
            name.to_string(),
            "-w".to_string(),
            config.working_dir.clone(),
        ];

        for vol in &config.volumes {
            if !self.supports_read_only_volumes && vol.read_only {
                tracing::warn!(
                    "{} does not support read-only volumes, mounting {} read-write",
                    self.name,
                    vol.container_path
                );
            }
            let mount = if vol.read_only && self.supports_read_only_volumes {
                format!("{}:{}:ro", vol.host_path, vol.container_path)
            } else {
                format!("{}:{}", vol.host_path, vol.container_path)
            };
            args.push("-v".to_string());
            args.push(mount);
        }

        for path in &config.anonymous_volumes {
            args.push("-v".to_string());
            args.push(path.clone());
        }

        for (key, value) in &config.environment {
            args.push("-e".to_string());
            args.push(format!("{}={}", key, value));
        }

        for port in &config.port_mappings {
            args.push("-p".to_string());
            args.push(port.clone());
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

        args
    }

    /// Run the container creation command (after existence has already been checked by the caller).
    pub fn run_create(&self, name: &str, image: &str, config: &ContainerConfig) -> Result<String> {
        let args = self.build_create_args(name, image, config);
        tracing::debug!("{} create args: {}", self.name, args.join(" "));
        let output = self.command().args(&args).output()?;

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

    pub fn start_container(&self, name: &str) -> Result<()> {
        let output = self.command().args(["start", name]).output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(DockerError::StartFailed(stderr.to_string()));
        }

        Ok(())
    }

    pub fn stop_container(&self, name: &str) -> Result<()> {
        let output = self.command().args(["stop", name]).output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("No such container") {
                return Err(DockerError::ContainerNotFound(name.to_string()));
            }
            return Err(DockerError::StopFailed(stderr.to_string()));
        }

        Ok(())
    }

    pub fn remove(&self, name: &str, force: bool) -> Result<()> {
        let mut args = vec![self.remove_subcommand.to_string()];
        if force {
            args.push("-f".to_string());
        }
        // Remove anonymous volumes with the container to prevent orphaned volume buildup.
        // This does NOT affect named volumes (like auth volumes).
        args.push("-v".to_string());
        args.push(name.to_string());

        let output = self.command().args(&args).output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("No such container") {
                return Err(DockerError::ContainerNotFound(name.to_string()));
            }
            return Err(DockerError::RemoveFailed(stderr.to_string()));
        }

        Ok(())
    }

    pub fn exec_command(&self, name: &str, options: Option<&str>) -> String {
        if let Some(opt_str) = options {
            [self.binary, "exec", "-it", opt_str, name].join(" ")
        } else {
            [self.binary, "exec", "-it", name].join(" ")
        }
    }

    pub fn exec(&self, name: &str, cmd: &[&str]) -> Result<std::process::Output> {
        let mut args = vec!["exec", name];
        args.extend(cmd);

        let output = self.command().args(&args).output()?;

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::containers::container_interface::VolumeMount;

    #[test]
    fn test_build_create_args_read_only_supported() {
        let base = RuntimeBase::DOCKER;
        let config = ContainerConfig {
            working_dir: "/workspace/project".to_string(),
            volumes: vec![VolumeMount {
                host_path: "/host/path".to_string(),
                container_path: "/container/path".to_string(),
                read_only: true,
            }],
            anonymous_volumes: vec![],
            environment: vec![],
            cpu_limit: None,
            memory_limit: None,
            port_mappings: vec![],
        };

        let args = base.build_create_args("test-container", "alpine:latest", &config);

        // Should include :ro suffix
        assert!(args.contains(&"/host/path:/container/path:ro".to_string()));
    }

    #[test]
    fn test_build_create_args_read_only_not_supported() {
        let base = RuntimeBase::APPLE_CONTAINER;
        let config = ContainerConfig {
            working_dir: "/workspace/project".to_string(),
            volumes: vec![VolumeMount {
                host_path: "/host/path".to_string(),
                container_path: "/container/path".to_string(),
                read_only: true,
            }],
            anonymous_volumes: vec![],
            environment: vec![],
            cpu_limit: None,
            memory_limit: None,
            port_mappings: vec![],
        };

        let args = base.build_create_args("test-container", "alpine:latest", &config);

        // Should NOT include :ro suffix (Apple Container doesn't support it)
        assert!(args.contains(&"/host/path:/container/path".to_string()));
        assert!(!args.iter().any(|a| a.ends_with(":ro")));
    }

    #[test]
    fn test_exec_command_with_options() {
        let base = RuntimeBase::DOCKER;
        let cmd = base.exec_command("my-container", Some("-w /workspace"));
        assert_eq!(cmd, "docker exec -it -w /workspace my-container");
    }

    #[test]
    fn test_exec_command_without_options() {
        let base = RuntimeBase::DOCKER;
        let cmd = base.exec_command("my-container", None);
        assert_eq!(cmd, "docker exec -it my-container");
    }

    #[test]
    fn test_exec_command_apple_container() {
        let base = RuntimeBase::APPLE_CONTAINER;
        let cmd = base.exec_command("my-container", None);
        assert_eq!(cmd, "container exec -it my-container");
    }

    #[test]
    fn test_build_create_args_full_config() {
        let base = RuntimeBase::DOCKER;
        let config = ContainerConfig {
            working_dir: "/workspace/project".to_string(),
            volumes: vec![VolumeMount {
                host_path: "/src".to_string(),
                container_path: "/dst".to_string(),
                read_only: false,
            }],
            anonymous_volumes: vec!["/tmp/cache".to_string()],
            environment: vec![("KEY".to_string(), "VALUE".to_string())],
            cpu_limit: Some("2".to_string()),
            memory_limit: Some("4g".to_string()),
            port_mappings: vec!["3000:3000".to_string()],
        };

        let args = base.build_create_args("test", "ubuntu:latest", &config);

        assert!(args.contains(&"run".to_string()));
        assert!(args.contains(&"-d".to_string()));
        assert!(args.contains(&"--name".to_string()));
        assert!(args.contains(&"test".to_string()));
        assert!(args.contains(&"-w".to_string()));
        assert!(args.contains(&"/workspace/project".to_string()));
        assert!(args.contains(&"/src:/dst".to_string()));
        assert!(args.contains(&"/tmp/cache".to_string()));
        assert!(args.contains(&"KEY=VALUE".to_string()));
        assert!(args.contains(&"--cpus".to_string()));
        assert!(args.contains(&"2".to_string()));
        assert!(args.contains(&"-m".to_string()));
        assert!(args.contains(&"4g".to_string()));
        assert!(args.contains(&"-p".to_string()));
        assert!(args.contains(&"3000:3000".to_string()));
        assert!(args.contains(&"ubuntu:latest".to_string()));
        assert!(args.contains(&"sleep".to_string()));
        assert!(args.contains(&"infinity".to_string()));
    }

    #[test]
    fn test_build_create_args_port_mappings() {
        let base = RuntimeBase::DOCKER;
        let config = ContainerConfig {
            working_dir: "/workspace".to_string(),
            volumes: vec![],
            anonymous_volumes: vec![],
            environment: vec![],
            cpu_limit: None,
            memory_limit: None,
            port_mappings: vec!["3000:3000".to_string(), "5432:5432".to_string()],
        };

        let args = base.build_create_args("test", "alpine:latest", &config);

        // Both port mappings should appear with -p flags
        let p_indices: Vec<usize> = args
            .iter()
            .enumerate()
            .filter(|(_, a)| *a == "-p")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(p_indices.len(), 2);
        assert_eq!(args[p_indices[0] + 1], "3000:3000");
        assert_eq!(args[p_indices[1] + 1], "5432:5432");
    }
}
