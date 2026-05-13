//! Auto-spawn an `aoe serve` daemon on first cockpit interaction.
//!
//! Rules (see plan-cockpit-inside-tui.md commit 2):
//!
//! 1. If `AOE_DAEMON_URL` is set, never auto-spawn. Discovery fails
//!    loud so the caller doesn't accidentally attach to the wrong
//!    daemon.
//! 2. If a live local daemon exists, use it. Stale PID files are
//!    cleaned up by [`crate::cli::serve::daemon_pid`] before we get
//!    here.
//! 3. Otherwise spawn a fresh daemon bound to loopback (127.0.0.1),
//!    no tunnel, no tailscale, no browser. The spawned daemon is
//!    long-lived; killing this process does not kill it, so the
//!    maintainer's "create in TUI, drive from road via web" flow
//!    still works. Use `aoe serve --stop` to stop it.
//!
//! Build-namespace discipline (debug vs release) is enforced by
//! `crate::session::get_app_dir` — the spawned daemon writes
//! `serve.pid` / `serve.url` to the same app dir we read from, so a
//! debug client never picks up a release daemon (or vice versa).

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use thiserror::Error;
use tracing::{info, warn};

use super::discovery::{discover, discover_env, DaemonEndpoint, DiscoveryError};

/// How long [`ensure_daemon`] waits for a freshly-spawned daemon to
/// write its `serve.url` file before giving up.
const SPAWN_READY_TIMEOUT: Duration = Duration::from_secs(10);
/// Polling interval while waiting for `serve.url`.
const SPAWN_POLL_INTERVAL: Duration = Duration::from_millis(150);

#[derive(Debug, Error)]
pub enum ManagerError {
    #[error(
        "AOE_DAEMON_URL is set but the daemon at that URL is unreachable; check the address or unset to use a local daemon"
    )]
    EnvOverrideUnreachable,
    #[error("failed to locate or spawn a local cockpit daemon: {0}")]
    Discovery(#[from] DiscoveryError),
    #[error("could not find the aoe executable to respawn: {0}")]
    NoExecutable(#[from] std::io::Error),
    #[error("auto-spawned `aoe serve` exited before becoming ready (check `aoe serve` log)")]
    SpawnFailedFast,
    #[error("auto-spawned `aoe serve` did not become ready within {0:?}")]
    SpawnTimeout(Duration),
}

/// Locate a daemon, spawning one if no local daemon is running and
/// `AOE_DAEMON_URL` is unset. Returns the resolved endpoint.
pub async fn ensure_daemon() -> Result<DaemonEndpoint, ManagerError> {
    // Env override path: never auto-spawn. The whole point of the
    // override is to attach to a *specific* daemon; silently starting
    // a different local one would be the wrong answer.
    if discover_env().is_some() {
        return discover().map_err(|_| ManagerError::EnvOverrideUnreachable);
    }

    if let Ok(endpoint) = discover() {
        return Ok(endpoint);
    }

    spawn_local().await?;
    discover().map_err(ManagerError::Discovery)
}

async fn spawn_local() -> Result<(), ManagerError> {
    let exe = std::env::current_exe()?;
    info!(
        target: "cockpit.client.daemon_manager",
        exe = %exe.display(),
        "spawning local cockpit daemon"
    );
    let port = default_port();

    let mut cmd = Command::new(&exe);
    cmd.args(["serve", "--host", "127.0.0.1", "--port", &port.to_string()]);
    cmd.stdin(Stdio::null());

    let log_path = daemon_log_path();
    match log_path
        .as_ref()
        .and_then(|p| std::fs::File::create(p).ok().map(|f| (p.clone(), f)))
    {
        Some((_, log_file)) => {
            let stdout = log_file.try_clone().map_err(ManagerError::NoExecutable)?;
            let stderr = log_file;
            cmd.stdout(Stdio::from(stdout)).stderr(Stdio::from(stderr));
        }
        None => {
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }
    }

    // Detach so the daemon outlives this process: new session via
    // setsid() so SIGHUP from a closing terminal doesn't kill it.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid() is async-signal-safe per POSIX, which is the
        // only requirement for pre_exec closures.
        unsafe {
            cmd.pre_exec(|| {
                nix::unistd::setsid().map_err(std::io::Error::other)?;
                Ok(())
            });
        }
    }

    let mut child = cmd.spawn().map_err(ManagerError::NoExecutable)?;
    let pid = child.id();

    let deadline = Instant::now() + SPAWN_READY_TIMEOUT;
    loop {
        if discover().is_ok() {
            info!(
                target: "cockpit.client.daemon_manager",
                pid,
                "local cockpit daemon ready"
            );
            return Ok(());
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                warn!(
                    target: "cockpit.client.daemon_manager",
                    ?status,
                    "auto-spawned aoe serve exited before becoming ready"
                );
                return Err(ManagerError::SpawnFailedFast);
            }
            Ok(None) => {}
            Err(e) => {
                warn!(
                    target: "cockpit.client.daemon_manager",
                    error = %e,
                    "try_wait on auto-spawned daemon failed"
                );
            }
        }
        if Instant::now() >= deadline {
            warn!(
                target: "cockpit.client.daemon_manager",
                "auto-spawned aoe serve timed out before writing serve.url"
            );
            return Err(ManagerError::SpawnTimeout(SPAWN_READY_TIMEOUT));
        }
        tokio::time::sleep(SPAWN_POLL_INTERVAL).await;
    }
}

fn default_port() -> u16 {
    // Match `aoe serve`'s convention so the auto-spawned daemon binds
    // to the same default port a user-launched `aoe serve` would.
    if cfg!(debug_assertions) {
        8081
    } else {
        8080
    }
}

fn daemon_log_path() -> Option<PathBuf> {
    let dir = crate::session::get_app_dir().ok()?;
    Some(dir.join("serve.log"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_port_matches_namespace() {
        // Debug builds ship a separate namespace + port to avoid
        // colliding with an installed release daemon.
        if cfg!(debug_assertions) {
            assert_eq!(default_port(), 8081);
        } else {
            assert_eq!(default_port(), 8080);
        }
    }
}
