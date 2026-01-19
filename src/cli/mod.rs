//! CLI command implementations

pub mod add;
pub mod definition;
pub mod group;
pub mod list;
pub mod profile;
pub mod remove;
pub mod session;
pub mod status;
pub mod tmux;
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

#[cfg(test)]
mod tests {
    use super::*;

    // Tests for truncate function
    #[test]
    fn test_truncate_shorter_than_max() {
        assert_eq!(truncate("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_equal_to_max() {
        assert_eq!(truncate("hello", 5), "hello");
    }

    #[test]
    fn test_truncate_longer_than_max() {
        assert_eq!(truncate("hello world", 8), "hello...");
    }

    #[test]
    fn test_truncate_with_small_max() {
        assert_eq!(truncate("hello", 3), "hel");
        assert_eq!(truncate("hello", 2), "he");
        assert_eq!(truncate("hello", 1), "h");
    }

    #[test]
    fn test_truncate_empty_string() {
        assert_eq!(truncate("", 5), "");
    }

    #[test]
    fn test_truncate_zero_max() {
        assert_eq!(truncate("hello", 0), "");
    }

    // Tests for truncate_id function
    #[test]
    fn test_truncate_id_shorter_than_max() {
        assert_eq!(truncate_id("abc123", 10), "abc123");
    }

    #[test]
    fn test_truncate_id_equal_to_max() {
        assert_eq!(truncate_id("abc123", 6), "abc123");
    }

    #[test]
    fn test_truncate_id_longer_than_max() {
        assert_eq!(truncate_id("abc123def456", 8), "abc123de");
    }

    #[test]
    fn test_truncate_id_empty_string() {
        assert_eq!(truncate_id("", 5), "");
    }

    // Tests for resolve_session function
    #[test]
    fn test_resolve_session_by_exact_id() {
        let instances = vec![
            Instance::new("Test 1", "/path/one"),
            Instance::new("Test 2", "/path/two"),
        ];
        let result = resolve_session(&instances[0].id, &instances);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().title, "Test 1");
    }

    #[test]
    fn test_resolve_session_by_id_prefix() {
        let instances = vec![Instance::new("Test Session", "/path/test")];
        let id_prefix = &instances[0].id[..8];
        let result = resolve_session(id_prefix, &instances);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().title, "Test Session");
    }

    #[test]
    fn test_resolve_session_by_exact_title() {
        let instances = vec![
            Instance::new("My Project", "/path/one"),
            Instance::new("Another Project", "/path/two"),
        ];
        let result = resolve_session("My Project", &instances);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().project_path, "/path/one");
    }

    #[test]
    fn test_resolve_session_by_path() {
        let instances = vec![
            Instance::new("Test", "/home/user/project"),
            Instance::new("Other", "/home/user/other"),
        ];
        let result = resolve_session("/home/user/project", &instances);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().title, "Test");
    }

    #[test]
    fn test_resolve_session_not_found() {
        let instances = vec![Instance::new("Test", "/path/test")];
        let result = resolve_session("nonexistent", &instances);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Session not found"));
    }

    #[test]
    fn test_resolve_session_empty_list() {
        let instances: Vec<Instance> = vec![];
        let result = resolve_session("anything", &instances);
        assert!(result.is_err());
    }

    #[test]
    fn test_resolve_session_prefers_exact_id_over_title() {
        let mut instances = vec![
            Instance::new("abc123", "/path/one"), // title matches an id pattern
            Instance::new("Test", "/path/two"),
        ];
        // Manually set the second instance's ID to match the first's title
        instances[1].id = "abc123def456ghij".to_string();

        // Should find by exact ID first
        let result = resolve_session("abc123def456ghij", &instances);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().title, "Test");
    }
}
