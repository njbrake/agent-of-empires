use serial_test::serial;

use crate::harness::TuiTestHarness;

#[test]
#[serial]
fn test_cli_remove_nonexistent() {
    let h = TuiTestHarness::new("cli_rm_noexist");

    let output = h.run_cli(&["remove", "nonexistent-session-id-12345"]);
    assert!(
        !output.status.success(),
        "aoe remove should fail for nonexistent session"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, stderr);
    assert!(
        combined.contains("not found")
            || combined.contains("No session")
            || combined.contains("error")
            || combined.contains("Error"),
        "expected error message about missing session.\nstdout: {}\nstderr: {}",
        stdout,
        stderr
    );
}
