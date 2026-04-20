//! Integration tests for the config merge pipeline: global + profile overrides with real TOML files.

use agent_of_empires::session::{
    load_profile_config, merge_configs, save_config, save_profile_config, Config, ProfileConfig,
    SandboxConfigOverride, ThemeConfigOverride, UpdatesConfigOverride, WorktreeConfigOverride,
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
fn test_merge_overrides_global() -> Result<()> {
    let _temp = setup_temp_home();

    // Save global config with sandbox.auto_cleanup = true (default)
    let mut global = Config::default();
    global.sandbox.auto_cleanup = true;
    save_config(&global)?;

    // Save profile override with sandbox.auto_cleanup = false
    let profile = ProfileConfig {
        sandbox: Some(SandboxConfigOverride {
            auto_cleanup: Some(false),
            ..Default::default()
        }),
        ..Default::default()
    };
    save_profile_config("default", &profile)?;

    // Load and merge
    let loaded_global = Config::load()?;
    let loaded_profile = load_profile_config("default")?;
    let merged = merge_configs(loaded_global, &loaded_profile);

    assert!(
        !merged.sandbox.auto_cleanup,
        "Profile override should take precedence"
    );

    Ok(())
}

#[test]
#[serial]
fn test_merge_inherits_unset_fields() -> Result<()> {
    let _temp = setup_temp_home();

    // Save global config with specific values
    let mut global = Config::default();
    global.updates.check_interval_hours = 12;
    global.worktree.enabled = true;
    save_config(&global)?;

    // Profile only overrides theme
    let profile = ProfileConfig {
        theme: Some(ThemeConfigOverride {
            name: Some("dark".to_string()),
        }),
        ..Default::default()
    };
    save_profile_config("default", &profile)?;

    let loaded_global = Config::load()?;
    let loaded_profile = load_profile_config("default")?;
    let merged = merge_configs(loaded_global, &loaded_profile);

    assert_eq!(merged.theme.name, "dark", "Theme should be overridden");
    assert_eq!(
        merged.updates.check_interval_hours, 12,
        "check_interval_hours should inherit from global"
    );
    assert!(
        merged.worktree.enabled,
        "worktree.enabled should inherit from global"
    );

    Ok(())
}

#[test]
#[serial]
fn test_config_toml_round_trip() -> Result<()> {
    let _temp = setup_temp_home();

    let mut config = Config::default();
    config.theme.name = "monokai".to_string();
    config.updates.check_enabled = false;
    config.updates.check_interval_hours = 72;
    config.worktree.enabled = true;
    config.worktree.auto_cleanup = false;
    config.sandbox.enabled_by_default = true;
    config.sandbox.auto_cleanup = false;

    save_config(&config)?;
    let loaded = Config::load()?;

    assert_eq!(loaded.theme.name, "monokai");
    assert!(!loaded.updates.check_enabled);
    assert_eq!(loaded.updates.check_interval_hours, 72);
    assert!(loaded.worktree.enabled);
    assert!(!loaded.worktree.auto_cleanup);
    assert!(loaded.sandbox.enabled_by_default);
    assert!(!loaded.sandbox.auto_cleanup);

    Ok(())
}

#[test]
#[serial]
fn test_profile_config_toml_round_trip() -> Result<()> {
    let _temp = setup_temp_home();

    let profile = ProfileConfig {
        updates: Some(UpdatesConfigOverride {
            check_enabled: Some(false),
            check_interval_hours: Some(48),
            ..Default::default()
        }),
        worktree: Some(WorktreeConfigOverride {
            enabled: Some(true),
            auto_cleanup: Some(false),
            ..Default::default()
        }),
        sandbox: Some(SandboxConfigOverride {
            auto_cleanup: Some(false),
            ..Default::default()
        }),
        ..Default::default()
    };

    save_profile_config("default", &profile)?;
    let loaded = load_profile_config("default")?;

    let updates = loaded.updates.unwrap();
    assert_eq!(updates.check_enabled, Some(false));
    assert_eq!(updates.check_interval_hours, Some(48));

    let worktree = loaded.worktree.unwrap();
    assert_eq!(worktree.enabled, Some(true));
    assert_eq!(worktree.auto_cleanup, Some(false));

    let sandbox = loaded.sandbox.unwrap();
    assert_eq!(sandbox.auto_cleanup, Some(false));

    Ok(())
}

#[test]
#[serial]
fn test_empty_profile_config_returns_global() -> Result<()> {
    let _temp = setup_temp_home();

    let mut global = Config::default();
    global.updates.check_interval_hours = 99;
    save_config(&global)?;

    // Load profile config for a profile with no override file
    let profile = load_profile_config("default")?;
    let loaded_global = Config::load()?;
    let merged = merge_configs(loaded_global, &profile);

    assert_eq!(
        merged.updates.check_interval_hours, 99,
        "With no profile overrides, merged config should equal global"
    );

    Ok(())
}
