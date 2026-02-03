//! Integration tests for hooks config resolution across global, profile, and repo levels.

use agent_of_empires::session::{
    merge_configs, merge_repo_config, resolve_config, save_config, save_profile_config, Config,
    HooksConfig, HooksConfigOverride, ProfileConfig, RepoConfig,
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

// T014: Global hooks resolve when no repo config exists
#[test]
#[serial]
fn test_global_hooks_resolve_without_repo() -> Result<()> {
    let _temp = setup_temp_home();

    let mut global = Config::default();
    global.hooks.on_create = vec!["npm install".to_string()];
    global.hooks.on_launch = vec!["echo hello".to_string()];
    save_config(&global)?;

    let resolved = resolve_config("default")?;
    assert_eq!(resolved.hooks.on_create, vec!["npm install"]);
    assert_eq!(resolved.hooks.on_launch, vec!["echo hello"]);

    Ok(())
}

// T015: Repo hooks override global hooks per-field
#[test]
#[serial]
fn test_repo_hooks_override_global_per_field() -> Result<()> {
    let _temp = setup_temp_home();

    let mut global = Config::default();
    global.hooks.on_create = vec!["global_create".to_string()];
    global.hooks.on_launch = vec!["global_launch".to_string()];
    save_config(&global)?;

    let resolved = resolve_config("default")?;

    // Repo only defines on_create
    let repo = RepoConfig {
        hooks: Some(HooksConfig {
            on_create: vec!["repo_create".to_string()],
            on_launch: vec![],
        }),
        ..Default::default()
    };

    let merged = merge_repo_config(resolved, &repo);

    // Repo on_create should override global
    assert_eq!(merged.hooks.on_create, vec!["repo_create"]);
    // Global on_launch should be preserved (repo on_launch is empty)
    assert_eq!(merged.hooks.on_launch, vec!["global_launch"]);

    Ok(())
}

// T015 additional: repo hooks override both fields
#[test]
#[serial]
fn test_repo_hooks_override_both_fields() -> Result<()> {
    let _temp = setup_temp_home();

    let mut global = Config::default();
    global.hooks.on_create = vec!["global_create".to_string()];
    global.hooks.on_launch = vec!["global_launch".to_string()];
    save_config(&global)?;

    let resolved = resolve_config("default")?;

    let repo = RepoConfig {
        hooks: Some(HooksConfig {
            on_create: vec!["repo_create".to_string()],
            on_launch: vec!["repo_launch".to_string()],
        }),
        ..Default::default()
    };

    let merged = merge_repo_config(resolved, &repo);
    assert_eq!(merged.hooks.on_create, vec!["repo_create"]);
    assert_eq!(merged.hooks.on_launch, vec!["repo_launch"]);

    Ok(())
}

// T016: Global/profile hooks are NOT subject to trust checking.
// This is a design invariant: check_hook_trust() only reads .aoe/config.toml,
// so global/profile hooks never enter the trust pipeline. We verify that
// resolve_config returns hooks without any trust gate.
#[test]
#[serial]
fn test_global_profile_hooks_bypass_trust() -> Result<()> {
    let _temp = setup_temp_home();

    let mut global = Config::default();
    global.hooks.on_create = vec!["global_cmd".to_string()];
    save_config(&global)?;

    // resolve_config returns hooks directly - no trust check involved
    let resolved = resolve_config("default")?;
    assert_eq!(resolved.hooks.on_create, vec!["global_cmd"]);

    // With profile override - also no trust check
    let profile = ProfileConfig {
        hooks: Some(HooksConfigOverride {
            on_launch: Some(vec!["profile_launch".to_string()]),
            ..Default::default()
        }),
        ..Default::default()
    };
    save_profile_config("default", &profile)?;

    let resolved = resolve_config("default")?;
    assert_eq!(resolved.hooks.on_create, vec!["global_cmd"]);
    assert_eq!(resolved.hooks.on_launch, vec!["profile_launch"]);

    Ok(())
}

// T018: Profile on_create override replaces global on_create, on_launch falls back
#[test]
#[serial]
fn test_profile_overrides_on_create_only() -> Result<()> {
    let _temp = setup_temp_home();

    let mut global = Config::default();
    global.hooks.on_create = vec!["global_create".to_string()];
    global.hooks.on_launch = vec!["global_launch".to_string()];
    save_config(&global)?;

    let profile = ProfileConfig {
        hooks: Some(HooksConfigOverride {
            on_create: Some(vec!["profile_create".to_string()]),
            on_launch: None,
        }),
        ..Default::default()
    };
    save_profile_config("default", &profile)?;

    let resolved = resolve_config("default")?;
    assert_eq!(resolved.hooks.on_create, vec!["profile_create"]);
    assert_eq!(resolved.hooks.on_launch, vec!["global_launch"]);

    Ok(())
}

// T019: Clearing profile hooks override restores global hooks
#[test]
#[serial]
fn test_clearing_profile_hooks_restores_global() -> Result<()> {
    let _temp = setup_temp_home();

    let mut global = Config::default();
    global.hooks.on_create = vec!["global_create".to_string()];
    global.hooks.on_launch = vec!["global_launch".to_string()];
    save_config(&global)?;

    // First set profile override
    let profile = ProfileConfig {
        hooks: Some(HooksConfigOverride {
            on_create: Some(vec!["profile_create".to_string()]),
            on_launch: Some(vec!["profile_launch".to_string()]),
        }),
        ..Default::default()
    };
    save_profile_config("default", &profile)?;

    let resolved = resolve_config("default")?;
    assert_eq!(resolved.hooks.on_create, vec!["profile_create"]);
    assert_eq!(resolved.hooks.on_launch, vec!["profile_launch"]);

    // Clear profile override (set to None)
    let cleared_profile = ProfileConfig {
        hooks: None,
        ..Default::default()
    };
    save_profile_config("default", &cleared_profile)?;

    let resolved = resolve_config("default")?;
    assert_eq!(resolved.hooks.on_create, vec!["global_create"]);
    assert_eq!(resolved.hooks.on_launch, vec!["global_launch"]);

    Ok(())
}

// T020: Full three-level resolution (global + profile + repo) with per-field semantics
#[test]
#[serial]
fn test_three_level_resolution() -> Result<()> {
    let _temp = setup_temp_home();

    // Global: both hooks
    let mut global = Config::default();
    global.hooks.on_create = vec!["global_create".to_string()];
    global.hooks.on_launch = vec!["global_launch".to_string()];
    save_config(&global)?;

    // Profile: only overrides on_create
    let profile = ProfileConfig {
        hooks: Some(HooksConfigOverride {
            on_create: Some(vec!["profile_create".to_string()]),
            on_launch: None,
        }),
        ..Default::default()
    };
    save_profile_config("default", &profile)?;

    let resolved = resolve_config("default")?;
    assert_eq!(resolved.hooks.on_create, vec!["profile_create"]);
    assert_eq!(resolved.hooks.on_launch, vec!["global_launch"]);

    // Repo: only overrides on_launch
    let repo = RepoConfig {
        hooks: Some(HooksConfig {
            on_create: vec![],
            on_launch: vec!["repo_launch".to_string()],
        }),
        ..Default::default()
    };

    let final_config = merge_repo_config(resolved, &repo);
    // on_create: profile > global (repo is empty, so profile value stays)
    assert_eq!(final_config.hooks.on_create, vec!["profile_create"]);
    // on_launch: repo > profile > global
    assert_eq!(final_config.hooks.on_launch, vec!["repo_launch"]);

    Ok(())
}

// T020 additional: Verify merge_configs directly
#[test]
#[serial]
fn test_merge_configs_hooks_override() -> Result<()> {
    let _temp = setup_temp_home();

    let mut global = Config::default();
    global.hooks.on_create = vec!["g1".to_string(), "g2".to_string()];
    global.hooks.on_launch = vec!["gl".to_string()];

    let profile = ProfileConfig {
        hooks: Some(HooksConfigOverride {
            on_create: Some(vec!["p1".to_string()]),
            on_launch: None,
        }),
        ..Default::default()
    };

    let merged = merge_configs(global, &profile);
    assert_eq!(merged.hooks.on_create, vec!["p1"]);
    assert_eq!(merged.hooks.on_launch, vec!["gl"]);

    Ok(())
}
