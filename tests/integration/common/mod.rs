//! Shared helpers for integration tests.
//!
//! Declared once from `tests/integration/main.rs`; consumers import via
//! `use crate::common::...`.

use std::path::Path;
use tempfile::TempDir;

/// Set `HOME` (and `XDG_CONFIG_HOME` on Linux) to a fresh temp dir so tests
/// read and write to isolated state. Returns the guard; drop it to clean up.
///
/// # Safety caveat
/// `set_var` is not thread-safe. Callers must be `#[serial]`.
pub fn setup_temp_home() -> TempDir {
    let temp = TempDir::new().unwrap();
    set_temp_home(temp.path());
    temp
}

/// Variant for tests that already own a `TempDir` (e.g. ones that also seed
/// files under the same path before returning the guard).
pub fn set_temp_home(path: &Path) {
    std::env::set_var("HOME", path);
    #[cfg(target_os = "linux")]
    std::env::set_var("XDG_CONFIG_HOME", path.join(".config"));
}
