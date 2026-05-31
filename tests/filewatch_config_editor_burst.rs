//! Editor burst coalescing for config live-reload (design §9 test 4).
//!
//! Subscribes to `<dir>/config.toml` with the same spec
//! (`FileMatcher::Exact`, 100ms debounce) the TUI uses for the global
//! and per-profile config consumers. Then simulates a vim-style save:
//!
//! 1. Write a tempfile (`config.toml.tmp~`). The primitive's tempfile
//!    filter (`src/file_watch.rs` §8.1) drops the event.
//! 2. Rename the tempfile to `config.toml`. A Modify event for the
//!    final path fires once content has landed.
//! 3. `chmod` `config.toml`. A second Modify event for the final path
//!    fires within microseconds of the rename.
//!
//! The 100ms debounce coalesces (2) and (3) into a single delivery.
//! End to end, exactly ONE event reaches the consumer side per logical
//! save, which means `refresh_from_config` runs exactly once per save.
//!
//! This is the primitive-level proof of the property; the e2e tests
//! cover the integration-level proof (TUI process, real watcher, real
//! tick loop).

use std::path::PathBuf;
use std::time::Duration;

use agent_of_empires::file_watch::{FileMatcher, FileWatchService, WatchSpec};
use serial_test::serial;
use tempfile::TempDir;
use tokio::time::timeout;

const BURST_DEBOUNCE: Duration = Duration::from_millis(100);
const POST_BURST_QUIET: Duration = Duration::from_millis(400);

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[serial(file_watch)]
async fn vim_style_save_burst_collapses_to_a_single_delivery() {
    let svc = FileWatchService::new().expect("init service");
    let tmp = TempDir::new().expect("tempdir");
    let dir: PathBuf = tmp
        .path()
        .canonicalize()
        .expect("canonicalize tempdir (macOS resolves /var to /private/var)");
    let final_path = dir.join("config.toml");
    let temp_path = dir.join("config.toml.tmp~");

    let (mut rx, _handle) = svc
        .subscribe_channel(
            WatchSpec {
                dir: dir.clone(),
                matcher: FileMatcher::Exact(final_path.clone()),
                debounce: Some(BURST_DEBOUNCE),
            },
            4,
        )
        .expect("subscribe_channel");

    std::fs::write(&final_path, b"theme = { idle_decay_minutes = 5 }\n")
        .expect("seed final_path so rename has something to overwrite");
    let first = timeout(Duration::from_millis(2_500), rx.recv())
        .await
        .expect("seed event arrives within 2.5s")
        .expect("seed event channel open");
    assert_eq!(
        first.path, final_path,
        "the seed write should match the spec's exact matcher"
    );

    while timeout(POST_BURST_QUIET, rx.recv()).await.is_ok() {}

    std::fs::write(&temp_path, b"theme = { idle_decay_minutes = 7 }\n")
        .expect("write tempfile (vim writebackup pattern)");
    std::fs::rename(&temp_path, &final_path).expect("rename tempfile to final path");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::metadata(&final_path)
            .expect("stat final_path")
            .permissions();
        let mut perms = perms;
        perms.set_mode(0o644);
        std::fs::set_permissions(&final_path, perms).expect("chmod final_path");
    }

    let burst_event = timeout(Duration::from_millis(2_500), rx.recv())
        .await
        .expect("burst event arrives within 2.5s")
        .expect("burst event channel open");
    assert_eq!(
        burst_event.path, final_path,
        "burst event must target final config.toml"
    );

    let trailing = timeout(POST_BURST_QUIET, rx.recv()).await;
    assert!(
        trailing.is_err(),
        "100ms debounce must coalesce write + rename + chmod into a \
         single delivery; saw extra event {trailing:?}"
    );
}
