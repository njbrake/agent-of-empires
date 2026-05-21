//! Integration tests for the in-process per-profile lock added in #1175.
//!
//! These exercise the public locked API (`Storage::update`, `commit`,
//! `update_workspace_ordering`) end-to-end from outside the crate, the same
//! surface a third-party consumer would see.

use agent_of_empires::session::{update_workspace_ordering, Group, GroupTree, Instance, Storage};
use anyhow::Result;
use serial_test::serial;
use std::sync::{Arc, Barrier};

use crate::common::setup_temp_home;

#[test]
#[serial]
fn test_concurrent_updates_no_lost_updates() -> Result<()> {
    let _temp = setup_temp_home();

    let storage = Storage::new("default")?;
    storage.commit(&[], &GroupTree::new_with_groups(&[], &[]))?;

    let n_threads = 16usize;
    let start = Arc::new(Barrier::new(n_threads));
    std::thread::scope(|scope| {
        for tid in 0..n_threads {
            let start = Arc::clone(&start);
            scope.spawn(move || {
                start.wait();
                let storage = Storage::new("default").unwrap();
                storage
                    .update(|instances, _groups| {
                        instances.push(Instance::new(
                            &format!("worker-{tid}"),
                            &format!("/tmp/worker-{tid}"),
                        ));
                        Ok(())
                    })
                    .unwrap();
            });
        }
    });

    let loaded = storage.load()?;
    assert_eq!(loaded.len(), n_threads);
    let mut titles: Vec<_> = loaded.iter().map(|i| i.title.clone()).collect();
    titles.sort();
    for tid in 0..n_threads {
        assert!(titles.contains(&format!("worker-{tid}")));
    }
    Ok(())
}

#[test]
#[serial]
fn test_concurrent_workspace_ordering_merge() -> Result<()> {
    let _temp = setup_temp_home();

    update_workspace_ordering(|ord| {
        ord.order.clear();
        Ok(())
    })?;

    let n_threads = 12usize;
    let start = Arc::new(Barrier::new(n_threads));
    std::thread::scope(|scope| {
        for tid in 0..n_threads {
            let start = Arc::clone(&start);
            scope.spawn(move || {
                start.wait();
                update_workspace_ordering(|ord| {
                    ord.order.push(format!("/repo/{tid}::main"));
                    Ok(())
                })
                .unwrap();
            });
        }
    });

    let loaded = agent_of_empires::session::load_workspace_ordering()?;
    assert_eq!(loaded.order.len(), n_threads);
    for tid in 0..n_threads {
        assert!(loaded.order.contains(&format!("/repo/{tid}::main")));
    }
    Ok(())
}

#[test]
#[serial]
fn test_update_with_groups_and_instances_round_trip() -> Result<()> {
    let _temp = setup_temp_home();

    let storage = Storage::new("default")?;
    storage.commit(&[], &GroupTree::new_with_groups(&[], &[]))?;

    storage.update(|instances, groups| {
        let mut inst = Instance::new("project-a", "/tmp/a");
        inst.group_path = "work/clients".to_string();
        instances.push(inst);
        groups.push(Group::new("clients", "work/clients"));
        Ok(())
    })?;

    let (loaded_instances, loaded_groups) = storage.load_with_groups()?;
    assert_eq!(loaded_instances.len(), 1);
    assert_eq!(loaded_instances[0].group_path, "work/clients");
    assert_eq!(loaded_groups.len(), 1);
    assert_eq!(loaded_groups[0].name, "clients");
    Ok(())
}

#[test]
#[serial]
fn test_commit_overwrites_concurrent_update_by_design() -> Result<()> {
    let _temp = setup_temp_home();

    let storage = Storage::new("default")?;
    storage.commit(&[], &GroupTree::new_with_groups(&[], &[]))?;

    let entered = Arc::new(Barrier::new(2));
    let release = Arc::new(Barrier::new(2));
    let entered_clone = Arc::clone(&entered);
    let release_clone = Arc::clone(&release);

    let updater = std::thread::spawn(move || {
        let storage = Storage::new("default").unwrap();
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
    let committer = std::thread::spawn(|| {
        let storage = Storage::new("default").unwrap();
        storage
            .commit(
                &[Instance::new("from-commit", "/tmp/c")],
                &GroupTree::new_with_groups(&[], &[]),
            )
            .unwrap();
    });

    std::thread::sleep(std::time::Duration::from_millis(80));
    assert!(
        !committer.is_finished(),
        "commit should be blocked by update lock"
    );
    release.wait();
    updater.join().unwrap();
    committer.join().unwrap();

    let loaded = storage.load()?;
    assert_eq!(loaded.len(), 1);
    assert_eq!(
        loaded[0].title, "from-commit",
        "commit semantics: last writer wins after the lock is released"
    );
    Ok(())
}

/// Concurrent per-field updates on the same instance must not clobber each
/// other. Mirrors the cockpit-handler / status-poll pattern: thread A
/// mutates one field (`title`, simulating status poll), thread B mutates
/// a different field (`notify_on_idle`, standing in for the cockpit
/// handler's `cockpit_mode`; the latter is `#[cfg(feature = "serve")]`,
/// `notify_on_idle` is always available and exhibits the same lost-update
/// class). Both should land. Regression guard for review #5: a
/// wholesale-replace closure (using a pre-lock snapshot) would lose one
/// of the writes.
#[test]
#[serial]
fn test_concurrent_per_field_updates_no_clobber() -> Result<()> {
    let _temp = setup_temp_home();

    let storage = Storage::new("default")?;
    let seed = vec![Instance::new("session", "/tmp/session")];
    storage.commit(&seed, &GroupTree::new_with_groups(&seed, &[]))?;
    let target_id = storage.load()?[0].id.clone();

    let n_iterations = 16usize;
    let start = Arc::new(Barrier::new(2));
    let id_for_a = target_id.clone();
    let id_for_b = target_id.clone();
    let start_a = Arc::clone(&start);
    let start_b = Arc::clone(&start);

    let thread_a = std::thread::spawn(move || -> Result<()> {
        let storage = Storage::new("default")?;
        start_a.wait();
        for i in 0..n_iterations {
            storage.update(|all, _groups| {
                if let Some(slot) = all.iter_mut().find(|i| i.id == id_for_a) {
                    slot.title = format!("from-A-{i}");
                }
                Ok(())
            })?;
        }
        Ok(())
    });

    let thread_b = std::thread::spawn(move || -> Result<()> {
        let storage = Storage::new("default")?;
        start_b.wait();
        for _ in 0..n_iterations {
            storage.update(|all, _groups| {
                if let Some(slot) = all.iter_mut().find(|i| i.id == id_for_b) {
                    slot.notify_on_idle = Some(true);
                }
                Ok(())
            })?;
        }
        Ok(())
    });

    thread_a.join().unwrap()?;
    thread_b.join().unwrap()?;

    let loaded = storage.load()?;
    assert_eq!(loaded.len(), 1);
    assert!(
        loaded[0].title.starts_with("from-A-"),
        "thread A's title write must be preserved (got: {})",
        loaded[0].title
    );
    assert_eq!(
        loaded[0].notify_on_idle,
        Some(true),
        "thread B's notify_on_idle write must be preserved"
    );
    Ok(())
}

/// `Storage::update` must surface a disk-write failure as `Err` so the
/// `delete_session` handler can return HTTP 500 instead of silently
/// dropping the in-memory entry. Regression guard for review #7.
#[cfg(unix)]
#[test]
#[serial]
fn test_update_propagates_disk_write_failure() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let _temp = setup_temp_home();

    let storage = Storage::new("default")?;
    let seed = vec![Instance::new("session", "/tmp/s")];
    storage.commit(&seed, &GroupTree::new_with_groups(&seed, &[]))?;

    let profile_dir = agent_of_empires::session::get_profile_dir("default")?;

    let original_perms = std::fs::metadata(&profile_dir)?.permissions();
    let mut readonly_perms = original_perms.clone();
    readonly_perms.set_mode(0o555);
    std::fs::set_permissions(&profile_dir, readonly_perms)?;

    // Root bypasses 0o555 on most Unixes, so the disk write would succeed
    // and this regression guard would falsely fail. Probe the lock before
    // running the real assertion and skip cleanly when we can still write.
    let probe = profile_dir.join(".perm_probe");
    if std::fs::write(&probe, b"x").is_ok() {
        let _ = std::fs::remove_file(&probe);
        std::fs::set_permissions(&profile_dir, original_perms)?;
        eprintln!(
            "test_update_propagates_disk_write_failure: skipping \
             (parent dir writable despite 0o555; likely running as root)"
        );
        return Ok(());
    }

    let result: Result<()> = storage.update(|instances, _groups| {
        instances.clear();
        Ok(())
    });

    std::fs::set_permissions(&profile_dir, original_perms)?;

    assert!(
        result.is_err(),
        "update must Err when disk write fails (read-only parent dir)"
    );

    let reloaded = storage.load()?;
    assert_eq!(
        reloaded.len(),
        1,
        "disk state must be unchanged when atomic_write fails"
    );
    Ok(())
}
