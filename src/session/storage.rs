//! Session storage - JSON file persistence

use anyhow::Result;
use std::fs;
use std::path::PathBuf;
use tracing::warn;

use super::{get_app_dir, get_profile_dir, Group, GroupTree, Instance, DEFAULT_PROFILE};

pub struct Storage {
    profile: String,
    sessions_path: PathBuf,
}

// Cross-device-syncable sidebar ordering. Workspaces are a client
// construct (a group of sessions keyed on `repoPath::branch` or
// `repoPath::__session__::session_id`), so the server treats the entries
// here as opaque strings. The list is a partial order: workspace ids not
// in the list fall back to the default newest-first ordering. Persisted
// per-profile so different profiles keep independent layouts. See #1169.
#[derive(serde::Deserialize, serde::Serialize, Default)]
pub struct WorkspaceOrdering {
    pub order: Vec<String>,
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
        // Create backup
        if self.sessions_path.exists() {
            let backup_path = self.sessions_path.with_extension("json.bak");
            if let Err(e) = fs::copy(&self.sessions_path, &backup_path) {
                warn!("Failed to create backup: {}", e);
            }
        }

        let content = serde_json::to_string_pretty(instances)?;
        fs::write(&self.sessions_path, content)?;
        Ok(())
    }

    pub fn save_with_groups(&self, instances: &[Instance], group_tree: &GroupTree) -> Result<()> {
        self.save(instances)?;

        // Save groups
        let groups_path = self.sessions_path.with_file_name("groups.json");
        let groups = group_tree.get_all_groups();
        let content = serde_json::to_string_pretty(&groups)?;
        fs::write(&groups_path, content)?;

        Ok(())
    }

}

// Workspace ordering is stored at the app-data root, not per-profile:
// `list_sessions` returns sessions across all profiles, so the sidebar
// is a single global view and a per-profile file would only fragment
// the user's chosen layout. Workspace ids derive from `repoPath::branch`
// (or `repoPath::__session__::session_id`) and are profile-independent.
fn workspace_ordering_path() -> Result<PathBuf> {
    Ok(get_app_dir()?.join("workspace-ordering.json"))
}

pub fn load_workspace_ordering() -> Result<WorkspaceOrdering> {
    let path = workspace_ordering_path()?;
    if !path.exists() {
        return Ok(WorkspaceOrdering::default());
    }
    let content = fs::read_to_string(&path)?;
    if content.trim().is_empty() {
        return Ok(WorkspaceOrdering::default());
    }
    Ok(serde_json::from_str(&content)?)
}

pub fn save_workspace_ordering(ordering: &WorkspaceOrdering) -> Result<()> {
    let path = workspace_ordering_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(ordering)?;
    fs::write(&path, content)?;
    Ok(())
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
    #[serial]
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
    fn test_storage_save_creates_backup() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let storage = Storage::new("test-backup")?;

        // First save
        let instances = vec![Instance::new("test1", "/tmp/test1")];
        storage.save(&instances)?;

        // Second save (should create backup)
        let instances2 = vec![Instance::new("test2", "/tmp/test2")];
        storage.save(&instances2)?;

        // Check backup exists
        let backup_path = storage.sessions_path.with_extension("json.bak");
        assert!(backup_path.exists());

        // Backup should contain first save content
        let backup_content = fs::read_to_string(&backup_path)?;
        assert!(backup_content.contains("test1"));
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

    #[test]
    #[serial]
    fn test_workspace_ordering_roundtrip() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        // Empty by default.
        let empty = load_workspace_ordering()?;
        assert!(empty.order.is_empty());

        let saved = WorkspaceOrdering {
            order: vec![
                "/repo/a::main".to_string(),
                "/repo/b::feature/x".to_string(),
                "/repo/c::__session__::abc123".to_string(),
            ],
        };
        save_workspace_ordering(&saved)?;

        let loaded = load_workspace_ordering()?;
        assert_eq!(loaded.order, saved.order);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_workspace_ordering_overwrites_on_save() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        save_workspace_ordering(&WorkspaceOrdering {
            order: vec!["a".to_string(), "b".to_string()],
        })?;
        save_workspace_ordering(&WorkspaceOrdering {
            order: vec!["b".to_string()],
        })?;

        let loaded = load_workspace_ordering()?;
        assert_eq!(loaded.order, vec!["b".to_string()]);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_workspace_ordering_handles_empty_file() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let path = workspace_ordering_path()?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, "   ")?;

        let loaded = load_workspace_ordering()?;
        assert!(loaded.order.is_empty());
        Ok(())
    }
}
