use serial_test::serial;

use crate::harness::TuiTestHarness;

/// Helper to read a session field from the sessions.json in the harness's isolated home.
fn read_sessions_json(h: &TuiTestHarness) -> serde_json::Value {
    let sessions_path = if cfg!(target_os = "linux") {
        h.home_path()
            .join(".config/agent-of-empires/profiles/default/sessions.json")
    } else {
        h.home_path()
            .join(".agent-of-empires/profiles/default/sessions.json")
    };
    let content = std::fs::read_to_string(&sessions_path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", sessions_path.display(), e));
    serde_json::from_str(&content).expect("invalid sessions JSON")
}

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

#[test]
#[serial]
fn test_cli_add_respects_config_extra_args() {
    let h = TuiTestHarness::new("cli_add_config_extra_args");
    let project = h.project_path();

    // Write config with agent_extra_args for claude
    let config_dir = if cfg!(target_os = "linux") {
        h.home_path().join(".config/agent-of-empires")
    } else {
        h.home_path().join(".agent-of-empires")
    };
    let config_content = format!(
        r#"[updates]
check_enabled = false

[app_state]
has_seen_welcome = true
last_seen_version = "{}"

[session]
agent_extra_args = {{ claude = "--verbose --debug" }}
"#,
        env!("CARGO_PKG_VERSION")
    );
    std::fs::write(config_dir.join("config.toml"), config_content).expect("write config.toml");

    let add_output = h.run_cli(&["add", project.to_str().unwrap(), "-t", "ConfigExtraArgs"]);
    assert!(
        add_output.status.success(),
        "aoe add failed: {}",
        String::from_utf8_lossy(&add_output.stderr)
    );

    let sessions = read_sessions_json(&h);
    let session = &sessions[0];
    assert_eq!(
        session["extra_args"].as_str().unwrap_or(""),
        "--verbose --debug",
        "extra_args should be populated from config"
    );
}

#[test]
#[serial]
fn test_cli_add_respects_config_command_override() {
    let h = TuiTestHarness::new("cli_add_config_cmd_override");
    let project = h.project_path();

    // Write config with agent_command_override for claude
    let config_dir = if cfg!(target_os = "linux") {
        h.home_path().join(".config/agent-of-empires")
    } else {
        h.home_path().join(".agent-of-empires")
    };
    let config_content = format!(
        r#"[updates]
check_enabled = false

[app_state]
has_seen_welcome = true
last_seen_version = "{}"

[session]
agent_command_override = {{ claude = "my-custom-claude" }}
"#,
        env!("CARGO_PKG_VERSION")
    );
    std::fs::write(config_dir.join("config.toml"), config_content).expect("write config.toml");

    let add_output = h.run_cli(&["add", project.to_str().unwrap(), "-t", "ConfigCmdOverride"]);
    assert!(
        add_output.status.success(),
        "aoe add failed: {}",
        String::from_utf8_lossy(&add_output.stderr)
    );

    let sessions = read_sessions_json(&h);
    let session = &sessions[0];
    assert_eq!(
        session["command"].as_str().unwrap_or(""),
        "my-custom-claude",
        "command should be populated from config agent_command_override"
    );
}

#[test]
#[serial]
fn test_cli_add_cli_flags_override_config() {
    let h = TuiTestHarness::new("cli_add_flags_override");
    let project = h.project_path();

    // Write config with agent_extra_args for claude
    let config_dir = if cfg!(target_os = "linux") {
        h.home_path().join(".config/agent-of-empires")
    } else {
        h.home_path().join(".agent-of-empires")
    };
    let config_content = format!(
        r#"[updates]
check_enabled = false

[app_state]
has_seen_welcome = true
last_seen_version = "{}"

[session]
agent_extra_args = {{ claude = "--from-config" }}
agent_command_override = {{ claude = "config-claude" }}
"#,
        env!("CARGO_PKG_VERSION")
    );
    std::fs::write(config_dir.join("config.toml"), config_content).expect("write config.toml");

    // CLI flags should take priority over config
    let add_output = h.run_cli(&[
        "add",
        project.to_str().unwrap(),
        "-t",
        "FlagsOverride",
        "--extra-args",
        "from-cli-extra",
        "--cmd-override",
        "cli-claude",
    ]);
    assert!(
        add_output.status.success(),
        "aoe add failed: {}",
        String::from_utf8_lossy(&add_output.stderr)
    );

    let sessions = read_sessions_json(&h);
    let session = &sessions[0];
    assert_eq!(
        session["extra_args"].as_str().unwrap_or(""),
        "from-cli-extra",
        "CLI --extra-args should override config"
    );
    assert_eq!(
        session["command"].as_str().unwrap_or(""),
        "cli-claude",
        "CLI --cmd-override should override config"
    );
}
