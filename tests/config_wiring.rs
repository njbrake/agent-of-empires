//! Integration tests for config wiring
//!
//! These tests verify that config settings are properly wired up to the code
//! that uses them, specifically the auto_cleanup settings for worktrees and
//! sandbox containers.

use agent_of_empires::session::{save_config, Config, SandboxConfig, WorktreeConfig};
use agent_of_empires::tui::dialogs::{DeleteDialogConfig, UnifiedDeleteDialog};
use serial_test::serial;

fn setup_temp_home() -> tempfile::TempDir {
    let temp = tempfile::TempDir::new().unwrap();
    std::env::set_var("HOME", temp.path());
    temp
}

#[test]
#[serial]
fn test_delete_dialog_respects_worktree_auto_cleanup_true() {
    let _temp = setup_temp_home();

    let mut config = Config::default();
    config.worktree.auto_cleanup = true;
    save_config(&config).unwrap();

    let dialog = UnifiedDeleteDialog::new(
        "Test".to_string(),
        DeleteDialogConfig {
            worktree_branch: Some("main".to_string()),
            has_sandbox: false,
        },
    );
    assert!(
        dialog.options().delete_worktree,
        "When worktree.auto_cleanup is true, delete_worktree should default to true"
    );
}

#[test]
#[serial]
fn test_delete_dialog_respects_worktree_auto_cleanup_false() {
    let _temp = setup_temp_home();

    let mut config = Config::default();
    config.worktree.auto_cleanup = false;
    save_config(&config).unwrap();

    let dialog = UnifiedDeleteDialog::new(
        "Test".to_string(),
        DeleteDialogConfig {
            worktree_branch: Some("main".to_string()),
            has_sandbox: false,
        },
    );
    assert!(
        !dialog.options().delete_worktree,
        "When worktree.auto_cleanup is false, delete_worktree should default to false"
    );
}

#[test]
#[serial]
fn test_delete_dialog_respects_sandbox_auto_cleanup_true() {
    let _temp = setup_temp_home();

    let mut config = Config::default();
    config.sandbox.auto_cleanup = true;
    save_config(&config).unwrap();

    let dialog = UnifiedDeleteDialog::new(
        "Test".to_string(),
        DeleteDialogConfig {
            worktree_branch: None,
            has_sandbox: true,
        },
    );
    assert!(
        dialog.options().delete_sandbox,
        "When sandbox.auto_cleanup is true, delete_sandbox should default to true"
    );
}

#[test]
#[serial]
fn test_delete_dialog_respects_sandbox_auto_cleanup_false() {
    let _temp = setup_temp_home();

    let mut config = Config::default();
    config.sandbox.auto_cleanup = false;
    save_config(&config).unwrap();

    let dialog = UnifiedDeleteDialog::new(
        "Test".to_string(),
        DeleteDialogConfig {
            worktree_branch: None,
            has_sandbox: true,
        },
    );
    assert!(
        !dialog.options().delete_sandbox,
        "When sandbox.auto_cleanup is false, delete_sandbox should default to false"
    );
}

#[test]
fn test_default_config_has_auto_cleanup_true() {
    let config = Config::default();
    assert!(
        config.worktree.auto_cleanup,
        "Default worktree.auto_cleanup should be true"
    );
    assert!(
        config.sandbox.auto_cleanup,
        "Default sandbox.auto_cleanup should be true"
    );
}

#[test]
#[serial]
fn test_config_roundtrip_preserves_auto_cleanup() {
    let _temp = setup_temp_home();

    let mut config = Config::default();
    config.worktree.auto_cleanup = false;
    config.sandbox.auto_cleanup = false;
    save_config(&config).unwrap();

    let loaded = Config::load().unwrap();
    assert!(
        !loaded.worktree.auto_cleanup,
        "worktree.auto_cleanup should persist as false"
    );
    assert!(
        !loaded.sandbox.auto_cleanup,
        "sandbox.auto_cleanup should persist as false"
    );
}

#[test]
fn test_all_worktree_config_fields_accessible() {
    let config = WorktreeConfig::default();
    let _ = config.enabled;
    let _ = config.path_template.as_str();
    let _ = config.auto_cleanup;
    let _ = config.show_branch_in_tui;
}

#[test]
fn test_all_sandbox_config_fields_accessible() {
    let config = SandboxConfig::default();
    let _ = config.enabled_by_default;
    let _ = config.default_image.as_str();
    let _ = &config.extra_volumes;
    let _ = &config.environment;
    let _ = config.auto_cleanup;
    let _ = &config.cpu_limit;
    let _ = &config.memory_limit;
}
