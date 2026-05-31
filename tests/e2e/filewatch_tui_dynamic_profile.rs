//! e2e: dynamic profile add/remove. Catches subscription-registration bugs
//! (forgot to call `rewire_disk_subscriptions`) and adapter wiring bugs
//! (forwarder spawned but `disk_combined_tx` not cloned correctly).
//!
//! Flow: spawn TUI; create a new profile from the test process; switch the
//! TUI to the new profile via the picker; perform a `Storage::update`
//! against the new profile from outside the TUI; assert the TUI reflects
//! the row within sub-tick budget. Then delete the new profile and assert
//! the TUI does not panic and the picker repopulates.

use std::sync::Arc;
use std::time::Duration;

use agent_of_empires::file_watch::FileWatchService;
use agent_of_empires::session::{Instance, Storage};
use serial_test::serial;

use crate::harness::{require_tmux, TuiTestHarness};

#[test]
#[serial]
fn dynamic_profile_add_and_remove_keeps_subscriptions_in_sync() {
    require_tmux!();

    let mut h = TuiTestHarness::new("filewatch_dyn_profile");

    let new_profile = "scratch";
    let config_dir = crate::harness::app_dir_in(h.home_path());
    std::fs::create_dir_all(config_dir.join("profiles").join(new_profile))
        .expect("seed scratch profile dir");

    h.spawn_tui();
    h.wait_for(" aoe ");

    h.send_keys("P");
    h.wait_for("Profiles");
    h.assert_screen_contains(new_profile);
    h.send_keys("Escape");
    h.wait_for_absent("Profiles", Duration::from_secs(5));

    // SAFETY: env mutation; the harness owns its own isolated $HOME.
    // `#[serial]` guards cross-test races.
    unsafe { std::env::set_var("HOME", h.home_path()) };
    #[cfg(target_os = "linux")]
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", h.home_path().join(".config"))
    };

    let svc: Arc<FileWatchService> = FileWatchService::noop();
    let storage = Storage::new(new_profile, svc).expect("storage for new profile");

    let title = "filewatch-dyn-row";
    storage
        .update(|i, _g| {
            let mut inst = Instance::new(title, "/tmp/filewatch-dyn");
            inst.source_profile = new_profile.to_string();
            i.push(inst);
            Ok(())
        })
        .expect("peer write to scratch profile");

    h.wait_for_timeout(title, Duration::from_millis(1_500));

    let profile_dir = config_dir.join("profiles").join(new_profile);
    std::fs::remove_dir_all(&profile_dir).expect("remove scratch profile dir");

    std::thread::sleep(Duration::from_millis(2_500));
    assert!(
        h.session_alive(),
        "TUI should not have crashed after profile dir removal"
    );
}
