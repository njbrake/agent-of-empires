mod apple_container;
pub mod container_interface;
mod docker;
pub mod error;
pub(crate) mod runtime_base;

use std::collections::HashMap;

use crate::cli::truncate_id;
use crate::session::{Config, ContainerRuntimeName};
use apple_container::AppleContainer;
pub use container_interface::{ContainerConfig, ContainerRuntimeInterface, VolumeMount};
use docker::Docker;
use enum_dispatch::enum_dispatch;
use error::Result;

#[enum_dispatch(ContainerRuntimeInterface)]
pub enum ContainerRuntime {
    AppleContainer,
    Docker,
}

impl Default for ContainerRuntime {
    fn default() -> Self {
        Docker::default().into()
    }
}

/// Returns the CLI binary name for the configured container runtime.
pub fn runtime_binary() -> &'static str {
    if let Ok(cfg) = Config::load() {
        match cfg.sandbox.container_runtime {
            ContainerRuntimeName::AppleContainer => "container",
            ContainerRuntimeName::Docker => "docker",
        }
    } else {
        "docker"
    }
}

pub fn get_container_runtime() -> ContainerRuntime {
    if let Ok(cfg) = Config::load() {
        match cfg.sandbox.container_runtime {
            ContainerRuntimeName::AppleContainer => AppleContainer::default().into(),
            ContainerRuntimeName::Docker => Docker::default().into(),
        }
    } else {
        ContainerRuntime::default()
    }
}

/// Check running state of all aoe sandbox containers in a single subprocess call.
/// Returns a map of container name -> is_running.
pub fn batch_container_health() -> HashMap<String, bool> {
    get_container_runtime().batch_running_states("aoe-sandbox-")
}

pub struct DockerContainer {
    pub name: String,
    pub image: String,
    runtime: ContainerRuntime,
}

impl DockerContainer {
    pub fn new(session_id: &str, image: &str) -> Self {
        Self {
            name: Self::generate_name(session_id),
            image: image.to_string(),
            runtime: get_container_runtime(),
        }
    }

    pub fn generate_name(session_id: &str) -> String {
        format!("aoe-sandbox-{}", truncate_id(session_id, 8))
    }

    pub fn from_session_id(session_id: &str) -> Self {
        Self {
            name: Self::generate_name(session_id),
            image: String::new(),
            runtime: get_container_runtime(),
        }
    }

    pub fn exists(&self) -> Result<bool> {
        self.runtime.does_container_exist(&self.name)
    }

    pub fn is_running(&self) -> Result<bool> {
        self.runtime.is_container_running(&self.name)
    }

    pub fn build_create_args(&self, config: &ContainerConfig) -> Vec<String> {
        self.runtime
            .build_create_args(&self.name, &self.image, config)
    }

    pub fn create(&self, config: &ContainerConfig) -> Result<String> {
        self.runtime
            .create_container(&self.name, &self.image, config)
    }

    pub fn start(&self) -> Result<()> {
        self.runtime.start_container(&self.name)
    }

    pub fn stop(&self) -> Result<()> {
        self.runtime.stop_container(&self.name)
    }

    pub fn remove(&self, force: bool) -> Result<()> {
        self.runtime.remove(&self.name, force)
    }

    pub fn exec_command(&self, options: Option<&str>) -> String {
        self.runtime.exec_command(&self.name, options)
    }

    pub fn exec(&self, cmd: &[&str]) -> Result<std::process::Output> {
        self.runtime.exec(&self.name, cmd)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_generate_name_short_id() {
        let name = DockerContainer::generate_name("abc");
        assert_eq!(name, "aoe-sandbox-abc");
    }

    #[test]
    fn test_container_generate_name_long_id() {
        let name = DockerContainer::generate_name("abcdefghijklmnop");
        assert_eq!(name, "aoe-sandbox-abcdefgh");
    }

    #[test]
    fn test_container_exec_command() {
        let container = DockerContainer::new("test1234567890ab", "ubuntu:latest");
        let cmd = container.exec_command(None);
        assert_eq!(cmd, "docker exec -it aoe-sandbox-test1234");
    }
    #[test]
    fn test_anonymous_volumes_in_create_args() {
        let container = DockerContainer::new("test1234567890ab", "alpine:latest");
        let config = ContainerConfig {
            working_dir: "/workspace/myproject".to_string(),
            volumes: vec![],
            anonymous_volumes: vec![
                "/workspace/myproject/target".to_string(),
                "/workspace/myproject/node_modules".to_string(),
            ],
            environment: vec![],
            cpu_limit: None,
            memory_limit: None,
            port_mappings: vec![],
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
            anonymous_volumes: vec![],
            environment: vec![],
            cpu_limit: None,
            memory_limit: None,
            port_mappings: vec![],
        };

        let args = container.build_create_args(&config);

        // No -v flags at all
        assert!(!args.contains(&"-v".to_string()));
    }
}
