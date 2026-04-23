//! tmux utility functions

use std::process::Command;

pub fn strip_ansi(content: &str) -> String {
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

pub fn sanitize_session_name(name: &str) -> String {
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

/// Append `; set-option -p -t <target> remain-on-exit on` to an in-flight
/// tmux argument list so that remain-on-exit is set atomically with session
/// creation. Using pane-level (`-p`) avoids bleeding into user-created panes
/// in the same session.
///
/// Note: the `-p` (pane-level) flag requires tmux >= 3.0.
pub fn append_remain_on_exit_args(args: &mut Vec<String>, target: &str) {
    args.extend([
        ";".to_string(),
        "set-option".to_string(),
        "-p".to_string(),
        "-t".to_string(),
        target.to_string(),
        "remain-on-exit".to_string(),
        "on".to_string(),
    ]);
}

/// Append `; set-option -t <target> pane-base-index 0` to an in-flight tmux
/// argument list so that pane indices always start at 0 regardless of the
/// user's global config.  This lets status checks use `.0` to reliably target
/// the agent's pane.  See #488.
pub fn append_pane_base_index_args(args: &mut Vec<String>, target: &str) {
    args.extend([
        ";".to_string(),
        "set-option".to_string(),
        "-t".to_string(),
        target.to_string(),
        "pane-base-index".to_string(),
        "0".to_string(),
    ]);
}

/// Append `; set-option -t <target> mouse on` to an in-flight tmux argument
/// list so that mouse/wheel events are forwarded into tmux copy-mode.
/// Required for the web dashboard's two-finger scroll on mobile, which
/// emits SGR mouse-wheel escape sequences that tmux must interpret.
pub fn append_mouse_on_args(args: &mut Vec<String>, target: &str) {
    args.extend([
        ";".to_string(),
        "set-option".to_string(),
        "-t".to_string(),
        target.to_string(),
        "mouse".to_string(),
        "on".to_string(),
    ]);
}

/// Append `; set-option -t <target> window-size latest` so the tmux window
/// follows the most recently active client. Required for the primary-client
/// resize model: without this, a user's `~/.tmux.conf` could set
/// `window-size smallest`, which would shrink the window to the smallest
/// attached PTY regardless of which client is primary.
pub fn append_window_size_args(args: &mut Vec<String>, target: &str) {
    args.extend([
        ";".to_string(),
        "set-option".to_string(),
        "-t".to_string(),
        target.to_string(),
        "window-size".to_string(),
        "latest".to_string(),
    ]);
}

/// Key table name for aoe's custom wheel-scroll bindings. Sessions that
/// opt in via `set-option key-table aoe-scroll` get our overrides;
/// sessions left on the default `root` table are unaffected.
const AOE_SCROLL_KEY_TABLE: &str = "aoe-scroll";

/// Append custom wheel-scroll bindings for this aoe session.
///
/// Fixes the "scroll-up wraps to bottom" bug reported on the mobile web
/// client. Root cause: tmux's default `WheelUpPane` binding enters
/// copy-mode with the `-e` flag, which exits copy-mode when the user
/// subsequently scrolls down past the bottom of the history. On mobile
/// that oscillation is easy to trigger accidentally and the snap-to-live
/// discards the user's scroll position. Without `-e` the user stays in
/// copy-mode until they explicitly exit; the web UI provides a "Back
/// to live" button that sends Escape.
///
/// **Isolation from user's own tmux:** the binding lives in a custom
/// `aoe-scroll` key table (not `root`). `bind-key -T aoe-scroll ...` is
/// server-global but inert for any session that hasn't opted in, so it
/// does NOT clobber a user's `~/.tmux.conf` WheelUpPane customization.
/// Per-session opt-in is done via `set-option -t <session> key-table
/// aoe-scroll` which tmux honors for mouse keys the same way it does
/// for keyboard keys: lookup hits `aoe-scroll` first, falls through to
/// `root` on miss.
///
/// We intentionally do NOT override the `copy-mode` / `copy-mode-vi`
/// tables. Those are shared across every tmux session on the server
/// with no way to scope per-session, so changing them there would leak
/// into users' own sessions. The frontend already emits 20-30 wheel
/// events per swipe; at tmux's default 5 lines/tick that's still
/// 100-150 lines per swipe — plenty of progress on scroll.
///
/// `bind-key` and `set-option` are idempotent; re-issuing on every
/// aoe session create is harmless and keeps the binding in place after
/// tmux server restarts.
///
/// Uses tmux 3.0 brace-block command syntax so nested if-shell branches
/// don't require escaping nightmares.
pub fn append_aoe_wheel_bindings_args(args: &mut Vec<String>, target: &str) {
    tracing::debug!(
        "Appending aoe-scroll key-table opt-in for session {}",
        target
    );

    // Install the WheelUpPane binding in our custom key table. Only
    // sessions that set `key-table aoe-scroll` use it; all other
    // sessions on the tmux server are completely unaffected.
    args.extend([
        ";".to_string(),
        "bind-key".to_string(),
        "-T".to_string(),
        AOE_SCROLL_KEY_TABLE.to_string(),
        "WheelUpPane".to_string(),
        r##"{ if-shell -F "#{mouse_any_flag}" { send-keys -M } { if-shell -F "#{pane_in_mode}" { send-keys -M } { copy-mode ; send-keys -M } } }"##.to_string(),
    ]);

    // Opt this session into the custom key table. Mouse keys (and any
    // other keys we might add in the future) hit `aoe-scroll` first
    // and fall through to `root` if unbound.
    args.extend([
        ";".to_string(),
        "set-option".to_string(),
        "-t".to_string(),
        target.to_string(),
        "key-table".to_string(),
        AOE_SCROLL_KEY_TABLE.to_string(),
    ]);
}

pub fn is_pane_dead(session_name: &str) -> bool {
    // Use `^.0` to target the first window's first pane regardless of
    // base-index or which pane is active, so the check always hits the
    // agent's pane even when the user has created additional tmux windows
    // or split panes.  See #435, #488.
    let target = format!("{session_name}:^.0");
    Command::new("tmux")
        .args(["display-message", "-t", &target, "-p", "#{pane_dead}"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim() == "1")
        .unwrap_or(false)
}

pub(crate) fn pane_current_command(session_name: &str) -> Option<String> {
    // Use `^.0` to target the first window's first pane regardless of
    // base-index or which pane is active.  See #435, #488.
    let target = format!("{session_name}:^.0");
    Command::new("tmux")
        .args([
            "display-message",
            "-t",
            &target,
            "-p",
            "#{pane_current_command}",
        ])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

// Shells that indicate the agent is not running (the pane was restored by
// tmux-resurrect, the agent crashed back to a prompt, or the user exited).
const KNOWN_SHELLS: &[&str] = &[
    "bash", "zsh", "sh", "fish", "dash", "ksh", "tcsh", "csh", "nu", "pwsh",
];

pub(crate) fn is_shell_command(cmd: &str) -> bool {
    let normalized = cmd.strip_prefix('-').unwrap_or(cmd);
    KNOWN_SHELLS.contains(&normalized)
}

pub fn is_pane_running_shell(session_name: &str) -> bool {
    pane_current_command(session_name)
        .map(|cmd| is_shell_command(&cmd))
        .unwrap_or(false)
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
    fn test_strip_ansi() {
        assert_eq!(strip_ansi("\x1b[32mgreen\x1b[0m"), "green");
        assert_eq!(strip_ansi("no codes here"), "no codes here");
        assert_eq!(strip_ansi("\x1b[1;34mbold blue\x1b[0m"), "bold blue");
    }

    #[test]
    fn test_strip_ansi_empty_string() {
        assert_eq!(strip_ansi(""), "");
    }

    #[test]
    fn test_strip_ansi_multiple_codes() {
        assert_eq!(
            strip_ansi("\x1b[1m\x1b[32mbold green\x1b[0m normal"),
            "bold green normal"
        );
    }

    #[test]
    fn test_strip_ansi_osc_sequences() {
        assert_eq!(strip_ansi("\x1b]0;Window Title\x07text"), "text");
    }

    #[test]
    fn test_strip_ansi_nested_sequences() {
        assert_eq!(strip_ansi("\x1b[38;5;196mred\x1b[0m"), "red");
    }

    #[test]
    fn test_strip_ansi_with_256_colors() {
        assert_eq!(
            strip_ansi("\x1b[38;2;255;100;50mRGB color\x1b[0m"),
            "RGB color"
        );
    }

    #[test]
    fn test_sanitize_session_name_special_chars() {
        assert_eq!(sanitize_session_name("test/path"), "test_path");
        assert_eq!(sanitize_session_name("test.name"), "test_name");
        assert_eq!(sanitize_session_name("test@name"), "test_name");
        assert_eq!(sanitize_session_name("test:name"), "test_name");
    }

    #[test]
    fn test_sanitize_session_name_preserves_valid_chars() {
        assert_eq!(sanitize_session_name("test-name_123"), "test-name_123");
    }

    #[test]
    fn test_sanitize_session_name_empty() {
        assert_eq!(sanitize_session_name(""), "");
    }

    #[test]
    fn test_sanitize_session_name_unicode() {
        let result = sanitize_session_name("test😀emoji");
        assert!(result.starts_with("test"));
        assert!(result.contains('_'));
        assert!(!result.contains('😀'));
    }

    #[test]
    fn test_is_shell_command_recognizes_common_shells() {
        for shell in KNOWN_SHELLS {
            assert!(
                is_shell_command(shell),
                "{shell} should be recognized as a shell"
            );
        }
    }

    #[test]
    fn test_is_shell_command_recognizes_login_shells() {
        for shell in ["-bash", "-zsh", "-sh", "-fish"] {
            assert!(
                is_shell_command(shell),
                "{shell} should be recognized as a login shell"
            );
        }
    }

    #[test]
    fn test_is_shell_command_rejects_agent_binaries() {
        for cmd in [
            "claude", "opencode", "codex", "gemini", "cursor", "droid", "sleep", "python",
        ] {
            assert!(
                !is_shell_command(cmd),
                "{cmd} should not be recognized as a shell"
            );
        }
    }
}
