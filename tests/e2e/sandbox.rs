use serial_test::serial;

use crate::harness::TuiTestHarness;

/// Exercises `aoe add --sandbox` which builds the full container config.
/// This would have caught the duplicate mount points bug (commit 92d2e53).
///
/// Requires a running Docker daemon -- marked `#[ignore]` for CI.
#[test]
#[serial]
#[ignore = "requires Docker daemon"]
fn test_cli_add_with_sandbox() {
    let h = TuiTestHarness::new("cli_sandbox");
    let project = h.project_path();

    let output = h.run_cli(&[
        "add",
        project.to_str().unwrap(),
        "-t",
        "Sandbox E2E",
        "--sandbox",
    ]);
    assert!(
        output.status.success(),
        "aoe add --sandbox failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let list_output = h.run_cli(&["list", "--json"]);
    assert!(list_output.status.success());

    let stdout = String::from_utf8_lossy(&list_output.stdout);
    assert!(
        stdout.contains("Sandbox E2E"),
        "list should contain the sandboxed session.\nOutput:\n{}",
        stdout
    );
}
