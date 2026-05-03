//! Session templates module
//!
//! Templates provide pre-configured settings for common agent workflows.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

use crate::session::get_app_dir;

/// A template for creating sessions with pre-configured settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionTemplate {
    /// Unique name of the template.
    pub name: String,
    /// Human-readable description.
    pub description: String,
    /// Command to run (e.g., "claude", "cursor").
    pub cmd: Option<String>,
    /// Environment variables to set.
    pub env_vars: Option<HashMap<String, String>>,
    // Future: worktree_enabled, sandbox_enabled, etc.
}

impl SessionTemplate {
    /// Create a new template.
    pub fn new(name: String, description: String) -> Self {
        Self {
            name,
            description,
            cmd: None,
            env_vars: None,
        }
    }
}

/// Default built-in templates.
pub const DEFAULT_TEMPLATES: &[SessionTemplate] = &[
    SessionTemplate {
        name: "debugging".to_string(),
        description: "Template for debugging sessions with verbose output".to_string(),
        cmd: Some("claude".to_string()),
        env_vars: Some({
            let mut map = HashMap::new();
            map.insert("DEBUG".to_string(), "1".to_string());
            map
        }),
    },
    SessionTemplate {
        name: "refactoring".to_string(),
        description: "Template for code refactoring with focused environment".to_string(),
        cmd: Some("claude".to_string()),
        env_vars: None,
    },
    SessionTemplate {
        name: "documentation".to_string(),
        description: "Template for writing documentation".to_string(),
        cmd: Some("claude".to_string()),
        env_vars: None,
    },
];

/// Get the path to the templates file.
fn templates_path() -> Result<PathBuf> {
    Ok(get_app_dir()?.join("templates.toml"))
}

/// Load templates from disk, or return defaults if no file exists.
pub fn load_templates() -> Result<Vec<SessionTemplate>> {
    let path = templates_path()?;
    if !path.exists() {
        return Ok(DEFAULT_TEMPLATES.to_vec());
    }

    let content = fs::read_to_string(&path)?;
    let templates: Vec<SessionTemplate> = toml::from_str(&content)?;
    Ok(templates)
}

/// Save templates to disk.
pub fn save_templates(templates: &[SessionTemplate]) -> Result<()> {
    let path = templates_path()?;
    let content = toml::to_string_pretty(templates)?;
    fs::write(&path, content)?;
    Ok(())
}

/// Find a template by name.
pub fn find_template(templates: &[SessionTemplate], name: &str) -> Option<&SessionTemplate> {
    templates.iter().find(|t| t.name == name)
}

/// Add or update a template.
pub fn upsert_template(mut templates: Vec<SessionTemplate>, template: SessionTemplate) -> Vec<SessionTemplate> {
    if let Some(pos) = templates.iter().position(|t| t.name == template.name) {
        templates[pos] = template;
    } else {
        templates.push(template);
    }
    templates
}

/// Remove a template by name.
pub fn remove_template(mut templates: Vec<SessionTemplate>, name: &str) -> Vec<SessionTemplate> {
    templates.retain(|t| t.name != name);
    templates
}