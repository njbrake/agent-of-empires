//! Integration tests for the migration pipeline: execution, versioning, and idempotency.

use anyhow::Result;
use serial_test::serial;
use std::fs;

fn setup_temp_home() -> tempfile::TempDir {
    let temp = tempfile::TempDir::new().unwrap();
    std::env::set_var("HOME", temp.path());
    #[cfg(target_os = "linux")]
    std::env::set_var("XDG_CONFIG_HOME", temp.path().join(".config"));
    temp
}

fn get_schema_version_path() -> Result<std::path::PathBuf> {
    let app_dir = agent_of_empires::session::get_app_dir()?;
    Ok(app_dir.join(".schema_version"))
}

#[test]
#[serial]
fn test_fresh_dir_runs_all_migrations() -> Result<()> {
    let _temp = setup_temp_home();

    // Ensure no schema version file exists
    let version_path = get_schema_version_path()?;
    assert!(!version_path.exists());

    agent_of_empires::migrations::run_migrations()?;

    // After migrations, .schema_version should exist and be at the current version
    assert!(version_path.exists());
    let version: u32 = fs::read_to_string(&version_path)?.trim().parse()?;
    assert!(version >= 1, "Schema version should be at least 1");

    Ok(())
}

#[test]
#[serial]
fn test_up_to_date_is_noop() -> Result<()> {
    let _temp = setup_temp_home();

    // Run migrations to get to current version
    agent_of_empires::migrations::run_migrations()?;

    let version_path = get_schema_version_path()?;
    let version_before = fs::read_to_string(&version_path)?;

    // Run again - should be a no-op
    agent_of_empires::migrations::run_migrations()?;

    let version_after = fs::read_to_string(&version_path)?;
    assert_eq!(version_before, version_after, "Version should not change");

    Ok(())
}

#[test]
#[serial]
fn test_idempotent_double_run() -> Result<()> {
    let _temp = setup_temp_home();

    // Run migrations twice from fresh state
    agent_of_empires::migrations::run_migrations()?;
    agent_of_empires::migrations::run_migrations()?;

    let version_path = get_schema_version_path()?;
    let version: u32 = fs::read_to_string(&version_path)?.trim().parse()?;
    assert!(version >= 1);

    Ok(())
}

#[test]
#[serial]
fn test_partial_version_runs_remaining() -> Result<()> {
    let _temp = setup_temp_home();

    // Manually set schema version to 0 (as if no migrations have run but file exists)
    let app_dir = agent_of_empires::session::get_app_dir()?;
    let version_path = app_dir.join(".schema_version");
    fs::write(&version_path, "0")?;

    agent_of_empires::migrations::run_migrations()?;

    let version: u32 = fs::read_to_string(&version_path)?.trim().parse()?;
    assert!(version >= 1, "Should have run migrations beyond version 0");

    Ok(())
}
