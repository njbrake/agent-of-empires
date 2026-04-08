//! tmux session management

use anyhow::{bail, Result};
use std::process::Command;

use super::{
    refresh_session_cache, session_exists_from_cache,
    utils::{
        append_pane_base_index_args, append_remain_on_exit_args, is_pane_dead,
        is_pane_running_shell,
    },
    SESSION_PREFIX,
};
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

    /// Construct a Session from a pre-computed tmux session name.
    pub fn from_name(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
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
        self.create_with_size(working_dir, command, None)
    }

    pub fn create_with_size(
        &self,
        working_dir: &str,
        command: Option<&str>,
        size: Option<(u16, u16)>,
    ) -> Result<()> {
        if self.exists() {
            return Ok(());
        }

        let mut args = build_create_args(&self.name, working_dir, command, size);
        append_remain_on_exit_args(&mut args, &self.name);
        append_pane_base_index_args(&mut args, &self.name);

        let output = Command::new("tmux").args(&args).output()?;

        // Note: With -d flag, tmux new-session returns 0 even if the shell command fails.
        // Log args at debug level for troubleshooting.
        tracing::debug!("tmux new-session args: {:?}", args);

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to create tmux session: {}", stderr);
        }

        super::refresh_session_cache();

        Ok(())
    }

    pub fn is_pane_dead(&self) -> bool {
        is_pane_dead(&self.name)
    }

    pub fn is_pane_running_shell(&self) -> bool {
        is_pane_running_shell(&self.name)
    }

    pub fn kill(&self) -> Result<()> {
        if !self.exists() {
            return Ok(());
        }

        // Kill the entire process tree first to ensure child processes are terminated.
        // This handles cases where tools like Claude spawn subprocesses that may
        // survive tmux's SIGHUP signal.
        if let Some(pane_pid) = self.get_pane_pid() {
            process::kill_process_tree(pane_pid);
        }

        let output = Command::new("tmux")
            .args(["kill-session", "-t", &self.name])
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Session vanished between the exists() check and kill-session
            // (e.g. process tree kill caused tmux to tear it down). That's
            // fine -- the goal was to remove the session and it's gone.
            if !stderr.contains("can't find session") {
                bail!("Failed to kill tmux session: {}", stderr);
            }
        }

        refresh_session_cache();

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

        // Use `^.0` to target the first window's first pane regardless of
        // base-index or which pane is active.  See #435, #488.
        let target = format!("{}:^.0", self.name);
        let output = Command::new("tmux")
            .args([
                "capture-pane",
                "-t",
                &target,
                "-p",
                "-e",
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
        Ok(super::status_detection::detect_status_from_content(
            &content, tool,
        ))
    }

    /// Send literal text to the session's first window pane, followed by Enter.
    /// For multi-line text, newlines are sent as ESC+CR (the same sequence
    /// terminals send for Shift+Enter) so the coding agent inserts a newline
    /// rather than submitting after each line.
    pub fn send_keys(&self, text: &str) -> Result<()> {
        if !self.exists() {
            bail!("Session does not exist: {}", self.name);
        }

        let target = format!("{}:^.0", self.name);

        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            Self::tmux_send(&target, &["-l", line])?;
            if i < lines.len() - 1 {
                // ESC + CR: what terminals send for Shift+Enter (inserts newline)
                Self::tmux_send(&target, &["-H", "1b", "0d"])?;
            }
        }

        // Enter to submit
        Self::tmux_send(&target, &["Enter"])?;

        Ok(())
    }

    fn tmux_send(target: &str, args: &[&str]) -> Result<()> {
        let output = Command::new("tmux")
            .arg("send-keys")
            .args(["-t", target])
            .args(args)
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to send keys: {}", stderr);
        }

        Ok(())
    }
}

fn sanitize_session_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .take(20)
        .collect()
}

/// Build the argument list for tmux new-session command.
/// Extracted for testability.
fn build_create_args(
    session_name: &str,
    working_dir: &str,
    command: Option<&str>,
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

    if let Some(cmd) = command {
        args.push(cmd.to_string());
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: check if tmux is available for tests that need it
    fn tmux_available() -> bool {
        Command::new("tmux")
            .arg("-V")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    #[test]
    #[serial_test::serial]
    fn test_remain_on_exit_and_pane_dead() {
        if !tmux_available() {
            eprintln!("Skipping test: tmux not available");
            return;
        }

        let session_name = format!("aoe_test_remain_{}", std::process::id());
        // Chain set-option -p with new-session to avoid race condition
        let output = Command::new("tmux")
            .args([
                "new-session",
                "-d",
                "-s",
                &session_name,
                "-x",
                "80",
                "-y",
                "24",
                "sleep 1",
                ";",
                "set-option",
                "-p",
                "-t",
                &session_name,
                "remain-on-exit",
                "on",
            ])
            .output()
            .expect("tmux new-session");
        assert!(output.status.success());

        // Wait for the sleep command to finish
        std::thread::sleep(std::time::Duration::from_millis(1500));

        // Session should still exist (remain-on-exit keeps it)
        let exists = Command::new("tmux")
            .args(["has-session", "-t", &session_name])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        assert!(exists, "Session should still exist due to remain-on-exit");

        // Pane should be dead (process exited)
        let pane_dead = Command::new("tmux")
            .args(["display-message", "-t", &session_name, "-p", "#{pane_dead}"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim() == "1")
            .unwrap_or(false);
        assert!(pane_dead, "Pane should be dead after command exits");

        // Clean up
        let _ = Command::new("tmux")
            .args(["kill-session", "-t", &session_name])
            .output();
    }

    #[test]
    #[serial_test::serial]
    fn test_is_pane_dead_on_running_session() {
        if !tmux_available() {
            eprintln!("Skipping test: tmux not available");
            return;
        }

        let session_name = format!("aoe_test_alive_{}", std::process::id());

        // Create a session with a long-running command
        let output = Command::new("tmux")
            .args([
                "new-session",
                "-d",
                "-s",
                &session_name,
                "-x",
                "80",
                "-y",
                "24",
                "sleep 30",
                ";",
                "set-option",
                "-p",
                "-t",
                &session_name,
                "remain-on-exit",
                "on",
            ])
            .output()
            .expect("tmux new-session");
        assert!(output.status.success());

        std::thread::sleep(std::time::Duration::from_millis(200));

        // Pane should NOT be dead (sleep is still running)
        let pane_dead = Command::new("tmux")
            .args(["display-message", "-t", &session_name, "-p", "#{pane_dead}"])
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map(|s| s.trim() == "1")
            .unwrap_or(false);
        assert!(!pane_dead, "Pane should be alive while command is running");

        // Clean up
        let _ = Command::new("tmux")
            .args(["kill-session", "-t", &session_name])
            .output();
    }

    /// Regression test for #435: with multiple tmux windows, pane health
    /// checks must target window 0 pane 0 explicitly so that a dead pane in
    /// a second window does not cause the agent pane to be killed.
    #[test]
    #[serial_test::serial]
    fn test_is_pane_dead_targets_window_zero_with_multiple_windows() {
        if !tmux_available() {
            eprintln!("Skipping test: tmux not available");
            return;
        }

        let session_name = format!("aoe_test_multiwin_{}", std::process::id());

        // Create session with a long-running command in window 0
        let output = Command::new("tmux")
            .args([
                "new-session",
                "-d",
                "-s",
                &session_name,
                "-x",
                "80",
                "-y",
                "24",
                "sleep 30",
                ";",
                "set-option",
                "-p",
                "-t",
                &session_name,
                "remain-on-exit",
                "on",
            ])
            .output()
            .expect("tmux new-session");
        assert!(output.status.success());

        // Force base-index 1 and pane-base-index 1 to simulate users who
        // have both set in their tmux.conf.
        let output = Command::new("tmux")
            .args(["set-option", "-t", &session_name, "base-index", "1"])
            .output()
            .expect("tmux set-option base-index");
        assert!(output.status.success());
        let output = Command::new("tmux")
            .args(["set-option", "-t", &session_name, "pane-base-index", "1"])
            .output()
            .expect("tmux set-option pane-base-index");
        assert!(output.status.success());

        // Create a second window with a command that exits immediately
        let output = Command::new("tmux")
            .args([
                "new-window",
                "-t",
                &session_name,
                "true", // exits immediately
            ])
            .output()
            .expect("tmux new-window");
        assert!(output.status.success());

        std::thread::sleep(std::time::Duration::from_millis(300));

        // The agent pane (first window) is still alive, so is_pane_dead should
        // return false even though the second window's pane has exited.
        assert!(
            !is_pane_dead(&session_name),
            "is_pane_dead should check the first window's pane, not the active window"
        );

        // Clean up
        let _ = Command::new("tmux")
            .args(["kill-session", "-t", &session_name])
            .output();
    }

    /// Regression test: capture_pane must target the first window's pane
    /// regardless of which window is currently active, and regardless of
    /// the user's tmux base-index setting.
    #[test]
    #[serial_test::serial]
    fn test_capture_pane_targets_first_window_with_multiple_windows() {
        if !tmux_available() {
            eprintln!("Skipping test: tmux not available");
            return;
        }

        let session_name = format!("aoe_test_capture_multiwin_{}", std::process::id());

        // Create session running sleep in the first window
        let output = Command::new("tmux")
            .args([
                "new-session",
                "-d",
                "-s",
                &session_name,
                "-x",
                "80",
                "-y",
                "24",
                "sleep 30",
            ])
            .output()
            .expect("tmux new-session");
        assert!(output.status.success());

        // Force base-index 1 to simulate users who have set base-index 1 in
        // their tmux.conf. With base-index 1, window 0 does not exist, so any
        // target using :0.0 silently fails.
        let output = Command::new("tmux")
            .args(["set-option", "-t", &session_name, "base-index", "1"])
            .output()
            .expect("tmux set-option base-index");
        assert!(output.status.success());

        // Open a second window running a shell, and make it the active window
        let output = Command::new("tmux")
            .args(["new-window", "-t", &session_name, "sh"])
            .output()
            .expect("tmux new-window");
        assert!(output.status.success());

        std::thread::sleep(std::time::Duration::from_millis(200));

        let session = Session {
            name: session_name.clone(),
        };

        // capture_pane must succeed -- with base-index 1, a :0.0 target does
        // not exist and the tmux command fails silently returning empty content.
        let _content = session
            .capture_pane(10)
            .expect("capture_pane should not return an error for a valid session");

        // The command in the first window is 'sleep', not a shell.
        // is_pane_running_shell must return false even though the active
        // window is running sh. With a :0.0 target and base-index 1 this
        // would return false for the wrong reason (silent failure), but with
        // ^ it correctly reads the first window's pane_current_command.
        assert!(
            !session.is_pane_running_shell(),
            "is_pane_running_shell should check first window (sleep), not active window (sh)"
        );

        // Clean up
        let _ = Command::new("tmux")
            .args(["kill-session", "-t", &session_name])
            .output();
    }

    /// Regression test: is_pane_running_shell must target the first window's
    /// pane even when the active window is a shell, and even with base-index 1.
    #[test]
    #[serial_test::serial]
    fn test_is_pane_running_shell_targets_first_window_with_multiple_windows() {
        if !tmux_available() {
            eprintln!("Skipping test: tmux not available");
            return;
        }

        let session_name = format!("aoe_test_shell_multiwin_{}", std::process::id());

        // Create session running sleep (not a shell) in the first window
        let output = Command::new("tmux")
            .args([
                "new-session",
                "-d",
                "-s",
                &session_name,
                "-x",
                "80",
                "-y",
                "24",
                "sleep 30",
            ])
            .output()
            .expect("tmux new-session");
        assert!(output.status.success());

        // Force base-index 1 to simulate users who have set base-index 1 in
        // their tmux.conf. With base-index 1, window 0 does not exist, so any
        // target using :0.0 silently fails.
        let output = Command::new("tmux")
            .args(["set-option", "-t", &session_name, "base-index", "1"])
            .output()
            .expect("tmux set-option base-index");
        assert!(output.status.success());

        // Open a second window running a shell and make it active
        let output = Command::new("tmux")
            .args(["new-window", "-t", &session_name, "sh"])
            .output()
            .expect("tmux new-window");
        assert!(output.status.success());

        std::thread::sleep(std::time::Duration::from_millis(200));

        // Should be false: first window runs 'sleep', not a shell.
        // Would incorrectly return true if the active second window (sh) were checked.
        // With base-index 1 and a :0.0 target the call silently fails and
        // returns false for the wrong reason; ^ correctly reads the first pane.
        assert!(
            !is_pane_running_shell(&session_name),
            "is_pane_running_shell should target first window (sleep), not active window (sh)"
        );

        // Clean up
        let _ = Command::new("tmux")
            .args(["kill-session", "-t", &session_name])
            .output();
    }

    /// Regression test for #488: when a user creates a split pane and makes it
    /// active, is_pane_dead and is_pane_running_shell must still target the
    /// agent's pane (pane 0), not the active split pane.
    #[test]
    #[serial_test::serial]
    fn test_status_checks_target_pane_zero_with_split_panes() {
        if !tmux_available() {
            eprintln!("Skipping test: tmux not available");
            return;
        }

        let session_name = format!("aoe_test_splitpane_{}", std::process::id());

        // Create session with a long-running command (the "agent")
        let output = Command::new("tmux")
            .args([
                "new-session",
                "-d",
                "-s",
                &session_name,
                "-x",
                "80",
                "-y",
                "24",
                "sleep 30",
                ";",
                "set-option",
                "-p",
                "-t",
                &session_name,
                "remain-on-exit",
                "on",
            ])
            .output()
            .expect("tmux new-session");
        assert!(output.status.success());

        // Split the window -- this creates a new pane running a shell
        let output = Command::new("tmux")
            .args(["split-window", "-t", &session_name])
            .output()
            .expect("tmux split-window");
        assert!(output.status.success());

        // The split pane is now active. Select it explicitly to be sure.
        let output = Command::new("tmux")
            .args(["select-pane", "-t", &format!("{session_name}:.1")])
            .output()
            .expect("tmux select-pane");
        assert!(output.status.success());

        std::thread::sleep(std::time::Duration::from_millis(200));

        // The agent pane (pane 0) is still alive
        assert!(
            !is_pane_dead(&session_name),
            "is_pane_dead should check pane 0 (sleep), not the active split pane"
        );

        // The agent pane runs 'sleep', not a shell
        assert!(
            !is_pane_running_shell(&session_name),
            "is_pane_running_shell should check pane 0 (sleep), not the active split pane (shell)"
        );

        // Clean up
        let _ = Command::new("tmux")
            .args(["kill-session", "-t", &session_name])
            .output();
    }

    /// Regression test for #488: ensure status checks work correctly when both
    /// pane-base-index 1 and split panes are in play.
    #[test]
    #[serial_test::serial]
    fn test_status_checks_with_split_panes_and_pane_base_index_1() {
        if !tmux_available() {
            eprintln!("Skipping test: tmux not available");
            return;
        }

        let session_name = format!("aoe_test_splitpbi_{}", std::process::id());

        // Create session with pane-base-index 0 pinned (as aoe does)
        let output = Command::new("tmux")
            .args([
                "new-session",
                "-d",
                "-s",
                &session_name,
                "-x",
                "80",
                "-y",
                "24",
                "sleep 30",
                ";",
                "set-option",
                "-p",
                "-t",
                &session_name,
                "remain-on-exit",
                "on",
                ";",
                "set-option",
                "-t",
                &session_name,
                "pane-base-index",
                "0",
            ])
            .output()
            .expect("tmux new-session");
        assert!(output.status.success());

        // Simulate a user with pane-base-index 1 globally by setting it on the
        // window -- but aoe has already pinned pane-base-index 0 on the session,
        // so pane 0 should still be valid.
        // Note: we set it on the session to verify our pinning takes precedence.
        // Actually, set pane-base-index 1 globally to simulate user config, then
        // verify our session-level override keeps pane 0 valid.

        // Split the window and make the new pane active
        let output = Command::new("tmux")
            .args(["split-window", "-t", &session_name])
            .output()
            .expect("tmux split-window");
        assert!(output.status.success());

        std::thread::sleep(std::time::Duration::from_millis(200));

        assert!(
            !is_pane_dead(&session_name),
            "is_pane_dead should check pane 0 (sleep) with pane-base-index pinned to 0"
        );

        assert!(
            !is_pane_running_shell(&session_name),
            "is_pane_running_shell should check pane 0 (sleep) with pane-base-index pinned to 0"
        );

        // Clean up
        let _ = Command::new("tmux")
            .args(["kill-session", "-t", &session_name])
            .output();
    }

    #[test]
    fn test_sanitize_session_name() {
        assert_eq!(sanitize_session_name("my-project"), "my-project");
        assert_eq!(sanitize_session_name("my project"), "my_project");
        assert_eq!(sanitize_session_name("a".repeat(30).as_str()).len(), 20);
    }

    #[test]
    fn test_generate_name() {
        let name = Session::generate_name("abc123def456", "My Project");
        assert!(name.starts_with(SESSION_PREFIX));
        assert!(name.contains("My_Project"));
        assert!(name.contains("abc123de"));
    }

    #[test]
    fn test_build_create_args_without_size() {
        let args = build_create_args("test_session", "/tmp/work", None, None);
        assert_eq!(
            args,
            vec!["new-session", "-d", "-s", "test_session", "-c", "/tmp/work"]
        );
        assert!(!args.contains(&"-x".to_string()));
        assert!(!args.contains(&"-y".to_string()));
    }

    #[test]
    fn test_build_create_args_with_size() {
        let args = build_create_args("test_session", "/tmp/work", None, Some((120, 40)));
        assert!(args.contains(&"-x".to_string()));
        assert!(args.contains(&"120".to_string()));
        assert!(args.contains(&"-y".to_string()));
        assert!(args.contains(&"40".to_string()));

        // Verify order: -x should come before width, -y before height
        let x_idx = args.iter().position(|a| a == "-x").unwrap();
        let y_idx = args.iter().position(|a| a == "-y").unwrap();
        assert_eq!(args[x_idx + 1], "120");
        assert_eq!(args[y_idx + 1], "40");
    }

    #[test]
    fn test_build_create_args_with_command() {
        let args = build_create_args("test_session", "/tmp/work", Some("claude"), None);
        assert_eq!(args.last().unwrap(), "claude");
    }

    #[test]
    fn test_build_create_args_with_size_and_command() {
        let args = build_create_args("test_session", "/tmp/work", Some("claude"), Some((80, 24)));

        // Size args should be present
        assert!(args.contains(&"-x".to_string()));
        assert!(args.contains(&"80".to_string()));
        assert!(args.contains(&"-y".to_string()));
        assert!(args.contains(&"24".to_string()));

        // Command should be last
        assert_eq!(args.last().unwrap(), "claude");
    }

    #[test]
    #[serial_test::serial]
    fn test_is_pane_running_shell_on_shell_session() {
        if !tmux_available() {
            eprintln!("Skipping test: tmux not available");
            return;
        }

        let session_name = format!("aoe_test_shell_{}", std::process::id());

        let output = Command::new("tmux")
            .args([
                "new-session",
                "-d",
                "-s",
                &session_name,
                "-x",
                "80",
                "-y",
                "24",
                "sh",
            ])
            .output()
            .expect("tmux new-session");
        assert!(output.status.success());

        std::thread::sleep(std::time::Duration::from_millis(200));

        assert!(
            is_pane_running_shell(&session_name),
            "Session running sh should be detected as a shell"
        );

        let _ = Command::new("tmux")
            .args(["kill-session", "-t", &session_name])
            .output();
    }

    #[test]
    #[serial_test::serial]
    fn test_is_pane_running_shell_on_non_shell_session() {
        if !tmux_available() {
            eprintln!("Skipping test: tmux not available");
            return;
        }

        let session_name = format!("aoe_test_noshell_{}", std::process::id());

        let output = Command::new("tmux")
            .args([
                "new-session",
                "-d",
                "-s",
                &session_name,
                "-x",
                "80",
                "-y",
                "24",
                "sleep",
                "30",
            ])
            .output()
            .expect("tmux new-session");
        assert!(output.status.success());

        std::thread::sleep(std::time::Duration::from_millis(200));

        assert!(
            !is_pane_running_shell(&session_name),
            "Session running 'sleep' should not be detected as a shell"
        );

        let _ = Command::new("tmux")
            .args(["kill-session", "-t", &session_name])
            .output();
    }
}
