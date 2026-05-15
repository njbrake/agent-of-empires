//! Sandbox container lifecycle for cockpit sessions.
//!
//! The cockpit spawn path mirrors the tmux path's sandbox handling: if
//! the session's `Instance` has `sandbox_info`, create + start the
//! Docker container, run on_launch hooks inside it, and hand the
//! `SandboxInfo` back to the caller so the supervisor can wrap the
//! agent argv in `docker exec`.
//!
//! This lives in its own module so every cockpit-spawn entry point
//! (auto-spawn after create, substrate switch, manual `POST /spawn`,
//! reconciler reattach fallback) goes through the same code path.

use anyhow::{Context, Result};
use tokio::sync::RwLock;

use crate::session::{Instance, SandboxInfo};

/// Ensure the sandbox container for the named session is running and
/// run any on_launch hooks inside it. Returns the session's
/// `sandbox_info` (with `container_id` populated) to thread into
/// `SpawnRequest`, or `None` for non-sandboxed sessions.
///
/// On success the in-memory `Instance.sandbox_info.container_id` is
/// updated; persistence to disk is the caller's responsibility.
pub async fn ensure_container_for_session(
    instances: &RwLock<Vec<Instance>>,
    session_id: &str,
) -> Result<Option<SandboxInfo>> {
    let (sandbox_info, container_name, container_workdir, hooks) = {
        let mut guard = instances.write().await;
        let Some(inst) = guard.iter_mut().find(|i| i.id == session_id) else {
            anyhow::bail!("session {session_id} not found");
        };
        if !inst.is_sandboxed() {
            return Ok(None);
        }
        let _container = inst
            .get_container_for_instance()
            .context("ensuring sandbox container")?;
        let workdir = inst.container_workdir();
        let profile = inst.source_profile.clone();
        let hooks = inst.resolve_on_launch_hooks(false, &profile);
        let info = inst.sandbox_info.clone();
        let name: Option<String> = info.as_ref().map(|s| s.container_name.clone());
        (info, name, workdir, hooks)
    };

    if let (Some(cmds), Some(name)) = (hooks, container_name) {
        if !cmds.is_empty() {
            let errors = crate::session::repo_config::execute_hooks_in_container_best_effort(
                &cmds,
                &name,
                &container_workdir,
                true,
            );
            for err in errors {
                tracing::warn!(
                    target: "cockpit.sandbox",
                    session = %session_id,
                    "on_launch hook failed: {err}"
                );
            }
        }
    }

    Ok(sandbox_info)
}
