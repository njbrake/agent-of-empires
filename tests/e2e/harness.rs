//! Core e2e test harness built on tmux.
//!
//! `TuiTestHarness` launches `aoe` in a detached tmux session with an isolated
//! `$HOME`, sends keystrokes, captures screen output, and polls for expected
//! text. It also provides `run_cli` for exercising CLI subcommands as plain
//! subprocesses (no tmux).
//!
//! ## Recording
//!
//! Set `RECORD_E2E=1` to record each TUI test as an asciinema `.cast` file and
//! convert it to a GIF via `agg`. Recordings are saved to
//! `target/e2e-recordings/`. Both `asciinema` and `agg` must be on `$PATH`.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use tempfile::TempDir;

// ---------------------------------------------------------------------------
// tmux availability guard
// ---------------------------------------------------------------------------

pub fn tmux_available() -> bool {
    Command::new("tmux")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Skip the calling test if tmux is not installed.
macro_rules! require_tmux {
    () => {
        if !$crate::harness::tmux_available() {
            eprintln!("Skipping test: tmux not available");
            return;
        }
    };
}
pub(crate) use require_tmux;

// ---------------------------------------------------------------------------
// Recording helpers
// ---------------------------------------------------------------------------

fn recording_enabled() -> bool {
    std::env::var("RECORD_E2E").is_ok_and(|v| v == "1" || v == "true")
}

fn asciinema_available() -> bool {
    Command::new("asciinema")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn agg_available() -> bool {
    Command::new("agg")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn recordings_dir() -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/e2e-recordings");
    std::fs::create_dir_all(&dir).expect("create recordings dir");
    dir
}

fn convert_cast_to_gif(cast_path: &Path) {
    if !agg_available() {
        eprintln!(
            "agg not found -- skipping GIF conversion for {}",
            cast_path.display()
        );
        return;
    }

    let gif_path = cast_path.with_extension("gif");
    let status = Command::new("agg")
        .args(["--font-size", "14"])
        .arg(cast_path)
        .arg(&gif_path)
        .status();

    match status {
        Ok(s) if s.success() => {
            eprintln!("Recorded GIF: {}", gif_path.display());
        }
        Ok(s) => {
            eprintln!("agg exited with {}, GIF not created", s);
        }
        Err(e) => {
            eprintln!("agg failed: {}", e);
        }
    }
}

// ---------------------------------------------------------------------------
// TuiTestHarness
// ---------------------------------------------------------------------------

pub struct TuiTestHarness {
    session_name: String,
    test_name: String,
    home_dir: TempDir,
    _stub_dir: TempDir,
    binary_path: PathBuf,
    stub_path: PathBuf,
    socket_path: PathBuf,
    spawned: bool,
    recording: bool,
    cast_path: Option<PathBuf>,
}

#[allow(dead_code)]
impl TuiTestHarness {
    /// Create a new harness with an isolated `$HOME` and a fake `claude` stub
    /// so tool detection succeeds.
    pub fn new(test_name: &str) -> Self {
        let home_dir = TempDir::new().expect("failed to create temp home");
        let stub_dir = TempDir::new().expect("failed to create stub dir");

        // Unique session name to avoid collisions.
        let session_name = format!("aoe_e2e_{}_{}", test_name, std::process::id());

        // Path to unique tmux socket for this test.
        let socket_path = home_dir.path().join("tmux.sock");

        // Create a fake `claude` script so `which claude` succeeds.
        let stub_path = stub_dir.path().to_path_buf();
        let claude_stub = stub_path.join("claude");
        std::fs::write(&claude_stub, "#!/bin/sh\nexit 0\n").expect("write claude stub");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&claude_stub, std::fs::Permissions::from_mode(0o755))
                .expect("chmod claude stub");
        }

        // Pre-seed config.toml to skip the welcome dialog and update checks.
        // On Linux the app uses $XDG_CONFIG_HOME/agent-of-empires/ (set below),
        // on macOS it uses $HOME/.agent-of-empires/.
        let config_dir = if cfg!(target_os = "linux") {
            home_dir.path().join(".config").join("agent-of-empires")
        } else {
            home_dir.path().join(".agent-of-empires")
        };
        std::fs::create_dir_all(&config_dir).expect("create config dir");
        let config_content = format!(
            r#"[updates]
check_enabled = false

[app_state]
has_seen_welcome = true
last_seen_version = "{}"
"#,
            env!("CARGO_PKG_VERSION")
        );
        std::fs::write(config_dir.join("config.toml"), config_content).expect("write config.toml");

        // Create default profile directory.
        std::fs::create_dir_all(config_dir.join("profiles").join("default"))
            .expect("create default profile dir");

        let binary_path = PathBuf::from(env!("CARGO_BIN_EXE_aoe"));

        let recording = recording_enabled() && asciinema_available();
        if recording_enabled() && !asciinema_available() {
            eprintln!("RECORD_E2E is set but asciinema is not installed -- recording disabled");
        }

        Self {
            session_name,
            test_name: test_name.to_string(),
            home_dir,
            _stub_dir: stub_dir,
            binary_path,
            stub_path,
            socket_path,
            spawned: false,
            recording,
            cast_path: None,
        }
    }

    /// Build the PATH with the stub directory prepended so fake `claude` is found.
    fn env_path(&self) -> String {
        let system_path = std::env::var("PATH").unwrap_or_default();
        format!("{}:{}", self.stub_path.display(), system_path)
    }

    /// Build the shell command string to run inside the tmux session.
    /// When recording, wraps the command with `asciinema rec`.
    fn build_tmux_command(&mut self, args: &[&str]) -> String {
        let mut aoe_cmd = self.binary_path.display().to_string();
        for arg in args {
            aoe_cmd.push(' ');
            aoe_cmd.push_str(arg);
        }

        if self.recording {
            let cast_path = recordings_dir().join(format!("{}.cast", self.test_name));
            let cmd = format!(
                "asciinema rec --overwrite --cols 100 --rows 30 -c '{}' {}",
                aoe_cmd,
                cast_path.display()
            );
            self.cast_path = Some(cast_path);
            cmd
        } else {
            aoe_cmd
        }
    }

    /// Spawn `aoe` (no arguments = TUI mode) inside a detached tmux session
    /// with a fixed 100x30 terminal.
    pub fn spawn_tui(&mut self) {
        self.spawn(&[]);
    }

    /// Spawn `aoe <args>` inside a detached tmux session.
    pub fn spawn(&mut self, args: &[&str]) {
        let cmd_str = self.build_tmux_command(args);

        let output = Command::new("tmux")
            .arg("-S")
            .arg(&self.socket_path)
            .arg("new-session")
            .arg("-d")
            .arg("-s")
            .arg(&self.session_name)
            .arg("-x")
            .arg("100")
            .arg("-y")
            .arg("30")
            .arg(&cmd_str)
            .env("HOME", self.home_dir.path())
            .env("XDG_CONFIG_HOME", self.home_dir.path().join(".config"))
            .env("PATH", self.env_path())
            .env("TERM", "xterm-256color")
            .output()
            .expect("failed to run tmux new-session");

        assert!(
            output.status.success(),
            "tmux new-session failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );

        self.spawned = true;

        // Brief pause for the process to initialize.
        // Recording adds overhead so wait a bit longer.
        let delay = if self.recording { 500 } else { 300 };
        std::thread::sleep(Duration::from_millis(delay));
    }

    /// Send one or more tmux key names (e.g. "Enter", "Escape", "q", "C-c").
    pub fn send_keys(&self, keys: &str) {
        assert!(self.spawned, "must call spawn_tui() or spawn() first");
        let output = Command::new("tmux")
            .arg("-S")
            .arg(&self.socket_path)
            .arg("send-keys")
            .arg("-t")
            .arg(&self.session_name)
            .arg(keys)
            .output()
            .expect("failed to send keys");
        assert!(
            output.status.success(),
            "send-keys failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        // Let the TUI process the keystroke.
        std::thread::sleep(Duration::from_millis(50));
    }

    /// Send literal text (prevents "Enter" in text from being interpreted as
    /// the Enter key).
    pub fn type_text(&self, text: &str) {
        assert!(self.spawned, "must call spawn_tui() or spawn() first");
        let output = Command::new("tmux")
            .arg("-S")
            .arg(&self.socket_path)
            .arg("send-keys")
            .arg("-t")
            .arg(&self.session_name)
            .arg("-l")
            .arg(text)
            .output()
            .expect("failed to type text");
        assert!(
            output.status.success(),
            "type_text failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        std::thread::sleep(Duration::from_millis(50));
    }

    /// Capture the current screen contents as plain text (no ANSI escapes).
    pub fn capture_screen(&self) -> String {
        assert!(self.spawned, "must call spawn_tui() or spawn() first");
        let output = Command::new("tmux")
            .arg("-S")
            .arg(&self.socket_path)
            .arg("capture-pane")
            .arg("-t")
            .arg(&self.session_name)
            .arg("-p")
            .output()
            .expect("failed to capture pane");
        String::from_utf8_lossy(&output.stdout).to_string()
    }

    /// Poll `capture_screen()` until `text` appears. Panics with a screen dump
    /// if the default timeout (10s) is exceeded.
    pub fn wait_for(&self, text: &str) {
        self.wait_for_timeout(text, Duration::from_secs(10));
    }

    /// Like `wait_for` but with a custom timeout.
    pub fn wait_for_timeout(&self, text: &str, timeout: Duration) {
        let start = Instant::now();
        loop {
            let screen = self.capture_screen();
            if screen.contains(text) {
                return;
            }
            if start.elapsed() > timeout {
                panic!(
                    "Timed out waiting for {:?} after {:?}.\n\n--- Screen capture ---\n{}\n--- End screen capture ---",
                    text, timeout, screen
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Poll until `text` disappears from the screen.
    pub fn wait_for_absent(&self, text: &str, timeout: Duration) {
        let start = Instant::now();
        loop {
            let screen = self.capture_screen();
            if !screen.contains(text) {
                return;
            }
            if start.elapsed() > timeout {
                panic!(
                    "Timed out waiting for {:?} to disappear after {:?}.\n\n--- Screen capture ---\n{}\n--- End screen capture ---",
                    text, timeout, screen
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Assert that the screen currently contains `text`.
    pub fn assert_screen_contains(&self, text: &str) {
        let screen = self.capture_screen();
        assert!(
            screen.contains(text),
            "Expected screen to contain {:?}.\n\n--- Screen capture ---\n{}\n--- End screen capture ---",
            text, screen
        );
    }

    /// Assert that the screen does NOT contain `text`.
    pub fn assert_screen_not_contains(&self, text: &str) {
        let screen = self.capture_screen();
        assert!(
            !screen.contains(text),
            "Expected screen NOT to contain {:?}.\n\n--- Screen capture ---\n{}\n--- End screen capture ---",
            text, screen
        );
    }

    /// Run `aoe <args>` as a subprocess (not in tmux) with the same env
    /// isolation. Returns the `Output` (stdout, stderr, status).
    pub fn run_cli(&self, args: &[&str]) -> Output {
        Command::new(&self.binary_path)
            .args(args)
            .env("HOME", self.home_dir.path())
            .env("XDG_CONFIG_HOME", self.home_dir.path().join(".config"))
            .env("PATH", self.env_path())
            .output()
            .expect("failed to run aoe CLI")
    }

    /// Path to the isolated home directory for custom test setup.
    pub fn home_path(&self) -> &Path {
        self.home_dir.path()
    }

    /// Create and return a test project directory inside the temp home.
    pub fn project_path(&self) -> PathBuf {
        let p = self.home_dir.path().join("test-project");
        std::fs::create_dir_all(&p).expect("create project dir");
        p
    }

    /// Check whether the tmux session is still alive.
    pub fn session_alive(&self) -> bool {
        Command::new("tmux")
            .arg("-S")
            .arg(&self.socket_path)
            .arg("has-session")
            .arg("-t")
            .arg(&self.session_name)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    /// Wait until the tmux session terminates (the process exits).
    pub fn wait_for_exit(&self, timeout: Duration) {
        let start = Instant::now();
        loop {
            if !self.session_alive() {
                return;
            }
            if start.elapsed() > timeout {
                panic!(
                    "Timed out waiting for session {} to exit after {:?}",
                    self.session_name, timeout
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn kill_session(&self) {
        let _ = Command::new("tmux")
            .arg("-S")
            .arg(&self.socket_path)
            .arg("kill-session")
            .arg("-t")
            .arg(&self.session_name)
            .output();
    }
}

impl Drop for TuiTestHarness {
    fn drop(&mut self) {
        if self.spawned {
            self.kill_session();
        }

        // Convert recording to GIF if one was produced.
        if let Some(cast_path) = &self.cast_path {
            // Give asciinema a moment to finalize the file after the session ends.
            std::thread::sleep(Duration::from_millis(200));
            if cast_path.exists() {
                convert_cast_to_gif(cast_path);
            }
        }
    }
}
