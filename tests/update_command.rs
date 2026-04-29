//! Smoke tests for `aoe update --check` and `--dry-run`.
//!
//! These hit the real GitHub releases API because both flags pass
//! `force=true` to `check_for_update`, which bypasses the on-disk cache.
//! That makes them flaky on CI runners that share IPs with many other
//! jobs (the unauthenticated GitHub API limit is 60 req/hr/IP and
//! macos-latest runners regularly hit it). They're marked `#[ignore]`
//! so CI doesn't gate on them; run locally with:
//!
//!     cargo test --test update_command -- --ignored
//!
//! The actual update flow is covered hermetically by
//! `tests/e2e/update_command.rs::update_via_tarball_replaces_binary_at_target_path`,
//! which spins up a local axum fixture server and exercises the full
//! download → verify → atomic-replace path without touching GitHub.

use std::process::Command;

fn aoe_binary() -> &'static str {
    env!("CARGO_BIN_EXE_aoe")
}

#[test]
#[ignore = "hits real GitHub API; rate-limited on shared CI runners"]
fn update_check_prints_three_lines_and_exits_zero() {
    let tmp = tempfile::TempDir::new().unwrap();
    let output = Command::new(aoe_binary())
        .args(["update", "--check"])
        .env("HOME", tmp.path())
        .env("XDG_CONFIG_HOME", tmp.path())
        .output()
        .expect("running aoe update --check");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("current:"), "stdout was: {stdout}");
    assert!(stdout.contains("latest:"), "stdout was: {stdout}");
    assert!(stdout.contains("available:"), "stdout was: {stdout}");
}

#[test]
#[ignore = "hits real GitHub API; rate-limited on shared CI runners"]
fn update_dry_run_prints_prompt_block_and_exits_zero() {
    let tmp = tempfile::TempDir::new().unwrap();
    let output = Command::new(aoe_binary())
        .args(["update", "--dry-run"])
        .env("HOME", tmp.path())
        .env("XDG_CONFIG_HOME", tmp.path())
        .output()
        .expect("running aoe update --dry-run");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    // When no update is available (current == latest), the binary exits
    // with "You're on v... (latest). Nothing to do."
    // When an update is available it prints the prompt block.
    // Either way, the output must be non-empty and well-formed.
    assert!(
        stdout.contains("Nothing to do.") || stdout.contains("Update v"),
        "unexpected dry-run stdout: {stdout}"
    );
}
