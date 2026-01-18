//! tmux session management

use anyhow::{bail, Result};
use std::process::Command;

use super::{session_exists_from_cache, SESSION_PREFIX, TERMINAL_PREFIX};
use crate::cli::truncate_id;
use crate::process;
use crate::session::Status;

const SPINNER_CHARS: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

fn strip_ansi(content: &str) -> String {
    let mut result = content.to_string();

    while let Some(start) = result.find("\x1b[") {
        let rest = &result[start + 2..];
        let end_offset = rest
            .find(|c: char| c.is_ascii_alphabetic())
            .map(|i| i + 1)
            .unwrap_or(rest.len());
        result = format!("{}{}", &result[..start], &result[start + 2 + end_offset..]);
    }

    while let Some(start) = result.find("\x1b]") {
        if let Some(end) = result[start..].find('\x07') {
            result = format!("{}{}", &result[..start], &result[start + end + 1..]);
        } else {
            break;
        }
    }

    result
}

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

        super::refresh_session_cache();

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

fn detect_status_from_content(content: &str, tool: &str, _fg_pid: Option<u32>) -> Status {
    let content_lower = content.to_lowercase();
    let effective_tool = if tool == "shell" && is_opencode_content(&content_lower) {
        "opencode"
    } else if tool == "shell" && is_claude_code_content(&content_lower) {
        "claude"
    } else {
        tool
    };

    match effective_tool {
        "claude" => detect_claude_status(content),
        "opencode" => detect_opencode_status(&content_lower),
        _ => detect_claude_status(content),
    }
}

fn is_opencode_content(content: &str) -> bool {
    let opencode_indicators = ["tab switch agent", "ctrl+p commands", "/compact", "/status"];
    opencode_indicators.iter().any(|ind| content.contains(ind))
}

fn is_claude_code_content(content: &str) -> bool {
    let claude_indicators = [
        "esc to interrupt",
        "yes, allow once",
        "yes, allow always",
        "do you trust the files",
        "claude code",
        "anthropic",
        "/ to search",
        "? for help",
    ];
    if claude_indicators.iter().any(|ind| content.contains(ind)) {
        return true;
    }
    let lines: Vec<&str> = content.lines().collect();
    if let Some(last_line) = lines.iter().rev().find(|l| !l.trim().is_empty()) {
        let trimmed = last_line.trim();
        if trimmed == ">" || trimmed == "> " {
            let has_box_chars = content.contains('─') || content.contains('│');
            if has_box_chars {
                return true;
            }
        }
    }
    false
}

pub fn detect_claude_status(content: &str) -> Status {
    let lines: Vec<&str> = content.lines().collect();
    let non_empty_lines: Vec<&str> = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .copied()
        .collect();

    let last_lines: String = non_empty_lines
        .iter()
        .rev()
        .take(30)
        .rev()
        .copied()
        .collect::<Vec<&str>>()
        .join("\n");
    let last_lines_lower = last_lines.to_lowercase();

    if last_lines_lower.contains("esc to interrupt")
        || last_lines_lower.contains("ctrl+c to interrupt")
    {
        return Status::Running;
    }

    for line in &lines {
        for spinner in SPINNER_CHARS {
            if line.contains(spinner) {
                return Status::Running;
            }
        }
    }

    if last_lines_lower.contains("enter to select") || last_lines_lower.contains("esc to cancel") {
        return Status::Waiting;
    }

    let permission_prompts = [
        "Yes, allow once",
        "Yes, allow always",
        "Allow once",
        "Allow always",
        "❯ Yes",
        "❯ No",
        "Do you trust the files in this folder?",
    ];
    for prompt in &permission_prompts {
        if last_lines.contains(prompt) {
            return Status::Waiting;
        }
    }

    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with("❯") && trimmed.len() > 2 {
            let rest = &trimmed[3..].trim_start();
            if rest.starts_with("1.") || rest.starts_with("2.") || rest.starts_with("3.") {
                return Status::Waiting;
            }
        }
    }

    for line in non_empty_lines.iter().rev().take(10) {
        let clean_line = strip_ansi(line).trim().to_string();
        if clean_line == ">" || clean_line == "> " {
            return Status::Waiting;
        }
        if clean_line.starts_with("> ")
            && !clean_line.to_lowercase().contains("esc")
            && clean_line.len() < 100
        {
            return Status::Waiting;
        }
    }

    // WAITING: Y/N confirmation prompts
    // Only check in last lines
    let question_prompts = ["(Y/n)", "(y/N)", "[Y/n]", "[y/N]"];
    for prompt in &question_prompts {
        if last_lines.contains(prompt) {
            return Status::Waiting;
        }
    }

    Status::Idle
}

pub fn detect_opencode_status(content: &str) -> Status {
    let lines: Vec<&str> = content.lines().collect();
    let non_empty_lines: Vec<&str> = lines
        .iter()
        .filter(|l| !l.trim().is_empty())
        .copied()
        .collect();

    // Get last 30 lines for UI status checks (to avoid matching code/comments in terminal output)
    let last_lines: String = non_empty_lines
        .iter()
        .rev()
        .take(30)
        .rev()
        .copied()
        .collect::<Vec<&str>>()
        .join("\n");
    let last_lines_lower = last_lines.to_lowercase();

    // RUNNING: OpenCode shows "esc to interrupt" when busy (same as Claude Code)
    // Only check in last lines to avoid matching comments/code in terminal output
    if last_lines_lower.contains("esc to interrupt") || last_lines_lower.contains("esc interrupt") {
        return Status::Running;
    }

    for line in &lines {
        for spinner in SPINNER_CHARS {
            if line.contains(spinner) {
                return Status::Running;
            }
        }
    }

    // WAITING: Selection menus (shows "Enter to select" or "Esc to cancel")
    // Only check in last lines to avoid matching comments/code
    if last_lines_lower.contains("enter to select") || last_lines_lower.contains("esc to cancel") {
        return Status::Waiting;
    }

    // WAITING: Permission/confirmation prompts
    // Only check in last lines
    let permission_prompts = [
        "(y/n)",
        "[y/n]",
        "continue?",
        "proceed?",
        "approve",
        "allow",
    ];
    for prompt in &permission_prompts {
        if last_lines_lower.contains(prompt) {
            return Status::Waiting;
        }
    }

    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with("❯") && trimmed.len() > 2 {
            let after_cursor = trimmed.get(3..).unwrap_or("").trim_start();
            if after_cursor.starts_with("1.")
                || after_cursor.starts_with("2.")
                || after_cursor.starts_with("3.")
            {
                return Status::Waiting;
            }
        }
    }
    if lines.iter().any(|line| {
        line.contains("❯") && (line.contains(" 1.") || line.contains(" 2.") || line.contains(" 3."))
    }) {
        return Status::Waiting;
    }

    for line in non_empty_lines.iter().rev().take(10) {
        let clean_line = strip_ansi(line).trim().to_string();

        if clean_line == ">" || clean_line == "> " || clean_line == ">>" {
            return Status::Waiting;
        }
        if clean_line.starts_with("> ")
            && !clean_line.to_lowercase().contains("esc")
            && clean_line.len() < 100
        {
            return Status::Waiting;
        }
    }

    // WAITING - Completion indicators + input prompt nearby
    // Only check in last lines
    let completion_indicators = [
        "complete",
        "done",
        "finished",
        "ready",
        "what would you like",
        "what else",
        "anything else",
        "how can i help",
        "let me know",
    ];
    let has_completion = completion_indicators
        .iter()
        .any(|ind| last_lines_lower.contains(ind));
    if has_completion {
        for line in non_empty_lines.iter().rev().take(10) {
            let clean = strip_ansi(line).trim().to_string();
            if clean == ">" || clean == "> " || clean == ">>" {
                return Status::Waiting;
            }
        }
    }

    Status::Idle
}

/// Terminal session for paired terminal functionality
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
        if self.exists() {
            return Ok(());
        }

        let args = vec![
            "new-session".to_string(),
            "-d".to_string(),
            "-s".to_string(),
            self.name.clone(),
            "-c".to_string(),
            working_dir.to_string(),
        ];

        let output = Command::new("tmux").args(&args).output()?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("Failed to create terminal session: {}", stderr);
        }

        super::refresh_session_cache();

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
            bail!("Failed to kill terminal session: {}", stderr);
        }

        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_detect_claude_status_running() {
        // "esc to interrupt" indicates Claude is actively working
        assert_eq!(
            detect_claude_status("Working on your request (esc to interrupt)"),
            Status::Running
        );
        assert_eq!(
            detect_claude_status("Thinking... · esc to interrupt"),
            Status::Running
        );
        assert_eq!(
            detect_claude_status("✶ Hashing… (ctrl+c to interrupt)"),
            Status::Running
        );
        assert_eq!(detect_claude_status("Processing ⠋"), Status::Running);
        assert_eq!(detect_claude_status("Loading ⠹"), Status::Running);
    }

    #[test]
    fn test_detect_claude_status_waiting() {
        assert_eq!(detect_claude_status("Yes, allow once"), Status::Waiting);
        assert_eq!(
            detect_claude_status("Do you trust the files in this folder?"),
            Status::Waiting
        );
        assert_eq!(detect_claude_status("Task complete.\n>"), Status::Waiting);
        assert_eq!(detect_claude_status("Done!\n> "), Status::Waiting);
        assert_eq!(detect_claude_status("Continue? (Y/n)"), Status::Waiting);
        assert_eq!(
            detect_claude_status("Enter to select · Tab/Arrow keys to navigate · Esc to cancel"),
            Status::Waiting
        );
        assert_eq!(
            detect_claude_status("❯ 1. Planned activities\n  2. Spontaneous"),
            Status::Waiting
        );
    }

    #[test]
    fn test_detect_claude_status_idle() {
        assert_eq!(detect_claude_status("completed the task"), Status::Idle);
        assert_eq!(detect_claude_status("some random output"), Status::Idle);
    }

    #[test]
    fn test_strip_ansi() {
        assert_eq!(strip_ansi("\x1b[32mgreen\x1b[0m"), "green");
        assert_eq!(strip_ansi("no codes here"), "no codes here");
        assert_eq!(strip_ansi("\x1b[1;34mbold blue\x1b[0m"), "bold blue");
    }

    #[test]
    fn test_detect_opencode_status_running() {
        assert_eq!(
            detect_opencode_status("Processing your request\nesc to interrupt"),
            Status::Running
        );
        assert_eq!(
            detect_opencode_status("Working... esc interrupt"),
            Status::Running
        );
        assert_eq!(detect_opencode_status("Generating ⠋"), Status::Running);
        assert_eq!(detect_opencode_status("Loading ⠹"), Status::Running);
    }

    #[test]
    fn test_detect_opencode_status_waiting() {
        assert_eq!(
            detect_opencode_status("allow this action? [y/n]"),
            Status::Waiting
        );
        assert_eq!(detect_opencode_status("continue? (y/n)"), Status::Waiting);
        assert_eq!(detect_opencode_status("approve changes"), Status::Waiting);
        assert_eq!(detect_opencode_status("task complete.\n>"), Status::Waiting);
        assert_eq!(
            detect_opencode_status("ready for input\n> "),
            Status::Waiting
        );
        assert_eq!(
            detect_opencode_status("done! what else can i help with?\n>"),
            Status::Waiting
        );
    }

    #[test]
    fn test_detect_opencode_status_idle() {
        assert_eq!(detect_opencode_status("some random output"), Status::Idle);
        assert_eq!(
            detect_opencode_status("file saved successfully"),
            Status::Idle
        );
    }

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
}
