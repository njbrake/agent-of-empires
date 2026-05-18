//! Session storage - JSON file persistence

use anyhow::{anyhow, Result};
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use super::{get_profile_dir, Group, GroupTree, Instance, DEFAULT_PROFILE};

/// Write `content` to `path` atomically (temp file + fsync + rename + dir fsync).
/// Existing perms are preserved; on a fresh file the result is tempfile's 0o600 default.
pub(crate) fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    let dir = path.parent().ok_or_else(|| {
        anyhow!(
            "atomic_write needs a path with a parent: {}",
            path.display()
        )
    })?;
    let existing_perms = fs::metadata(path).ok().map(|m| m.permissions());
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(content)?;
    tmp.as_file().sync_data()?;
    let file = tmp.persist(path)?;
    if let Some(perms) = existing_perms {
        file.set_permissions(perms)?;
    }
    // Best-effort dir fsync so the rename itself survives power loss.
    if let Ok(dir_file) = fs::File::open(dir) {
        let _ = dir_file.sync_all();
    }
    Ok(())
}

pub struct Storage {
    profile: String,
    sessions_path: PathBuf,
}

impl Storage {
    pub fn new(profile: &str) -> Result<Self> {
        let profile_name = if profile.is_empty() {
            DEFAULT_PROFILE.to_string()
        } else {
            profile.to_string()
        };

        let profile_dir = get_profile_dir(&profile_name)?;
        let sessions_path = profile_dir.join("sessions.json");

        Ok(Self {
            profile: profile_name,
            sessions_path,
        })
    }

    pub fn profile(&self) -> &str {
        &self.profile
    }

    pub fn load(&self) -> Result<Vec<Instance>> {
        if !self.sessions_path.exists() {
            return Ok(Vec::new());
        }

        let content = fs::read_to_string(&self.sessions_path)?;
        if content.trim().is_empty() {
            return Ok(Vec::new());
        }

        let instances: Vec<Instance> = serde_json::from_str(&content)?;
        Ok(instances)
    }

    pub fn load_with_groups(&self) -> Result<(Vec<Instance>, Vec<Group>)> {
        let instances = self.load()?;

        // Load groups from separate file
        let groups_path = self.sessions_path.with_file_name("groups.json");
        let groups = if groups_path.exists() {
            let content = fs::read_to_string(&groups_path)?;
            if content.trim().is_empty() {
                Vec::new()
            } else {
                serde_json::from_str(&content)?
            }
        } else {
            Vec::new()
        };

        Ok((instances, groups))
    }

    pub fn save(&self, instances: &[Instance]) -> Result<()> {
        let content = serde_json::to_string_pretty(instances)?;
        atomic_write(&self.sessions_path, content.as_bytes())?;
        Ok(())
    }

    pub fn save_with_groups(&self, instances: &[Instance], group_tree: &GroupTree) -> Result<()> {
        self.save(instances)?;

        let groups_path = self.sessions_path.with_file_name("groups.json");
        let groups = group_tree.get_all_groups();
        let content = serde_json::to_string_pretty(&groups)?;
        atomic_write(&groups_path, content.as_bytes())?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::tempdir;

    fn setup_test_home(temp: &std::path::Path) {
        std::env::set_var("HOME", temp);
        #[cfg(target_os = "linux")]
        std::env::set_var("XDG_CONFIG_HOME", temp.join(".config"));
    }

    #[test]
    fn test_storage_roundtrip() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let storage = Storage::new("test-profile")?;

        let instances = vec![
            Instance::new("test1", "/tmp/test1"),
            Instance::new("test2", "/tmp/test2"),
        ];

        storage.save(&instances)?;
        let loaded = storage.load()?;

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].title, "test1");
        assert_eq!(loaded[1].title, "test2");

        Ok(())
    }

    #[test]
    #[serial]
    fn test_storage_new_with_empty_profile() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let storage = Storage::new("")?;
        assert_eq!(storage.profile(), "default");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_storage_new_with_custom_profile() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let storage = Storage::new("custom-profile")?;
        assert_eq!(storage.profile(), "custom-profile");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_storage_load_nonexistent_file() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let storage = Storage::new("test-empty")?;
        let loaded = storage.load()?;

        assert!(loaded.is_empty());
        Ok(())
    }

    #[test]
    #[serial]
    fn test_storage_load_empty_file() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let storage = Storage::new("test-empty-file")?;

        // Create empty file
        fs::create_dir_all(storage.sessions_path.parent().unwrap())?;
        fs::write(&storage.sessions_path, "")?;

        let loaded = storage.load()?;
        assert!(loaded.is_empty());
        Ok(())
    }

    #[test]
    #[serial]
    fn test_storage_load_whitespace_only_file() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let storage = Storage::new("test-whitespace")?;

        fs::create_dir_all(storage.sessions_path.parent().unwrap())?;
        fs::write(&storage.sessions_path, "   \n  \t  ")?;

        let loaded = storage.load()?;
        assert!(loaded.is_empty());
        Ok(())
    }

    #[test]
    #[serial]
    fn test_storage_save_leaves_no_temp_files() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let storage = Storage::new("test-no-debris")?;

        for i in 0..5 {
            let instances = vec![Instance::new(&format!("iter{i}"), "/tmp/test")];
            storage.save(&instances)?;
        }

        let dir = storage.sessions_path.parent().unwrap();
        let entries: Vec<_> = fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();

        assert_eq!(entries, vec!["sessions.json"]);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_storage_save_empty_array() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let storage = Storage::new("test-empty-save")?;
        storage.save(&[])?;

        let content = fs::read_to_string(&storage.sessions_path)?;
        assert_eq!(content.trim(), "[]");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_storage_load_with_groups_no_groups_file() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let storage = Storage::new("test-no-groups")?;

        let instances = vec![Instance::new("test", "/tmp/test")];
        storage.save(&instances)?;

        let (loaded_instances, loaded_groups) = storage.load_with_groups()?;
        assert_eq!(loaded_instances.len(), 1);
        assert!(loaded_groups.is_empty());
        Ok(())
    }

    #[test]
    #[serial]
    fn test_storage_save_and_load_with_groups() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let storage = Storage::new("test-with-groups")?;

        let mut instances = vec![Instance::new("test", "/tmp/test")];
        instances[0].group_path = "work/projects".to_string();

        let groups = vec![Group::new("projects", "work/projects")];
        let group_tree = GroupTree::new_with_groups(&instances, &groups);

        storage.save_with_groups(&instances, &group_tree)?;

        let (loaded_instances, loaded_groups) = storage.load_with_groups()?;
        assert_eq!(loaded_instances.len(), 1);
        assert_eq!(loaded_instances[0].group_path, "work/projects");
        assert!(!loaded_groups.is_empty());
        Ok(())
    }

    #[test]
    #[serial]
    fn test_storage_load_invalid_json() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let storage = Storage::new("test-invalid")?;

        fs::create_dir_all(storage.sessions_path.parent().unwrap())?;
        fs::write(&storage.sessions_path, "{ invalid json }")?;

        let result = storage.load();
        assert!(result.is_err());
        Ok(())
    }

    #[test]
    #[serial]
    fn test_storage_preserves_instance_fields() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let storage = Storage::new("test-fields")?;

        let mut instance = Instance::new("Test Project", "/home/user/project");
        instance.tool = "opencode".to_string();
        instance.command = "opencode --config test".to_string();
        instance.group_path = "work/clients".to_string();

        storage.save(&[instance.clone()])?;
        let loaded = storage.load()?;

        assert_eq!(loaded.len(), 1);
        let loaded_instance = &loaded[0];
        assert_eq!(loaded_instance.title, "Test Project");
        assert_eq!(loaded_instance.project_path, "/home/user/project");
        assert_eq!(loaded_instance.tool, "opencode");
        assert_eq!(loaded_instance.command, "opencode --config test");
        assert_eq!(loaded_instance.group_path, "work/clients");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_storage_profile_accessor() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        // Verify profiles are correctly named
        let storage1 = Storage::new("profile-alpha")?;
        let storage2 = Storage::new("profile-beta")?;

        assert_eq!(storage1.profile(), "profile-alpha");
        assert_eq!(storage2.profile(), "profile-beta");

        // Verify they use different paths (implying isolation)
        assert_ne!(storage1.sessions_path, storage2.sessions_path);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_storage_groups_file_empty() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let storage = Storage::new("test-empty-groups")?;

        // Save sessions
        storage.save(&[Instance::new("test", "/tmp/test")])?;

        // Create empty groups file
        let groups_path = storage.sessions_path.with_file_name("groups.json");
        fs::write(&groups_path, "   ")?;

        let (instances, groups) = storage.load_with_groups()?;
        assert_eq!(instances.len(), 1);
        assert!(groups.is_empty());
        Ok(())
    }
}
