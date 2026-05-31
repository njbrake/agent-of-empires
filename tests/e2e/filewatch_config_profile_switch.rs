//! e2e: writes to a non-active profile's `config.toml` are subscribed even
//! when the profile is not active. Switching to that profile picks up the
//! external edit without a second tick of latency (§6 "watch every known
//! profile, not only the active one").
//!
//! The TUI subscribes to all profile configs at startup. The test
//! peer-writes profile B's `config.toml` while profile A (`default`) is
//! active, then uses the profile picker to switch to B. After the
//! switch, `switch_profile` calls `refresh_from_config()` which resolves
//! B's config (including the just-written `confirm_before_quit = true`),
//! and pressing `q` shows the quit confirmation dialog instead of
//! exiting immediately.

use std::time::Duration;

use serial_test::serial;

use crate::harness::{app_dir_in, require_tmux, TuiTestHarness};

#[test]
#[serial(file_watch)]
fn peer_edit_to_inactive_profile_surfaces_after_switch() {
    require_tmux!();

    let mut h = TuiTestHarness::new("filewatch_config_profile_switch");

    let new_profile = "scratch_b";
    let config_dir = app_dir_in(h.home_path());
    let profile_dir = config_dir.join("profiles").join(new_profile);
    std::fs::create_dir_all(&profile_dir).expect("seed profile B dir");

    h.spawn(&["--profile", "default"]);
    h.wait_for(" aoe ");

    let profile_b_config = profile_dir.join("config.toml");
    std::fs::write(
        &profile_b_config,
        r#"[session]
confirm_before_quit = true
"#,
    )
    .expect("peer-write profile B config.toml");

    std::thread::sleep(Duration::from_millis(800));

    h.send_keys("P");
    h.wait_for("Profiles");
    h.send_keys("Down");
    h.send_keys("Enter");
    h.wait_for_absent("Profiles", Duration::from_secs(5));

    h.send_keys("q");
    h.wait_for_timeout("Quit Agent of Empires", Duration::from_millis(3_000));
    h.send_keys("Escape");
}
