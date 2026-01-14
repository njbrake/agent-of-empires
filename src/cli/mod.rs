//! CLI command implementations

pub mod add;
pub mod definition;
pub mod group;
pub mod list;
pub mod profile;
pub mod remove;
pub mod session;
pub mod status;
pub mod uninstall;
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
    if s.len() <= max {
        s.to_string()
    } else if max <= 3 {
        s[..max].to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}

pub fn truncate_id(id: &str, max_len: usize) -> &str {
    if id.len() > max_len {
        &id[..max_len]
    } else {
        id
    }
}
