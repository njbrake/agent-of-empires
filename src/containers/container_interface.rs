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
    pub named_volumes: Vec<(String, String)>,
    pub environment: Vec<(String, String)>,
    pub cpu_limit: Option<String>,
    pub memory_limit: Option<String>,
}

#[enum_dispatch]
pub trait ContainerRuntimeInterface {
    // backend stuff
    fn is_docker_available(&self) -> bool;

    fn is_daemon_running(&self) -> bool;

    fn get_docker_version(&self) -> Result<String>;

    fn pull_image(&self, image: &str) -> Result<()>;

    fn ensure_image(&self, image: &str) -> Result<()>;

    fn ensure_named_volume(&self, name: &str) -> Result<()>;

    fn default_sandbox_image(&self) -> &'static str;

    fn effective_default_image(&self) -> String;

    fn image_exists_locally(&self, image: &str) -> bool;

    // container management
    fn does_container_exist(&self, name: &str) -> Result<bool>;

    fn is_container_running(&self, name: &str) -> Result<bool>;

    fn create_container(&self, name: &str, image: &str, config: &ContainerConfig)
        -> Result<String>;

    fn start_container(&self, name: &str) -> Result<()>;

    fn stop_container(&self, name: &str) -> Result<()>;

    fn remove(&self, name: &str, force: bool) -> Result<()>;

    fn exec_command(&self, name: &str, options: Option<&str>) -> String;

    fn exec(&self, name: &str, cmd: &[&str]) -> Result<std::process::Output>;
}
