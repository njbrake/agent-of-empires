//! tmux status bar configuration for aoe sessions

use anyhow::Result;
use std::process::Command;

/// Information about a sandboxed session for status bar display.
pub struct SandboxDisplay {
    pub container_name: String,
}

/// Apply aoe-styled status bar configuration to a tmux session.
///
/// Sets tmux user options (@aoe_title, @aoe_branch, @aoe_sandbox) and configures
/// the status-right to display session information.
pub fn apply_status_bar(
    session_name: &str,
    title: &str,
    branch: Option<&str>,
    sandbox: Option<&SandboxDisplay>,
) -> Result<()> {
    // Set the session title as a tmux user option
    set_session_option(session_name, "@aoe_title", title)?;

    // Set branch if provided (for worktree sessions)
    if let Some(branch_name) = branch {
        set_session_option(session_name, "@aoe_branch", branch_name)?;
    }

    // Set sandbox info if running in docker container
    if let Some(sandbox_info) = sandbox {
        set_session_option(session_name, "@aoe_sandbox", &sandbox_info.container_name)?;
    }

    // Configure the status bar format using aoe's phosphor green theme
    // colour46 = bright green (matches aoe accent), colour48 = cyan (matches running)
    // colour235 = dark background
    //
    // Format: "aoe: Title | branch | [container] | 14:30"
    // - #{@aoe_title}: session title
    // - #{?#{@aoe_branch}, | #{@aoe_branch},}: conditional branch display
    // - #{?#{@aoe_sandbox}, [#{@aoe_sandbox}],}: conditional sandbox container display
    let status_format = concat!(
        " #[fg=colour46,bold]aoe#[fg=colour252,nobold]: ",
        "#{@aoe_title}",
        "#{?#{@aoe_branch}, #[fg=colour48]| #{@aoe_branch}#[fg=colour252],}",
        "#{?#{@aoe_sandbox}, #[fg=colour214]⬡ #{@aoe_sandbox}#[fg=colour252],}",
        " | %H:%M "
    );

    set_session_option(session_name, "status-right", status_format)?;
    set_session_option(session_name, "status-right-length", "80")?;

    // Dark background with light text - matches aoe phosphor theme
    set_session_option(session_name, "status-style", "bg=colour235,fg=colour252")?;
    set_session_option(
        session_name,
        "status-left",
        " #[fg=colour46,bold]#S#[fg=colour252,nobold] ",
    )?;

    Ok(())
}

/// Set a tmux option for a specific session.
fn set_session_option(session_name: &str, option: &str, value: &str) -> Result<()> {
    let output = Command::new("tmux")
        .args(["set-option", "-t", session_name, option, value])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Don't fail on option errors - status bar is non-critical
        tracing::debug!("Failed to set tmux option {}: {}", option, stderr);
    }

    Ok(())
}

/// Session info retrieved from tmux user options.
pub struct SessionInfo {
    pub title: String,
    pub branch: Option<String>,
    pub sandbox: Option<String>,
}

/// Get session info for the current tmux session (used by `aoe tmux-status` command).
/// Returns structured session info for use in user's custom tmux status bar.
pub fn get_session_info_for_current() -> Option<SessionInfo> {
    let session_name = crate::tmux::get_current_session_name()?;

    // Check if this is an aoe session
    if !session_name.starts_with(crate::tmux::SESSION_PREFIX) {
        return None;
    }

    // Try to get the aoe title from tmux user option
    let title = get_session_option(&session_name, "@aoe_title").unwrap_or_else(|| {
        // Fallback: extract title from session name
        // Session names are: aoe_<title>_<id>
        let name_without_prefix = session_name
            .strip_prefix(crate::tmux::SESSION_PREFIX)
            .unwrap_or(&session_name);
        if let Some(last_underscore) = name_without_prefix.rfind('_') {
            name_without_prefix[..last_underscore].to_string()
        } else {
            name_without_prefix.to_string()
        }
    });

    let branch = get_session_option(&session_name, "@aoe_branch");
    let sandbox = get_session_option(&session_name, "@aoe_sandbox");

    Some(SessionInfo {
        title,
        branch,
        sandbox,
    })
}

/// Get formatted status string for the current tmux session.
/// Returns a plain text string like "aoe: Title | branch | [container]"
pub fn get_status_for_current_session() -> Option<String> {
    let info = get_session_info_for_current()?;

    let mut result = format!("aoe: {}", info.title);

    if let Some(b) = &info.branch {
        result.push_str(" | ");
        result.push_str(b);
    }

    if let Some(s) = &info.sandbox {
        result.push_str(" [");
        result.push_str(s);
        result.push(']');
    }

    Some(result)
}

/// Get a tmux option value for a session.
fn get_session_option(session_name: &str, option: &str) -> Option<String> {
    let output = Command::new("tmux")
        .args(["show-options", "-t", session_name, "-v", option])
        .output()
        .ok()?;

    if output.status.success() {
        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !value.is_empty() {
            return Some(value);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_status_returns_none_for_non_tmux() {
        // When not in tmux, get_current_session_name returns None
        // so get_status_for_current_session should also return None
        // This test just verifies the function doesn't panic
        let _ = get_status_for_current_session();
    }
}
