//! Status detection for agent sessions

use crate::session::Status;

use super::utils::strip_ansi;

const SPINNER_CHARS: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// Claude Code's active spinner characters, observed as of Claude Code 1.0.33 (2025-06).
/// See also PR #381 for history of spinner changes.
const ACTIVE_SPINNER_CHARS: &[char] = &['·', '✢', '✳', '✶', '✻', '✽', '●'];

/// Check if a line is an active Claude Code spinner line.
/// Active spinners start with a spinner char and contain `…` (e.g. `✳ Twisting… (10m 6s)`).
/// Completion lines start with a spinner char but lack `…` (e.g. `✻ Worked for 1m 52s`).
fn is_active_spinner_line(line: &str) -> bool {
    let trimmed = line.trim();
    let Some(first) = trimmed.chars().next() else {
        return false;
    };
    ACTIVE_SPINNER_CHARS.contains(&first) && trimmed.contains('…')
}

pub fn detect_status_from_content(content: &str, tool: &str, _fg_pid: Option<u32>) -> Status {
    let status = crate::agents::get_agent(tool)
        .map(|a| (a.detect_status)(content))
        .unwrap_or(Status::Idle);

    if status == Status::Idle {
        let last_lines: Vec<&str> = content.lines().rev().take(5).collect();
        tracing::debug!(
            "status detection returned Idle for tool '{}', last 5 lines: {:?}",
            tool,
            last_lines
        );
    }

    status
}

/// Fallback status detection for Claude Code via tmux pane content parsing.
///
/// Primary detection uses hooks (file-based). This fallback runs when the hook
/// status file is missing or stale (e.g. long-running MCP calls exceeding the
/// staleness threshold, first seconds of a session, or crashed hooks).
pub fn detect_claude_status(raw_content: &str) -> Status {
    let non_empty_lines: Vec<&str> = raw_content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .collect();

    // Keyword scan window (30 lines) -- matches other detectors
    let last_lines: String = non_empty_lines
        .iter()
        .rev()
        .take(30)
        .rev()
        .copied()
        .collect::<Vec<&str>>()
        .join("\n");
    let last_lines_lower = last_lines.to_lowercase();

    // Spinner/prompt scan window (10 lines) -- tighter to avoid false positives
    // from spinner chars lingering in scroll history.
    let recent_lines: Vec<&str> = non_empty_lines.iter().rev().take(10).copied().collect();

    // RUNNING: "esc to interrupt" shown during active processing (same signal
    // as OpenCode, Codex, and Gemini -- see L52 comment)
    if last_lines_lower.contains("esc to interrupt") {
        return Status::Running;
    }

    // RUNNING: Active spinner line (spinner char + ellipsis)
    // e.g. "✳ Catapulting… (12m 33s · ↓ 118 tokens)"
    for line in &recent_lines {
        if is_active_spinner_line(line) {
            return Status::Running;
        }
    }

    // RUNNING: Braille spinners (older Claude Code versions)
    for line in &recent_lines {
        for spinner in SPINNER_CHARS {
            if line.contains(spinner) {
                return Status::Running;
            }
        }
    }

    // WAITING: Permission/approval prompts
    let approval_prompts = [
        "(y/n)",
        "[y/n]",
        "approve",
        "allow",
        "continue?",
        "proceed?",
    ];
    for prompt in &approval_prompts {
        if last_lines_lower.contains(prompt) {
            return Status::Waiting;
        }
    }

    // WAITING: Input prompt ready
    for line in non_empty_lines.iter().rev().take(10) {
        let clean = strip_ansi(line).trim().to_string();
        if clean == ">" || clean == "> " {
            return Status::Waiting;
        }
        if clean.starts_with("> ") && !clean.to_lowercase().contains("esc") && clean.len() < 100 {
            return Status::Waiting;
        }
    }

    Status::Idle
}

pub fn detect_opencode_status(raw_content: &str) -> Status {
    let content = raw_content.to_lowercase();
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

pub fn detect_vibe_status(raw_content: &str) -> Status {
    let content = raw_content.to_lowercase();
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

    // Vibe uses Textual TUI which can render text vertically (one char per line).
    // Join recent single-char lines to reconstruct words for detection.
    let recent_text: String = non_empty_lines
        .iter()
        .rev()
        .take(50)
        .rev()
        .map(|l| l.trim())
        .collect::<Vec<&str>>()
        .join("");
    let recent_text_lower = recent_text.to_lowercase();

    // WAITING checks come first - they're more specific than Running indicators

    // WAITING: Vibe's approval prompts show navigation hints
    // Pattern: "↑↓ navigate  Enter select  ESC reject"
    if last_lines_lower.contains("↑↓ navigate")
        || last_lines_lower.contains("enter select")
        || last_lines_lower.contains("esc reject")
    {
        return Status::Waiting;
    }

    // WAITING: Tool approval warning (shows "⚠ {tool_name} command")
    if last_lines.contains("⚠") && last_lines_lower.contains("command") {
        return Status::Waiting;
    }

    // WAITING: Approval options shown by Vibe
    let approval_options = [
        "yes and always allow",
        "no and tell the agent",
        "› 1.", // Selected numbered option
        "› 2.",
        "› 3.",
    ];
    for option in &approval_options {
        if last_lines_lower.contains(option) {
            return Status::Waiting;
        }
    }

    // WAITING: Generic selection cursor (› followed by text)
    for line in &lines {
        let trimmed = line.trim();
        if trimmed.starts_with("›") && trimmed.len() > 2 {
            return Status::Waiting;
        }
    }

    // RUNNING: Check for braille spinners anywhere in recent content
    // Vibe renders vertically so spinner may be on its own line
    for spinner in SPINNER_CHARS {
        if recent_text.contains(spinner) {
            return Status::Running;
        }
    }

    // RUNNING: Activity indicators (may be rendered vertically)
    let activity_indicators = [
        "running",
        "reading",
        "writing",
        "executing",
        "processing",
        "generating",
        "thinking",
    ];
    for indicator in &activity_indicators {
        if recent_text_lower.contains(indicator) {
            return Status::Running;
        }
    }

    // RUNNING: Ellipsis at end often indicates ongoing activity
    if recent_text.ends_with("…") || recent_text.ends_with("...") {
        return Status::Running;
    }

    Status::Idle
}

pub fn detect_codex_status(raw_content: &str) -> Status {
    let content = raw_content.to_lowercase();
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

    // RUNNING: Codex shows "esc to interrupt" or similar when processing
    if last_lines_lower.contains("esc to interrupt")
        || last_lines_lower.contains("ctrl+c to interrupt")
        || last_lines_lower.contains("working")
        || last_lines_lower.contains("thinking")
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

    // WAITING: Approval prompts (Codex uses ask-for-approval modes)
    let approval_prompts = [
        "approve",
        "allow",
        "(y/n)",
        "[y/n]",
        "continue?",
        "proceed?",
        "execute?",
        "run command?",
    ];
    for prompt in &approval_prompts {
        if last_lines_lower.contains(prompt) {
            return Status::Waiting;
        }
    }

    // WAITING: Selection menus
    if last_lines_lower.contains("enter to select") || last_lines_lower.contains("esc to cancel") {
        return Status::Waiting;
    }

    // WAITING: Numbered selection
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

    // WAITING: Input prompt ready
    for line in non_empty_lines.iter().rev().take(10) {
        let clean_line = strip_ansi(line).trim().to_string();
        if clean_line == ">" || clean_line == "> " || clean_line == "codex>" {
            return Status::Waiting;
        }
        if clean_line.starts_with("> ")
            && !clean_line.to_lowercase().contains("esc")
            && clean_line.len() < 100
        {
            return Status::Waiting;
        }
    }

    Status::Idle
}

/// Cursor agent status is detected via hooks (file-based), same as Claude Code.
pub fn detect_cursor_status(_content: &str) -> Status {
    Status::Idle
}

pub fn detect_gemini_status(raw_content: &str) -> Status {
    let content = raw_content.to_lowercase();
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

    // RUNNING: Gemini shows activity indicators
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

    // WAITING: Approval prompts
    let approval_prompts = [
        "(y/n)",
        "[y/n]",
        "allow",
        "approve",
        "execute?",
        "enter to select",
        "esc to cancel",
    ];
    for prompt in &approval_prompts {
        if last_lines_lower.contains(prompt) {
            return Status::Waiting;
        }
    }

    // WAITING: Input prompt
    for line in non_empty_lines.iter().rev().take(10) {
        let clean_line = strip_ansi(line).trim().to_string();
        if clean_line == ">" || clean_line == "> " {
            return Status::Waiting;
        }
    }

    Status::Idle
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_claude_status_running_esc_to_interrupt() {
        assert_eq!(
            detect_claude_status("Processing your request\nesc to interrupt"),
            Status::Running
        );
    }

    #[test]
    fn test_detect_claude_status_running_active_spinner() {
        assert_eq!(
            detect_claude_status("✳ Catapulting… (12m 33s · ↓ 118 tokens)"),
            Status::Running
        );
        assert_eq!(
            detect_claude_status("● Working… (3s · thinking)"),
            Status::Running
        );
        assert_eq!(detect_claude_status("✶ Reading files…"), Status::Running);
    }

    #[test]
    fn test_detect_claude_status_idle_completion() {
        // Completion lines have spinner char but no ellipsis
        assert_eq!(detect_claude_status("✻ Worked for 1m 52s"), Status::Idle);
        assert_eq!(detect_claude_status("✶ Completed in 30s"), Status::Idle);
    }

    #[test]
    fn test_detect_claude_status_running_braille_spinner() {
        assert_eq!(detect_claude_status("Processing ⠋"), Status::Running);
        assert_eq!(detect_claude_status("Loading ⠹"), Status::Running);
    }

    #[test]
    fn test_detect_claude_status_waiting() {
        assert_eq!(
            detect_claude_status("Allow this action? (y/n)"),
            Status::Waiting
        );
        assert_eq!(detect_claude_status("ready\n>"), Status::Waiting);
        // Partial input prompt
        assert_eq!(detect_claude_status("done\n> fix the bu"), Status::Waiting);
    }

    #[test]
    fn test_detect_claude_status_idle() {
        assert_eq!(detect_claude_status("some random output"), Status::Idle);
        assert_eq!(detect_claude_status("file saved"), Status::Idle);
    }

    #[test]
    fn test_detect_claude_status_ignores_old_spinner_in_history() {
        // Active spinner buried under >10 non-empty lines should not trigger Running
        let mut lines = vec!["✳ Old task…".to_string()];
        for _ in 0..11 {
            lines.push("some output line".to_string());
        }
        let content = lines.join("\n");
        assert_eq!(detect_claude_status(&content), Status::Idle);
    }

    #[test]
    fn test_detect_cursor_status_is_stub() {
        assert_eq!(detect_cursor_status("anything"), Status::Idle);
    }

    #[test]
    fn test_detect_status_from_content_unknown_tool_returns_idle() {
        let status = detect_status_from_content("Processing ⠋", "unknown_tool", None);
        assert_eq!(status, Status::Idle);
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
    fn test_detect_vibe_status_running() {
        // Braille spinners
        assert_eq!(detect_vibe_status("processing ⠋"), Status::Running);
        assert_eq!(detect_vibe_status("⠹"), Status::Running);

        // Activity indicators
        assert_eq!(detect_vibe_status("Running bash"), Status::Running);
        assert_eq!(detect_vibe_status("Reading file"), Status::Running);
        assert_eq!(detect_vibe_status("Writing changes"), Status::Running);
        assert_eq!(detect_vibe_status("Generating code"), Status::Running);

        // Vertical text (Vibe's Textual TUI renders one char per line)
        assert_eq!(
            detect_vibe_status("⠋\nR\nu\nn\nn\ni\nn\ng\nb\na\ns\nh\n…"),
            Status::Running
        );

        // Ellipsis indicates ongoing activity
        assert_eq!(detect_vibe_status("Working…"), Status::Running);
        assert_eq!(detect_vibe_status("Loading..."), Status::Running);
    }

    #[test]
    fn test_detect_vibe_status_waiting() {
        // Vibe's approval prompt navigation hints
        assert_eq!(
            detect_vibe_status("↑↓ navigate  Enter select  ESC reject"),
            Status::Waiting
        );
        // Tool approval warning
        assert_eq!(
            detect_vibe_status("⚠ bash command\nExecute this?"),
            Status::Waiting
        );
        // Approval options
        assert_eq!(
            detect_vibe_status(
                "› Yes\n  Yes and always allow bash for this session\n  No and tell the agent"
            ),
            Status::Waiting
        );
    }

    #[test]
    fn test_detect_vibe_status_idle() {
        assert_eq!(detect_vibe_status("some random output"), Status::Idle);
        assert_eq!(detect_vibe_status("file saved successfully"), Status::Idle);
        assert_eq!(detect_vibe_status("Done!"), Status::Idle);
    }

    #[test]
    fn test_detect_codex_status_running() {
        assert_eq!(
            detect_codex_status("processing request\nesc to interrupt"),
            Status::Running
        );
        assert_eq!(
            detect_codex_status("thinking about your request"),
            Status::Running
        );
        assert_eq!(detect_codex_status("working on task"), Status::Running);
        assert_eq!(detect_codex_status("generating ⠋"), Status::Running);
    }

    #[test]
    fn test_detect_codex_status_waiting() {
        assert_eq!(
            detect_codex_status("run this command? (y/n)"),
            Status::Waiting
        );
        assert_eq!(detect_codex_status("approve changes?"), Status::Waiting);
        assert_eq!(
            detect_codex_status("execute this action? [y/n]"),
            Status::Waiting
        );
        assert_eq!(detect_codex_status("ready\ncodex>"), Status::Waiting);
        assert_eq!(detect_codex_status("done\n>"), Status::Waiting);
    }

    #[test]
    fn test_detect_codex_status_idle() {
        assert_eq!(detect_codex_status("file saved"), Status::Idle);
        assert_eq!(detect_codex_status("random output text"), Status::Idle);
    }

    #[test]
    fn test_detect_gemini_status_running() {
        assert_eq!(
            detect_gemini_status("processing request\nesc to interrupt"),
            Status::Running
        );
        assert_eq!(detect_gemini_status("generating ⠋"), Status::Running);
        assert_eq!(detect_gemini_status("working ⠹"), Status::Running);
    }

    #[test]
    fn test_detect_gemini_status_waiting() {
        assert_eq!(
            detect_gemini_status("run this command? (y/n)"),
            Status::Waiting
        );
        assert_eq!(detect_gemini_status("approve changes?"), Status::Waiting);
        assert_eq!(
            detect_gemini_status("execute this action? [y/n]"),
            Status::Waiting
        );
        assert_eq!(detect_gemini_status("ready\n>"), Status::Waiting);
    }

    #[test]
    fn test_detect_gemini_status_idle() {
        assert_eq!(detect_gemini_status("file saved"), Status::Idle);
        assert_eq!(detect_gemini_status("random output text"), Status::Idle);
    }
}
