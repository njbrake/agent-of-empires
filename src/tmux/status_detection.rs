//! Status detection for agent sessions

use crate::session::Status;

use super::utils::strip_ansi;

const SPINNER_CHARS: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

pub fn detect_status_from_content(content: &str, tool: &str) -> Status {
    // Strip ANSI escape codes before passing to detectors. capture-pane is
    // called with -e (to preserve colors for the TUI preview), but color codes
    // interspersed in text like "esc interrupt" break plain substring matches.
    let clean = strip_ansi(content);
    let status = crate::agents::get_agent(tool)
        .map(|a| (a.detect_status)(&clean))
        .unwrap_or(Status::Idle);

    if status == Status::Idle {
        let last_lines: Vec<&str> = clean.lines().rev().take(5).collect();
        tracing::debug!(
            "status detection returned Idle for tool '{}', last 5 lines: {:?}",
            tool,
            last_lines
        );
    }

    status
}

/// Spinner frame characters Claude Code rotates through next to its active
/// verb. macOS uses `· ✢ ✳ ✶ ✻ ✽`, other platforms swap `✽` for `*`, and
/// reduced-motion mode renders a static `●`.
const CLAUDE_SPINNER_CHARS: &[char] = &['·', '✢', '✳', '✶', '✻', '✽', '*', '●'];

/// Claude Code status is primarily detected via hooks (file-based) installed
/// in `~/.claude/settings.json`. When hooks aren't reachable (first few
/// seconds before a hook fires, custom `--cmd` wrappers, `docker exec` into
/// a user-managed container that aoe didn't provision), the dispatcher falls
/// back to this pane-based detector.
///
/// The dispatcher strips ANSI before calling us, so we only match on
/// human-readable text shapes:
///   1. The interrupt hint ("esc to interrupt" / "ctrl+c to interrupt").
///   2. The live token counter ("(4s · ↓ 88 tokens)") that only renders
///      while a turn is generating.
///   3. The spinner+verb shape ("✶ Working…") on a recent line.
///
/// The `…` in shape (3) is what distinguishes active from completed lines.
/// Claude renders active verbs as gerunds with a trailing `…` (`Working…`)
/// and past-tense completions without one (`Worked for 1m 52s`), so we
/// don't need a separate past-tense verb list.
pub fn detect_claude_status(content: &str) -> Status {
    // Claude often leaves the bottom of the pane blank (cursor parked below
    // the spinner line, or a small response in a tall pane), so we filter
    // empty lines first and look at the last 30 non-empty lines. Matches
    // the pattern used by detect_opencode_status and friends.
    let non_empty: Vec<&str> = content.lines().filter(|l| !l.trim().is_empty()).collect();
    let recent: Vec<&str> = non_empty.iter().rev().take(30).rev().copied().collect();
    let recent_joined = recent.join("\n");
    let recent_lower = recent_joined.to_lowercase();

    if recent_lower.contains("esc to interrupt") || recent_lower.contains("ctrl+c to interrupt") {
        return Status::Running;
    }

    if has_claude_live_token_counter(&recent_joined) {
        return Status::Running;
    }

    for line in &recent {
        if claude_line_is_active_spinner(line) {
            return Status::Running;
        }
    }

    Status::Idle
}

/// Detect the live token counter Claude Code prints during generation,
/// e.g. `(4s · ↓ 88 tokens)`. The `s · ↓ N tokens` substring is unique to
/// the active counter; an idle pane never contains it.
fn has_claude_live_token_counter(content: &str) -> bool {
    let mut search = content;
    while let Some(pos) = search.find("s · ↓") {
        let after = search[pos + "s · ↓".len()..].trim_start();
        let mut digits_end = 0;
        for (i, c) in after.char_indices() {
            if c.is_ascii_digit() {
                digits_end = i + c.len_utf8();
            } else {
                break;
            }
        }
        if digits_end > 0 && after[digits_end..].trim_start().starts_with("tokens") {
            return true;
        }
        // Advance past this match so we don't loop on the same position.
        search = &search[pos + "s · ↓".len()..];
    }
    false
}

/// Match the `<frame> <Verb…>` shape on a single pane line. The ellipsis must
/// be inside the first word after the frame char so we match `Working…` but
/// not past-tense completions (`Worked for 1m 52s`, no `…`) or rendered
/// markdown bullets (`* Cooked an amazing dish today…`, `…` is several words
/// in).
fn claude_line_is_active_spinner(line: &str) -> bool {
    let trimmed = line.trim_start();
    let mut chars = trimmed.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !CLAUDE_SPINNER_CHARS.contains(&first) {
        return false;
    }
    let rest = chars.as_str().trim_start();
    if rest.is_empty() {
        return false;
    }

    let first_word_end = rest.find(|c: char| c.is_whitespace()).unwrap_or(rest.len());
    let first_word = &rest[..first_word_end];
    let starts_uppercase = first_word.chars().next().is_some_and(|c| c.is_uppercase());
    starts_uppercase && first_word.contains('…')
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

/// Copilot CLI status detection via tmux pane parsing.
/// Copilot CLI is a full-screen TUI. It shows "Thinking" while the model is
/// processing and displays tool approval prompts when actions need confirmation.
pub fn detect_copilot_status(raw_content: &str) -> Status {
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

    // RUNNING: Copilot shows spinners and "Thinking" while the model is processing
    for line in &lines {
        for spinner in SPINNER_CHARS {
            if line.contains(spinner) {
                return Status::Running;
            }
        }
    }

    if last_lines_lower.contains("thinking")
        || last_lines_lower.contains("working")
        || last_lines_lower.contains("esc to interrupt")
        || last_lines_lower.contains("ctrl+c to interrupt")
    {
        return Status::Running;
    }

    // WAITING: Tool approval prompts
    let approval_prompts = [
        "approve",
        "allow",
        "(y/n)",
        "[y/n]",
        "continue?",
        "run command?",
        "allow this tool",
        "approve for the rest",
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

    // WAITING: Input prompt ready
    for line in non_empty_lines.iter().rev().take(10) {
        let clean_line = strip_ansi(line).trim().to_string();
        if clean_line == ">" || clean_line == "> " || clean_line == "copilot>" {
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

/// Pi coding agent status detection via tmux pane parsing.
/// Pi always auto-approves tool use (no approval gates), so we only detect
/// Running vs Idle/Waiting-for-input states.
pub fn detect_pi_status(raw_content: &str) -> Status {
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

    // RUNNING: Pi shows spinners and activity indicators
    for line in &lines {
        for spinner in SPINNER_CHARS {
            if line.contains(spinner) {
                return Status::Running;
            }
        }
    }

    if last_lines_lower.contains("esc to interrupt")
        || last_lines_lower.contains("ctrl+c to interrupt")
    {
        return Status::Running;
    }

    // WAITING: Check for input prompt before activity indicators, since words
    // like "reading" or "writing" can linger in scrollback after the agent
    // finishes and shows a prompt.
    for line in non_empty_lines.iter().rev().take(5) {
        let clean_line = strip_ansi(line).trim().to_string();
        if clean_line == ">" || clean_line == "> " || clean_line == "pi>" {
            return Status::Waiting;
        }
        if clean_line.starts_with("> ")
            && !clean_line.to_lowercase().contains("esc")
            && clean_line.len() < 100
        {
            return Status::Waiting;
        }
    }

    // RUNNING: Activity indicators in the last few lines
    let activity_indicators = ["thinking", "working", "reading", "writing", "executing"];
    for indicator in &activity_indicators {
        if last_lines_lower.contains(indicator) {
            return Status::Running;
        }
    }

    Status::Idle
}

/// Factory Droid CLI status detection via tmux pane parsing.
/// Droid uses an interactive REPL similar to other coding agents. It shows
/// activity indicators while processing and prompts for input when idle.
pub fn detect_droid_status(raw_content: &str) -> Status {
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

    // RUNNING: Spinners indicate active processing
    for line in &lines {
        for spinner in SPINNER_CHARS {
            if line.contains(spinner) {
                return Status::Running;
            }
        }
    }

    if last_lines_lower.contains("esc to interrupt")
        || last_lines_lower.contains("ctrl+c to interrupt")
        || last_lines_lower.contains("thinking")
        || last_lines_lower.contains("working")
        || last_lines_lower.contains("executing")
    {
        return Status::Running;
    }

    // WAITING: Approval prompts
    let approval_prompts = [
        "approve",
        "allow",
        "(y/n)",
        "[y/n]",
        "continue?",
        "proceed?",
        "execute?",
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

    // WAITING: Input prompt ready
    for line in non_empty_lines.iter().rev().take(10) {
        let clean_line = strip_ansi(line).trim().to_string();
        if clean_line == ">" || clean_line == "> " || clean_line == "droid>" {
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

/// Hermes status is detected via shell-script hooks (YAML-based) registered
/// in `~/.hermes/config.yaml`, not tmux pane parsing. This stub exists so
/// the agent registry has a valid function pointer; it only runs as a
/// fallback when the hook hasn't written a status file yet.
pub fn detect_hermes_status(_content: &str) -> Status {
    Status::Idle
}

/// settl status is detected via hooks (TOML-based), not tmux pane parsing.
/// This stub exists so the agent registry has a valid function pointer.
pub fn detect_settl_status(_content: &str) -> Status {
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
    fn test_detect_cursor_status_is_stub() {
        // Cursor uses hook-based detection; the stub always returns Idle
        assert_eq!(detect_cursor_status("anything"), Status::Idle);
    }

    #[test]
    fn test_detect_claude_status_idle_on_plain_text() {
        // No spinner, no interrupt hint, no token counter: Idle.
        assert_eq!(detect_claude_status(""), Status::Idle);
        assert_eq!(detect_claude_status("Some output\n> "), Status::Idle);
        assert_eq!(
            detect_claude_status("file saved successfully"),
            Status::Idle
        );
    }

    #[test]
    fn test_detect_claude_status_running_on_interrupt_hint() {
        // The most reliable signal: Claude prints an interrupt hint while
        // a turn is generating.
        assert_eq!(
            detect_claude_status("✶ Working…\n  esc to interrupt"),
            Status::Running
        );
        assert_eq!(
            detect_claude_status("Generating...\nctrl+c to interrupt"),
            Status::Running
        );
    }

    #[test]
    fn test_detect_claude_status_running_on_live_token_counter() {
        // The (Xs · ↓ N tokens) counter only renders during generation.
        assert_eq!(
            detect_claude_status("✶ Working… (4s · ↓ 88 tokens)"),
            Status::Running
        );
        assert_eq!(
            detect_claude_status("● Cooking… (12s · ↓ 1234 tokens)"),
            Status::Running
        );
    }

    #[test]
    fn test_detect_claude_status_running_on_spinner_verb_shape() {
        // <frame> <Verb…> is the live spinner line.
        assert_eq!(detect_claude_status("✶ Working…"), Status::Running);
        assert_eq!(detect_claude_status("✻ Herding…"), Status::Running);
        assert_eq!(detect_claude_status("● Pondering…"), Status::Running);
        assert_eq!(detect_claude_status("· Sautéing…"), Status::Running);
        // Reduced-motion mode renders a static ●.
        assert_eq!(detect_claude_status("● Working…"), Status::Running);
    }

    #[test]
    fn test_detect_claude_status_idle_on_past_tense_completion() {
        // Same frame char, but "Worked for 1m 52s" means the turn is done.
        assert_eq!(detect_claude_status("✻ Worked for 1m 52s"), Status::Idle);
        assert_eq!(detect_claude_status("● Cooked for 30s"), Status::Idle);
        assert_eq!(detect_claude_status("· Brewed for 2m 10s"), Status::Idle);
    }

    #[test]
    fn test_detect_claude_status_ignores_lowercase_after_frame() {
        // "* foo…" (e.g. a markdown bullet that happens to end with an
        // ellipsis) should not be mistaken for an active spinner. Active
        // verbs are always capitalized.
        assert_eq!(detect_claude_status("* foo…"), Status::Idle);
    }

    #[test]
    fn test_detect_claude_status_ignores_markdown_bullet_with_trailing_ellipsis() {
        // Rendered markdown bullets can start with a frame char and a
        // capitalized word and end with a trailing `…`. The live spinner
        // line always has the ellipsis inside the first word
        // (`Cooking…`), not several words later, so we don't flag this
        // as Running.
        assert_eq!(
            detect_claude_status("* Cooked an amazing dish today…"),
            Status::Idle
        );
        assert_eq!(
            detect_claude_status("· Some random response text ending with…"),
            Status::Idle
        );
    }

    #[test]
    fn test_detect_claude_status_finds_signal_above_blank_padding() {
        // Real `tmux capture-pane -S -50` typically returns 50 lines even
        // when the agent has only painted 2-3 lines at the top, with the
        // rest blank. The detector must skip blank lines, not just look at
        // the literal last N lines, or it'll miss every signal.
        let mut content = String::from("✶ Working… (4s · ↓ 88 tokens)\n  esc to interrupt\n");
        for _ in 0..40 {
            content.push('\n');
        }
        assert_eq!(detect_claude_status(&content), Status::Running);
    }

    #[test]
    fn test_detect_claude_status_handles_v2_1_118_per_word_ansi() {
        // Regression for #890: Claude Code v2.1.118 wraps each word in ANSI
        // color escapes. After the dispatcher strips ANSI we should still
        // see the spinner+verb shape and the interrupt hint.
        let ansi_running = "\x1b[38;5;174m✶\x1b[39m \x1b[38;5;180mWorking…\x1b[38;5;174m \x1b[38;5;246m(4s · ↓\x1b[39m \x1b[38;5;246m88 tokens)\x1b[39m\n\x1b[39m  \x1b[38;5;246mesc\x1b[39m \x1b[38;5;246mto\x1b[39m \x1b[38;5;246minterrupt\x1b[39m";
        assert_eq!(
            detect_status_from_content(ansi_running, "claude"),
            Status::Running,
            "Per-word ANSI coloring must not prevent Running detection for Claude Code"
        );
    }

    #[test]
    fn test_detect_status_from_content_unknown_tool_returns_idle() {
        let status = detect_status_from_content("Processing ⠋", "unknown_tool");
        assert_eq!(status, Status::Idle);
    }

    #[test]
    fn test_detect_status_strips_ansi_before_matching() {
        // capture-pane -e injects ANSI color codes between characters, which
        // can split signal strings like "esc interrupt" so they no longer match
        // as plain substrings. The dispatcher must strip ANSI before calling
        // any agent detector.
        let ansi_running =
            "\x1b[38;2;39;62;94m⬝⬝⬝⬝⬝⬝⬝⬝\x1b[0m  \x1b[38;2;238;238;238mesc \x1b[38;2;128;128;128minterrupt\x1b[0m";
        assert_eq!(
            detect_status_from_content(ansi_running, "opencode"),
            Status::Running,
            "ANSI codes around 'esc interrupt' should not prevent Running detection"
        );

        let ansi_spinner = "\x1b[38;2;255;255;255m⠋\x1b[0m generating";
        assert_eq!(
            detect_status_from_content(ansi_spinner, "opencode"),
            Status::Running,
            "ANSI codes around spinner chars should not prevent Running detection"
        );
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

    #[test]
    fn test_detect_copilot_status_running() {
        assert_eq!(
            detect_copilot_status("processing request\nesc to interrupt"),
            Status::Running
        );
        assert_eq!(
            detect_copilot_status("Thinking about your request"),
            Status::Running
        );
        assert_eq!(detect_copilot_status("working ⠋"), Status::Running);
        assert_eq!(detect_copilot_status("loading ⠹"), Status::Running);
    }

    #[test]
    fn test_detect_copilot_status_waiting() {
        assert_eq!(detect_copilot_status("run command? (y/n)"), Status::Waiting);
        assert_eq!(
            detect_copilot_status("Allow this tool to run?"),
            Status::Waiting
        );
        assert_eq!(
            detect_copilot_status("pick an option\nenter to select"),
            Status::Waiting
        );
        assert_eq!(detect_copilot_status("done\n>"), Status::Waiting);
        assert_eq!(detect_copilot_status("done\ncopilot>"), Status::Waiting);
    }

    #[test]
    fn test_detect_copilot_status_idle() {
        assert_eq!(detect_copilot_status("file saved"), Status::Idle);
        assert_eq!(detect_copilot_status("random output text"), Status::Idle);
    }

    #[test]
    fn test_detect_pi_status_running() {
        assert_eq!(detect_pi_status("generating ⠋"), Status::Running);
        assert_eq!(detect_pi_status("loading ⠹"), Status::Running);
        assert_eq!(
            detect_pi_status("processing request\nesc to interrupt"),
            Status::Running
        );
        assert_eq!(detect_pi_status("thinking about code"), Status::Running);
        assert_eq!(detect_pi_status("reading file.ts"), Status::Running);
    }

    #[test]
    fn test_detect_pi_status_waiting() {
        assert_eq!(detect_pi_status("done\n>"), Status::Waiting);
        assert_eq!(detect_pi_status("ready\n> "), Status::Waiting);
        assert_eq!(detect_pi_status("complete\npi>"), Status::Waiting);
        // Prompt takes priority over activity words lingering in scrollback
        assert_eq!(
            detect_pi_status("reading config.toml\nDone.\n>"),
            Status::Waiting
        );
    }

    #[test]
    fn test_detect_pi_status_idle() {
        assert_eq!(detect_pi_status("file saved"), Status::Idle);
        assert_eq!(detect_pi_status("random output text"), Status::Idle);
    }

    #[test]
    fn test_detect_droid_status_running() {
        assert_eq!(
            detect_droid_status("processing request\nesc to interrupt"),
            Status::Running
        );
        assert_eq!(
            detect_droid_status("thinking about your request"),
            Status::Running
        );
        assert_eq!(detect_droid_status("working on task"), Status::Running);
        assert_eq!(detect_droid_status("executing command"), Status::Running);
        assert_eq!(detect_droid_status("generating ⠋"), Status::Running);
    }

    #[test]
    fn test_detect_droid_status_waiting() {
        assert_eq!(
            detect_droid_status("run this command? (y/n)"),
            Status::Waiting
        );
        assert_eq!(detect_droid_status("approve changes?"), Status::Waiting);
        assert_eq!(
            detect_droid_status("execute this action? [y/n]"),
            Status::Waiting
        );
        assert_eq!(detect_droid_status("ready\ndroid>"), Status::Waiting);
        assert_eq!(detect_droid_status("done\n>"), Status::Waiting);
    }

    #[test]
    fn test_detect_droid_status_idle() {
        assert_eq!(detect_droid_status("file saved"), Status::Idle);
        assert_eq!(detect_droid_status("random output text"), Status::Idle);
    }

    #[test]
    fn test_detect_hermes_status_is_stub() {
        // Hermes uses hook-based detection; the stub always returns Idle
        assert_eq!(detect_hermes_status("anything"), Status::Idle);
    }

    #[test]
    fn test_detect_settl_status_is_stub() {
        // settl uses hook-based detection; the stub always returns Idle
        assert_eq!(detect_settl_status("anything"), Status::Idle);
    }
}
