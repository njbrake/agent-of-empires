//! Session storage - JSON file persistence

use anyhow::Result;
use std::fs;
use std::path::PathBuf;
use tracing::warn;

use super::{get_profile_dir, Group, GroupTree, Instance, DEFAULT_PROFILE};

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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_storage_roundtrip() -> Result<()> {
        let temp = tempdir()?;
        std::env::set_var("HOME", temp.path());

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
}
