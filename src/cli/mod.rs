//! CLI command implementations

pub mod add;
pub mod agents;
pub mod clone;
pub mod definition;
pub mod group;
pub mod init;
pub mod list;
pub mod output;
pub mod profile;
pub mod remove;
pub mod send;
pub mod template;
#[cfg(feature = "serve")]
pub mod serve;
pub mod session;
pub mod sounds;
pub mod status;
pub mod theme;
pub mod tmux;
pub mod uninstall;
pub mod update;
pub mod worktree;

pub use definition::{Cli, Commands};

use crate::session::Instance;
use anyhow::{bail, Result};

pub fn resolve_session<'a>(identifier: &str, instances: &'a [Instance]) -> Result<&'a Instance> {
    // Try exact ID match
    if let Some(inst) = instances.iter().find(|i| i.id == identifier) {
        return Ok(inst);
    }

    // Try ID prefix match
    if let Some(inst) = instances.iter().find(|i| i.id.starts_with(identifier)) {
        return Ok(inst);
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
    let char_count = id.chars().count();
    if char_count <= max_len {
        id
    } else {
        let byte_pos = id.char_indices().nth(max_len).map(|(i, _)| i).unwrap_or(id.len());
        &id[..byte_pos]
    }
}
