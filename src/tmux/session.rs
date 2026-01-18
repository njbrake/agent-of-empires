//! tmux session management

use anyhow::{bail, Result};
use std::process::Command;

use super::status_detection::detect_status_from_content;
use super::utils::sanitize_session_name;
use super::{refresh_session_cache, session_exists_from_cache, SESSION_PREFIX};
use crate::cli::truncate_id;
use crate::process;
use crate::session::Status;

pub struct Session {
    name: String,
}

impl Session {
    pub fn new(id: &str, title: &str) -> Result<Self> {
        Ok(Self {
            name: Self::generate_name(id, title),
        })
    }

    pub fn generate_name(id: &str, title: &str) -> String {
        let safe_title = sanitize_session_name(title);
        format!("{}{}_{}", SESSION_PREFIX, safe_title, truncate_id(id, 8))
    }

    pub fn exists(&self) -> bool {
        if let Some(exists) = session_exists_from_cache(&self.name) {
            return exists;
        }

        Command::new("tmux")
            .args(["has-session", "-t", &self.name])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn create(&self, working_dir: &str, command: Option<&str>) -> Result<()> {
        if self.exists() {
            return Ok(());
        }

        let mut args = vec![
            "new-session".to_string(),
            "-d".to_string(),
            "-s".to_string(),
            self.name.clone(),
            "-c".to_string(),
            working_dir.to_string(),
        ];

        if let Some(cmd) = command {
            args.push(cmd.to_string());
        }

        let output = Command::new("tmux").args(&args).output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to create tmux session: {}", stderr);
        }

        refresh_session_cache();

        Ok(())
    }

    pub fn kill(&self) -> Result<()> {
        if !self.exists() {
            return Ok(());
        }

        let output = Command::new("tmux")
            .args(["kill-session", "-t", &self.name])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to kill tmux session: {}", stderr);
        }

        Ok(())
    }

    pub fn rename(&self, new_name: &str) -> Result<()> {
        if !self.exists() {
            return Ok(());
        }

        let output = Command::new("tmux")
            .args(["rename-session", "-t", &self.name, new_name])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to rename tmux session: {}", stderr);
        }

        Ok(())
    }

    pub fn attach(&self) -> Result<()> {
        if !self.exists() {
            bail!("Session does not exist: {}", self.name);
        }

        if std::env::var("TMUX").is_ok() {
            let status = Command::new("tmux")
                .args(["switch-client", "-t", &self.name])
                .status()?;

            if !status.success() {
                // Fall back to attach-session if switch-client fails.
                // This handles cases where TMUX env var is inherited but we're
                // not actually inside a tmux client (e.g., terminal spawned
                // from within tmux via `open -a Terminal`).
                let status = Command::new("tmux")
                    .args(["attach-session", "-t", &self.name])
                    .status()?;

                if !status.success() {
                    bail!("Failed to attach to tmux session");
                }
            }
        } else {
            let status = Command::new("tmux")
                .args(["attach-session", "-t", &self.name])
                .status()?;

            if !status.success() {
                bail!("Failed to attach to tmux session");
            }
        }

        Ok(())
    }

    pub fn capture_pane(&self, lines: usize) -> Result<String> {
        self.capture_pane_with_size(lines, None, None)
    }

    pub fn capture_pane_with_size(
        &self,
        lines: usize,
        _width: Option<u16>,
        _height: Option<u16>,
    ) -> Result<String> {
        if !self.exists() {
            return Ok(String::new());
        }

        let output = Command::new("tmux")
            .args([
                "capture-pane",
                "-t",
                &self.name,
                "-p",
                "-S",
                &format!("-{}", lines),
            ])
            .output()?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            Ok(String::new())
        }
    }

    pub fn get_pane_pid(&self) -> Option<u32> {
        process::get_pane_pid(&self.name)
    }

    pub fn get_foreground_pid(&self) -> Option<u32> {
        let pane_pid = self.get_pane_pid()?;
        process::get_foreground_pid(pane_pid).or(Some(pane_pid))
    }

    pub fn detect_status(&self, tool: &str) -> Result<Status> {
        let content = self.capture_pane(50)?;
        let fg_pid = self.get_foreground_pid();
        Ok(detect_status_from_content(&content, tool, fg_pid))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_name() {
        let name = Session::generate_name("abc123def456", "My Project");
        assert!(name.starts_with(SESSION_PREFIX));
        assert!(name.contains("My_Project"));
        assert!(name.contains("abc123de"));
    }

    #[test]
    fn test_generate_name_with_long_title() {
        let name = Session::generate_name(
            "abc123",
            "This is a very long project name that exceeds the limit",
        );
        assert!(name.len() < 50);
        assert!(name.starts_with(SESSION_PREFIX));
    }

    #[test]
    fn test_generate_name_with_short_id() {
        let name = Session::generate_name("abc", "Test");
        assert!(name.contains("abc"));
    }

    #[test]
    fn test_generate_name_consistency() {
        let name1 = Session::generate_name("test123", "Project");
        let name2 = Session::generate_name("test123", "Project");
        assert_eq!(name1, name2);
    }
}
