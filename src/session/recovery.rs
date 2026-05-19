//! Startup auto-recovery for AI agent sessions.
//!
//! After a system reboot, tmux loses all its sessions. AoE sessions whose
//! agent supports `--resume <sid>` (claude, opencode, codex, gemini, vibe,
//! pi, hermes, kiro, qwen) can be transparently recreated by replaying the
//! resume cascade in `start_with_resume_fallback`. This module centralises
//! the candidate selection and the cross-process exclusion needed to make
//! that safe when both the TUI (`aoe`) and the daemon (`aoe serve`) are
//! running.
//!
//! The recovery cascade itself lives in `instance::start_with_resume_fallback`;
//! this module is the policy layer (who runs it, when, with what serialization)
//! that the TUI and daemon entry points share.
//!
//! # Cross-process exclusion
//!
//! Both the TUI and the daemon may attempt recovery on startup. To avoid
//! duplicate cascades against the same `(profile, id)` (which would race on
//! `tmux new-session` and on `sessions.json`), we acquire a non-blocking
//! exclusive `flock` on a marker file in the app data directory. The losing
//! party skips recovery entirely and lets the winner proceed. The file lock
//! is held for the entire recovery pass so that:
//!
//! - A late-starting daemon cannot duplicate a TUI's in-flight workers.
//! - A late-starting TUI cannot duplicate a daemon's in-flight workers.
//!
//! `daemon_pid()` alone is not sufficient because the daemon writes its PID
//! file *after* fork+exec, leaving a tens-to-hundreds-of-millisecond window
//! where both sides observe "no daemon running" and both decide they own
//! recovery.

use std::path::PathBuf;
#[cfg(feature = "serve")]
use std::sync::Arc;
#[cfg(feature = "serve")]
use std::time::{Duration, Instant};

use anyhow::Result;
use fs2::FileExt;

use super::instance::should_attempt_resume;
use super::{Instance, StartOutcome};

/// File-system claim that the holder is the sole recovery owner for this
/// machine. Dropped automatically (releases the `flock`) when the holder goes
/// out of scope.
pub struct RecoveryLock {
    _file: std::fs::File,
}

/// Try to acquire the cross-process recovery lock without blocking.
///
/// Returns `Some(RecoveryLock)` if this process is now the recovery owner;
/// `None` if another process (TUI or daemon) already holds it. The lock is
/// released when the returned guard is dropped.
///
/// The lock file lives at `<app_dir>/.recovery.lock`. It is created if
/// missing and never deleted (the lock is on the file, not its existence).
pub fn try_acquire_recovery_lock() -> Result<Option<RecoveryLock>> {
    let path = recovery_lock_path()?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)?;
    match file.try_lock_exclusive() {
        Ok(()) => Ok(Some(RecoveryLock { _file: file })),
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
        Err(e) => Err(e.into()),
    }
}

fn recovery_lock_path() -> Result<PathBuf> {
    Ok(super::get_app_dir()?.join(".recovery.lock"))
}

/// Pure predicate: should this instance go through the startup recovery
/// cascade? Excludes cockpit-mode sessions (handled by `cockpit_reconciler`),
/// sessions whose agent has `ResumeStrategy::Unsupported`, and sessions
/// without a valid `agent_session_id`. Live tmux panes are filtered separately
/// by the caller using `Instance::tmux_session().exists() && !is_pane_dead()`.
pub fn is_recovery_candidate(inst: &Instance) -> bool {
    !inst.is_cockpit_mode() && should_attempt_resume(inst.agent_session_id.as_deref(), &inst.tool)
}

/// Warm up the tmux server so that the first concurrent `new-session` from
/// recovery workers does not race the server's cold start. On macOS post-reboot,
/// tmux is not running until the first client connects; without this warm-up,
/// three workers calling `new-session` simultaneously can hit a connect-race
/// window where the socket file exists but no listener accepts yet.
///
/// Best-effort: `tmux start-server` is idempotent; if tmux is unavailable the
/// caller will fail downstream with a more specific error.
pub fn warm_tmux_server() {
    let _ = std::process::Command::new("tmux")
        .arg("start-server")
        .status();
}

/// Time-to-live entries in the `recently_restarted` map remain authoritative
/// for. Sized to cover the worst-case cascade latency
/// (`RESUME_PROBE_MAX` ~3s × 2 tiers + kill_clean grace ~150ms ≈ 6.5s) plus a
/// 1.5s margin for slow cold-start agents (opencode importing on a cold
/// cache). Lower values cause spurious `Status::Error` chips on still-starting
/// sessions; higher values delay the first real status update past the user's
/// patience window.
#[cfg(feature = "serve")]
pub const RECENTLY_RESTARTED_TTL: Duration = Duration::from_secs(8);

/// Periodic GC interval for `recently_restarted`. Long-running daemons may
/// accumulate thousands of entries over a session if they never GC; the TTL
/// check on read filters but does not remove. Sweeping every 60s keeps the
/// map bounded by `O(recoveries_in_last_60s)` rather than total uptime.
#[cfg(feature = "serve")]
pub const RECENTLY_RESTARTED_GC_INTERVAL: Duration = Duration::from_secs(60);

/// Shared `recently_restarted` map: instance id → time of last successful
/// recovery start. Status pollers consult this to suppress the
/// `Status::Error` transition while a freshly-restarted agent is still
/// settling. Entries older than `RECENTLY_RESTARTED_TTL` are ignored on read
/// and removed by the GC task.
#[cfg(feature = "serve")]
pub type RecentlyRestarted = Arc<std::sync::RwLock<std::collections::HashMap<String, Instant>>>;

/// Construct an empty `recently_restarted` map.
#[cfg(feature = "serve")]
pub fn new_recently_restarted() -> RecentlyRestarted {
    Arc::new(std::sync::RwLock::new(std::collections::HashMap::new()))
}

#[cfg(feature = "serve")]
pub fn is_recently_restarted(map: &RecentlyRestarted, id: &str) -> bool {
    let guard = match map.read() {
        Ok(g) => g,
        Err(_) => return false,
    };
    guard
        .get(id)
        .is_some_and(|t| t.elapsed() < RECENTLY_RESTARTED_TTL)
}

#[cfg(feature = "serve")]
pub fn mark_recently_restarted(map: &RecentlyRestarted, id: &str) {
    if let Ok(mut guard) = map.write() {
        guard.insert(id.to_string(), Instant::now());
    }
}

/// Remove entries older than `2 × RECENTLY_RESTARTED_TTL`. The 2x factor
/// avoids a tight read-vs-GC race where a reader observes an entry just
/// before GC removes it; with 2x, a reader that saw the entry at age T has
/// at least T more time before GC reaps it.
#[cfg(feature = "serve")]
pub fn gc_recently_restarted(map: &RecentlyRestarted) {
    let cutoff = RECENTLY_RESTARTED_TTL * 2;
    if let Ok(mut guard) = map.write() {
        guard.retain(|_, t| t.elapsed() < cutoff);
    }
}

/// Run the recovery cascade for one instance. Thin wrapper around
/// `restart_with_size_opts(None, false)`.
///
/// `skip_on_launch=false` is mandatory: `on_launch` hooks (npm install, env
/// setup) must run on the first start after a reboot, conceptually identical
/// to a fresh launch. The Tier-2 retry inside `start_with_resume_fallback`
/// hardcodes `true` internally to prevent double-firing on the same restart.
///
/// Blocks until the cascade returns (up to ~7s on the fallback path); callers
/// must invoke it off the main/event-loop thread (`spawn_blocking` or a
/// dedicated worker).
pub fn run_recovery_for_instance(inst: &mut Instance) -> Result<StartOutcome> {
    inst.restart_with_size_opts(None, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "serve")]
    #[test]
    fn recently_restarted_is_recently_restarted_returns_true_within_ttl() {
        let map = new_recently_restarted();
        mark_recently_restarted(&map, "abc");
        assert!(is_recently_restarted(&map, "abc"));
        assert!(!is_recently_restarted(&map, "other"));
    }

    #[cfg(feature = "serve")]
    #[test]
    fn recently_restarted_gc_removes_stale_entries() {
        let map = new_recently_restarted();
        let stale = Instant::now() - RECENTLY_RESTARTED_TTL * 3;
        let fresh = Instant::now();
        {
            let mut g = map.write().unwrap();
            g.insert("stale".into(), stale);
            g.insert("fresh".into(), fresh);
        }
        gc_recently_restarted(&map);
        let g = map.read().unwrap();
        assert!(!g.contains_key("stale"));
        assert!(g.contains_key("fresh"));
    }

    /// Cross-process exclusion is a POSIX `flock(2)` guarantee, not
    /// something this unit test can verify (BSD flock and Linux flock
    /// both treat all fds in the same process as one holder; only a
    /// distinct process would be locked out). This test only verifies
    /// the wrapper successfully creates the lock file and acquires/
    /// releases the lock without erroring. The cross-process behavior
    /// is exercised by the e2e suite (TUI + daemon spawned together).
    #[test]
    fn recovery_lock_acquires_and_releases() {
        let first = try_acquire_recovery_lock().unwrap();
        assert!(first.is_some(), "acquisition should succeed");
        drop(first);
        let second = try_acquire_recovery_lock().unwrap();
        assert!(second.is_some(), "re-acquisition after drop should succeed");
    }
}
