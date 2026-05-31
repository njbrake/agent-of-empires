//! Dynamic per-profile disk-watch rewire (server-migration design doc §6 /
//! §8.1 test 3). Two layers of coverage:
//!
//! * Lower layer (`dynamic_profile_rewire_inserts_and_removes_entries`):
//!   drives `rewire_disk_watch_for_profile_{add,remove}` directly against an
//!   in-process `AppState`, asserting `disk_watch_handles` insert/remove
//!   under the canonical drop-then-abort order. Observable only at this
//!   layer because the handles map is daemon-internal state that the HTTP
//!   surface intentionally does not expose.
//! * HTTP API layer (`dynamic_profile_create_via_http_api`,
//!   `dynamic_profile_delete_via_http_api`): spawns a real `aoe serve`
//!   subprocess against an isolated `HOME` and drives `POST /api/profiles`
//!   and `DELETE /api/profiles/{name}`. These are the entry points that
//!   trigger `rewire_disk_watch_for_profile_add` / `_remove` in
//!   production, so this layer guards the daemon-boot path the design
//!   commits to.

#![cfg(feature = "serve")]

use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Arc;
use std::time::{Duration, Instant};

use agent_of_empires::file_watch::FileWatchService;
use agent_of_empires::server::test_support::build_test_app_state;
use agent_of_empires::server::{
    rewire_disk_watch_for_profile_add, rewire_disk_watch_for_profile_remove,
};
use serial_test::serial;
use tempfile::TempDir;

#[tokio::test]
#[serial]
async fn dynamic_profile_rewire_inserts_and_removes_entries() {
    let temp = tempfile::tempdir().unwrap();
    isolate_home(temp.path());
    let _ = agent_of_empires::session::get_profile_dir("rewire-profile").expect("profile dir");

    let state = build_test_app_state(Vec::new());
    let live = FileWatchService::new().expect("live svc");
    let mut state_mut = Arc::try_unwrap(state).map_err(|_| ()).expect("unique");
    state_mut.file_watch = live;
    let state = Arc::new(state_mut);

    rewire_disk_watch_for_profile_add(&state, "rewire-profile").await;
    {
        let handles = state.disk_watch_handles.lock().await;
        assert!(
            handles.contains_key("rewire-profile"),
            "add must insert the per-profile entry"
        );
    }

    rewire_disk_watch_for_profile_remove(&state, "rewire-profile").await;
    {
        let handles = state.disk_watch_handles.lock().await;
        assert!(
            !handles.contains_key("rewire-profile"),
            "remove must drop the per-profile entry"
        );
    }
}

#[tokio::test]
#[serial]
async fn dynamic_profile_create_via_http_api() {
    let Some(daemon) = ServeDaemon::spawn() else {
        return;
    };
    let client = reqwest::Client::new();

    let resp = client
        .post(daemon.url("/api/profiles"))
        .json(&serde_json::json!({"name": "alt"}))
        .send()
        .await
        .expect("POST /api/profiles");
    assert_eq!(
        resp.status().as_u16(),
        201,
        "POST /api/profiles must succeed (got {})",
        resp.status()
    );

    let profiles = list_profiles(&client, &daemon).await;
    assert!(
        profiles.iter().any(|name| name == "alt"),
        "GET /api/profiles must list the new profile, got {:?}",
        profiles
    );

    let profile_dir = daemon.app_dir().join("profiles").join("alt");
    assert!(
        profile_dir.is_dir(),
        "POST /api/profiles must create the on-disk profile dir at {}",
        profile_dir.display()
    );
}

#[tokio::test]
#[serial]
async fn dynamic_profile_delete_via_http_api() {
    let Some(daemon) = ServeDaemon::spawn() else {
        return;
    };
    let client = reqwest::Client::new();

    let create = client
        .post(daemon.url("/api/profiles"))
        .json(&serde_json::json!({"name": "alt"}))
        .send()
        .await
        .expect("POST /api/profiles");
    assert_eq!(create.status().as_u16(), 201, "create must succeed");

    let delete = client
        .delete(daemon.url("/api/profiles/alt"))
        .send()
        .await
        .expect("DELETE /api/profiles/alt");
    assert_eq!(
        delete.status().as_u16(),
        200,
        "DELETE /api/profiles/alt must succeed (got {})",
        delete.status()
    );

    let profiles = list_profiles(&client, &daemon).await;
    assert!(
        !profiles.iter().any(|name| name == "alt"),
        "GET /api/profiles must NOT list a deleted profile, got {:?}",
        profiles
    );

    let profile_dir = daemon.app_dir().join("profiles").join("alt");
    assert!(
        !profile_dir.exists(),
        "DELETE /api/profiles/alt must remove the on-disk profile dir at {}",
        profile_dir.display()
    );
}

fn isolate_home(temp: &Path) {
    // SAFETY: env mutation; #[serial] guards cross-test races.
    unsafe { std::env::set_var("HOME", temp) };
    #[cfg(target_os = "linux")]
    unsafe {
        std::env::set_var("XDG_CONFIG_HOME", temp.join(".config"))
    };
}

async fn list_profiles(client: &reqwest::Client, daemon: &ServeDaemon) -> Vec<String> {
    #[derive(serde::Deserialize)]
    struct ProfileInfo {
        name: String,
    }
    let resp = client
        .get(daemon.url("/api/profiles"))
        .send()
        .await
        .expect("GET /api/profiles");
    assert_eq!(resp.status().as_u16(), 200);
    let parsed: Vec<ProfileInfo> = resp.json().await.expect("decode profiles");
    parsed.into_iter().map(|p| p.name).collect()
}

/// RAII guard around a foreground `aoe serve` subprocess scoped to an
/// isolated `HOME`. `Drop` kills the child and waits, even on test panic.
struct ServeDaemon {
    child: Option<Child>,
    port: u16,
    home: TempDir,
}

impl ServeDaemon {
    /// Spawn `aoe serve --no-auth --host 127.0.0.1 --port <free>` against a
    /// fresh `HOME`. Returns `None` when the binary's `serve` feature is
    /// unavailable in this build. Panics on any other startup failure so
    /// the test gives a useful diagnostic.
    fn spawn() -> Option<Self> {
        let aoe = env!("CARGO_BIN_EXE_aoe");
        let home = tempfile::tempdir().expect("home tempdir");
        let port = pick_free_port();

        let mut cmd = Command::new(aoe);
        cmd.args([
            "serve",
            "--no-auth",
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
        ]);
        cmd.env("HOME", home.path());
        #[cfg(target_os = "linux")]
        cmd.env("XDG_CONFIG_HOME", home.path().join(".config"));
        cmd.env_remove("AGENT_OF_EMPIRES_DEBUG");

        let mut child = cmd.spawn().expect("spawn aoe serve");
        if !wait_for_port(port, Duration::from_secs(15)) {
            let _ = child.kill();
            let _ = child.wait();
            panic!(
                "aoe serve did not bind 127.0.0.1:{} within 15s; likely missing serve feature",
                port
            );
        }
        Some(Self {
            child: Some(child),
            port,
            home,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{}", self.port, path)
    }

    fn app_dir(&self) -> PathBuf {
        let home = self.home.path();
        if cfg!(target_os = "linux") {
            home.join(".config")
                .join(agent_of_empires::session::APP_DIR_NAME_LINUX)
        } else {
            home.join(agent_of_empires::session::APP_DIR_NAME_OTHER)
        }
    }
}

impl Drop for ServeDaemon {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Bind ephemeral, drop, return the port. Tiny TOCTOU window before the
/// daemon binds; acceptable under `#[serial]`. Mirrors
/// `tests/e2e/serve.rs::pick_free_port`.
fn pick_free_port() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    l.local_addr().expect("local_addr").port()
}

fn wait_for_port(port: u16, deadline: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < deadline {
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
