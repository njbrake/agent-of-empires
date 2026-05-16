//! Sandbox container lifecycle for cockpit sessions.
//!
//! The cockpit spawn path mirrors the tmux path's sandbox handling: if
//! the session's `Instance` has `sandbox_info`, create + start the
//! Docker container, optionally run on_launch hooks inside it, and
//! hand the `SandboxInfo` back to the caller so the supervisor can
//! wrap the agent argv in `docker exec`.
//!
//! This lives in its own module so every cockpit-spawn entry point
//! (auto-spawn after create, substrate switch, manual `POST /spawn`,
//! reconciler reattach fallback) goes through the same code path.

use anyhow::{Context, Result};
use tokio::sync::RwLock;

use crate::session::{Instance, SandboxInfo};

/// Ensure the sandbox container for the named session is running.
///
/// `run_on_launch_hooks` controls whether profile/repo `on_launch`
/// hooks fire after the container is up. Only the initial
/// `auto-spawn after create` path passes `true`; every other entry
/// point (substrate switch, manual `/spawn`, reconciler resume)
/// passes `false` so hooks don't re-run on every reattach.
///
/// Docker work runs on a blocking thread; the in-memory `Instance`
/// is only locked for a short read + a short write (to stamp the
/// resolved `container_id` back onto its `SandboxInfo`).
///
/// Returns the session's `sandbox_info` (with `container_id`
/// populated) for non-sandboxed sessions it returns `None`.
pub async fn ensure_container_for_session(
    instances: &RwLock<Vec<Instance>>,
    session_id: &str,
    run_on_launch_hooks: bool,
) -> Result<Option<SandboxInfo>> {
    // Phase 1: short read lock. Clone the Instance so the docker work
    // can run on a blocking thread without holding any tokio lock.
    let mut instance_clone = {
        let guard = instances.read().await;
        let Some(inst) = guard.iter().find(|i| i.id == session_id) else {
            anyhow::bail!("session {session_id} not found");
        };
        if !inst.is_sandboxed() {
            return Ok(None);
        }
        inst.clone()
    };

    // Phase 2: docker create/start + workdir/hook resolution on a
    // blocking thread. `get_container_for_instance` may pull an image
    // and create the container, both of which can take many seconds;
    // running it here keeps the tokio worker free for other requests.
    let session_id_owned = session_id.to_string();
    let (sandbox_info, container_workdir, hooks) =
        tokio::task::spawn_blocking(move || -> Result<_> {
            let _container = instance_clone
                .get_container_for_instance()
                .context("ensuring sandbox container")?;
            let workdir = instance_clone.container_workdir();
            let hooks = if run_on_launch_hooks {
                let profile = instance_clone.source_profile.clone();
                instance_clone.resolve_on_launch_hooks(false, &profile)
            } else {
                None
            };
            Ok((instance_clone.sandbox_info.clone(), workdir, hooks))
        })
        .await
        .context("docker ensure task failed to join")??;

    // Phase 3: brief write lock to stamp the resolved container_id
    // back onto the live Instance so subsequent reads observe it.
    if let Some(info) = &sandbox_info {
        let mut guard = instances.write().await;
        if let Some(inst) = guard.iter_mut().find(|i| i.id == session_id_owned) {
            if let Some(ref mut sb) = inst.sandbox_info {
                if sb.container_id.is_none() {
                    sb.container_id = info.container_id.clone();
                }
            }
        }
    }

    // Phase 4: run on_launch hooks outside any lock. Hooks are shell
    // commands that can themselves take seconds, so wrap in
    // spawn_blocking too. Best-effort: failures are logged and the
    // spawn proceeds.
    if let (Some(cmds), Some(info)) = (hooks, sandbox_info.as_ref()) {
        if !cmds.is_empty() {
            let container_name = info.container_name.clone();
            let workdir = container_workdir.clone();
            let sid = session_id.to_string();
            match tokio::task::spawn_blocking(move || {
                crate::session::repo_config::execute_hooks_in_container_best_effort(
                    &cmds,
                    &container_name,
                    &workdir,
                    true,
                )
            })
            .await
            {
                Ok(errors) => {
                    for err in errors {
                        tracing::warn!(
                            target: "cockpit.sandbox",
                            session = %sid,
                            "on_launch hook failed: {err}"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        target: "cockpit.sandbox",
                        session = %sid,
                        "on_launch hook task failed to join: {e}"
                    );
                }
            }
        }
    }

    Ok(sandbox_info)
}
