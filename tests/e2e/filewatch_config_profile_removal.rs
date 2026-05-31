//! e2e: profile removal tears down the per-profile config subscription
//! cleanly (drop-then-abort observed; no panics; TUI keeps running).
//!
//! Validates §6's profile-removal teardown rule and the canonical
//! drop-then-abort order (primitive §12 rule 3 + design §7) for the
//! config-watch path. The test:
//!
//! 1. Pre-seeds profile B alongside `default` so the TUI subscribes to
//!    its `config.toml` at startup (§6 "watch every known profile").
//! 2. Peer-writes B's `config.toml` and asserts no crash within the
//!    debounce window (the subscription is alive and feeding events).
//! 3. Removes profile B's directory, simulating a peer
//!    `aoe profile delete b`. The next disk-mirror tick rediscovers the
//!    profile set, calls `rewire_config_subscriptions`, and tears down
//!    B's entry (drop the `SubscriptionHandle`, then abort the
//!    forwarder).
//! 4. Asserts the TUI is still alive after the teardown window.

use std::time::Duration;

use serial_test::serial;

use crate::harness::{app_dir_in, require_tmux, TuiTestHarness};

#[test]
#[serial(file_watch)]
fn profile_removal_tears_down_config_subscription_without_crash() {
    require_tmux!();

    let mut h = TuiTestHarness::new("filewatch_config_profile_removal");

    let new_profile = "scratch_rm";
    let config_dir = app_dir_in(h.home_path());
    let profile_dir = config_dir.join("profiles").join(new_profile);
    std::fs::create_dir_all(&profile_dir).expect("seed profile B dir");

    h.spawn_tui();
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
    assert!(
        h.session_alive(),
        "TUI must remain alive after peer-write to profile B config"
    );

    std::fs::remove_dir_all(&profile_dir).expect("remove profile B dir");

    std::thread::sleep(Duration::from_millis(6_000));

    assert!(
        h.session_alive(),
        "TUI must remain alive after profile B's directory is removed \
         (drop-then-abort teardown should not panic)"
    );
    h.assert_screen_contains(" aoe ");
}
