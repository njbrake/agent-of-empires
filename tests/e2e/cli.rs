use serial_test::serial;

use crate::harness::TuiTestHarness;

#[test]
#[serial]
fn test_cli_add_and_list() {
    let h = TuiTestHarness::new("cli_add_list");
    let project = h.project_path();

    let add_output = h.run_cli(&["add", project.to_str().unwrap(), "-t", "E2E Test Session"]);
    assert!(
        add_output.status.success(),
        "aoe add failed: {}",
        String::from_utf8_lossy(&add_output.stderr)
    );

    let list_output = h.run_cli(&["list"]);
    assert!(
        list_output.status.success(),
        "aoe list failed: {}",
        String::from_utf8_lossy(&list_output.stderr)
    );

    let stdout = String::from_utf8_lossy(&list_output.stdout);
    assert!(
        stdout.contains("E2E Test Session"),
        "list output should contain session title.\nOutput:\n{}",
        stdout
    );
}

#[test]
#[serial]
fn test_cli_add_invalid_path() {
    let h = TuiTestHarness::new("cli_add_invalid");

    let output = h.run_cli(&["add", "/nonexistent/path/that/does/not/exist"]);
    assert!(
        !output.status.success(),
        "aoe add should fail for nonexistent path"
    );

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let combined = format!("{}{}", stdout, stderr);
    assert!(
        combined.contains("not")
            || combined.contains("exist")
            || combined.contains("error")
            || combined.contains("Error")
            || combined.contains("invalid")
            || combined.contains("No such"),
        "expected error message about invalid path.\nstdout: {}\nstderr: {}",
        stdout,
        stderr
    );
}
