use serial_test::serial;
use std::time::Duration;

use crate::harness::{require_tmux, TuiTestHarness};

/// Helper: create a profile with a session in the harness's isolated home.
fn create_profile_with_session(h: &TuiTestHarness, profile: &str, title: &str) {
    let config_dir = if cfg!(target_os = "linux") {
        h.home_path().join(".config").join("agent-of-empires")
    } else {
        h.home_path().join(".agent-of-empires")
    };
    let profile_dir = config_dir.join("profiles").join(profile);
    std::fs::create_dir_all(&profile_dir).expect("create profile dir");

    let session = format!(
        r#"[{{"id":"test_{profile}","title":"{title}","project_path":"/tmp/{profile}","group_path":"","command":"","tool":"claude","yolo_mode":false,"status":"idle","created_at":"2026-01-01T00:00:00Z"}}]"#,
    );
    std::fs::write(profile_dir.join("sessions.json"), session).expect("write sessions.json");
}

#[test]
#[serial]
fn test_default_view_shows_all_profiles() {
    require_tmux!();

    let mut h = TuiTestHarness::new("unified_all");
    create_profile_with_session(&h, "alpha", "Alpha Session");
    create_profile_with_session(&h, "beta", "Beta Session");
    h.spawn_tui();

    h.wait_for("[all]");
    h.assert_screen_contains("alpha");
    h.assert_screen_contains("beta");
    h.assert_screen_contains("Alpha Session");
    h.assert_screen_contains("Beta Session");
}

#[test]
#[serial]
fn test_profile_filter_via_picker() {
    require_tmux!();

    let mut h = TuiTestHarness::new("unified_filter");
    create_profile_with_session(&h, "alpha", "Alpha Session");
    create_profile_with_session(&h, "beta", "Beta Session");
    h.spawn_tui();

    h.wait_for("[all]");

    // Open picker and select "alpha"
    h.send_keys("P");
    h.wait_for("Profiles");
    // In all-mode, profiles are listed directly (no "all" entry)
    // "alpha" should be first alphabetically
    h.send_keys("Enter");

    h.wait_for("[alpha]");
    h.assert_screen_contains("Alpha Session");
    h.assert_screen_not_contains("Beta Session");
}

#[test]
#[serial]
fn test_return_to_all_view_via_picker() {
    require_tmux!();

    let mut h = TuiTestHarness::new("unified_return");
    create_profile_with_session(&h, "alpha", "Alpha Session");
    create_profile_with_session(&h, "beta", "Beta Session");
    h.spawn_tui();

    h.wait_for("[all]");

    // Filter to alpha
    h.send_keys("P");
    h.wait_for("Profiles");
    h.send_keys("Enter");
    h.wait_for("[alpha]");

    // Return to all via picker ("all" should be at top in filtered mode)
    h.send_keys("P");
    h.wait_for("Profiles");
    // Navigate to top where "all" entry is and select it
    h.send_keys("k");
    std::thread::sleep(Duration::from_millis(50));
    h.send_keys("k");
    std::thread::sleep(Duration::from_millis(50));
    h.send_keys("k");
    std::thread::sleep(Duration::from_millis(50));
    h.send_keys("Enter");

    h.wait_for("[all]");
    h.assert_screen_contains("Alpha Session");
    h.assert_screen_contains("Beta Session");
}

#[test]
#[serial]
fn test_profile_header_collapse() {
    require_tmux!();

    let mut h = TuiTestHarness::new("unified_collapse");
    create_profile_with_session(&h, "alpha", "Alpha Session");
    create_profile_with_session(&h, "beta", "Beta Session");
    h.spawn_tui();

    h.wait_for("[all]");
    h.assert_screen_contains("Alpha Session");

    // Cursor should be on "alpha" header (first item). Press Enter to collapse.
    h.send_keys("Enter");
    std::thread::sleep(Duration::from_millis(200));

    // Alpha's session should be hidden, beta's still visible
    h.assert_screen_not_contains("Alpha Session");
    h.assert_screen_contains("Beta Session");

    // Expand again
    // Navigate back to alpha header (might have moved)
    h.send_keys("g"); // go to top
    std::thread::sleep(Duration::from_millis(50));
    h.send_keys("Enter");
    std::thread::sleep(Duration::from_millis(200));

    h.assert_screen_contains("Alpha Session");
}
