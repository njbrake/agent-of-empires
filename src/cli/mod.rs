//! CLI command implementations

pub mod add;
pub mod agents;
#[cfg(feature = "serve")]
pub mod cockpit;
pub mod definition;
pub mod group;
pub mod init;
pub mod list;
#[cfg(feature = "serve")]
pub mod log_level;
pub mod logs;
pub mod output;
pub mod profile;
pub mod project;
pub mod remove;
pub mod send;
#[cfg(feature = "serve")]
pub mod serve;
pub mod session;
pub mod sounds;
pub mod status;
pub mod theme;
pub mod tmux;
pub mod uninstall;
pub mod update;
#[cfg(feature = "serve")]
pub mod url;
pub mod worktree;

pub use definition::{Cli, Commands};

use crate::session::Instance;
use anyhow::{bail, Result};

pub fn resolve_session<'a>(identifier: &str, instances: &'a [Instance]) -> Result<&'a Instance> {
    // Try exact ID match. Exact matches always win over prefix matches and
    // can never be ambiguous (IDs are unique).
    if let Some(inst) = instances.iter().find(|i| i.id == identifier) {
        return Ok(inst);
    }

    // Try ID prefix match. If more than one session has an ID starting with
    // `identifier`, fail loudly instead of silently mutating the first one.
    // Mutating commands (archive, kill, snooze) could otherwise act on the
    // wrong session when the user provides a too-short prefix.
    let prefix_matches: Vec<&Instance> = instances
        .iter()
        .filter(|i| i.id.starts_with(identifier))
        .collect();
    match prefix_matches.len() {
        0 => {}
        1 => return Ok(prefix_matches[0]),
        _ => {
            let mut candidates: Vec<String> = prefix_matches
                .iter()
                .map(|i| format!("  {} ({})", i.id, i.title))
                .collect();
            candidates.sort();
            bail!(
                "Ambiguous session identifier {:?} matches {} sessions:\n{}\nUse a longer prefix or the full ID.",
                identifier,
                prefix_matches.len(),
                candidates.join("\n")
            );
        }
    }

    // Try exact title match
    if let Some(inst) = instances.iter().find(|i| i.title == identifier) {
        return Ok(inst);
    }

    // Try path match
    if let Some(inst) = instances.iter().find(|i| i.project_path == identifier) {
        return Ok(inst);
    }

    bail!("Session not found: {}", identifier)
}

pub fn truncate(s: &str, max: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max {
        s.to_string()
    } else if max <= 3 {
        s.chars().take(max).collect()
    } else {
        let truncated: String = s.chars().take(max - 3).collect();
        format!("{}...", truncated)
    }
}

pub fn truncate_id(id: &str, max_len: usize) -> &str {
    match id.char_indices().nth(max_len) {
        Some((byte_pos, _)) => &id[..byte_pos],
        None => id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_id_shorter_than_max_returns_input() {
        assert_eq!(truncate_id("abc", 8), "abc");
    }

    #[test]
    fn truncate_id_equal_to_max_returns_input() {
        assert_eq!(truncate_id("abcdefgh", 8), "abcdefgh");
    }

    #[test]
    fn truncate_id_ascii_truncates_to_max_chars() {
        assert_eq!(truncate_id("abcdefghij", 8), "abcdefgh");
    }

    #[test]
    fn truncate_id_multibyte_does_not_panic_and_respects_char_boundary() {
        // "café" is 4 chars / 5 bytes. The naive byte-slice version would have
        // panicked on max_len=4 mid-codepoint.
        assert_eq!(truncate_id("café", 3), "caf");
        assert_eq!(truncate_id("café", 4), "café");
        assert_eq!(truncate_id("café", 10), "café");
    }

    #[test]
    fn truncate_id_zero_max_returns_empty() {
        assert_eq!(truncate_id("abc", 0), "");
        assert_eq!(truncate_id("café", 0), "");
    }
}
