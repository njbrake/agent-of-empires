//! HTTP REST handlers for the web dashboard backend.
//!
//! Originally a single 2,151-line module; split into:
//!   - `sessions` — session CRUD, ensure-* lifecycle endpoints, and rich diff
//!   - `git`      — repo cloning and branch listing
//!   - `system`   — agents, settings, themes, profiles, filesystem,
//!     groups, docker, about, devices
//!   - this file  — shared validation helpers + module declarations and
//!     re-exports so external callers keep `api::*` paths.

pub(super) use super::AppState;

mod git;
mod sessions;
mod system;

pub use git::{clone_repo, list_branches};
pub use sessions::{
    create_session, delete_session, ensure_container_terminal, ensure_session, ensure_terminal,
    list_sessions, read_output, rename_session, send_message, session_diff_file,
    session_diff_files, update_session_notifications, CleanupDefaults, OutputQuery,
    SendMessageRequest, SessionResponse,
};
pub use system::{
    browse_filesystem, create_profile, default_profile, delete_profile, docker_status,
    filesystem_home, get_about, get_profile_settings, get_settings, list_agents, list_devices,
    list_groups, list_profiles, list_sounds, list_themes, rename_profile, update_profile_settings,
    update_settings,
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
    // web: audited 2026-04-24, contains only boolean notification toggles
    // (notifications_enabled, notify_on_waiting, notify_on_idle, notify_on_error).
    // No shell commands, no binary paths, no RCE surface.
    "web",
];

pub(super) const SESSION_BLOCKED_FIELDS: &[&str] = &[
    "agent_command_override",
    "agent_extra_args",
    "extra_env",
    // custom_agents maps names to arbitrary shell commands (e.g., "ssh -t host claude").
    // agent_detect_as maps names to detection targets but is part of the agent config
    // surface that should only be editable locally.
    "custom_agents",
    "agent_detect_as",
];

/// Validate that a profile name contains only safe characters.
/// Rejects path traversal attempts (../, /) and shell metacharacters.
pub(super) fn validate_profile_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Profile name cannot be empty".to_string());
    }
    if name.len() > 64 {
        return Err("Profile name must be 64 characters or fewer".to_string());
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(
            "Profile name must contain only letters, digits, hyphens, and underscores".to_string(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    //! Regression tests that pin security-critical constants.
    //!
    //! These three constants were silently rewritten in an earlier hand-
    //! assembled version of this split. The failure mode was specific: a
    //! refactor PR that claimed "no behavior changes" dropped 4 shell
    //! metacharacters (`#`, `[`, `]`, `~`) from the injection blocklist,
    //! added `"hooks"` to the settings-write allowlist (a hooks section
    //! set via the API runs arbitrary shell commands on session start —
    //! local RCE), and replaced the `SESSION_BLOCKED_FIELDS` contents
    //! with two field names that don't exist on `SessionConfig`,
    //! turning the blocklist into a no-op.
    //!
    //! Pin the contents here so the next refactor that touches this
    //! file fails CI instead of silently regressing security.
    use super::*;

    #[test]
    fn shell_metacharacters_blocklist_is_exhaustive() {
        // Every character here has a documented shell-injection vector when
        // interpolated into a command line. Removing a character from this
        // list without removing the corresponding regression below is a
        // security change that must be reviewed on its own, not smuggled
        // through a refactor.
        let expected: &[char] = &[
            ';', '&', '|', '$', '`', '(', ')', '{', '}', '<', '>', '\n', '\r', '\\', '"', '\'',
            '!', '#', '*', '?', '[', ']', '~', '\t', '\0',
        ];
        assert_eq!(
            SHELL_METACHARACTERS.len(),
            expected.len(),
            "SHELL_METACHARACTERS size changed — every addition/removal must be \
             reviewed as a security change, not a refactor tidy-up"
        );
        for c in expected {
            assert!(
                SHELL_METACHARACTERS.contains(c),
                "SHELL_METACHARACTERS lost character {:?}. Each character blocks \
                 a specific shell-injection vector: # starts a comment, [ ] are \
                 glob metacharacters, ~ triggers tilde expansion, etc. If the \
                 intent is to actually stop blocking this character, update both \
                 this test and the list in the same commit with justification.",
                c
            );
        }
    }

    #[test]
    fn validate_no_shell_injection_rejects_every_metacharacter() {
        for &c in SHELL_METACHARACTERS {
            let input = format!("prefix{}suffix", c);
            let result = validate_no_shell_injection(&input, "field");
            assert!(
                result.is_err(),
                "validate_no_shell_injection should reject {:?} but accepted {:?}",
                c,
                input
            );
        }
    }

    #[test]
    fn allowed_settings_sections_are_pinned() {
        // If you're adding a new top-level settings section, add it here AND
        // confirm the schema deserializes user input safely (no shell
        // commands that run on launch, no binary overrides). The `hooks`
        // section in particular must NOT be API-writable because global
        // hooks bypass the trust prompt that gates repo hooks.
        let expected: &[&str] = &[
            "theme", "session", "tmux", "updates", "sound", "sandbox", "worktree",
            // web: audited 2026-04-24. WebConfig has 4 boolean fields
            // (notifications_enabled, notify_on_waiting, notify_on_idle,
            // notify_on_error). No shell commands, no binary paths.
            "web",
        ];
        assert_eq!(
            ALLOWED_SETTINGS_SECTIONS.len(),
            expected.len(),
            "ALLOWED_SETTINGS_SECTIONS size changed — adding a section widens \
             the API write surface and must be reviewed as a security change. \
             In particular, do NOT add 'hooks' without auditing the RCE surface."
        );
        for section in expected {
            assert!(
                ALLOWED_SETTINGS_SECTIONS.contains(section),
                "ALLOWED_SETTINGS_SECTIONS lost section {:?}",
                section
            );
        }
        // Explicitly guard against accidental hooks re-addition.
        assert!(
            !ALLOWED_SETTINGS_SECTIONS.contains(&"hooks"),
            "hooks must not be API-writable: global/profile hooks bypass the \
             repo-hook trust prompt and run arbitrary shell commands on session \
             start (local RCE)"
        );
    }

    #[test]
    fn profile_name_rejects_path_traversal() {
        assert!(validate_profile_name("../etc").is_err());
        assert!(validate_profile_name("foo/bar").is_err());
        assert!(validate_profile_name("..").is_err());
        assert!(validate_profile_name(".hidden").is_err());
        assert!(validate_profile_name("").is_err());
        assert!(validate_profile_name(&"a".repeat(65)).is_err());
    }

    #[test]
    fn profile_name_accepts_valid_names() {
        assert!(validate_profile_name("default").is_ok());
        assert!(validate_profile_name("work").is_ok());
        assert!(validate_profile_name("my-profile").is_ok());
        assert!(validate_profile_name("profile_2").is_ok());
        assert!(validate_profile_name("A").is_ok());
    }

    #[test]
    fn session_blocked_fields_are_pinned() {
        // These fields let an API caller swap the agent binary,
        // append arbitrary argv, inject environment variables, or define
        // custom agent commands — all command-injection vectors. If Rust
        // renames a field it must be renamed here in the same commit.
        let expected: &[&str] = &[
            "agent_command_override",
            "agent_extra_args",
            "extra_env",
            // custom_agents: maps agent names to arbitrary shell commands
            "custom_agents",
            // agent_detect_as: part of the agent config surface
            "agent_detect_as",
        ];
        assert_eq!(
            SESSION_BLOCKED_FIELDS.len(),
            expected.len(),
            "SESSION_BLOCKED_FIELDS size changed — this is the blocklist \
             that strips attacker-supplied command-injection vectors from \
             incoming /api/settings session objects. Changes must be \
             reviewed as a security change."
        );
        for field in expected {
            assert!(
                SESSION_BLOCKED_FIELDS.contains(field),
                "SESSION_BLOCKED_FIELDS lost field {:?}",
                field
            );
        }
    }
}
