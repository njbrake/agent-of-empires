use super::container_interface::{ContainerConfig, ContainerRuntimeInterface};
use super::error::{DockerError, Result};
use super::runtime_base::RuntimeBase;
use serde_json::Value;

pub struct AppleContainer {
    base: RuntimeBase,
}

impl Default for AppleContainer {
    fn default() -> Self {
        Self {
            base: RuntimeBase::APPLE_CONTAINER,
        }
    }
}

impl ContainerRuntimeInterface for AppleContainer {
    fn is_available(&self) -> bool {
        self.base.is_available()
    }

    fn is_daemon_running(&self) -> bool {
        self.base.is_daemon_running()
    }

    fn get_version(&self) -> Result<String> {
        self.base.get_version()
    }

    fn pull_image(&self, image: &str) -> Result<()> {
        self.base.pull_image(image)
    }

    fn ensure_image(&self, image: &str) -> Result<()> {
        self.base.ensure_image(image)
    }

    fn default_sandbox_image(&self) -> &'static str {
        self.base.default_sandbox_image()
    }

    fn effective_default_image(&self) -> String {
        self.base.effective_default_image()
    }

    fn image_exists_locally(&self, image: &str) -> bool {
        self.base.image_exists_locally(image)
    }

    fn does_container_exist(&self, name: &str) -> Result<bool> {
        // Apple Container's `inspect` returns success(0) for non-existent containers,
        // so we use `logs` which properly fails for missing containers.
        let output = self.base.command().args(["logs", name]).output()?;
        Ok(output.status.success())
    }

    fn is_container_running(&self, name: &str) -> Result<bool> {
        let output = self.base.command().args(["inspect", name]).output()?;

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

    fn build_create_args(&self, name: &str, image: &str, config: &ContainerConfig) -> Vec<String> {
        self.base.build_create_args(name, image, config)
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
        self.base.run_create(name, image, config)
    }

    fn start_container(&self, name: &str) -> Result<()> {
        self.base.start_container(name)
    }

    fn stop_container(&self, name: &str) -> Result<()> {
        self.base.stop_container(name)
    }

    fn remove(&self, name: &str, force: bool) -> Result<()> {
        self.base.remove(name, force)
    }

    fn exec_command(&self, name: &str, options: Option<&str>) -> String {
        self.base.exec_command(name, options)
    }

    fn exec(&self, name: &str, cmd: &[&str]) -> Result<std::process::Output> {
        self.base.exec(name, cmd)
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    fn get_apple_container_runtime_if_available() -> Option<AppleContainer> {
        let apple_container = AppleContainer::default();
        if !apple_container.is_available() || !apple_container.is_daemon_running() {
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
