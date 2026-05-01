//! E2E coverage for the serve dialog state machine.
//!
//! Targeted regression tests for the `R`-key ModePicker + Confirm flow
//! introduced with the Tailscale Funnel transport picker. Compiled only
//! with `--features serve` since the serve dialog doesn't exist
//! otherwise; run via:
//!
//! ```sh
//! cargo test --test e2e --features serve -- tui_serve_dialog
//! ```
#![cfg(feature = "serve")]

use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use serial_test::serial;

use crate::harness::{require_tmux, TuiTestHarness};

/// Resolve the daemon's PID file inside the harness's isolated home.
/// Mirrors `crate::session::get_app_dir`'s platform split.
fn daemon_pid_path(h: &TuiTestHarness) -> PathBuf {
    let dir = if cfg!(target_os = "linux") {
        h.home_path().join(".config").join("agent-of-empires")
    } else {
        h.home_path().join(".agent-of-empires")
    };
    dir.join("serve.pid")
}

/// Bind a TCP listener to an ephemeral port, drop it, and return the port.
/// Tiny TOCTOU window before the daemon binds, but acceptable for a serial
/// test.
fn pick_free_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    l.local_addr().expect("local_addr").port()
}

/// Poll until the daemon accepts a TCP connection on `port`. The parent
/// `aoe serve --daemon` returns as soon as it has spawned the child, so a
/// successful exit doesn't prove the child bound the port; this is the
/// real signal that the daemon is up.
fn wait_for_port(port: u16, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if TcpStream::connect_timeout(
            &format!("127.0.0.1:{}", port).parse().unwrap(),
            Duration::from_millis(200),
        )
        .is_ok()
        {
            return true;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// True iff the kernel still has a process with this PID.
fn pid_alive(pid: i32) -> bool {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok()
}

/// Pressing `R` from the home screen opens the serve ModePicker,
/// which must render both cards (Local + Internet) and surface the
/// transport-picker-deferred hint on the Tunnel card ("Pick transport
/// on next screen.").
#[test]
#[serial]
fn tui_serve_dialog_opens_to_mode_picker() {
    require_tmux!();

    let mut h = TuiTestHarness::new("serve_mode_picker");
    h.spawn_tui();

    h.wait_for(" aoe [");
    h.send_keys("R");

    h.wait_for("How should this be reachable?");
    h.assert_screen_contains("Local network");
    h.assert_screen_contains("Internet (HTTPS)");
    // The Tunnel card defers the transport choice to the next screen.
    // If this line disappears, the ModePicker copy is out of sync with
    // the Confirm-screen picker it hands off to.
    h.assert_screen_contains("Pick transport on next screen.");
}

/// Esc dismisses the serve dialog and returns to the home screen
/// without spawning anything. Regression guard against state-transition
/// bugs where ModePicker might latch onto a stale mode.
#[test]
#[serial]
fn tui_serve_dialog_escape_returns_home() {
    require_tmux!();

    let mut h = TuiTestHarness::new("serve_mode_picker_esc");
    h.spawn_tui();

    h.wait_for(" aoe [");
    h.send_keys("R");
    h.wait_for("How should this be reachable?");

    h.send_keys("Escape");
    // Home-screen footer is the tell that we've returned.
    h.wait_for("No sessions yet");
}

/// `aoe serve --daemon` must spawn a child that actually binds the port and
/// stays alive. Regression guard for the self-detection bug where the parent
/// pre-wrote the child's PID into `serve.pid`, then the child re-entered
/// `run()`, found its own PID via `daemon_pid()`, and bailed with
/// "A serve daemon is already running" — about itself.
#[test]
#[serial]
fn cli_serve_daemon_starts_and_stops_cleanly() {
    let h = TuiTestHarness::new("serve_daemon_lifecycle");
    let port = pick_free_port();
    let port_s = port.to_string();

    let start = h.run_cli(&["serve", "--daemon", "--port", &port_s, "--no-auth"]);
    assert!(
        start.status.success(),
        "aoe serve --daemon failed.\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&start.stdout),
        String::from_utf8_lossy(&start.stderr),
    );

    let pid_path = daemon_pid_path(&h);
    assert!(
        wait_for_port(port, Duration::from_secs(10)),
        "daemon never bound port {} (child likely self-detected and exited).\n\
         pid file exists: {}\n\
         serve.log:\n{}",
        port,
        pid_path.exists(),
        std::fs::read_to_string(pid_path.with_file_name("serve.log")).unwrap_or_default(),
    );

    let pid: i32 = std::fs::read_to_string(&pid_path)
        .expect("serve.pid should exist after daemon starts")
        .trim()
        .parse()
        .expect("serve.pid should contain a valid integer");
    assert!(
        pid_alive(pid),
        "child PID {} not alive after port bind",
        pid
    );

    let stop = h.run_cli(&["serve", "--stop"]);
    assert!(
        stop.status.success(),
        "aoe serve --stop failed.\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&stop.stdout),
        String::from_utf8_lossy(&stop.stderr),
    );

    let deadline = Instant::now() + Duration::from_secs(3);
    while pid_alive(pid) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        !pid_alive(pid),
        "daemon PID {} still alive after --stop",
        pid
    );
    assert!(
        !pid_path.exists(),
        "serve.pid should be cleaned up after --stop, found at {}",
        pid_path.display()
    );
}
