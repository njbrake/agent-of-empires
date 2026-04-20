//! Integration tests for profile management: create, delete, list, default, rename, and isolation.

use agent_of_empires::session::{
    create_profile, delete_profile, list_profiles, rename_profile, set_default_profile, Config,
    Instance, Storage,
};
use anyhow::Result;
use serial_test::serial;

fn setup_temp_home() -> tempfile::TempDir {
    let temp = tempfile::TempDir::new().unwrap();
    std::env::set_var("HOME", temp.path());
    #[cfg(target_os = "linux")]
    std::env::set_var("XDG_CONFIG_HOME", temp.path().join(".config"));
    temp
}

#[test]
#[serial]
fn test_create_profile() -> Result<()> {
    let _temp = setup_temp_home();

    create_profile("work")?;

    let profiles = list_profiles()?;
    assert!(profiles.contains(&"work".to_string()));

    Ok(())
}

#[test]
#[serial]
fn test_list_profiles_includes_default() -> Result<()> {
    let _temp = setup_temp_home();

    // Trigger creation of default profile dir by accessing it
    let _ = Storage::new("default")?;

    let profiles = list_profiles()?;
    assert!(profiles.contains(&"default".to_string()));

    Ok(())
}

#[test]
#[serial]
fn test_delete_profile() -> Result<()> {
    let _temp = setup_temp_home();

    create_profile("temporary")?;
    assert!(list_profiles()?.contains(&"temporary".to_string()));

    delete_profile("temporary")?;
    assert!(!list_profiles()?.contains(&"temporary".to_string()));

    Ok(())
}

#[test]
#[serial]
fn test_cannot_delete_default_profile() -> Result<()> {
    let _temp = setup_temp_home();

    let result = delete_profile("default");
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Cannot delete the default profile"));

    Ok(())
}

#[test]
#[serial]
fn test_set_default_profile() -> Result<()> {
    let _temp = setup_temp_home();

    create_profile("work")?;
    set_default_profile("work")?;

    let config = Config::load()?;
    assert_eq!(config.default_profile, "work");

    Ok(())
}

#[test]
#[serial]
fn test_profile_session_isolation() -> Result<()> {
    let _temp = setup_temp_home();

    // Create two profiles
    create_profile("alpha")?;
    create_profile("beta")?;

    // Save a session in profile alpha
    let storage_a = Storage::new("alpha")?;
    let instance = Instance::new("Alpha Session", "/path/alpha");
    storage_a.save(&[instance])?;

    // Load from profile beta - should be empty
    let storage_b = Storage::new("beta")?;
    let loaded = storage_b.load()?;
    assert!(loaded.is_empty(), "Profile beta should have no sessions");

    // Verify alpha still has its session
    let loaded_a = storage_a.load()?;
    assert_eq!(loaded_a.len(), 1);
    assert_eq!(loaded_a[0].title, "Alpha Session");

    Ok(())
}

#[test]
#[serial]
fn test_rename_profile() -> Result<()> {
    let _temp = setup_temp_home();

    create_profile("old_name")?;
    // Add a session so we can verify data moves with the rename
    let storage = Storage::new("old_name")?;
    let instance = Instance::new("Test Session", "/path/test");
    storage.save(&[instance])?;

    rename_profile("old_name", "new_name")?;

    let profiles = list_profiles()?;
    assert!(!profiles.contains(&"old_name".to_string()));
    assert!(profiles.contains(&"new_name".to_string()));

    // Verify sessions moved with the profile
    let new_storage = Storage::new("new_name")?;
    let sessions = new_storage.load()?;
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0].title, "Test Session");

    Ok(())
}

#[test]
#[serial]
fn test_rename_profile_updates_default() -> Result<()> {
    let _temp = setup_temp_home();

    create_profile("primary")?;
    set_default_profile("primary")?;

    rename_profile("primary", "renamed")?;

    let config = Config::load()?;
    assert_eq!(config.default_profile, "renamed");

    Ok(())
}

#[test]
#[serial]
fn test_rename_profile_nondefault_keeps_default() -> Result<()> {
    let _temp = setup_temp_home();

    create_profile("main_profile")?;
    create_profile("other")?;
    set_default_profile("main_profile")?;

    rename_profile("other", "renamed_other")?;

    let config = Config::load()?;
    assert_eq!(config.default_profile, "main_profile");

    Ok(())
}

#[test]
#[serial]
fn test_rename_profile_to_existing_fails() -> Result<()> {
    let _temp = setup_temp_home();

    create_profile("first")?;
    create_profile("second")?;

    let result = rename_profile("first", "second");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("already exists"));

    Ok(())
}

#[test]
#[serial]
fn test_rename_nonexistent_profile_fails() -> Result<()> {
    let _temp = setup_temp_home();

    let result = rename_profile("nonexistent", "new_name");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("does not exist"));

    Ok(())
}

#[test]
#[serial]
fn test_rename_profile_empty_name_fails() -> Result<()> {
    let _temp = setup_temp_home();

    create_profile("valid")?;

    let result = rename_profile("valid", "");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("cannot be empty"));

    Ok(())
}

#[test]
#[serial]
fn test_rename_profile_with_path_separator_fails() -> Result<()> {
    let _temp = setup_temp_home();

    create_profile("valid")?;

    let result = rename_profile("valid", "bad/name");
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("path separators"));

    Ok(())
}

#[test]
#[serial]
fn test_profile_config_isolation() -> Result<()> {
    use agent_of_empires::session::{
        load_profile_config, save_profile_config, ProfileConfig, UpdatesConfigOverride,
    };

    let _temp = setup_temp_home();

    create_profile("custom")?;

    // Save a profile-specific config override for "custom"
    let custom_config = ProfileConfig {
        updates: Some(UpdatesConfigOverride {
            check_enabled: Some(false),
            ..Default::default()
        }),
        ..Default::default()
    };
    save_profile_config("custom", &custom_config)?;

    // Load config for "default" profile - should have no overrides
    let default_config = load_profile_config("default")?;
    assert!(
        default_config.updates.is_none(),
        "Default profile should have no update overrides"
    );

    // Verify custom profile has its override
    let loaded_custom = load_profile_config("custom")?;
    assert_eq!(
        loaded_custom.updates.unwrap().check_enabled,
        Some(false),
        "Custom profile should have check_enabled = false"
    );

    Ok(())
}
