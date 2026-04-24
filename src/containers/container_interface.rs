use std::collections::HashMap;

use super::error::Result;

pub struct VolumeMount {
    pub host_path: String,
    pub container_path: String,
    pub read_only: bool,
}

/// An environment variable entry for a container.
///
/// `Inherit` entries use Docker's `-e KEY` form (no value in argv), which reads
/// the value from the calling process's environment. This prevents secrets from
/// leaking into `ps` output.
///
/// `Literal` entries use `-e KEY=VALUE` and are appropriate for non-secret,
/// hard-coded values.
#[derive(Debug, Clone, PartialEq)]
pub enum EnvEntry {
    /// Value inherited from host environment. Only the key appears in argv;
    /// the value is passed to Docker via the process environment.
    Inherit { key: String, value: String },
    /// Literal (non-secret) value. Both key and value appear in argv.
    Literal { key: String, value: String },
}

impl EnvEntry {
    pub fn key(&self) -> &str {
        match self {
            EnvEntry::Inherit { key, .. } | EnvEntry::Literal { key, .. } => key,
        }
    }

    pub fn value(&self) -> &str {
        match self {
            EnvEntry::Inherit { value, .. } | EnvEntry::Literal { value, .. } => value,
        }
    }
}

pub struct ContainerConfig {
    pub working_dir: String,
    pub volumes: Vec<VolumeMount>,
    pub anonymous_volumes: Vec<String>,
    pub environment: Vec<EnvEntry>,
    pub cpu_limit: Option<String>,
    pub memory_limit: Option<String>,
    pub port_mappings: Vec<String>,
}

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

    fn exec_command(&self, name: &str, options: Option<&str>, cmd: &str) -> String;

    fn exec(&self, name: &str, cmd: &[&str]) -> Result<std::process::Output>;

    /// Check running state of all containers matching a name prefix in a single call.
    /// Returns a map of container name -> is_running.
    fn batch_running_states(&self, prefix: &str) -> HashMap<String, bool>;
}
