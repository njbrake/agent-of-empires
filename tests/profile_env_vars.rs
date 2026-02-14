//! Integration tests for profile environment variables
//!
//! These tests verify that profile environment variables work correctly
//! across the full stack: env var resolution, and basic functionality.

use agent_of_empires::session::Storage;
use anyhow::Result;
use serial_test::serial;
use std::collections::HashMap;

fn setup_temp_home() -> tempfile::TempDir {
    let temp = tempfile::TempDir::new().unwrap();
    std::env::set_var("HOME", temp.path());
    #[cfg(target_os = "linux")]
    std::env::set_var("XDG_CONFIG_HOME", temp.path().join(".config"));
    temp
}

#[test]
#[serial]
fn test_env_var_resolution_with_host_vars() -> Result<()> {
    let _temp = setup_temp_home();

    std::env::set_var("TEST_HOST_VAR", "from_host_env");

    let mut env_values = HashMap::new();
    env_values.insert("CONFIG_VAR".to_string(), "literal_value".to_string());

    let resolved = agent_of_empires::session::config::resolve_env_vars(&[], &env_values);

    assert!(resolved.contains_key("CONFIG_VAR"));

    std::env::remove_var("TEST_HOST_VAR");

    Ok(())
}

#[test]
#[serial]
fn test_env_var_resolution_with_expansion() -> Result<()> {
    let _temp = setup_temp_home();

    std::env::set_var("TEST_EXPAND", "expanded_value");

    let mut env_values = HashMap::new();
    env_values.insert("KEY".to_string(), "$TEST_EXPAND".to_string());

    let resolved = agent_of_empires::session::config::resolve_env_vars(&[], &env_values);

    assert_eq!(resolved.get("KEY"), Some(&"expanded_value".to_string()));

    std::env::remove_var("TEST_EXPAND");

    Ok(())
}

#[test]
#[serial]
fn test_dollar_escape_in_env_vars() -> Result<()> {
    let _temp = setup_temp_home();

    let mut env_values = HashMap::new();
    env_values.insert("LITERAL".to_string(), "$$HOME".to_string());

    let resolved = agent_of_empires::session::config::resolve_env_vars(&[], &env_values);

    assert_eq!(resolved.get("LITERAL"), Some(&"$HOME".to_string()));

    Ok(())
}

#[test]
#[serial]
fn test_env_var_override_behavior() -> Result<()> {
    let _temp = setup_temp_home();

    std::env::set_var("OVERRIDE_TEST", "from_environment");

    let env_keys = vec!["OVERRIDE_TEST".to_string()];
    let mut env_values = HashMap::new();
    env_values.insert("OVERRIDE_TEST".to_string(), "from_config".to_string());

    let resolved = agent_of_empires::session::config::resolve_env_vars(&env_keys, &env_values);

    // Config values should override environment keys
    assert_eq!(
        resolved.get("OVERRIDE_TEST"),
        Some(&"from_config".to_string())
    );

    std::env::remove_var("OVERRIDE_TEST");

    Ok(())
}

#[test]
#[serial]
fn test_session_persistence_with_sandbox_info() -> Result<()> {
    let _temp = setup_temp_home();

    let storage = Storage::new("default")?;

    // Create session with sandbox config
    let mut inst = agent_of_empires::session::Instance::new("Sandbox Test", "/tmp/test", "default");
    inst.sandbox_info = Some(agent_of_empires::session::SandboxInfo {
        enabled: true,
        container_id: None,
        image: "test:latest".to_string(),
        container_name: "test-container".to_string(),
        created_at: None,
        yolo_mode: None,
        extra_env_keys: Some(vec!["SANDBOX_VAR".to_string()]),
        extra_env_values: None,
    });

    storage.save(std::slice::from_ref(&inst))?;
    let loaded = storage.load()?;

    assert_eq!(loaded.len(), 1);
    assert!(loaded[0].sandbox_info.is_some());
    assert_eq!(
        loaded[0].sandbox_info.as_ref().unwrap().extra_env_keys,
        Some(vec!["SANDBOX_VAR".to_string()])
    );

    Ok(())
}
