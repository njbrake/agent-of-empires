//! Cockpit worker reconciler. Runs every 2s tick (and on cold start,
//! the first tick fires immediately) to reconcile on-disk session
//! state against the supervisor's live worker pool.
//!
//! Responsibilities:
//!
//! 1. Honor the master switch (`cockpit.enabled`) and the
//!    `aoe cockpit stop|kill|restart` side-channel.
//! 2. Sweep orphan registry entries whose session is gone.
//! 3. For every cockpit-mode session without a live worker, run a
//!    resume task: reattach to an existing runner if one is alive,
//!    otherwise fresh-spawn the agent.
//!
//! The resume tasks run in parallel under a `tokio::sync::Semaphore`
//! cap derived from `cockpit.max_concurrent_resumes` (default 4,
//! clamped to `max_concurrent_workers`). The supervisor's per-agent
//! install gate (see `Supervisor::spawn`) serialises only the first
//! spawn of each agent per daemon lifetime so the claude-agent-acp
//! lazy-install race never bites; every subsequent spawn for that
//! agent runs in parallel. See #1088.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tokio::time::timeout;

use super::AppState;

/// Per-target resume outcome. Drives whether the reconciler should
/// retry on the next tick or leave `attempted` set so the same target
/// isn't poked every 2s.
#[derive(Debug, Clone)]
enum ResumeOutcome {
    /// Reattach succeeded; nothing else to do for this id.
    Attached,
    /// Reattach timed out; the orphan registry entry was swept and the
    /// reconciler should drop the id from `attempted` so the next tick
    /// can try a fresh spawn cleanly.
    RetryAfterAttachTimeout,
    /// Fresh spawn finished, with or without error. `attempted` stays
    /// populated; a permanently-failing spawn (e.g. missing
    /// claude-agent-acp) does not loop forever.
    SpawnFinished,
}

/// A single cockpit session that needs a worker. Snapshotted from the
/// instance list under the outer read lock so the parallel resume
/// tasks don't have to re-take it.
#[derive(Clone)]
struct ResumeTarget {
    id: String,
    tool: String,
    agent_override: Option<String>,
    model: Option<String>,
    project_path: String,
    stored_acp_session_id: Option<String>,
    in_flight_turn: bool,
}

/// Tuple shape used by the instance-list snapshot. Aliased to dodge
/// clippy::type_complexity since the columns are fixed by the
/// upstream `Instance` schema.
type RawTargetTuple = (
    String,
    String,
    Option<String>,
    Option<String>,
    String,
    Option<String>,
);

pub async fn reconcile_cockpit_workers(state: &Arc<AppState>, attempted: &mut HashSet<String>) {
    // Honor `cockpit.enabled = false` from config.toml — the persistent
    // master switch. Mirrored as an atomic; `PATCH /api/cockpit/master`
    // flips it live without restarting `aoe serve`.
    if !state
        .cockpit_master_enabled
        .load(std::sync::atomic::Ordering::Relaxed)
    {
        return;
    }

    // Detect `aoe cockpit stop|kill|restart` (a separate process that
    // deletes the registry entry + SIGTERMs the runner) and surface it
    // as a typed Stopped event. The daemon's protocol-layer connection
    // task blocks on `cmd_rx.recv()` while idle, so socket EOF doesn't
    // propagate to the drain task on its own — without this poll, the
    // UI stays stuck on "thinking" and the supervisor keeps a phantom
    // worker. For the `restart` case, the reaper returns the ids it
    // marked as `restart_pending`; clear them from `attempted` so the
    // spawn pass below treats them as fresh and the next 2s tick
    // reattaches with the cached `acp_session_id`.
    let restart_pending = state.cockpit_supervisor.reap_user_stopped().await;
    for id in &restart_pending {
        attempted.remove(id);
    }

    // Snapshot per-target resume inputs under the instances read lock.
    // We then drop the lock so the parallel resume tasks (each ~3s for
    // a fresh spawn) don't pin it.
    let raw_targets: Vec<RawTargetTuple> = {
        let instances = state.instances.read().await;
        instances
            .iter()
            .filter(|i| i.cockpit_mode)
            .map(|i| {
                (
                    i.id.clone(),
                    i.tool.clone(),
                    i.cockpit_agent.clone(),
                    i.cockpit_model.clone(),
                    i.project_path.clone(),
                    i.cockpit_acp_session_id.clone(),
                )
            })
            .collect()
    };

    let live: HashSet<&String> = raw_targets.iter().map(|t| &t.0).collect();
    attempted.retain(|id| live.contains(id));

    // ORDERING INVARIANT: this orphan sweep MUST run before the
    // resume scheduling pass below. The capacity check counts both
    // in-memory workers AND on-disk registry entries (so a fresh
    // daemon can't race the reconciler and over-spawn). If the sweep
    // ran after, dead-PID entries from a previous unclean shutdown
    // would still count toward `max_concurrent_workers` and could
    // block legitimate spawns until the next tick. Do not reorder.
    sweep_orphan_workers(state, &live).await;

    // Build the work list. Skip ids already in `attempted` (a
    // permanently-failing spawn shouldn't loop every tick) and ids the
    // supervisor already knows about (REST-triggered spawn or
    // already-attached). For the rest, decide attach vs fresh-spawn at
    // task time so concurrent tasks see consistent registry state.
    let mut tasks: Vec<ResumeTarget> = Vec::new();
    for (id, tool, agent_override, model, project_path, stored_acp_session_id) in raw_targets {
        if attempted.contains(&id) {
            continue;
        }
        if state.cockpit_supervisor.is_running(&id).await {
            // A REST-triggered spawn (POST /api/sessions or
            // /api/cockpit/sessions/:id/enable) already owns the worker;
            // record the id so we don't poll is_running every tick.
            attempted.insert(id);
            continue;
        }
        let in_flight_turn = state.cockpit_event_store.has_in_flight_turn(&id);
        // Mark before spawning so the next 2s tick doesn't double-poke
        // while the parallel resume task is still in flight. A task
        // that returns RetryAfterAttachTimeout will clear itself below.
        attempted.insert(id.clone());
        tasks.push(ResumeTarget {
            id,
            tool,
            agent_override,
            model,
            project_path,
            stored_acp_session_id,
            in_flight_turn,
        });
    }

    if tasks.is_empty() {
        return;
    }

    // Resume concurrency cap. Bounded by total worker capacity so this
    // setting can never exceed `max_concurrent_workers`. Floor at 1
    // so a misconfigured zero doesn't deadlock the reconciler.
    let cfg = crate::session::profile_config::resolve_config_or_warn(&state.profile);
    let resume_limit = cfg
        .cockpit
        .max_concurrent_resumes
        .min(cfg.cockpit.max_concurrent_workers)
        .max(1);
    let semaphore = Arc::new(Semaphore::new(resume_limit as usize));

    let mut set: JoinSet<(String, ResumeOutcome)> = JoinSet::new();
    for target in tasks {
        let state = Arc::clone(state);
        let sem = Arc::clone(&semaphore);
        set.spawn(async move {
            // Permit acquire is the only thing keeping us under the
            // cap; on shutdown the semaphore is dropped and acquire
            // returns Err, which we treat as "nothing to do".
            let _permit = match sem.acquire().await {
                Ok(p) => p,
                Err(_) => return (target.id, ResumeOutcome::SpawnFinished),
            };
            let id = target.id.clone();
            let outcome = resume_one(state, target).await;
            (id, outcome)
        });
    }

    while let Some(result) = set.join_next().await {
        match result {
            Ok((id, ResumeOutcome::RetryAfterAttachTimeout)) => {
                attempted.remove(&id);
            }
            Ok((_, ResumeOutcome::Attached)) | Ok((_, ResumeOutcome::SpawnFinished)) => {}
            Err(e) => {
                // Task panicked or was cancelled. Don't keep retrying
                // the same id every tick if the task panics on every
                // run; the `attempted` insert above already protects
                // us. Log so operators see it.
                tracing::error!(
                    target: "cockpit.supervisor",
                    "resume task panicked: {e}"
                );
            }
        }
    }
}

async fn resume_one(state: Arc<AppState>, target: ResumeTarget) -> ResumeOutcome {
    let ResumeTarget {
        id,
        tool,
        agent_override,
        model,
        project_path,
        stored_acp_session_id,
        in_flight_turn,
    } = target;

    // Reattach path: if a previous daemon detached a runner for this
    // session and the runner is still alive, dial its socket instead
    // of spawning a fresh agent. Bounded by the registry probe — no
    // network IO unless we have a live PID + socket on disk.
    if let Ok(Some(record)) = crate::cockpit::worker_registry::load(&id) {
        if crate::cockpit::worker_registry::is_record_live(&record) {
            let supervisor = Arc::clone(&state.cockpit_supervisor);
            let cwd = PathBuf::from(&project_path);
            let attach_res = timeout(
                Duration::from_secs(3),
                supervisor.attach(id.clone(), cwd, vec![], in_flight_turn),
            )
            .await;
            match attach_res {
                Ok(Ok(())) => {
                    tracing::info!(
                        target: "cockpit.supervisor",
                        session = %id,
                        pid = record.pid,
                        in_flight_turn,
                        "reattached to existing cockpit runner"
                    );
                    return ResumeOutcome::Attached;
                }
                Ok(Err(e)) => {
                    tracing::warn!(
                        target: "cockpit.supervisor",
                        session = %id,
                        "attach failed; falling back to fresh spawn: {e}"
                    );
                    crate::cockpit::worker_registry::delete(&id).ok();
                }
                Err(_) => {
                    tracing::warn!(
                        target: "cockpit.supervisor",
                        session = %id,
                        "attach timed out after 3s; falling back to fresh spawn"
                    );
                    crate::cockpit::worker_registry::delete(&id).ok();
                    return ResumeOutcome::RetryAfterAttachTimeout;
                }
            }
        } else {
            // Dead PID or missing socket: sweep the orphan registry
            // entry so the next attempt is a clean fresh spawn.
            crate::cockpit::worker_registry::delete(&id).ok();
        }
    }

    // Fresh-spawn fallback: we are about to spin up a brand new agent
    // process. The previous one (if any) was killed before it could
    // complete the in-flight prompt, so its turn is forever orphaned.
    // Publish a synthetic Stopped now so the UI doesn't keep
    // "thinking" after restart.
    if in_flight_turn {
        state
            .cockpit_supervisor
            .synthesize_stopped_for_orphan(&id, "orphaned_at_restart");
    }

    // Persisted cockpit-mode sessions auto-spawn even when
    // `AOE_EXPERIMENTAL_COCKPIT` is unset (the env-var gate is for
    // *new* sessions, not pre-existing ones). Log a warning per
    // session so operators who unset the env var on a daemon with
    // existing cockpit sessions know why those are still running.
    if !crate::cockpit::experimental_enabled() {
        tracing::warn!(
            target: "cockpit.supervisor",
            session = %id,
            "auto-spawning persisted cockpit-mode session while \
             AOE_EXPERIMENTAL_COCKPIT is not set. To stop cockpit \
             from running existing sessions, set \
             `cockpit.enabled = false` in config.toml and restart \
             `aoe serve`. To stop just this one, switch its \
             substrate to tmux from the dashboard."
        );
    }
    let supervisor = Arc::clone(&state.cockpit_supervisor);
    let agent = supervisor
        .pick_agent_for_tool(&tool, agent_override.as_deref())
        .await;
    let cwd = PathBuf::from(project_path);

    let spawn_result = supervisor
        .spawn(crate::cockpit::supervisor::SpawnRequest {
            session_id: id.clone(),
            agent: agent.clone(),
            cwd,
            additional_dirs: vec![],
            provider_env: vec![],
            model,
            stored_acp_session_id,
        })
        .await;
    if let Err(e) = spawn_result {
        // Re-check whether the session still exists in instances.
        // The user can delete a session during the spawn handshake
        // (2-3s for ACP), and the resulting error is noise for a
        // session that no longer exists. Demote to debug rather
        // than warn + AgentStartupError publish in that case.
        let still_present = state.instances.read().await.iter().any(|i| i.id == id);
        let message = format!("Failed to start cockpit agent {agent:?}: {e}");
        if still_present {
            tracing::warn!(
                target: "cockpit.supervisor",
                session = %id,
                agent = %agent,
                "auto-spawn reconciler failed: {message}"
            );
            supervisor.publish_startup_error(&id, message);
        } else {
            tracing::debug!(
                target: "cockpit.supervisor",
                session = %id,
                agent = %agent,
                "auto-spawn reconciler error after session removed (ignored): {message}"
            );
        }
    }
    ResumeOutcome::SpawnFinished
}

async fn sweep_orphan_workers(state: &Arc<AppState>, live: &HashSet<&String>) {
    // Sweep registry entries whose session no longer exists (deleted
    // while serve was down) and SIGTERM the orphan runner so the user
    // doesn't see a phantom in `aoe cockpit ps`. Only runs against
    // entries that aren't currently in our `workers` map.
    let Ok(records) = crate::cockpit::worker_registry::list() else {
        return;
    };
    for record in records {
        if live.contains(&record.session_id) {
            continue;
        }
        if state
            .cockpit_supervisor
            .is_running(&record.session_id)
            .await
        {
            continue;
        }
        tracing::info!(
            target: "cockpit.supervisor",
            session = %record.session_id,
            pid = record.pid,
            "sweeping orphan worker (no matching session on disk)"
        );
        #[cfg(unix)]
        if crate::cockpit::worker_registry::is_pid_alive(record.pid) {
            use nix::sys::signal::{kill, Signal};
            use nix::unistd::Pid;
            let _ = kill(Pid::from_raw(record.pid as i32), Signal::SIGTERM);
        }
        crate::cockpit::worker_registry::delete(&record.session_id).ok();
    }
}
