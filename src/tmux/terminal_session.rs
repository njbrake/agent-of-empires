//! Terminal session for paired terminal functionality

use anyhow::{bail, Result};
use std::process::Command;

use super::utils::sanitize_session_name;
use super::{refresh_session_cache, session_exists_from_cache, TERMINAL_PREFIX};
use crate::cli::truncate_id;
use crate::process;

pub struct TerminalSession {
    name: String,
}

impl TerminalSession {
    pub fn new(id: &str, title: &str) -> Result<Self> {
        Ok(Self {
            name: Self::generate_name(id, title),
        })
    }

    pub fn generate_name(id: &str, title: &str) -> String {
        let safe_title = sanitize_session_name(title);
        format!("{}{}_{}", TERMINAL_PREFIX, safe_title, truncate_id(id, 8))
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

    pub fn create(&self, working_dir: &str) -> Result<()> {
        self.create_with_size(working_dir, None)
    }

    pub fn create_with_size(&self, working_dir: &str, size: Option<(u16, u16)>) -> Result<()> {
        if self.exists() {
            return Ok(());
        }

        let args = build_terminal_create_args(&self.name, working_dir, size);
        let output = Command::new("tmux").args(&args).output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to create terminal session: {}", stderr);
        }

        refresh_session_cache();

        Ok(())
    }

    pub fn kill(&self) -> Result<()> {
        if !self.exists() {
            return Ok(());
        }

        // Kill the entire process tree first to ensure child processes are terminated
        if let Some(pane_pid) = self.get_pane_pid() {
            process::kill_process_tree(pane_pid);
        }

        let output = Command::new("tmux")
            .args(["kill-session", "-t", &self.name])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to kill terminal session: {}", stderr);
        }

        refresh_session_cache();

        Ok(())
    }

    pub fn get_pane_pid(&self) -> Option<u32> {
        process::get_pane_pid(&self.name)
    }

    pub fn attach(&self) -> Result<()> {
        if !self.exists() {
            bail!("Terminal session does not exist: {}", self.name);
        }

        if std::env::var("TMUX").is_ok() {
            let status = Command::new("tmux")
                .args(["switch-client", "-t", &self.name])
                .status()?;

            if !status.success() {
                let status = Command::new("tmux")
                    .args(["attach-session", "-t", &self.name])
                    .status()?;

                if !status.success() {
                    bail!("Failed to attach to terminal session");
                }
            }
        } else {
            let status = Command::new("tmux")
                .args(["attach-session", "-t", &self.name])
                .status()?;

            if !status.success() {
                bail!("Failed to attach to terminal session");
            }
        }

        Ok(())
    }

    pub fn capture_pane(&self, lines: usize) -> Result<String> {
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
}

/// Build the argument list for tmux new-session command (terminal sessions).
/// Extracted for testability.
fn build_terminal_create_args(
    session_name: &str,
    working_dir: &str,
    size: Option<(u16, u16)>,
) -> Vec<String> {
    let mut args = vec![
        "new-session".to_string(),
        "-d".to_string(),
        "-s".to_string(),
        session_name.to_string(),
        "-c".to_string(),
        working_dir.to_string(),
    ];

    if let Some((width, height)) = size {
        args.push("-x".to_string());
        args.push(width.to_string());
        args.push("-y".to_string());
        args.push(height.to_string());
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tmux::{Session, SESSION_PREFIX};

    #[test]
    fn test_terminal_session_generate_name() {
        let name = TerminalSession::generate_name("abc123def456", "My Project");
        assert!(name.starts_with(TERMINAL_PREFIX));
        assert!(name.contains("My_Project"));
        assert!(name.contains("abc123de"));
    }

    #[test]
    fn test_terminal_session_name_differs_from_agent_session() {
        let agent_name = Session::generate_name("abc123def456", "My Project");
        let terminal_name = TerminalSession::generate_name("abc123def456", "My Project");
        assert_ne!(agent_name, terminal_name);
        assert!(agent_name.starts_with(SESSION_PREFIX));
        assert!(terminal_name.starts_with(TERMINAL_PREFIX));
    }

    #[test]
    fn test_build_terminal_create_args_without_size() {
        let args = build_terminal_create_args("test_terminal", "/tmp/work", None);
        assert_eq!(
            args,
            vec![
                "new-session",
                "-d",
                "-s",
                "test_terminal",
                "-c",
                "/tmp/work"
            ]
        );
        assert!(!args.contains(&"-x".to_string()));
        assert!(!args.contains(&"-y".to_string()));
    }

    #[test]
    fn test_build_terminal_create_args_with_size() {
        let args = build_terminal_create_args("test_terminal", "/tmp/work", Some((100, 30)));
        assert!(args.contains(&"-x".to_string()));
        assert!(args.contains(&"100".to_string()));
        assert!(args.contains(&"-y".to_string()));
        assert!(args.contains(&"30".to_string()));

        // Verify order: -x should come before width, -y before height
        let x_idx = args.iter().position(|a| a == "-x").unwrap();
        let y_idx = args.iter().position(|a| a == "-y").unwrap();
        assert_eq!(args[x_idx + 1], "100");
        assert_eq!(args[y_idx + 1], "30");
    }
}
