pub mod apple_container;
pub mod container_interface;
pub mod docker;
pub mod error;

use crate::cli::truncate_id;
pub use container_interface::{ContainerConfig, ContainerRuntimeInterface, VolumeMount};
use error::Result;

pub const CLAUDE_AUTH_VOLUME: &str = "aoe-claude-auth";
pub const OPENCODE_AUTH_VOLUME: &str = "aoe-opencode-auth";
pub const CODEX_AUTH_VOLUME: &str = "aoe-codex-auth";
pub const VIBE_AUTH_VOLUME: &str = "aoe-vibe-auth";

pub struct DockerContainer<T: ContainerRuntimeInterface> {
    pub name: String,
    pub image: String,
    runtime: T,
}

pub fn default_container_runtime() -> impl ContainerRuntimeInterface {
    docker::Docker
}

impl<T> DockerContainer<T>
where
    T: ContainerRuntimeInterface + Default,
{
    pub fn generate_name(session_id: &str) -> String {
        format!("aoe-sandbox-{}", truncate_id(session_id, 8))
    }

    pub fn new(session_id: &str, image: &str) -> Self {
        Self {
            name: Self::generate_name(session_id),
            image: image.to_string(),
            runtime: T::default(),
        }
    }

    pub fn from_session_id(session_id: &str) -> Self {
        Self {
            name: Self::generate_name(session_id),
            image: String::new(),
            runtime: T::default(),
        }
    }

    pub fn exists(&self) -> Result<bool> {
        self.runtime.does_container_exist(&self.name)
    }

    pub fn is_running(&self) -> Result<bool> {
        self.runtime.is_container_running(&self.name)
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

    pub fn exec_command(&self) -> Vec<String> {
        self.runtime.exec_command(&self.name)
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
        let name = DockerContainer::<docker::Docker>::generate_name("abc");
        assert_eq!(name, "aoe-sandbox-abc");
    }

    #[test]
    fn test_container_generate_name_long_id() {
        let name = DockerContainer::<docker::Docker>::generate_name("abcdefghijklmnop");
        assert_eq!(name, "aoe-sandbox-abcdefgh");
    }

    #[test]
    fn test_container_exec_command() {
        let container = DockerContainer::<docker::Docker>::new("test1234567890ab", "ubuntu:latest");
        let cmd = container.exec_command();
        assert_eq!(cmd, vec!["docker", "exec", "-it", "aoe-sandbox-test1234"]);
    }
}
