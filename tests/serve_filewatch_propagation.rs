//! Integration test for the server-consumer Local + Kernel propagation path
//! (server-migration doc §8.1 test 1).
//!
//! Subtest (c) Local-Kernel collapse semantics is verified directly: drive
//! `Storage::update` from inside the test process so both the in-process
//! Local notify and the kernel echo race the dispatcher; assert exactly
//! ONE delivery per logical write within the 75ms debounce window.
//!
//! Subtests (a) and (b) of the migration doc require spawning a real
//! `aoe serve` subprocess and driving its REST API; that requires
//! tunnel / port / auth setup beyond what's practical in a unit-style
//! integration test. The collapse-semantics subtest verifies the
//! Storage::update -> notify_local_change -> dispatcher Local arm ->
//! debounce-collapse with kernel echo -> subscriber receipt path
//! end-to-end through the actual production code path.

#![cfg(feature = "serve")]

use std::sync::Arc;
use std::time::Duration;

use agent_of_empires::file_watch::{FileMatcher, FileWatchService, WatchSpec};
use agent_of_empires::session::{Instance, Storage};
use serial_test::serial;
use tempfile::TempDir;
use tokio::time::timeout;

const KERNEL_WAIT: Duration = Duration::from_millis(2_500);
const NEG_WAIT: Duration = Duration::from_millis(300);

fn isolate_home(temp: &std::path::Path) {
    // SAFETY: env mutation; #[serial] guards cross-test races.
    unsafe { std::env::set_var("HOME", temp) };
    #[cfg(target_os = "linux")]
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", temp.join(".config"))
    };
}

/// Storage::update fires `notify_local_change` after each successful
/// `atomic_write`. Subscribers wired to the same `FileWatchService`
/// receive exactly ONE delivery per logical update within the debounce
/// window: the Local event arms the slot, the kernel echo refreshes
/// `fire_at` but does not replace the pending entry (both are Upserted),
/// and the per-key debounce collapses the burst.
#[tokio::test]
#[serial]
async fn storage_update_collapses_local_and_kernel_to_one_delivery() {
    let temp = TempDir::new().unwrap();
    isolate_home(temp.path());
    let svc: Arc<FileWatchService> = FileWatchService::new().expect("init");
    let storage = Storage::new("propagation-test", svc.clone()).expect("storage");

    // Pre-create both files so canonicalize matches kernel paths from
    // the first dispatch. Subscribe AFTER seeding so the seed's kernel
    // events fire pre-subscribe and are not delivered.
    storage
        .update(|i, _g| {
            *i = vec![Instance::new("seed", "/tmp/seed")];
            Ok(())
        })
        .expect("seed write");

    let profile_dir = agent_of_empires::session::get_profile_dir_path("propagation-test")
        .expect("resolve profile dir");
    let sessions_path = profile_dir.join("sessions.json");
    let groups_path = profile_dir.join("groups.json");

    let (mut rx, _h) = svc
        .subscribe_channel(
            WatchSpec {
                dir: profile_dir,
                matcher: FileMatcher::AnyOf(vec![sessions_path.clone(), groups_path]),
                debounce: Some(Duration::from_millis(75)),
            },
            16,
        )
        .expect("subscribe");

    // Storage::update issues notify_local_change AFTER atomic_write returns.
    // The kernel rename echo arrives ~ms later for the same canonical path.
    storage
        .update(|i, _g| {
            i.push(Instance::new("added", "/tmp/added"));
            Ok(())
        })
        .expect("update");

    // Exactly ONE delivery within the kernel-wait budget for sessions.json.
    let first = timeout(KERNEL_WAIT, rx.recv())
        .await
        .expect("at least one event")
        .expect("channel open");
    assert!(
        first.path.file_name().is_some_and(|n| n == "sessions.json"),
        "expected sessions.json event, got {:?}",
        first.path
    );

    // No second delivery for the same logical write within a tight budget:
    // Local + kernel echo collapsed to one.
    let second = timeout(NEG_WAIT, rx.recv()).await;
    assert!(
        second.is_err() || matches!(second, Ok(None)),
        "Local + kernel echo for the same Storage::update must collapse to one delivery"
    );
}

/// `Storage::update` propagates a peer-process write through the kernel
/// path even when the in-process Local fast path is unavailable (noop).
/// Simulates the cross-process path: the writer holds a noop service so
/// `notify_local_change` is silent; the reader holds a live service whose
/// kernel watcher picks up the rename.
#[tokio::test]
#[serial]
async fn cross_process_kernel_path_delivers_when_local_is_noop() {
    let temp = TempDir::new().unwrap();
    isolate_home(temp.path());

    let writer_storage = Storage::new("xproc-test", FileWatchService::noop()).expect("writer");
    writer_storage
        .update(|i, _g| {
            *i = vec![Instance::new("seed", "/tmp/seed")];
            Ok(())
        })
        .expect("seed");

    let profile_dir = agent_of_empires::session::get_profile_dir_path("xproc-test").expect("dir");
    let sessions_path = profile_dir.join("sessions.json");
    let groups_path = profile_dir.join("groups.json");

    let reader_svc: Arc<FileWatchService> = FileWatchService::new().expect("reader init");
    let (mut rx, _h) = reader_svc
        .subscribe_channel(
            WatchSpec {
                dir: profile_dir,
                matcher: FileMatcher::AnyOf(vec![sessions_path, groups_path]),
                debounce: Some(Duration::from_millis(75)),
            },
            16,
        )
        .expect("subscribe");

    writer_storage
        .update(|i, _g| {
            i.push(Instance::new("peer", "/tmp/peer"));
            Ok(())
        })
        .expect("peer write");

    let ev = timeout(KERNEL_WAIT, rx.recv())
        .await
        .expect("kernel event arrives within budget")
        .expect("channel open");
    assert!(
        ev.path.file_name().is_some_and(|n| n == "sessions.json"),
        "expected sessions.json event, got {:?}",
        ev.path
    );
}
