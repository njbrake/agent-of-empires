//! The unified `ContainerRuntime`. Shared behavior lives on `RuntimeBase`;
//! this impl dispatches the four genuinely runtime-specific operations
//! (existence probe, running-state probe, exec-command formatting, and
//! batch status query) on a `RuntimeKind` discriminant.

use std::collections::HashMap;

use serde_json::Value;

use super::container_interface::{ContainerConfig, ContainerRuntimeInterface};
use super::error::{DockerError, Result};
use super::runtime_base::RuntimeBase;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RuntimeKind {
    Docker,
    AppleContainer,
}

pub struct ContainerRuntime {
    pub(crate) base: RuntimeBase,
    pub(crate) kind: RuntimeKind,
}

impl ContainerRuntime {
    pub fn docker() -> Self {
        Self {
            base: RuntimeBase::DOCKER,
            kind: RuntimeKind::Docker,
        }
    }

    pub fn apple_container() -> Self {
        Self {
            base: RuntimeBase::APPLE_CONTAINER,
            kind: RuntimeKind::AppleContainer,
        }
    }
}

impl Default for ContainerRuntime {
    fn default() -> Self {
        Self::docker()
    }
}

impl ContainerRuntimeInterface for ContainerRuntime {
    fn is_available(&self) -> bool {
        self.base.is_available()
    }

    fn is_daemon_running(&self) -> bool {
        self.base.is_daemon_running()
    }

    fn get_version(&self) -> Result<String> {
        self.base.get_version()
    }

    fn image_exists_locally(&self, image: &str) -> bool {
        self.base.image_exists_locally(image)
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

    fn does_container_exist(&self, name: &str) -> Result<bool> {
        match self.kind {
            RuntimeKind::Docker => {
                let output = self
                    .base
                    .command()
                    .args(["container", "inspect", name])
                    .output()?;
                Ok(output.status.success())
            }
            RuntimeKind::AppleContainer => {
                // Apple Container's `inspect` returns success(0) for non-existent
                // containers, so we use `logs` which properly fails for missing
                // containers.
                let output = self.base.command().args(["logs", name]).output()?;
                Ok(output.status.success())
            }
        }
    }

    fn is_container_running(&self, name: &str) -> Result<bool> {
        match self.kind {
            RuntimeKind::Docker => {
                let output = self
                    .base
                    .command()
                    .args(["container", "inspect", "-f", "{{.State.Running}}", name])
                    .output()?;

                if !output.status.success() {
                    return Ok(false);
                }

                let stdout = String::from_utf8_lossy(&output.stdout);
                Ok(stdout.trim() == "true")
            }
            RuntimeKind::AppleContainer => {
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

    fn exec_command(&self, name: &str, options: Option<&str>, cmd: &str) -> String {
        match self.kind {
            RuntimeKind::Docker => {
                // Docker containers inherit a full PATH, so the command can be
                // appended directly without wrapping in `sh -c`.
                self.base.exec_command(name, options, cmd)
            }
            RuntimeKind::AppleContainer => {
                // Apple Container has a very limited initial PATH, so we wrap
                // the command in `sh -c` to get a proper shell environment.
                // Single-quote with escaped embedded quotes to avoid issues
                // with double-quote metacharacters ($, `, \, !) in the command.
                let escaped = cmd.replace('\'', "'\\''");
                let cmd_str = format!("'{}'", escaped);

                if let Some(opt_str) = options {
                    [
                        "container",
                        "exec",
                        "-it",
                        opt_str,
                        name,
                        "sh",
                        "-c",
                        &cmd_str,
                    ]
                    .join(" ")
                } else {
                    ["container", "exec", "-it", name, "sh", "-c", &cmd_str].join(" ")
                }
            }
        }
    }

    fn exec(&self, name: &str, cmd: &[&str]) -> Result<std::process::Output> {
        self.base.exec(name, cmd)
    }

    fn batch_running_states(&self, prefix: &str) -> HashMap<String, bool> {
        match self.kind {
            RuntimeKind::Docker => {
                let output = self
                    .base
                    .command()
                    .args([
                        "ps",
                        "-a",
                        "--filter",
                        &format!("name={}", prefix),
                        "--format",
                        "{{.Names}}\t{{.State}}",
                    ])
                    .output();

                let output = match output {
                    Ok(o) if o.status.success() => o,
                    _ => return HashMap::new(),
                };

                let stdout = String::from_utf8_lossy(&output.stdout);
                stdout
                    .lines()
                    .filter_map(|line| {
                        let mut parts = line.splitn(2, '\t');
                        let name = parts.next()?.trim();
                        let state = parts.next()?.trim();
                        // Docker's --filter name= does substring matching, so
                        // post-filter to ensure we only include exact prefix matches.
                        if name.is_empty() || !name.starts_with(prefix) {
                            return None;
                        }
                        Some((name.to_string(), state == "running"))
                    })
                    .collect()
            }
            RuntimeKind::AppleContainer => {
                let _ = prefix;
                HashMap::new()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn docker_if_available() -> Option<ContainerRuntime> {
        let rt = ContainerRuntime::docker();
        if !rt.is_available() || !rt.is_daemon_running() {
            None
        } else {
            Some(rt)
        }
    }

    fn apple_container_if_available() -> Option<ContainerRuntime> {
        let rt = ContainerRuntime::apple_container();
        if !rt.is_available() || !rt.is_daemon_running() {
            None
        } else {
            Some(rt)
        }
    }

    #[test]
    fn test_image_exists_locally_with_common_image() {
        for rt in [docker_if_available(), apple_container_if_available()]
            .into_iter()
            .flatten()
        {
            rt.pull_image("hello-world").unwrap();
            assert!(rt.image_exists_locally("hello-world"));
        }
    }

    #[test]
    fn test_image_exists_locally_nonexistent() {
        for rt in [docker_if_available(), apple_container_if_available()]
            .into_iter()
            .flatten()
        {
            assert!(!rt.image_exists_locally("nonexistent-image-that-does-not-exist:v999"));
        }
    }

    #[test]
    fn test_ensure_image_uses_local_image() {
        for rt in [docker_if_available(), apple_container_if_available()]
            .into_iter()
            .flatten()
        {
            rt.pull_image("hello-world").unwrap();
            assert!(rt.ensure_image("hello-world").is_ok());
        }
    }

    #[test]
    fn test_ensure_image_fails_for_nonexistent_remote() {
        for rt in [docker_if_available(), apple_container_if_available()]
            .into_iter()
            .flatten()
        {
            assert!(rt
                .ensure_image("nonexistent-image-that-does-not-exist:v999")
                .is_err());
        }
    }
}
