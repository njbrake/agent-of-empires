//! Shared helpers for integration tests.
//!
//! Files in `tests/` are compiled as separate integration-test crates, so this
//! module lives under `tests/common/` (not `tests/common.rs`) to avoid being
//! compiled into its own binary. Each consumer declares `mod common;` at the
//! top of its test file.

use std::path::Path;
use tempfile::TempDir;

/// Set `HOME` (and `XDG_CONFIG_HOME` on Linux) to a fresh temp dir so tests
/// read and write to isolated state. Returns the guard; drop it to clean up.
///
/// # Safety caveat
/// `set_var` is not thread-safe. Callers must be `#[serial]`.
#[allow(dead_code)]
pub fn setup_temp_home() -> TempDir {
    let temp = TempDir::new().unwrap();
    set_temp_home(temp.path());
    temp
}

/// Variant for tests that already own a `TempDir` (e.g. ones that also seed
/// files under the same path before returning the guard).
#[allow(dead_code)]
pub fn set_temp_home(path: &Path) {
    std::env::set_var("HOME", path);
    #[cfg(target_os = "linux")]
    std::env::set_var("XDG_CONFIG_HOME", path.join(".config"));
}
