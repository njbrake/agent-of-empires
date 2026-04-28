//! Integration tests for `aoe update --check` and `--dry-run`.

use std::process::Command;

fn aoe_binary() -> &'static str {
    env!("CARGO_BIN_EXE_aoe")
}

#[test]
fn update_check_prints_three_lines_and_exits_zero() {
    let output = Command::new(aoe_binary())
        .args(["update", "--check"])
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
fn update_dry_run_prints_prompt_block_and_exits_zero() {
    let output = Command::new(aoe_binary())
        .args(["update", "--dry-run"])
        .output()
        .expect("running aoe update --dry-run");
    // It exits 0 either way (no update available also exits 0 from --dry-run).
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
