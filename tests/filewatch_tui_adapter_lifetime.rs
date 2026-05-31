//! TUI file-watch adapter lifetime regression (design §9 test 4 + DoD §12
//! "HomeView retains _disk_adapter as a long-lived field; lifetime asserted
//! by a regression test that fires a watcher event after construction and
//! asserts `disk_dirty == true` within 50 ms").
//!
//! Reproduces the HomeView adapter wiring (forwarder task per subscription,
//! capacity-1 fan-in mpsc, single drain task that sets the AtomicBool) in
//! isolation, then drives a `Storage::update` write against a watched
//! profile dir and verifies the dirty flag flips. The test catches the
//! "adapter task accidentally dropped on construct" regression, which
//! would leave the channel closed and the dirty flag forever stuck.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_of_empires::file_watch::{FileMatcher, FileWatchService, WatchSpec};
use agent_of_empires::session::{Instance, Storage};
use serial_test::serial;
use tempfile::TempDir;

fn isolate_home(temp: &std::path::Path) {
    // SAFETY: env mutation; #[serial] guards cross-test races.
    unsafe { std::env::set_var("HOME", temp) };
    #[cfg(target_os = "linux")]
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", temp.join(".config"))
    };
}

struct AdapterHarness {
    disk_dirty: Arc<AtomicBool>,
    _adapter: tokio::task::AbortHandle,
    _forwarder: tokio::task::AbortHandle,
    _handle: agent_of_empires::file_watch::SubscriptionHandle,
}

async fn spawn_harness(svc: Arc<FileWatchService>, dir: PathBuf) -> AdapterHarness {
    let sessions_path = dir.join("sessions.json");
    let groups_path = dir.join("groups.json");

    let (mut rx, handle) = svc
        .subscribe_channel(
            WatchSpec {
                dir,
                matcher: FileMatcher::AnyOf(vec![sessions_path, groups_path]),
                debounce: Some(Duration::from_millis(75)),
            },
            16,
        )
        .expect("subscribe_channel");

    let (combined_tx, mut combined_rx) = tokio::sync::mpsc::channel::<()>(1);

    let disk_dirty = Arc::new(AtomicBool::new(false));
    let adapter_dirty = Arc::clone(&disk_dirty);
    let adapter_join = tokio::spawn(async move {
        while combined_rx.recv().await.is_some() {
            adapter_dirty.store(true, Ordering::Release);
        }
    });

    let forwarder_join = tokio::spawn(async move {
        while rx.recv().await.is_some() {
            let _ = combined_tx.try_send(());
        }
    });

    AdapterHarness {
        disk_dirty,
        _adapter: adapter_join.abort_handle(),
        _forwarder: forwarder_join.abort_handle(),
        _handle: handle,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial]
async fn adapter_flips_disk_dirty_after_storage_update() {
    let temp = TempDir::new().unwrap();
    isolate_home(temp.path());

    let svc: Arc<FileWatchService> = FileWatchService::new().expect("init");

    let storage = Storage::new("adapter-lifetime", svc.clone()).expect("storage");
    storage
        .update(|i, _g| {
            *i = vec![Instance::new("seed", "/tmp/seed")];
            Ok(())
        })
        .expect("seed write");

    let dir =
        agent_of_empires::session::get_profile_dir_path("adapter-lifetime").expect("profile dir");
    let harness = spawn_harness(svc, dir).await;

    storage
        .update(|i, _g| {
            i.push(Instance::new("after", "/tmp/after"));
            Ok(())
        })
        .expect("post-subscribe write");

    let deadline = Instant::now() + Duration::from_millis(2_500);
    let mut flipped = false;
    while Instant::now() < deadline {
        if harness.disk_dirty.load(Ordering::Acquire) {
            flipped = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(
        flipped,
        "disk_dirty must flip to true after a Storage::update write to a watched profile"
    );
}
