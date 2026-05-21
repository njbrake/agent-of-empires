//! Session storage - JSON file persistence with in-process per-profile locking.
//!
//! `Storage` serialises read-modify-write cycles inside the same process via a
//! per-profile mutex (one `Arc<Mutex<()>>` per profile name, registered process-
//! wide). Mutators use `update` (load -> mutate -> save under the lock) or
//! `commit` (locked wholesale write, for callers that already own the
//! authoritative in-memory state, e.g. the TUI's `HomeView`). The
//! `save_workspace_ordering` entry point is `pub(crate)` and only consumed
//! by `update_workspace_ordering` internally; the per-profile `save` /
//! `save_groups` helpers have been removed entirely. This keeps it
//! structurally impossible to bypass the lock.
//!
//! Lock-ordering rule across the process: `AppState.instances` (tokio RwLock,
//! server side) is acquired BEFORE `Storage`'s per-profile mutex, never the
//! reverse. The closure passed to `update` is `FnOnce(...) -> Result<R>` and
//! cannot await, so `std::sync::Mutex` is safe across the body even on the
//! tokio runtime: server callers wrap `update` in `tokio::task::spawn_blocking`,
//! which is the existing pattern.
//!
//! Cross-process races (the TUI and `aoe serve` mutating the same profile
//! concurrently) are explicitly out of scope here; a future advisory `flock`
//! would close them.

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use super::{get_app_dir, get_profile_dir, Group, GroupTree, Instance};

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

/// Process-wide registry of per-profile save mutexes. Every `Storage::new` for
/// a given profile name resolves to the same `Arc<Mutex<()>>`, so independent
/// `Storage` handles in different parts of the process serialise correctly.
fn save_lock_for(profile: &str) -> Arc<Mutex<()>> {
    static REGISTRY: OnceLock<Mutex<HashMap<String, Arc<Mutex<()>>>>> = OnceLock::new();
    let registry = REGISTRY.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = registry
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    guard
        .entry(profile.to_string())
        .or_insert_with(|| Arc::new(Mutex::new(())))
        .clone()
}

/// Dedicated lock for the global `workspace-ordering.json` file. Separate from
/// the per-profile registry because the file lives at the app-data root and is
/// shared across profiles.
fn workspace_ordering_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub struct Storage {
    profile: String,
    sessions_path: PathBuf,
    save_lock: Arc<Mutex<()>>,
}

// Cross-device-syncable sidebar ordering. Workspaces are a client
// construct (a group of sessions keyed on `repoPath::branch` or
// `repoPath::__session__::session_id`), so the server treats the entries
// here as opaque strings. The list is a partial order: workspace ids not
// in the list fall back to the default newest-first ordering. Persisted
// globally (not per-profile) because the sidebar shows sessions across
// all profiles and a per-profile file would fragment the user's layout.
// See #1169.
#[derive(serde::Deserialize, serde::Serialize, Default)]
pub struct WorkspaceOrdering {
    pub order: Vec<String>,
}

impl Storage {
    pub fn new(profile: &str) -> Result<Self> {
        let profile_name = if profile.is_empty() {
            super::config::resolve_default_profile()
        } else {
            profile.to_string()
        };

        let profile_dir = get_profile_dir(&profile_name)?;
        let sessions_path = profile_dir.join("sessions.json");
        let save_lock = save_lock_for(&profile_name);

        Ok(Self {
            profile: profile_name,
            sessions_path,
            save_lock,
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

    /// Locked load -> mutate -> save. The closure receives mutable references
    /// to the current persisted state of `sessions.json` and `groups.json`.
    /// On `Ok` from the closure, both files are serialised before any disk
    /// write, so a serialisation failure on either side leaves both files
    /// untouched. Likewise, an `Err` from the closure leaves both files
    /// untouched. `groups.json` is only rewritten when the closure actually
    /// changed the groups vec (most callers only touch instances).
    ///
    /// `groups.json` is written first, `sessions.json` second. A disk-level
    /// failure on the second `atomic_write` (after the first succeeded) can
    /// leave a torn pair: the new groups are persisted with the prior
    /// instances. This window is bounded by two `rename(2)` syscalls on
    /// sibling files and is tolerated by the loader (`GroupTree` accepts
    /// orphan group rows).
    ///
    /// This is the only way to mutate persisted session state from any caller
    /// that does not already own the authoritative in-memory copy. Use `commit`
    /// when the caller (e.g. TUI `HomeView`) IS that authoritative copy.
    pub fn update<F, R>(&self, f: F) -> Result<R>
    where
        F: FnOnce(&mut Vec<Instance>, &mut Vec<Group>) -> Result<R>,
    {
        let _guard = self
            .save_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (mut instances, mut groups) = self.load_with_groups()?;
        let groups_before = groups.clone();
        let result = f(&mut instances, &mut groups)?;

        // Pre-serialise both buffers so a serde failure on either side
        // aborts before any file is touched.
        let instances_buf = serde_json::to_vec_pretty(&instances)?;
        let groups_changed = groups != groups_before;
        let groups_buf = if groups_changed {
            Some(serde_json::to_vec_pretty(&groups)?)
        } else {
            None
        };

        // groups first, sessions last: a torn pair leaves orphan groups
        // (loader-tolerant) rather than instances pointing at a missing
        // group_path.
        if let Some(buf) = groups_buf {
            let groups_path = self.sessions_path.with_file_name("groups.json");
            atomic_write(&groups_path, &buf)?;
        }
        atomic_write(&self.sessions_path, &instances_buf)?;
        Ok(result)
    }

    /// Locked wholesale write of `sessions.json` + `groups.json`. Last-writer-
    /// wins by design: any concurrent `update` whose load happened before this
    /// `commit` is overwritten. This is correct for HomeView (the TUI's
    /// authoritative in-memory copy); other callers should use `update`.
    ///
    /// Both buffers are serialised before any disk write, so a serialisation
    /// failure on either side leaves both files untouched. The two atomic
    /// writes happen in the same order as `update` (groups, then sessions);
    /// the same residual disk-failure window applies.
    pub fn commit(&self, instances: &[Instance], group_tree: &GroupTree) -> Result<()> {
        let _guard = self
            .save_lock
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let groups = group_tree.get_all_groups();
        let instances_buf = serde_json::to_vec_pretty(instances)?;
        let groups_buf = serde_json::to_vec_pretty(&groups)?;
        let groups_path = self.sessions_path.with_file_name("groups.json");
        atomic_write(&groups_path, &groups_buf)?;
        atomic_write(&self.sessions_path, &instances_buf)?;
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

/// Locked load -> mutate -> save for the global workspace ordering file.
/// On `Ok` from the closure, the file is rewritten atomically under the
/// dedicated workspace-ordering lock. On `Err`, the file is not touched.
pub fn update_workspace_ordering<F, R>(f: F) -> Result<R>
where
    F: FnOnce(&mut WorkspaceOrdering) -> Result<R>,
{
    let _guard = workspace_ordering_lock()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut ordering = load_workspace_ordering()?;
    let result = f(&mut ordering)?;
    save_workspace_ordering(&ordering)?;
    Ok(result)
}

pub(crate) fn save_workspace_ordering(ordering: &WorkspaceOrdering) -> Result<()> {
    let path = workspace_ordering_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let content = serde_json::to_string_pretty(ordering)?;
    atomic_write(&path, content.as_bytes())?;
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

        storage.commit(&instances, &GroupTree::new_with_groups(&instances, &[]))?;
        let loaded = storage.load()?;

        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].title, "test1");
        assert_eq!(loaded[1].title, "test2");

        Ok(())
    }

    #[test]
    #[serial]
    fn test_storage_new_with_empty_profile_bootstraps() -> Result<()> {
        // On a fresh install with no profiles, an empty profile argument
        // resolves through `resolve_default_profile`, which bootstraps the
        // first profile. The name is "main", never the magic "default".
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let storage = Storage::new("")?;
        assert_eq!(storage.profile(), "main");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_storage_new_with_empty_profile_uses_existing() -> Result<()> {
        // When profiles already exist, an empty profile argument resolves to
        // the first one (sorted), not a hard-coded name.
        let temp = tempdir()?;
        setup_test_home(temp.path());

        get_profile_dir("work")?;
        get_profile_dir("personal")?;

        let storage = Storage::new("")?;
        assert_eq!(storage.profile(), "personal");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_storage_new_with_empty_profile_honors_config() -> Result<()> {
        // An explicitly configured default_profile wins over the first-found
        // directory.
        let temp = tempdir()?;
        setup_test_home(temp.path());

        get_profile_dir("work")?;
        get_profile_dir("personal")?;
        let config = super::super::config::Config {
            default_profile: "work".to_string(),
            ..Default::default()
        };
        super::super::config::save_config(&config)?;

        let storage = Storage::new("")?;
        assert_eq!(storage.profile(), "work");
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
            storage.commit(&instances, &GroupTree::new_with_groups(&instances, &[]))?;
        }

        let dir = storage.sessions_path.parent().unwrap();
        let mut entries: Vec<_> = fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .collect();
        entries.sort();

        assert_eq!(entries, vec!["groups.json", "sessions.json"]);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_storage_save_empty_array() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let storage = Storage::new("test-empty-save")?;
        {
            let xs: Vec<Instance> = vec![];
            storage.commit(&xs, &GroupTree::new_with_groups(&xs, &[]))?
        };

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
        storage.commit(&instances, &GroupTree::new_with_groups(&instances, &[]))?;

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

        storage.commit(&instances, &group_tree)?;

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

        {
            let xs: Vec<Instance> = vec![instance.clone()];
            storage.commit(&xs, &GroupTree::new_with_groups(&xs, &[]))?
        };
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
        {
            let xs: Vec<Instance> = vec![Instance::new("test", "/tmp/test")];
            storage.commit(&xs, &GroupTree::new_with_groups(&xs, &[]))?
        };

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

    #[test]
    #[serial]
    fn test_update_atomic_load_modify_save() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let storage = Storage::new("test-update-roundtrip")?;
        storage.commit(
            &[Instance::new("seed", "/tmp/seed")],
            &GroupTree::new_with_groups(&[], &[]),
        )?;

        storage.update(|instances, _groups| {
            instances.push(Instance::new("added", "/tmp/added"));
            Ok(())
        })?;

        let loaded = storage.load()?;
        assert_eq!(loaded.len(), 2);
        assert_eq!(loaded[0].title, "seed");
        assert_eq!(loaded[1].title, "added");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_update_propagates_closure_error() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let storage = Storage::new("test-update-err")?;
        let initial = vec![Instance::new("keep", "/tmp/keep")];
        storage.commit(&initial, &GroupTree::new_with_groups(&initial, &[]))?;

        let result: Result<()> = storage.update(|instances, _| {
            instances.push(Instance::new("doomed", "/tmp/doomed"));
            Err(anyhow!("forced abort"))
        });
        assert!(result.is_err());

        let loaded = storage.load()?;
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].title, "keep");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_update_serializes_concurrent_writers_same_profile() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let storage = Storage::new("test-update-concurrent")?;
        storage.commit(&[], &GroupTree::new_with_groups(&[], &[]))?;

        let n_threads = 32usize;
        std::thread::scope(|scope| {
            for tid in 0..n_threads {
                scope.spawn(move || {
                    let storage = Storage::new("test-update-concurrent").unwrap();
                    storage
                        .update(|instances, _| {
                            instances.push(Instance::new(
                                &format!("inst-{tid}"),
                                &format!("/tmp/inst-{tid}"),
                            ));
                            Ok(())
                        })
                        .unwrap();
                });
            }
        });

        let loaded = storage.load()?;
        assert_eq!(
            loaded.len(),
            n_threads,
            "lost updates: expected {n_threads}, got {}",
            loaded.len()
        );
        let mut titles: Vec<_> = loaded.iter().map(|i| i.title.clone()).collect();
        titles.sort();
        for tid in 0..n_threads {
            assert!(
                titles.contains(&format!("inst-{tid}")),
                "missing inst-{tid}"
            );
        }
        Ok(())
    }

    #[test]
    #[serial]
    fn test_update_does_not_serialize_across_profiles() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let storage_a = Storage::new("test-update-profile-a")?;
        let storage_b = Storage::new("test-update-profile-b")?;

        std::thread::scope(|scope| {
            scope.spawn(|| {
                storage_a
                    .update(|instances, _| {
                        instances.push(Instance::new("a1", "/tmp/a1"));
                        Ok(())
                    })
                    .unwrap();
            });
            scope.spawn(|| {
                storage_b
                    .update(|instances, _| {
                        instances.push(Instance::new("b1", "/tmp/b1"));
                        Ok(())
                    })
                    .unwrap();
            });
        });

        assert_eq!(storage_a.load()?.len(), 1);
        assert_eq!(storage_b.load()?.len(), 1);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_commit_takes_same_lock_as_update() -> Result<()> {
        use std::sync::Barrier;
        use std::time::{Duration, Instant};

        let temp = tempdir()?;
        setup_test_home(temp.path());

        let storage = Storage::new("test-commit-lock")?;
        storage.commit(&[], &GroupTree::new_with_groups(&[], &[]))?;

        let entered = Arc::new(Barrier::new(2));
        let release = Arc::new(Barrier::new(2));
        let entered_clone = Arc::clone(&entered);
        let release_clone = Arc::clone(&release);

        let updater = std::thread::spawn(move || {
            let storage = Storage::new("test-commit-lock").unwrap();
            storage
                .update(|instances, _| {
                    instances.push(Instance::new("from-update", "/tmp/u"));
                    entered_clone.wait();
                    release_clone.wait();
                    Ok(())
                })
                .unwrap();
        });

        entered.wait();
        let start = Instant::now();
        let committer = std::thread::spawn(|| {
            let storage = Storage::new("test-commit-lock").unwrap();
            storage
                .commit(
                    &[Instance::new("from-commit", "/tmp/c")],
                    &GroupTree::new_with_groups(&[], &[]),
                )
                .unwrap();
        });

        std::thread::sleep(Duration::from_millis(80));
        assert!(
            !committer.is_finished(),
            "commit should be blocked by update's lock"
        );
        release.wait();
        updater.join().unwrap();
        committer.join().unwrap();

        assert!(
            start.elapsed() >= Duration::from_millis(50),
            "commit returned suspiciously fast"
        );

        let loaded = storage.load()?;
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].title, "from-commit");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_workspace_ordering_update_serializes() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        update_workspace_ordering(|ord| {
            ord.order.clear();
            Ok(())
        })?;

        let n_threads = 16usize;
        std::thread::scope(|scope| {
            for tid in 0..n_threads {
                scope.spawn(move || {
                    update_workspace_ordering(|ord| {
                        ord.order.push(format!("ws-{tid}"));
                        Ok(())
                    })
                    .unwrap();
                });
            }
        });

        let loaded = load_workspace_ordering()?;
        assert_eq!(loaded.order.len(), n_threads);
        for tid in 0..n_threads {
            assert!(
                loaded.order.contains(&format!("ws-{tid}")),
                "missing ws-{tid}"
            );
        }
        Ok(())
    }

    #[test]
    #[serial]
    fn test_profile_lock_registry_returns_same_arc_for_same_profile() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let s1 = Storage::new("test-registry-shared")?;
        let s2 = Storage::new("test-registry-shared")?;
        assert!(Arc::ptr_eq(&s1.save_lock, &s2.save_lock));

        let s3 = Storage::new("test-registry-distinct")?;
        assert!(!Arc::ptr_eq(&s1.save_lock, &s3.save_lock));
        Ok(())
    }

    #[test]
    #[serial]
    fn test_update_writes_both_sessions_and_groups_files() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let storage = Storage::new("test-update-both-files")?;
        storage.commit(&[], &GroupTree::new_with_groups(&[], &[]))?;

        storage.update(|instances, groups| {
            instances.push(Instance::new("inst", "/tmp/inst"));
            groups.push(Group::new("projects", "work/projects"));
            Ok(())
        })?;

        let groups_path = storage.sessions_path.with_file_name("groups.json");
        assert!(groups_path.exists(), "groups.json should exist");

        let (loaded_instances, loaded_groups) = storage.load_with_groups()?;
        assert_eq!(loaded_instances.len(), 1);
        assert_eq!(loaded_groups.len(), 1);
        assert_eq!(loaded_groups[0].name, "projects");
        Ok(())
    }

    #[test]
    #[serial]
    fn test_update_closure_err_leaves_both_files_untouched() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let storage = Storage::new("test-update-err-untouched")?;
        let seed = vec![Instance::new("seed", "/tmp/seed")];
        let seed_groups = vec![Group::new("seed-group", "work/seed")];
        let mut tree = GroupTree::new_with_groups(&seed, &seed_groups);
        tree.create_group("work/seed");
        storage.commit(&seed, &tree)?;

        let groups_path = storage.sessions_path.with_file_name("groups.json");
        let sessions_before = fs::read(&storage.sessions_path)?;
        let groups_before = fs::read(&groups_path)?;

        let outcome: Result<()> = storage.update(|instances, groups| {
            instances.push(Instance::new("doomed-inst", "/tmp/doomed"));
            groups.push(Group::new("doomed-group", "doomed/path"));
            Err(anyhow!("forced abort"))
        });
        assert!(outcome.is_err());

        assert_eq!(fs::read(&storage.sessions_path)?, sessions_before);
        assert_eq!(fs::read(&groups_path)?, groups_before);
        Ok(())
    }

    #[test]
    #[serial]
    fn test_update_skips_groups_write_when_groups_unchanged() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let storage = Storage::new("test-skip-groups-write")?;
        let seed_instances = vec![Instance::new("seed", "/tmp/seed")];
        storage.commit(
            &seed_instances,
            &GroupTree::new_with_groups(&seed_instances, &[]),
        )?;

        let groups_path = storage.sessions_path.with_file_name("groups.json");
        let groups_mtime_before = fs::metadata(&groups_path)?.modified()?;

        std::thread::sleep(std::time::Duration::from_millis(10));

        storage.update(|instances, _groups| {
            instances.push(Instance::new("added", "/tmp/added"));
            Ok(())
        })?;

        let groups_mtime_after = fs::metadata(&groups_path)?.modified()?;
        assert_eq!(
            groups_mtime_before, groups_mtime_after,
            "groups.json should not be rewritten when closure does not mutate groups"
        );
        Ok(())
    }

    #[test]
    #[serial]
    fn test_update_rewrites_groups_when_changed() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let storage = Storage::new("test-rewrite-groups")?;
        let seed_instances = vec![Instance::new("seed", "/tmp/seed")];
        storage.commit(
            &seed_instances,
            &GroupTree::new_with_groups(&seed_instances, &[]),
        )?;

        let groups_path = storage.sessions_path.with_file_name("groups.json");
        let groups_mtime_before = fs::metadata(&groups_path)?.modified()?;

        std::thread::sleep(std::time::Duration::from_millis(10));

        storage.update(|_instances, groups| {
            groups.push(Group::new("new-group", "work/new-group"));
            Ok(())
        })?;

        let groups_mtime_after = fs::metadata(&groups_path)?.modified()?;
        assert_ne!(
            groups_mtime_before, groups_mtime_after,
            "groups.json should be rewritten when closure mutates groups"
        );
        Ok(())
    }

    #[test]
    #[serial]
    fn test_save_lock_registry_recovers_from_poison() -> Result<()> {
        let temp = tempdir()?;
        setup_test_home(temp.path());

        let storage_outer = Storage::new("test-poison-recovery")?;
        let _ = std::thread::spawn(move || {
            let _ = storage_outer.update(|_instances, _groups| -> Result<()> {
                panic!("forced poison");
            });
        })
        .join();

        let storage_after = Storage::new("test-poison-recovery")?;
        storage_after.update(|instances, _groups| {
            instances.push(Instance::new("after-poison", "/tmp/after"));
            Ok(())
        })?;

        let loaded = storage_after.load()?;
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].title, "after-poison");
        Ok(())
    }
}
