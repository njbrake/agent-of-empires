//! e2e: in-process Storage::update fires the Local fast-path so the TUI's
//! own writes propagate within the primitive's debounce window.
//!
//! The test runs a peer `Storage::update` against the same app dir the TUI
//! watches, but in this scenario the dispatcher's per-key debounce window
//! collapses the in-process Local notify (from the test side, not the TUI
//! side) and the kernel echo into a single delivery. The TUI process sees
//! exactly one wake on the dirty flag, then `reload_storage_only` runs.
//! End-to-end, the screen reflects two back-to-back writes within sub-tick
//! budget. The C1 fix (operand order on `disk_dirty.swap`) keeps the
//! second event's kick latched if the first reload is still in flight.

use std::sync::Arc;
use std::time::Duration;

use agent_of_empires::file_watch::FileWatchService;
use agent_of_empires::session::{Instance, Storage};
use serial_test::serial;

use crate::harness::{require_tmux, TuiTestHarness};

#[test]
#[serial]
fn back_to_back_storage_updates_collapse_to_a_single_visible_reload() {
    require_tmux!();

    let mut h = TuiTestHarness::new("filewatch_live_send");
    h.spawn_tui();
    h.wait_for(" aoe ");

    // SAFETY: env mutation; the harness owns its own isolated $HOME.
    // `#[serial]` guards cross-test races.
    unsafe { std::env::set_var("HOME", h.home_path()) };
    #[cfg(target_os = "linux")]
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", h.home_path().join(".config"))
    };

    let svc: Arc<FileWatchService> = FileWatchService::noop();
    let storage = Storage::new("default", svc).expect("storage in test process");

    let first = "filewatch-live-row-a";
    let second = "filewatch-live-row-b";

    storage
        .update(|i, _g| {
            let mut inst = Instance::new(first, "/tmp/filewatch-a");
            inst.source_profile = "default".to_string();
            i.push(inst);
            Ok(())
        })
        .expect("first peer write");
    storage
        .update(|i, _g| {
            let mut inst = Instance::new(second, "/tmp/filewatch-b");
            inst.source_profile = "default".to_string();
            i.push(inst);
            Ok(())
        })
        .expect("second peer write");

    h.wait_for_timeout(first, Duration::from_millis(1_500));
    h.wait_for_timeout(second, Duration::from_millis(1_500));
}
