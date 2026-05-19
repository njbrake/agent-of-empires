//! Integration tests for the in-process per-profile lock added in #1175.
//!
//! These exercise the public locked API (`Storage::update`, `commit`,
//! `update_workspace_ordering`) end-to-end from outside the crate, the same
//! surface a third-party consumer would see.

use agent_of_empires::session::{update_workspace_ordering, Group, GroupTree, Instance, Storage};
use anyhow::Result;
use serial_test::serial;
use std::sync::{Arc, Barrier};

mod common;
use common::setup_temp_home;

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
