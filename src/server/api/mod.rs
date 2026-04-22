//! HTTP REST handlers for the web dashboard backend.
//!
//! Originally a single 2,151-line module; split into:
//!   - `sessions` — session CRUD, ensure-* lifecycle endpoints, and rich diff
//!   - `git`      — repo cloning and branch listing
//!   - `system`   — agents, settings, themes, profiles, filesystem,
//!                  groups, docker, about, devices
//!   - this file  — shared validation helpers + module declarations and
//!                  re-exports so external callers keep `api::*` paths.

pub(super) use super::AppState;

mod git;
mod sessions;
mod system;

pub use git::{clone_repo, list_branches};
pub use sessions::{
    create_session, delete_session, ensure_container_terminal, ensure_session, ensure_terminal,
    list_sessions, rename_session, session_diff_file, session_diff_files,
    update_session_notifications, CleanupDefaults, SessionResponse,
};
pub use system::{
    browse_filesystem, docker_status, filesystem_home, get_about, get_settings, list_agents,
    list_devices, list_groups, list_profiles, list_themes, update_settings,
};

const SHELL_METACHARACTERS: &[char] = &[
    ';', '&', '|', '$', '`', '(', ')', '{', '}', '<', '>', '\n', '\r', '\\', '"', '\'', '!', '#',
    '*', '?', '[', ']', '~', '\t', '\0',
];

pub(super) fn validate_no_shell_injection(value: &str, field_name: &str) -> Result<(), String> {
    if let Some(c) = value.chars().find(|c| SHELL_METACHARACTERS.contains(c)) {
        return Err(format!(
            "Invalid character '{}' in {}. Shell metacharacters are not allowed.",
            c, field_name
        ));
    }
    Ok(())
}

pub(super) const ALLOWED_SETTINGS_SECTIONS: &[&str] = &[
    "theme", "session", "tmux", "updates", "sound", "sandbox", "worktree",
];

pub(super) const SESSION_BLOCKED_FIELDS: &[&str] =
    &["agent_command_override", "agent_extra_args", "extra_env"];
