use super::error::{DockerError, Result};
use crate::cli::truncate_id;
use std::process::Command;

pub struct VolumeMount {
    pub host_path: String,
    pub container_path: String,
    pub read_only: bool,
}

pub struct ContainerConfig {
    pub working_dir: String,
    pub volumes: Vec<VolumeMount>,
    pub named_volumes: Vec<(String, String)>,
    pub anonymous_volumes: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub cpu_limit: Option<String>,
    pub memory_limit: Option<String>,
}

pub struct DockerContainer {
    pub name: String,
    pub image: String,
}

impl DockerContainer {
    pub fn new(session_id: &str, image: &str) -> Self {
        Self {
            name: Self::generate_name(session_id),
            image: image.to_string(),
        }
    }

    pub fn from_session_id(session_id: &str) -> Self {
        Self {
            name: Self::generate_name(session_id),
            image: String::new(),
        }
    }

    pub fn generate_name(session_id: &str) -> String {
        format!("aoe-sandbox-{}", truncate_id(session_id, 8))
    }

    pub fn exists(&self) -> Result<bool> {
        let output = Command::new("docker")
            .args(["container", "inspect", &self.name])
            .output()?;

        Ok(output.status.success())
    }

    pub fn is_running(&self) -> Result<bool> {
        let output = Command::new("docker")
            .args([
                "container",
                "inspect",
                "-f",
                "{{.State.Running}}",
                &self.name,
            ])
            .output()?;

        if !output.status.success() {
            return Ok(false);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.trim() == "true")
    }

    /// Build the docker run arguments from the container config.
    /// Separated from `create` to enable unit testing.
    pub(crate) fn build_create_args(&self, config: &ContainerConfig) -> Vec<String> {
        let mut args = vec![
            "run".to_string(),
            "-d".to_string(),
            "--name".to_string(),
            self.name.clone(),
            "-w".to_string(),
            config.working_dir.clone(),
        ];

        for vol in &config.volumes {
            let mount = if vol.read_only {
                format!("{}:{}:ro", vol.host_path, vol.container_path)
            } else {
                format!("{}:{}", vol.host_path, vol.container_path)
            };
            args.push("-v".to_string());
            args.push(mount);
        }

        for (vol_name, container_path) in &config.named_volumes {
            args.push("-v".to_string());
            args.push(format!("{}:{}", vol_name, container_path));
        }

        for path in &config.anonymous_volumes {
            args.push("-v".to_string());
            args.push(path.clone());
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

        args.push(self.image.clone());
        args.push("sleep".to_string());
        args.push("infinity".to_string());

        args
    }

    pub fn create(&self, config: &ContainerConfig) -> Result<String> {
        if self.exists()? {
            return Err(DockerError::ContainerAlreadyExists(self.name.clone()));
        }

        let args = self.build_create_args(config);

        let output = Command::new("docker").args(&args).output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("permission denied") {
                return Err(DockerError::PermissionDenied);
            }
            if stderr.contains("Cannot connect to the Docker daemon") {
                return Err(DockerError::DaemonNotRunning);
            }
            if stderr.contains("No such image") || stderr.contains("Unable to find image") {
                return Err(DockerError::ImageNotFound(self.image.clone()));
            }
            return Err(DockerError::CreateFailed(stderr.to_string()));
        }

        let container_id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Ok(container_id)
    }

    pub fn start(&self) -> Result<()> {
        let output = Command::new("docker")
            .args(["start", &self.name])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(DockerError::StartFailed(stderr.to_string()));
        }

        Ok(())
    }

    pub fn stop(&self) -> Result<()> {
        let output = Command::new("docker").args(["stop", &self.name]).output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("No such container") {
                return Err(DockerError::ContainerNotFound(self.name.clone()));
            }
            return Err(DockerError::StopFailed(stderr.to_string()));
        }

        Ok(())
    }

    pub fn remove(&self, force: bool) -> Result<()> {
        let mut args = vec!["rm".to_string()];
        if force {
            args.push("-f".to_string());
        }
        args.push(self.name.clone());

        let output = Command::new("docker").args(&args).output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("No such container") {
                return Err(DockerError::ContainerNotFound(self.name.clone()));
            }
            return Err(DockerError::RemoveFailed(stderr.to_string()));
        }

        Ok(())
    }

    pub fn exec_command(&self) -> Vec<String> {
        vec![
            "docker".to_string(),
            "exec".to_string(),
            "-it".to_string(),
            self.name.clone(),
        ]
    }

    pub fn exec(&self, cmd: &[&str]) -> Result<std::process::Output> {
        let mut args = vec!["exec", &self.name];
        args.extend(cmd);

        let output = Command::new("docker").args(&args).output()?;

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_name_short_id() {
        let name = DockerContainer::generate_name("abc");
        assert_eq!(name, "aoe-sandbox-abc");
    }

    #[test]
    fn test_generate_name_long_id() {
        let name = DockerContainer::generate_name("abcdefghijklmnop");
        assert_eq!(name, "aoe-sandbox-abcdefgh");
    }

    #[test]
    fn test_exec_command() {
        let container = DockerContainer::new("test1234567890ab", "ubuntu:latest");
        let cmd = container.exec_command();
        assert_eq!(cmd, vec!["docker", "exec", "-it", "aoe-sandbox-test1234"]);
    }

    #[test]
    fn test_anonymous_volumes_in_create_args() {
        let container = DockerContainer::new("test1234567890ab", "alpine:latest");
        let config = ContainerConfig {
            working_dir: "/workspace/myproject".to_string(),
            volumes: vec![],
            named_volumes: vec![],
            anonymous_volumes: vec![
                "/workspace/myproject/target".to_string(),
                "/workspace/myproject/node_modules".to_string(),
            ],
            environment: vec![],
            cpu_limit: None,
            memory_limit: None,
        };

        let args = container.build_create_args(&config);

        // Find the anonymous volume flags
        let v_positions: Vec<usize> = args
            .iter()
            .enumerate()
            .filter(|(_, a)| *a == "-v")
            .map(|(i, _)| i)
            .collect();

        let volume_values: Vec<&str> = v_positions.iter().map(|&i| args[i + 1].as_str()).collect();

        assert!(volume_values.contains(&"/workspace/myproject/target"));
        assert!(volume_values.contains(&"/workspace/myproject/node_modules"));
    }

    #[test]
    fn test_no_anonymous_volumes_when_empty() {
        let container = DockerContainer::new("test1234567890ab", "alpine:latest");
        let config = ContainerConfig {
            working_dir: "/workspace".to_string(),
            volumes: vec![],
            named_volumes: vec![],
            anonymous_volumes: vec![],
            environment: vec![],
            cpu_limit: None,
            memory_limit: None,
        };

        let args = container.build_create_args(&config);

        // No -v flags at all
        assert!(!args.contains(&"-v".to_string()));
    }
}
