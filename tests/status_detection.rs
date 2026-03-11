//! Golden tests for status detection
//!
//! These tests verify that status detection works correctly against real
//! terminal captures. When a tool updates their TUI, these tests will fail
//! if the detection logic no longer works.
//!
//! Note: Claude Code, Cursor, and OpenCode use hook-based detection (not tmux
//! pane parsing), so they have no fixture-based tests here.
//!
//! Each state is a directory containing one or more fixture files. This allows
//! users to submit additional screenshots for bug reports, and all examples
//! will be tested to ensure correct detection.
//!
//! To add fixtures after a bug report or tool update:
//! 1. Run: scripts/capture-fixtures.sh <tool> <state> <tmux_session> [description]
//! 2. Verify the new captures look correct
//! 3. Update detection logic if needed
//! 4. Re-run tests

// Suppress unused warnings: these helpers are infrastructure for fixture-based
// tests. Agents using tmux pane parsing should add test modules below that
// call test_all_fixtures_in_dir().
#![allow(dead_code)]

use agent_of_empires::session::Status;
use std::fs;
use std::path::PathBuf;

fn fixtures_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
}

fn strip_fixture_header(content: &str) -> String {
    content
        .lines()
        .filter(|line| !line.starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
}

fn test_all_fixtures_in_dir<F>(
    tool: &str,
    state: &str,
    expected: Status,
    preprocess: fn(String) -> String,
    detect_fn: F,
) where
    F: Fn(&str) -> Status,
{
    let dir = fixtures_path().join(tool).join(state);

    let entries: Vec<_> = fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("Failed to read fixture directory {:?}: {}", dir, e))
        .filter_map(|entry| entry.ok())
        .filter(|entry| {
            entry
                .path()
                .extension()
                .map(|ext| ext == "txt")
                .unwrap_or(false)
        })
        .collect();

    assert!(
        !entries.is_empty(),
        "No fixture files found in {:?}. Add at least one .txt fixture file.",
        dir
    );

    for entry in entries {
        let path = entry.path();
        let raw_content = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read fixture {:?}: {}", path, e));
        let content = preprocess(strip_fixture_header(&raw_content));
        let status = detect_fn(&content);

        assert_eq!(
            status,
            expected,
            "Fixture {:?} should detect as {:?}, but got {:?}.\n\
             Fixture content:\n{}\n\n\
             If the tool changed their TUI, update the detection logic in src/tmux/session.rs",
            path.file_name().unwrap(),
            expected,
            status,
            content
        );
    }
}

fn _identity(s: String) -> String {
    s
}

// Hook-based agents (Claude, Cursor, OpenCode) have no fixture tests.
// Agents that use tmux pane parsing should add fixture-based test modules here.
// Example:
//   mod some_agent {
//       use super::*;
//       fn detect(content: &str) -> Status { ... }
//       #[test] fn test_running() { test_all_fixtures_in_dir(...) }
//   }
