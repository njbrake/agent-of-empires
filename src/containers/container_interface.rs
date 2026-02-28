use std::collections::HashMap;

use super::error::Result;
use enum_dispatch::enum_dispatch;

pub struct VolumeMount {
    pub host_path: String,
    pub container_path: String,
    pub read_only: bool,
}

pub struct ContainerConfig {
    pub working_dir: String,
    pub volumes: Vec<VolumeMount>,
    pub anonymous_volumes: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub cpu_limit: Option<String>,
    pub memory_limit: Option<String>,
    pub port_mappings: Vec<String>,
}

#[enum_dispatch]
pub trait ContainerRuntimeInterface {
    /// Check if the container runtime CLI is available
    fn is_available(&self) -> bool;

    /// Check if the container runtime daemon is running
    fn is_daemon_running(&self) -> bool;

    /// Get the container runtime version string
    fn get_version(&self) -> Result<String>;

    fn pull_image(&self, image: &str) -> Result<()>;

    fn ensure_image(&self, image: &str) -> Result<()>;

    fn default_sandbox_image(&self) -> &'static str;

    fn effective_default_image(&self) -> String;

    fn image_exists_locally(&self, image: &str) -> bool;

    // container management
    fn does_container_exist(&self, name: &str) -> Result<bool>;

    fn is_container_running(&self, name: &str) -> Result<bool>;

    /// Build the docker run arguments from the container config.
    /// Separated from `create` to enable unit testing.
    fn build_create_args(&self, name: &str, image: &str, config: &ContainerConfig) -> Vec<String>;

    fn create_container(&self, name: &str, image: &str, config: &ContainerConfig)
        -> Result<String>;

    fn start_container(&self, name: &str) -> Result<()>;

    fn stop_container(&self, name: &str) -> Result<()>;

    fn remove(&self, name: &str, force: bool) -> Result<()>;

    fn exec_command(&self, name: &str, options: Option<&str>) -> String;

    fn exec(&self, name: &str, cmd: &[&str]) -> Result<std::process::Output>;

    /// Check running state of all containers matching a name prefix in a single call.
    /// Returns a map of container name -> is_running.
    fn batch_running_states(&self, prefix: &str) -> HashMap<String, bool>;
}
