//! Status detection for agent sessions

use crate::session::Status;

use super::utils::strip_ansi;

const SPINNER_CHARS: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn detect_status_from_content(content: &str, tool: &str, _fg_pid: Option<u32>) -> Status {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_claude_status_running() {
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
    fn test_detect_status_from_content_auto_detects_claude() {
        let content = "some output\nesc to interrupt\nclaude code";
        let status = detect_status_from_content(content, "shell", None);
        assert_eq!(status, Status::Running);
    }

    #[test]
    fn test_detect_status_from_content_auto_detects_opencode() {
        let content = "Tab switch agent\nesc to interrupt";
        let status = detect_status_from_content(content, "shell", None);
        assert_eq!(status, Status::Running);
    }

    #[test]
    fn test_detect_status_from_content_falls_back_to_claude() {
        let content = "Processing ⠋";
        let status = detect_status_from_content(content, "unknown_tool", None);
        assert_eq!(status, Status::Running);
    }

    #[test]
    fn test_detect_claude_status_numbered_list_selection() {
        let content = "Choose an option:\n❯ 1. First option\n  2. Second option\n  3. Third option";
        assert_eq!(detect_claude_status(content), Status::Waiting);
    }

    #[test]
    fn test_detect_claude_status_all_spinner_chars() {
        for spinner in SPINNER_CHARS {
            let content = format!("Working... {}", spinner);
            assert_eq!(
                detect_claude_status(&content),
                Status::Running,
                "Failed for spinner: {}",
                spinner
            );
        }
    }

    #[test]
    fn test_detect_claude_status_prompt_with_text() {
        assert_eq!(detect_claude_status("> hello"), Status::Waiting);
    }

    #[test]
    fn test_detect_claude_status_yn_variations() {
        assert_eq!(detect_claude_status("Continue? [Y/n]"), Status::Waiting);
        assert_eq!(detect_claude_status("Proceed? [y/N]"), Status::Waiting);
        assert_eq!(detect_claude_status("Confirm (Y/n)"), Status::Waiting);
        assert_eq!(detect_claude_status("Delete? (y/N)"), Status::Waiting);
    }

    #[test]
    fn test_detect_claude_status_allow_prompts() {
        assert_eq!(detect_claude_status("❯ Yes"), Status::Waiting);
        assert_eq!(detect_claude_status("❯ No"), Status::Waiting);
        assert_eq!(detect_claude_status("Allow once"), Status::Waiting);
        assert_eq!(detect_claude_status("Allow always"), Status::Waiting);
    }

    #[test]
    fn test_detect_opencode_status_numbered_selection() {
        let content = "Select:\n❯ 1. Option A\n  2. Option B";
        assert_eq!(detect_opencode_status(content), Status::Waiting);
    }

    #[test]
    fn test_detect_opencode_status_completion_with_prompt() {
        let content = "Task complete! What else can I help with?\n>";
        assert_eq!(detect_opencode_status(content), Status::Waiting);
    }

    #[test]
    fn test_detect_opencode_status_double_prompt() {
        assert_eq!(detect_opencode_status("Ready\n>>"), Status::Waiting);
    }

    #[test]
    fn test_is_opencode_content_indicators() {
        assert!(is_opencode_content("tab switch agent"));
        assert!(is_opencode_content("ctrl+p commands"));
        assert!(is_opencode_content("/compact"));
        assert!(is_opencode_content("/status"));
        assert!(!is_opencode_content("random text"));
    }

    #[test]
    fn test_is_claude_code_content_indicators() {
        assert!(is_claude_code_content("esc to interrupt"));
        assert!(is_claude_code_content("yes, allow once"));
        assert!(is_claude_code_content("/ to search"));
        assert!(is_claude_code_content("? for help"));
        assert!(!is_claude_code_content("random text"));
    }

    #[test]
    fn test_is_claude_code_content_prompt_with_box_chars() {
        let content = "│ some content ─\n>";
        assert!(is_claude_code_content(content));
    }
}
