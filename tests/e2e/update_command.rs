//! E2E tests for `aoe update`.

use serial_test::serial;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

fn aoe_binary() -> &'static str {
    env!("CARGO_BIN_EXE_aoe")
}

/// Test that `aoe update --dry-run` invokes `brew list aoe` during installation detection.
///
/// Requires: GitHub API access (check_for_update is called with force=true) and a newer
/// version than 1.4.6 to be published. In CI where both are available, the test validates
/// that the brew detection probe runs regardless of dry-run mode. Marked #[ignore] because
/// it depends on GitHub being reachable and a newer release being available.
#[test]
#[ignore]
#[serial]
fn update_calls_brew_when_method_is_homebrew() {
    // Set up a temp dir as PATH-shimmed `brew` that records argv.
    let shim_dir = tempfile::tempdir().unwrap();
    let brew_log = shim_dir.path().join("brew.log");
    let brew_shim = shim_dir.path().join("brew");
    fs::write(
        &brew_shim,
        format!(
            "#!/bin/sh\necho \"$@\" >> {}\nif [ \"$1\" = \"list\" ]; then\n  echo /usr/local/bin/aoe\n  exit 0\nfi\nexit 0\n",
            brew_log.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&brew_shim, fs::Permissions::from_mode(0o755)).unwrap();

    // Create an isolated config dir with a fake update cache so check_for_update
    // thinks there's a newer version available (without hitting GitHub).
    let config_home = tempfile::tempdir().unwrap();
    let app_dir = config_home.path().join("agent-of-empires");
    fs::create_dir_all(&app_dir).unwrap();

    let cache = serde_json::json!({
        "checked_at": "2026-04-27T00:00:00Z",
        "latest_version": "1.4.7",
        "releases": []
    });
    fs::write(
        app_dir.join("update_cache.json"),
        serde_json::to_string_pretty(&cache).unwrap(),
    )
    .unwrap();

    // Run `aoe update --dry-run` with the shim on PATH and isolated XDG_CONFIG_HOME.
    // The detection path probes brew via `brew list aoe` regardless of dry-run.
    let path = format!(
        "{}:{}",
        shim_dir.path().display(),
        std::env::var("PATH").unwrap()
    );

    let output = Command::new(aoe_binary())
        .args(["update", "--dry-run"])
        .env("PATH", &path)
        .env("XDG_CONFIG_HOME", config_home.path())
        .output()
        .expect("running aoe update --dry-run");

    // The detection probe ran `brew list aoe`, so the log must exist.
    let log = fs::read_to_string(&brew_log).unwrap_or_default();
    assert!(
        log.contains("list aoe"),
        "expected `brew list aoe` to be invoked; log was: {log:?}\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}
