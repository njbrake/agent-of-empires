//! E2E coverage for `aoe logs`.
//!
//! Drives the binary via `run_cli` against a fixture log seeded inside the
//! harness's isolated `$HOME` so the real user's logs are never read or
//! touched. We use `--no-pager` everywhere to keep the test deterministic
//! (no interactive viewer launch).

use serial_test::serial;

use crate::harness::{app_dir_in, TuiTestHarness};

/// Write a fake debug.log under the harness's isolated app dir.
fn seed_debug_log(h: &TuiTestHarness, content: &str) -> std::path::PathBuf {
    let app_dir = app_dir_in(h.home_path());
    std::fs::create_dir_all(&app_dir).expect("create app dir");
    let path = app_dir.join("debug.log");
    std::fs::write(&path, content).expect("write debug.log");
    path
}

/// Write a fake serve.log under the harness's isolated app dir.
fn seed_serve_log(h: &TuiTestHarness, content: &str) -> std::path::PathBuf {
    let app_dir = app_dir_in(h.home_path());
    std::fs::create_dir_all(&app_dir).expect("create app dir");
    let path = app_dir.join("serve.log");
    std::fs::write(&path, content).expect("write serve.log");
    path
}

#[test]
#[serial]
fn logs_no_pager_prints_debug_log_to_stdout() {
    let h = TuiTestHarness::new("logs_no_pager");
    seed_debug_log(
        &h,
        "2024-01-01T00:00:01Z  INFO line one\n\
         2024-01-01T00:00:02Z  INFO line two\n",
    );

    let out = h.run_cli(&["logs", "--no-pager"]);
    assert!(
        out.status.success(),
        "aoe logs --no-pager failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("line one"),
        "stdout missing 'line one': {stdout}"
    );
    assert!(
        stdout.contains("line two"),
        "stdout missing 'line two': {stdout}"
    );
}

#[test]
#[serial]
fn logs_lines_returns_only_tail() {
    let h = TuiTestHarness::new("logs_lines");
    seed_debug_log(&h, "a\nb\nc\nd\ne\n");

    let out = h.run_cli(&["logs", "--no-pager", "--lines", "2"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout, "d\ne\n");
}

#[test]
#[serial]
fn logs_path_prints_debug_log_path() {
    let h = TuiTestHarness::new("logs_path");
    let path = seed_debug_log(&h, "");

    let out = h.run_cli(&["logs", "--path"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout.trim(), path.to_string_lossy());
}

#[test]
#[serial]
fn logs_path_all_prints_both_paths() {
    // --all is gated on the `serve` feature; this test runs only when the
    // binary was built with it. `cargo test --features serve` covers it;
    // plain `cargo test` builds without the feature, so skip in that case.
    if !cfg!(feature = "serve") {
        eprintln!("Skipping: built without `serve` feature");
        return;
    }

    let h = TuiTestHarness::new("logs_path_all");
    let dbg = seed_debug_log(&h, "");
    let srv = seed_serve_log(&h, "");

    let out = h.run_cli(&["logs", "--all", "--path"]);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "expected 2 paths, got: {stdout}");
    assert_eq!(lines[0], dbg.to_string_lossy());
    assert_eq!(lines[1], srv.to_string_lossy());
}

#[test]
#[serial]
fn logs_serve_no_pager_prints_serve_log() {
    if !cfg!(feature = "serve") {
        eprintln!("Skipping: built without `serve` feature");
        return;
    }

    let h = TuiTestHarness::new("logs_serve");
    seed_serve_log(&h, "serve daemon line\n");

    let out = h.run_cli(&["logs", "--serve", "--no-pager"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("serve daemon line"));
}

#[test]
#[serial]
fn logs_all_no_pager_merges_streams_with_tags() {
    if !cfg!(feature = "serve") {
        eprintln!("Skipping: built without `serve` feature");
        return;
    }

    let h = TuiTestHarness::new("logs_all_merge");
    seed_debug_log(
        &h,
        "2024-01-01T00:00:01Z  INFO debug-a\n\
         2024-01-01T00:00:03Z  INFO debug-b\n",
    );
    seed_serve_log(
        &h,
        "2024-01-01T00:00:02Z  INFO serve-a\n\
         2024-01-01T00:00:04Z  INFO serve-b\n",
    );

    let out = h.run_cli(&["logs", "--all", "--no-pager"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines,
        vec![
            "[debug] 2024-01-01T00:00:01Z  INFO debug-a",
            "[serve] 2024-01-01T00:00:02Z  INFO serve-a",
            "[debug] 2024-01-01T00:00:03Z  INFO debug-b",
            "[serve] 2024-01-01T00:00:04Z  INFO serve-b",
        ]
    );
}

#[test]
#[serial]
fn logs_missing_debug_log_exits_zero_with_hint() {
    let h = TuiTestHarness::new("logs_missing");
    // Don't seed: app dir + debug.log absent.
    let out = h.run_cli(&["logs", "--no-pager"]);
    assert!(out.status.success(), "should exit 0 when missing");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("does not exist"),
        "stderr should explain missing file: {stderr}"
    );
}

#[test]
#[serial]
fn logs_all_missing_both_files_exits_zero_with_hints() {
    if !cfg!(feature = "serve") {
        eprintln!("Skipping: built without `serve` feature");
        return;
    }

    let h = TuiTestHarness::new("logs_all_missing");
    // Don't seed either log file.
    let out = h.run_cli(&["logs", "--all", "--no-pager"]);
    assert!(
        out.status.success(),
        "should exit 0 when both files missing; stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.matches("does not exist").count() >= 2,
        "stderr should list both missing paths: {stderr}"
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).is_empty(),
        "stdout should be empty when no viewer runs"
    );
}
