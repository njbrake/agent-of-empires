//! Status detection for agent sessions

use crate::session::Status;

const SPINNER_CHARS: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn detect_status_from_content(content: &str, tool: &str, _fg_pid: Option<u32>) -> Status {
    crate::agents::get_agent(tool)
        .map(|a| (a.detect_status)(content))
        .unwrap_or_else(|| detect_claude_status(content))
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

    // === RUNNING (highest priority) ===

    // 1. "esc to interrupt" / "ctrl+c to interrupt" — primary running signal
    if last_lines_lower.contains("esc to interrupt")
        || last_lines_lower.contains("ctrl+c to interrupt")
    {
        return Status::Running;
    }

    // 2. Activity indicator: [glyph] [Verb]… — Claude's status line (spinner or static)
    //    Examples: ✽ Boogieing…, ✶ Recombobulating…, · Thinking…, ✳ Pollinating…
    //    The … (U+2026 horizontal ellipsis) distinguishes active from completion (✻ Worked for 4m)
    //    Pattern: single non-alnum glyph + space + text with … — future-proof for new glyphs
    //    Excludes: ⎿ (tool output), ⏺ (tool marker), ● (output bullet), ❯ (prompt)
    for line in non_empty_lines.iter().rev().take(10) {
        let trimmed = line.trim();
        if trimmed.len() < 80 && trimmed.contains('\u{2026}') {
            let mut chars = trimmed.chars();
            if let Some(first) = chars.next() {
                if !first.is_alphanumeric()
                    && !first.is_whitespace()
                    && !matches!(first, '⎿' | '⏺' | '●' | '❯')
                    && chars.next() == Some(' ')
                {
                    return Status::Running;
                }
            }
        }
    }

    // 3. "⎿  Running…" — tool execution in progress
    //    Only last 5 lines: permission prompts add ~9 lines of UI after this line
    for line in non_empty_lines.iter().rev().take(5) {
        let trimmed = line.trim();
        if trimmed.starts_with('⎿') && trimmed.contains("Running") {
            return Status::Running;
        }
    }

    // 4. Braille spinners — ONLY last 5 lines (was: all lines → false positives)
    for line in non_empty_lines.iter().rev().take(5) {
        for spinner in SPINNER_CHARS {
            if line.contains(spinner) {
                return Status::Running;
            }
        }
    }

    // === WAITING (medium priority) ===

    // Permission prompts
    if last_lines.contains("Do you want to proceed?")
        || last_lines_lower.contains("enter to select")
        || last_lines_lower.contains("esc to cancel")
    {
        return Status::Waiting;
    }

    let permission_prompts = [
        "Yes, allow once",
        "Yes, allow always",
        "Allow once",
        "Allow always",
        "Yes, and don't ask again",
        "❯ Yes",
        "❯ No",
        "Do you trust the files in this folder?",
    ];
    for prompt in &permission_prompts {
        if last_lines.contains(prompt) {
            return Status::Waiting;
        }
    }

    // Numbered selections with ❯ cursor — only in last 30 lines
    for line in non_empty_lines.iter().rev().take(30) {
        let trimmed = line.trim();
        if trimmed.starts_with('❯') && trimmed.len() > 2 {
            let rest = trimmed.get(3..).unwrap_or("").trim_start();
            if rest.starts_with("1.") || rest.starts_with("2.") || rest.starts_with("3.") {
                return Status::Waiting;
            }
        }
    }

    // Checkbox UI: [ ] or [x] with ❯ cursor
    if last_lines.contains("[ ]") || last_lines.contains("[x]") {
        for line in non_empty_lines.iter().rev().take(30) {
            if line.trim().starts_with('❯') {
                return Status::Waiting;
            }
        }
    }

    // Plan mode waiting — status bar indicator
    if last_lines_lower.contains("plan mode on") {
        return Status::Waiting;
    }

    // Y/N confirmation prompts
    let question_prompts = ["(Y/n)", "(y/N)", "[Y/n]", "[y/N]"];
    for prompt in &question_prompts {
        if last_lines.contains(prompt) {
            return Status::Waiting;
        }
    }

    // === IDLE (fallback) ===
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

    // Braille spinners — ONLY last 5 non-empty lines to avoid stale false positives
    for line in non_empty_lines.iter().rev().take(5) {
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

    // Numbered selections — scoped to last 30 non-empty lines
    for line in non_empty_lines.iter().rev().take(30) {
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

    // Braille spinners — ONLY last 5 non-empty lines to avoid stale false positives
    for line in non_empty_lines.iter().rev().take(5) {
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

    // WAITING: Numbered selection — scoped to last 30 non-empty lines
    for line in non_empty_lines.iter().rev().take(30) {
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

    // Braille spinners — ONLY last 5 non-empty lines to avoid stale false positives
    for line in non_empty_lines.iter().rev().take(5) {
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
    fn test_detect_claude_activity_indicator_all_glyphs() {
        // Claude spinner glyphs — detected via [glyph] [Verb]… pattern
        // Excludes ● (output bullet with reduce motion off — dual meaning)
        let glyphs = ["·", "✻", "✽", "✶", "✳", "✢"];
        for glyph in &glyphs {
            let content = format!("{} Razzle-dazzling…", glyph);
            assert_eq!(
                detect_claude_status(&content),
                Status::Running,
                "Failed for glyph: {}",
                glyph
            );
        }
    }

    #[test]
    fn test_detect_claude_output_bullet_not_running() {
        // ● (output bullet with reduce motion off) must NOT trigger Running
        assert_eq!(
            detect_claude_status("● Done. The 3 worktrees remain."),
            Status::Idle
        );
        // Even with … it's excluded since ● has dual meaning
        assert_eq!(detect_claude_status("● Grooving…"), Status::Idle);
        // ⏺ (tool output marker) also excluded
        assert_eq!(detect_claude_status("⏺ Working…"), Status::Idle);
    }

    #[test]
    fn test_detect_claude_activity_indicator_with_context() {
        // Activity line with extra info (timing, thinking mode)
        assert_eq!(
            detect_claude_status("✶ Recombobulating… (thought for 2s)"),
            Status::Running
        );
        assert_eq!(
            detect_claude_status("✽ Grooving… (32s · ↓ 328 tokens)"),
            Status::Running
        );
        assert_eq!(detect_claude_status("· Recombobulating…"), Status::Running);
    }

    #[test]
    fn test_detect_claude_completion_marker_not_running() {
        // ✻ (U+273B) with completion text (no …) → NOT running
        assert_eq!(detect_claude_status("✻ Churned for 52s"), Status::Idle);
        assert_eq!(detect_claude_status("✻ Worked for 4m 22s"), Status::Idle);
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
        assert_eq!(detect_claude_status("> hello"), Status::Idle);
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
        assert_eq!(detect_opencode_status(content), Status::Idle);
    }

    #[test]
    fn test_detect_opencode_status_double_prompt() {
        assert_eq!(detect_opencode_status("Ready\n>>"), Status::Idle);
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
    }

    #[test]
    fn test_detect_gemini_status_idle() {
        assert_eq!(detect_gemini_status("file saved"), Status::Idle);
        assert_eq!(detect_gemini_status("random output text"), Status::Idle);
    }

    // === Claude Code v2.1.49 regression tests ===

    #[test]
    fn test_claude_v2_idle_with_completion_marker() {
        // ✻ (U+273B) = completed work, NOT active thinking
        // The ❯ prompt is visible — this is Idle (no action needed from user)
        let content = "⏺ All changes match the plan exactly.\n\n\
            ✻ Churned for 52s\n\n\
            ──────────────────────────────────────────────────\n\
            ❯\n\
            ──────────────────────────────────────────────────\n\
            25% ▰▰▱▱▱▱▱▱▱▱ :3000 :8082\n\
            ⏵⏵ bypass permissions on (shift+tab to cycle)";
        assert_eq!(detect_claude_status(content), Status::Idle);
    }

    #[test]
    fn test_claude_v2_running_with_esc_to_interrupt() {
        let content = "❯ please write me a story\n\n\
            ✳ Pollinating… (esc to interrupt · thinking)\n\n\
            ──────────────────────────────────────────────────\n\
            ❯\n\
            ──────────────────────────────────────────────────\n\
            ? for shortcuts";
        assert_eq!(detect_claude_status(content), Status::Running);
    }

    #[test]
    fn test_claude_v2_running_with_active_asterisk() {
        // ✳ (U+2733) = active thinking → Running
        let content = "⏺ Some output\n\n\
            ✳ Thinking…\n\
            some streamed text";
        assert_eq!(detect_claude_status(content), Status::Running);
    }

    #[test]
    fn test_claude_v2_running_with_tool_execution() {
        let content = "⏺ I'll remove the worktrees...\n\n\
            ⏺ Bash(git worktree remove /path)\n\
            ⎿  Running…";
        assert_eq!(detect_claude_status(content), Status::Running);
    }

    #[test]
    fn test_claude_v2_waiting_permission_prompt() {
        let content = "───────────────────────────────────\n\
            Bash command\n\n\
            echo 'hi' > ~/test.txt\n\n\
            Do you want to proceed?\n\
            1. Yes\n\
            2. Yes, and don't ask again for ~/test.txt commands\n\
            ❯ 3. Type here to tell Claude what to do differently\n\n\
            Esc to cancel";
        assert_eq!(detect_claude_status(content), Status::Waiting);
    }

    #[test]
    fn test_claude_v2_waiting_plan_mode() {
        let content = "✻ Worked for 6m 20s\n\n\
            ──────────────────────────────────────────────────\n\
            ❯\n\
            ──────────────────────────────────────────────────\n\
            57% ▰▰▰▰▰▱▱▱▱▱ agent-of-empires\n\
            ⏸ plan mode on (shift+tab to cycle)";
        assert_eq!(detect_claude_status(content), Status::Waiting);
    }

    #[test]
    fn test_stale_spinner_does_not_cause_false_running() {
        // Spinner on line 5 of 41 lines — well outside last 5
        let mut lines: Vec<String> = (0..40).map(|i| format!("Old output line {}", i)).collect();
        lines[5] = "Processing ⠋ some old tool call".to_string();
        lines.push("❯".to_string());
        let content = lines.join("\n");
        assert_eq!(detect_claude_status(&content), Status::Idle);
    }

    #[test]
    fn test_stale_completion_not_confused_with_active() {
        // ✻ (U+273B) = completed, NOT ✳ (U+2733) = active
        // Bare ❯ with completed marker = Idle
        let content = "✻ Worked for 4m 22s\n\n\
            ──────────────────────────────────────────────────\n\
            ❯\n\
            ──────────────────────────────────────────────────";
        assert_eq!(detect_claude_status(content), Status::Idle);
    }

    #[test]
    fn test_claude_v2_idle_with_chevron_prompt() {
        // ❯ (U+276F) is the actual Claude Code v2 prompt character — Idle, not Waiting
        assert_eq!(detect_claude_status("Task complete.\n❯"), Status::Idle);
        assert_eq!(detect_claude_status("Done!\n❯ "), Status::Idle);
    }

    #[test]
    fn test_claude_v2_idle_chevron_prompt_with_text() {
        // User typing at the ❯ prompt — Idle, not Waiting
        assert_eq!(detect_claude_status("❯ hello"), Status::Idle);
    }

    #[test]
    fn test_claude_v2_yes_and_dont_ask_again() {
        let content = "Allow this?\n\
            Yes, and don't ask again for this command";
        assert_eq!(detect_claude_status(content), Status::Waiting);
    }

    #[test]
    fn test_claude_v2_do_you_want_to_proceed() {
        let content = "⏺ Bash(rm -rf /tmp/test)\n\
            Do you want to proceed?";
        assert_eq!(detect_claude_status(content), Status::Waiting);
    }
}
