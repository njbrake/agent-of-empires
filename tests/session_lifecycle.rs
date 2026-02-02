//! Integration tests for the core session lifecycle: create, persist, load, remove.

use agent_of_empires::session::{GroupTree, Instance, Storage};
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

#[test]
#[serial]
fn test_create_session_persists() -> Result<()> {
    let _temp = setup_temp_home();

    let storage = Storage::new("default")?;
    let instance = Instance::new("My Project", "/home/user/project");
    let group_tree = GroupTree::new_with_groups(std::slice::from_ref(&instance), &[]);

    storage.save_with_groups(std::slice::from_ref(&instance), &group_tree)?;

    let (loaded, _groups) = storage.load_with_groups()?;
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].title, "My Project");
    assert_eq!(loaded[0].project_path, "/home/user/project");
    assert_eq!(loaded[0].id, instance.id);

    Ok(())
}

#[test]
#[serial]
fn test_create_multiple_sessions() -> Result<()> {
    let _temp = setup_temp_home();

    let storage = Storage::new("default")?;
    let instances = vec![
        Instance::new("Project A", "/path/a"),
        Instance::new("Project B", "/path/b"),
        Instance::new("Project C", "/path/c"),
    ];
    let group_tree = GroupTree::new_with_groups(&instances, &[]);

    storage.save_with_groups(&instances, &group_tree)?;

    let (loaded, _) = storage.load_with_groups()?;
    assert_eq!(loaded.len(), 3);
    assert_eq!(loaded[0].title, "Project A");
    assert_eq!(loaded[1].title, "Project B");
    assert_eq!(loaded[2].title, "Project C");

    Ok(())
}

#[test]
#[serial]
fn test_remove_session_by_id() -> Result<()> {
    let _temp = setup_temp_home();

    let storage = Storage::new("default")?;
    let inst_a = Instance::new("Keep Me", "/path/keep");
    let inst_b = Instance::new("Remove Me", "/path/remove");
    let remove_id = inst_b.id.clone();

    let instances = vec![inst_a, inst_b];
    let group_tree = GroupTree::new_with_groups(&instances, &[]);
    storage.save_with_groups(&instances, &group_tree)?;

    // Remove by filtering
    let (mut loaded, groups) = storage.load_with_groups()?;
    loaded.retain(|i| i.id != remove_id);
    let group_tree = GroupTree::new_with_groups(&loaded, &groups);
    storage.save_with_groups(&loaded, &group_tree)?;

    let (reloaded, _) = storage.load_with_groups()?;
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded[0].title, "Keep Me");

    Ok(())
}

#[test]
#[serial]
fn test_create_session_with_group() -> Result<()> {
    let _temp = setup_temp_home();

    let storage = Storage::new("default")?;
    let mut instance = Instance::new("Grouped Session", "/path/grouped");
    instance.group_path = "work".to_string();

    let mut group_tree = GroupTree::new_with_groups(std::slice::from_ref(&instance), &[]);
    group_tree.create_group("work");

    storage.save_with_groups(std::slice::from_ref(&instance), &group_tree)?;

    let (loaded, loaded_groups) = storage.load_with_groups()?;
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].group_path, "work");

    let reloaded_tree = GroupTree::new_with_groups(&loaded, &loaded_groups);
    assert!(reloaded_tree.group_exists("work"));

    Ok(())
}

#[test]
#[serial]
fn test_session_backup_created() -> Result<()> {
    let _temp = setup_temp_home();

    let storage = Storage::new("default")?;

    // First save
    let instances = vec![Instance::new("First", "/path/first")];
    storage.save(&instances)?;

    // Second save triggers backup of the first
    let instances2 = vec![Instance::new("Second", "/path/second")];
    storage.save(&instances2)?;

    // Verify backup exists by checking the profile directory
    let profile_dir = agent_of_empires::session::get_profile_dir("default")?;
    let backup_path = profile_dir.join("sessions.json.bak");
    assert!(backup_path.exists());

    let backup_content = fs::read_to_string(&backup_path)?;
    assert!(backup_content.contains("First"));

    Ok(())
}

#[test]
#[serial]
fn test_storage_defaults_to_default_profile() -> Result<()> {
    let _temp = setup_temp_home();

    let storage = Storage::new("")?;
    assert_eq!(storage.profile(), "default");

    // Verify it can save and load
    let instances = vec![Instance::new("Test", "/path/test")];
    storage.save(&instances)?;
    let loaded = storage.load()?;
    assert_eq!(loaded.len(), 1);

    Ok(())
}
