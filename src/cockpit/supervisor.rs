//! Cockpit worker supervisor.
//!
//! Owns a per-aoe-process map of session_id -> AcpClient handles. Spawns
//! the ACP agent subprocess on demand, bridges its events into the
//! per-AppState `cockpit_events_tx` broadcast channel, and fires push
//! notifications for ApprovalRequested events.
//!
//! Watchdog: when an agent's ACP connection task ends (subprocess exit,
//! transport break) the drain task respawns it. Up to
//! `MAX_RESPAWNS_IN_WINDOW` respawns are allowed inside `RESTART_WINDOW`;
//! beyond that the session is parked and an `AgentStartupError` event
//! is published so the UI can surface "session crashed" instead of
//! going silent.
//!
//! Producer side: `Supervisor::spawn(session_id, config)` creates an
//! AcpClient and a background task that drains its events.
//!
//! Consumer side: `Supervisor::send_prompt(session_id, text)` and
//! `Supervisor::resolve_permission(session_id, nonce, decision)` route
//! through the held client.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use thiserror::Error;
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use super::acp_client::{AcpClient, AcpError, SpawnConfig};
use super::agent_registry::{AgentRegistry, AgentSpec};
use super::approvals::{ApprovalDecision, Nonce};
use super::state::{CockpitSessionId, Event};

/// Maximum number of post-startup respawns within `RESTART_WINDOW`.
/// After this many crashes the session is parked and an
/// `AgentStartupError` event is published. The initial spawn does not
/// count toward this budget — it's always allowed.
const MAX_RESPAWNS_IN_WINDOW: u32 = 3;
const RESTART_WINDOW: Duration = Duration::from_secs(60);
/// Brief backoff before respawning an exited worker so we don't
/// hot-loop when the agent process crashes immediately on startup.
const RESPAWN_BACKOFF: Duration = Duration::from_millis(500);

#[derive(Debug, Error)]
pub enum SupervisorError {
    #[error("session {0:?} not found")]
    UnknownSession(String),
    #[error("acp client error: {0}")]
    Acp(#[from] AcpError),
    #[error("agent {0:?} not in registry")]
    UnknownAgent(String),
    #[error("session {0:?} already has a running cockpit worker")]
    AlreadyRunning(String),
    /// Configured `[cockpit] max_concurrent_workers` cap is full. The
    /// caller should surface this to the operator (REST: 503; CLI: a
    /// hint to delete an existing cockpit session or raise the cap)
    /// rather than retrying.
    #[error("cockpit worker capacity full ({current}/{limit}); raise [cockpit] max_concurrent_workers or delete an existing cockpit session")]
    CapacityFull { current: usize, limit: u32 },
    /// The spawn was cancelled by a concurrent `shutdown` call (e.g. the
    /// user clicked Disable while the ACP handshake was still in
    /// flight). The freshly-spawned client is dropped cleanly. Callers
    /// should treat this as a soft success: the requested end state
    /// (no worker for this session) holds.
    #[error("spawn for session {0:?} was cancelled by a concurrent shutdown")]
    SpawnCancelled(String),
}

/// Frame published to the broadcast channel; mirrors
/// `crate::server::CockpitBroadcastFrame` so the supervisor can be
/// tested without pulling in the server module.
pub trait BroadcastSink: Send + Sync + 'static {
    fn publish(&self, session_id: &str, seq: u64, event: &Event);
    fn approval_requested(&self, _session_id: &str, _approval_title: &str, _destructive: bool) {
        // Default: no-op. The server impl fires a push notification.
    }
}

struct WorkerHandle {
    client: Arc<Mutex<AcpClient>>,
    /// Background task draining events from the client. Aborted on
    /// shutdown.
    drain_task: JoinHandle<()>,
    /// Restart bookkeeping: timestamps of recent respawns (post-
    /// initial-spawn). Used by the watchdog to enforce
    /// `MAX_RESPAWNS_IN_WINDOW`. Empty on first spawn so the initial
    /// boot doesn't consume the budget.
    restart_history: Vec<Instant>,
    /// Stored so the watchdog can respawn the worker when its ACP
    /// connection task exits (subprocess crash, transport break).
    /// Populated for real workers; left as `None` for fake workers
    /// inserted by tests.
    spawn_config: Option<SpawnConfig>,
}

/// Per-session monotonically-increasing seq counter. Lives at the
/// supervisor level (not on `WorkerHandle`) so it survives shutdown
/// and respawn cycles, and also covers the no-worker
/// `publish_startup_error` path. Without this, both publishers
/// would start from seq=1 and collide in the replay buffer, which
/// the client-side `applyEvent` dedupe then turned into a silent
/// loss of the agent's first message after a retry.
type SeqMap = std::sync::Mutex<HashMap<String, u64>>;

pub struct Supervisor<S: BroadcastSink> {
    sink: Arc<S>,
    registry: Arc<Mutex<AgentRegistry>>,
    workers: Arc<Mutex<HashMap<String, WorkerHandle>>>,
    next_seqs: Arc<SeqMap>,
    /// Reservation set: a session_id present here means another task
    /// is mid-`spawn` for it. `AcpClient::spawn` takes 2-3s for the
    /// initial handshake; without this reservation, two concurrent
    /// callers (POST /api/sessions auto-spawn + 2s reconciler tick)
    /// both pass the empty-`workers` check and race to insert,
    /// silently overwriting the first WorkerHandle. The dropped
    /// client's cmd_tx then closes its connection task and burns the
    /// restart budget. The RAII `SpawnReservation` guard in `spawn`
    /// removes the entry on success, error, or panic.
    pending_spawns: Arc<Mutex<HashSet<String>>>,
    /// Session ids whose in-flight `spawn` should bail out instead of
    /// inserting the freshly-spawned WorkerHandle. Set by
    /// `shutdown` when it observes a session that's in
    /// `pending_spawns` but not yet in `workers` — without this, a
    /// `cockpit_disable` arriving during the 2-3s ACP handshake
    /// would no-op (shutdown returns UnknownSession) but the
    /// in-flight spawn would still complete a few seconds later,
    /// producing an orphaned worker the user can no longer manage.
    cancelled_spawns: Arc<Mutex<HashSet<String>>>,
    /// Cap on concurrently-running workers, snapshotted from
    /// `[cockpit] max_concurrent_workers` at startup. Enforced in
    /// `spawn`; new workers past the cap return `CapacityFull`.
    /// Tests use `Supervisor::new` (effectively unbounded); production
    /// uses `Supervisor::with_capacity`.
    max_concurrent_workers: u32,
}

/// RAII guard: ensures a session_id is removed from `pending_spawns`
/// when `spawn` returns or unwinds, no matter which path was taken.
/// Without this, a panic or early-return in the middle of `spawn`
/// would leave a phantom reservation that blocks every future spawn
/// for that session.
struct SpawnReservation {
    pending: Arc<Mutex<HashSet<String>>>,
    session_id: String,
}

impl Drop for SpawnReservation {
    fn drop(&mut self) {
        // Sync remove via blocking_lock would deadlock inside an
        // async runtime; spawn a detached task to release. The set
        // operation is constant-time and the task lives only for the
        // duration of one `lock().await` + remove.
        let pending = Arc::clone(&self.pending);
        let session_id = std::mem::take(&mut self.session_id);
        tokio::spawn(async move {
            pending.lock().await.remove(&session_id);
        });
    }
}

/// Inputs to `Supervisor::spawn`. A struct (rather than seven
/// positional params with `#[allow(clippy::too_many_arguments)]`)
/// because the previous signature was the kind that produces real
/// bugs the next time someone adds a field — the auto-spawn caller in
/// `create_session` had to thread six identical values through the
/// API plus a seventh on this PR.
#[derive(Debug, Clone)]
pub struct SpawnRequest {
    pub session_id: String,
    pub agent: String,
    pub cwd: PathBuf,
    pub additional_dirs: Vec<PathBuf>,
    pub provider_env: Vec<(String, String)>,
    pub model: Option<String>,
    /// ACP session id from a previous run; when `Some` and the agent
    /// advertises `load_session = true`, the spawn calls
    /// `LoadSessionRequest` instead of `NewSessionRequest`.
    pub stored_acp_session_id: Option<String>,
}

impl<S: BroadcastSink> Supervisor<S> {
    /// Constructor with no concurrency cap. Used in tests; production
    /// callers should use [`Supervisor::with_capacity`] so the
    /// configured `[cockpit] max_concurrent_workers` actually limits
    /// the worker pool.
    pub fn new(sink: Arc<S>) -> Self {
        Self::with_capacity(sink, u32::MAX)
    }

    pub fn with_capacity(sink: Arc<S>, max_concurrent_workers: u32) -> Self {
        Self {
            sink,
            registry: Arc::new(Mutex::new(AgentRegistry::with_defaults())),
            workers: Arc::new(Mutex::new(HashMap::new())),
            next_seqs: Arc::new(std::sync::Mutex::new(HashMap::new())),
            pending_spawns: Arc::new(Mutex::new(HashSet::new())),
            cancelled_spawns: Arc::new(Mutex::new(HashSet::new())),
            max_concurrent_workers,
        }
    }

    /// Resolve the agent spec from the registry. Surfaces UnknownAgent
    /// when the caller picks a name that hasn't been configured.
    pub async fn resolve_agent(&self, name: &str) -> Result<AgentSpec, SupervisorError> {
        self.registry
            .lock()
            .await
            .get(name)
            .cloned()
            .ok_or_else(|| SupervisorError::UnknownAgent(name.into()))
    }

    /// Pick the agent name to spawn for an instance. Precedence:
    ///   1. explicit `cockpit_agent` override on the instance
    ///   2. registry entry keyed on the instance's tool name
    ///      (so `tool="opencode"` → registry `"opencode"` →
    ///      `opencode acp`, etc.)
    ///   3. legacy fallback: `claude` for the claude tool, otherwise
    ///      `aoe-agent` (our bundled multi-provider agent)
    pub async fn pick_agent_for_tool(&self, tool: &str, explicit_override: Option<&str>) -> String {
        if let Some(name) = explicit_override {
            if !name.is_empty() {
                return name.to_string();
            }
        }
        // Step 2: tool-keyed registry lookup. Done under the same
        // lock as resolve_agent so a custom override registered via
        // upsert_agent is honored.
        {
            let reg = self.registry.lock().await;
            if reg.get(tool).is_some() {
                return tool.to_string();
            }
        }
        // Step 3: legacy fallbacks.
        if tool == "claude" {
            "claude".into()
        } else {
            "aoe-agent".into()
        }
    }

    pub async fn registry_snapshot(&self) -> AgentRegistry {
        self.registry.lock().await.clone()
    }

    /// Publish a synthetic AgentStartupError event for a session whose
    /// worker never came online. Used by the auto-spawn-after-create
    /// path so the UI shows a remediation hint instead of an empty,
    /// silent conversation when `claude-agent-acp` isn't installed (or
    /// `npx -y` is still downloading on first run).
    pub fn publish_startup_error(&self, session_id: &str, message: String) {
        let seq = next_seq(&self.next_seqs, session_id);
        self.sink
            .publish(session_id, seq, &Event::AgentStartupError { message });
    }

    /// Publish a UserPromptSent event before forwarding the prompt to
    /// the ACP agent. The replay buffer (and on-disk event store) needs
    /// the user's side of the conversation in the same stream as agent
    /// chunks; otherwise a reconnecting client sees only assistant text
    /// and every turn concatenates into one giant message.
    pub fn publish_user_prompt(&self, session_id: &str, text: String) {
        let seq = next_seq(&self.next_seqs, session_id);
        self.sink
            .publish(session_id, seq, &Event::UserPromptSent { text });
    }

    /// Drop per-session bookkeeping (replay seq counter). Called when
    /// the session is deleted or its substrate is switched away from
    /// cockpit, so the next cockpit_enable starts a fresh conversation
    /// from seq=1 with a clean replay buffer.
    pub fn forget_session(&self, session_id: &str) {
        if let Ok(mut guard) = self.next_seqs.lock() {
            guard.remove(session_id);
        }
    }

    /// Pre-populate `next_seqs` from `(session_id, max_seq)` pairs.
    /// Used at server startup to seed the counter from the on-disk
    /// event store so a fresh publish gets max_seq + 1, not 1, and
    /// doesn't collide with restored history.
    pub fn hydrate_seqs(&self, pairs: impl IntoIterator<Item = (String, u64)>) {
        if let Ok(mut guard) = self.next_seqs.lock() {
            for (session_id, seq) in pairs {
                guard.insert(session_id, seq);
            }
        }
    }

    pub async fn upsert_agent(&self, name: String, spec: AgentSpec) {
        self.registry.lock().await.upsert(name, spec);
    }

    /// Spawn a cockpit worker for the given session. Returns Err if a
    /// worker is already running for that session, if a spawn for
    /// the same session is already in progress, or if the
    /// `max_concurrent_workers` cap is full.
    ///
    /// Concurrency: `AcpClient::spawn` performs the ACP handshake
    /// (initialize + session/new), which takes 2-3s while no lock is
    /// held. Without the `pending_spawns` reservation below, two
    /// concurrent callers for the same session_id would both pass
    /// the empty-`workers` check, both finish the handshake, and
    /// both insert into `workers` — the second insert silently
    /// overwriting the first WorkerHandle. The dropped client's
    /// cmd_tx would then close, its connection task would exit
    /// cleanly, and the orphaned drain task would burn the restart
    /// budget respawning a worker the supervisor no longer points
    /// at. The reservation makes the second caller fail fast with
    /// AlreadyRunning instead.
    pub async fn spawn(&self, req: SpawnRequest) -> Result<(), SupervisorError> {
        let SpawnRequest {
            session_id,
            agent,
            cwd,
            additional_dirs,
            provider_env,
            model,
            stored_acp_session_id,
        } = req;
        let _reservation = {
            let workers = self.workers.lock().await;
            if workers.contains_key(&session_id) {
                return Err(SupervisorError::AlreadyRunning(session_id));
            }
            if workers.len() >= self.max_concurrent_workers as usize {
                return Err(SupervisorError::CapacityFull {
                    current: workers.len(),
                    limit: self.max_concurrent_workers,
                });
            }
            // Acquire pending_spawns under the same critical section
            // as the workers check so the (workers ∪ pending) set is
            // observed atomically. A second caller arriving here
            // sees either the workers entry (after insert below) or
            // the pending entry; in both cases it returns
            // AlreadyRunning.
            let mut pending = self.pending_spawns.lock().await;
            if !pending.insert(session_id.clone()) {
                return Err(SupervisorError::AlreadyRunning(session_id));
            }
            SpawnReservation {
                pending: Arc::clone(&self.pending_spawns),
                session_id: session_id.clone(),
            }
        };

        let mut spec = self.resolve_agent(&agent).await?;
        // Apply ${aoe_data_dir} placeholder substitution against the
        // appropriate path; if the placeholder is not consumed it stays
        // as-is and the spawn will fail with a clear error.
        if spec.command.contains("${aoe_data_dir}") {
            if let Ok(data_dir) = crate::session::get_app_dir() {
                spec.command = spec
                    .command
                    .replace("${aoe_data_dir}", &data_dir.to_string_lossy());
            }
        }

        let mut env = provider_env;
        if let Some(model) = model {
            env.push(("AOE_AGENT_MODEL".into(), model));
        }

        let config = SpawnConfig {
            spec,
            cwd,
            additional_dirs,
            provider_env: env,
            socket_path: None,
            stored_acp_session_id: stored_acp_session_id.clone(),
        };

        debug!(
            target: "cockpit.supervisor",
            session = %session_id,
            stored_id = ?stored_acp_session_id,
            "spawning cockpit worker"
        );

        let cockpit_session_id = CockpitSessionId(session_id.clone());
        let mut client = AcpClient::spawn(config.clone(), cockpit_session_id.clone()).await?;

        info!(target: "cockpit.supervisor", session = %session_id, "cockpit worker spawned");

        // Move the inbound receiver out so the drain task can poll events
        // without holding the client mutex (which would deadlock
        // send_prompt: drain holds the lock across recv().await). The
        // receiver is always Some on a freshly-spawned client.
        let inbound = client
            .take_inbound()
            .expect("freshly spawned AcpClient always has inbound receiver");
        let client = Arc::new(Mutex::new(client));

        let mut workers = self.workers.lock().await;
        // Belt-and-braces: even with the pending_spawns reservation,
        // re-check that no worker has been inserted under our nose.
        // If it has, drop the freshly-spawned client (its Drop will
        // close cmd_tx and tear down the subprocess cleanly) and
        // surface AlreadyRunning rather than overwriting the live
        // WorkerHandle.
        if workers.contains_key(&session_id) {
            drop(workers);
            drop(client);
            return Err(SupervisorError::AlreadyRunning(session_id));
        }
        // Cancellation: a concurrent shutdown observed this session
        // mid-handshake and asked us to bail. Drop the client cleanly
        // and skip the workers insert so the user's "disable" actually
        // takes effect instead of being silently overwritten by the
        // 2-3s-late spawn completion.
        if self.cancelled_spawns.lock().await.remove(&session_id) {
            debug!(
                target: "cockpit.supervisor",
                session = %session_id,
                "spawn cancelled by concurrent shutdown; dropping freshly-spawned client"
            );
            drop(workers);
            drop(client);
            return Err(SupervisorError::SpawnCancelled(session_id));
        }
        let drain_task = self.start_drain_task(session_id.clone(), inbound);
        workers.insert(
            session_id,
            WorkerHandle {
                client,
                drain_task,
                // Empty: the initial spawn doesn't count toward the
                // restart budget. Each crash-and-respawn appends one
                // entry; budget burns when entries-in-window exceed
                // MAX_RESPAWNS_IN_WINDOW.
                restart_history: vec![],
                spawn_config: Some(config),
            },
        );
        Ok(())
    }

    /// Drain events from a worker into the broadcast sink. When the
    /// inbound channel closes (subprocess exit / transport break) the
    /// drain task asks the supervisor to respawn the worker, falling
    /// back to a parked-error state if the restart budget is burned.
    fn start_drain_task(
        &self,
        session_id: String,
        initial_inbound: mpsc::Receiver<Event>,
    ) -> JoinHandle<()> {
        let sink = Arc::clone(&self.sink);
        let workers = Arc::clone(&self.workers);
        let next_seqs = Arc::clone(&self.next_seqs);
        tokio::spawn(async move {
            let mut inbound = initial_inbound;
            loop {
                while let Some(event) = inbound.recv().await {
                    // Mirror the agent-assigned id into the cached
                    // spawn_config so a subsequent crash respawn picks
                    // up the latest id and calls session/load instead
                    // of session/new. Mirror SessionContextReset the
                    // other way so a load failure on this run doesn't
                    // keep retrying the same dead id on the next
                    // respawn.
                    match &event {
                        Event::AcpSessionAssigned { acp_session_id } => {
                            let mut guard = workers.lock().await;
                            if let Some(handle) = guard.get_mut(&session_id) {
                                if let Some(cfg) = handle.spawn_config.as_mut() {
                                    info!(
                                        target: "cockpit.supervisor",
                                        session = %session_id,
                                        acp_session_id = %acp_session_id,
                                        "caching agent-assigned id for future respawn"
                                    );
                                    cfg.stored_acp_session_id = Some(acp_session_id.clone());
                                }
                            }
                        }
                        Event::SessionContextReset { reason } => {
                            let mut guard = workers.lock().await;
                            if let Some(handle) = guard.get_mut(&session_id) {
                                if let Some(cfg) = handle.spawn_config.as_mut() {
                                    info!(
                                        target: "cockpit.supervisor",
                                        session = %session_id,
                                        %reason,
                                        "clearing cached id after session/load failure"
                                    );
                                    cfg.stored_acp_session_id = None;
                                }
                            }
                        }
                        _ => {}
                    }
                    let seq = next_seq(&next_seqs, &session_id);
                    sink.publish(&session_id, seq, &event);
                    if let Event::ApprovalRequested { approval } = &event {
                        sink.approval_requested(
                            &session_id,
                            &approval.tool_call.name,
                            approval.destructive,
                        );
                    }
                }

                // Channel closed: the agent's connection task ended.
                // Either the subprocess exited or the transport broke.
                // Try to respawn within the restart budget; otherwise
                // park the session with a synthetic error event.
                warn!(
                    target: "cockpit.supervisor",
                    session = %session_id,
                    "drain channel closed (agent connection task ended); evaluating respawn"
                );
                let respawn_config: SpawnConfig =
                    match restart_decision(&workers, &session_id).await {
                        RestartDecision::Respawn(cfg) => {
                            info!(
                                target: "cockpit.supervisor",
                                session = %session_id,
                                command = %cfg.spec.command,
                                stored_id = ?cfg.stored_acp_session_id,
                                "respawn approved; sleeping {}ms before restart",
                                RESPAWN_BACKOFF.as_millis()
                            );
                            *cfg
                        }
                        RestartDecision::BudgetBurned => {
                            warn!(
                                target: "cockpit.supervisor",
                                session = %session_id,
                                max_respawns = MAX_RESPAWNS_IN_WINDOW,
                                window_secs = RESTART_WINDOW.as_secs(),
                                "restart budget burned; parking session"
                            );
                            let seq = next_seq(&next_seqs, &session_id);
                            sink.publish(
                                &session_id,
                                seq,
                                &Event::AgentStartupError {
                                    message: format!(
                                        "ACP agent crashed more than {} times in {}s; \
                                     not respawning. Use the web dashboard to retry.",
                                        MAX_RESPAWNS_IN_WINDOW,
                                        RESTART_WINDOW.as_secs()
                                    ),
                                },
                            );
                            // Remove the dead WorkerHandle so a retry
                            // (POST /api/sessions/:id/cockpit/spawn) doesn't
                            // hit AlreadyRunning. The seq counter and replay
                            // buffer survive so the retry's events stay
                            // monotonic and the user keeps the conversation
                            // log up to the crash point.
                            let mut guard = workers.lock().await;
                            guard.remove(&session_id);
                            return;
                        }
                        RestartDecision::Gone => {
                            // The worker entry was removed (shutdown / delete).
                            // Exit quietly.
                            return;
                        }
                    };

                tokio::time::sleep(RESPAWN_BACKOFF).await;

                let cockpit_session_id = CockpitSessionId(session_id.clone());
                let mut new_client =
                    match AcpClient::spawn(respawn_config.clone(), cockpit_session_id).await {
                        Ok(c) => c,
                        Err(e) => {
                            warn!(
                                target: "cockpit.supervisor",
                                session = %session_id,
                                "respawn failed: {e}"
                            );
                            let seq = next_seq(&next_seqs, &session_id);
                            sink.publish(
                                &session_id,
                                seq,
                                &Event::AgentStartupError {
                                    message: format!("ACP agent respawn failed: {e}"),
                                },
                            );
                            // Drop the dead WorkerHandle so the user can
                            // retry via POST /api/sessions/:id/cockpit/spawn
                            // without hitting AlreadyRunning. Without this
                            // the entry sticks around with a closed cmd_tx
                            // and every send_prompt fails until the daemon
                            // restarts. Mirrors the BudgetBurned and
                            // missing-inbound branches.
                            let mut guard = workers.lock().await;
                            guard.remove(&session_id);
                            return;
                        }
                    };
                let new_inbound = match new_client.take_inbound() {
                    Some(rx) => rx,
                    None => {
                        // Belt-and-braces: AcpClient::spawn pairs the
                        // inbound receiver with the client today, so
                        // this branch never fires. Logging instead of
                        // panicking guards the daemon if a future
                        // refactor breaks the invariant.
                        warn!(
                            target: "cockpit.supervisor",
                            session = %session_id,
                            "respawned client missing inbound receiver; parking",
                        );
                        let seq = next_seq(&next_seqs, &session_id);
                        sink.publish(
                            &session_id,
                            seq,
                            &Event::AgentStartupError {
                                message: "respawned ACP client had no inbound channel".into(),
                            },
                        );
                        let mut guard = workers.lock().await;
                        guard.remove(&session_id);
                        return;
                    }
                };

                {
                    let mut guard = workers.lock().await;
                    let Some(handle) = guard.get_mut(&session_id) else {
                        return;
                    };
                    handle.client = Arc::new(Mutex::new(new_client));
                }

                info!(
                    target: "cockpit.supervisor",
                    session = %session_id,
                    "cockpit worker respawned"
                );
                inbound = new_inbound;
            }
        })
    }

    /// Wait until the worker for `session_id` is fully spawned, or the
    /// pending spawn drops out (failed/cancelled), or `deadline` elapses.
    /// Returns true if the worker is now in the map.
    ///
    /// Hooks for the prompt/cancel/set_mode REST handlers: the user can
    /// click Send right after enabling cockpit, while `Supervisor::spawn`
    /// is still in the 2-3s ACP handshake. Without this wait, those
    /// requests would 404 because the WorkerHandle isn't in `workers`
    /// yet, even though it's about to be. Polling at 50ms keeps the
    /// happy-path latency negligible while bounding the wait.
    async fn wait_for_worker(&self, session_id: &str, deadline: std::time::Duration) -> bool {
        let start = std::time::Instant::now();
        loop {
            if self.workers.lock().await.contains_key(session_id) {
                return true;
            }
            // No worker yet. If a spawn is in flight, wait for it;
            // otherwise the worker isn't coming and we should fail
            // fast rather than burn the full deadline.
            if !self.pending_spawns.lock().await.contains(session_id) {
                return false;
            }
            if start.elapsed() >= deadline {
                return false;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    /// Send a user prompt to a running cockpit worker.
    pub async fn send_prompt(&self, session_id: &str, text: &str) -> Result<(), SupervisorError> {
        self.wait_for_worker(session_id, std::time::Duration::from_secs(10))
            .await;
        let workers = self.workers.lock().await;
        let handle = workers
            .get(session_id)
            .ok_or_else(|| SupervisorError::UnknownSession(session_id.into()))?;
        let client = handle.client.lock().await;
        client.send_prompt(text).await?;
        Ok(())
    }

    /// Cancel the current turn for a running cockpit worker. Best-effort:
    /// returns Ok if the worker exists even when no turn is in flight.
    pub async fn cancel_prompt(&self, session_id: &str) -> Result<(), SupervisorError> {
        self.wait_for_worker(session_id, std::time::Duration::from_secs(10))
            .await;
        let workers = self.workers.lock().await;
        let handle = workers
            .get(session_id)
            .ok_or_else(|| SupervisorError::UnknownSession(session_id.into()))?;
        let client = handle.client.lock().await;
        client.cancel_prompt().await?;
        Ok(())
    }

    /// Set the active session mode via ACP session/set_mode.
    pub async fn set_mode(&self, session_id: &str, mode_id: &str) -> Result<(), SupervisorError> {
        self.wait_for_worker(session_id, std::time::Duration::from_secs(10))
            .await;
        let workers = self.workers.lock().await;
        let handle = workers
            .get(session_id)
            .ok_or_else(|| SupervisorError::UnknownSession(session_id.into()))?;
        let client = handle.client.lock().await;
        client.set_mode(mode_id).await?;
        Ok(())
    }

    /// Resolve a pending approval.
    pub async fn resolve_permission(
        &self,
        session_id: &str,
        nonce: Nonce,
        decision: ApprovalDecision,
    ) -> Result<(), SupervisorError> {
        let workers = self.workers.lock().await;
        let handle = workers
            .get(session_id)
            .ok_or_else(|| SupervisorError::UnknownSession(session_id.into()))?;
        let client = handle.client.lock().await;
        client.resolve_permission(nonce, decision).await?;
        Ok(())
    }

    /// Shutdown a single cockpit worker.
    pub async fn shutdown(&self, session_id: &str) -> Result<(), SupervisorError> {
        // Hold workers + pending_spawns simultaneously so the spawn
        // can't observe an empty workers map, finish the handshake,
        // and insert a WorkerHandle while we're walking through this
        // function. Lock order matches `spawn`: workers, then pending.
        let mut workers = self.workers.lock().await;
        let pending_has_it = self.pending_spawns.lock().await.contains(session_id);
        if let Some(handle) = workers.remove(session_id) {
            // Worker is alive — tear it down.
            drop(workers);
            {
                let client = handle.client.lock().await;
                let _ = client.shutdown().await;
            }
            handle.drain_task.abort();
            return Ok(());
        }
        if pending_has_it {
            // Spawn is mid-handshake. Mark it cancelled so
            // `Supervisor::spawn`'s pre-insert check bails instead of
            // installing an orphaned worker. The reservation cleanup
            // (SpawnReservation::Drop) clears `pending_spawns` on
            // exit, so we don't have to.
            drop(workers);
            self.cancelled_spawns
                .lock()
                .await
                .insert(session_id.to_string());
            debug!(
                target: "cockpit.supervisor",
                session = %session_id,
                "shutdown: spawn in flight; marked for cancellation"
            );
            return Ok(());
        }
        Err(SupervisorError::UnknownSession(session_id.into()))
    }

    /// Shutdown every worker. Called on aoe serve shutdown.
    pub async fn shutdown_all(&self) {
        let mut workers = self.workers.lock().await;
        for (id, handle) in workers.drain() {
            debug!(target: "cockpit.supervisor", session = %id, "shutting down");
            {
                let client = handle.client.lock().await;
                let _ = client.shutdown().await;
            }
            handle.drain_task.abort();
        }
    }

    /// Whether this session has a running cockpit worker, or a
    /// spawn currently in-flight. The pending check prevents the
    /// reconciler from racing the auto-spawn-after-create path: a
    /// freshly-created cockpit session takes 2-3s for the ACP
    /// handshake to insert the WorkerHandle, and during that window
    /// `workers.contains_key` is false.
    pub async fn is_running(&self, session_id: &str) -> bool {
        if self.workers.lock().await.contains_key(session_id) {
            return true;
        }
        self.pending_spawns.lock().await.contains(session_id)
    }

    /// Return the number of running workers (for the doctor + stats).
    pub async fn count(&self) -> usize {
        self.workers.lock().await.len()
    }
}

enum RestartDecision {
    // Boxed because `SpawnConfig` is significantly larger than the
    // unit variants — clippy::large_enum_variant flags the size
    // imbalance, and the indirection costs nothing on the cold-path
    // respawn flow.
    Respawn(Box<SpawnConfig>),
    BudgetBurned,
    /// The worker entry was removed (e.g. shutdown).
    Gone,
}

async fn restart_decision(
    workers: &Arc<Mutex<HashMap<String, WorkerHandle>>>,
    session_id: &str,
) -> RestartDecision {
    let mut guard = workers.lock().await;
    let Some(handle) = guard.get_mut(session_id) else {
        debug!(
            target: "cockpit.supervisor",
            session = %session_id,
            "restart_decision: worker entry gone (shutdown / delete)"
        );
        return RestartDecision::Gone;
    };
    let now = Instant::now();
    let window_start = now - RESTART_WINDOW;
    let pre_count = handle.restart_history.len();
    handle.restart_history.retain(|t| *t >= window_start);
    let pruned = pre_count - handle.restart_history.len();
    handle.restart_history.push(now);
    let count = handle.restart_history.len() as u32;
    debug!(
        target: "cockpit.supervisor",
        session = %session_id,
        respawns_in_window = count,
        max_in_window = MAX_RESPAWNS_IN_WINDOW,
        window_secs = RESTART_WINDOW.as_secs(),
        pruned_old_entries = pruned,
        "restart_decision: tallied recent crashes"
    );
    if count > MAX_RESPAWNS_IN_WINDOW {
        return RestartDecision::BudgetBurned;
    }
    match handle.spawn_config.clone() {
        Some(cfg) => RestartDecision::Respawn(Box::new(cfg)),
        // Test handles inserted via fake_for_test: the entry exists but
        // we have no real spawn config. Treat as budget-burned so the
        // drain task exits cleanly.
        None => RestartDecision::BudgetBurned,
    }
}

/// Increment and return the per-session seq counter. Lives at the
/// supervisor level so the no-worker `publish_startup_error` path
/// and the drain task share a single source of truth — otherwise
/// both used to start at seq=1 and collide in the replay buffer
/// after a retry, which the client-side dedupe then rendered as a
/// silently-lost first message.
fn next_seq(next_seqs: &SeqMap, session_id: &str) -> u64 {
    let mut guard = match next_seqs.lock() {
        Ok(g) => g,
        Err(poisoned) => poisoned.into_inner(),
    };
    let entry = guard.entry(session_id.to_string()).or_insert(0);
    *entry = entry.saturating_add(1);
    *entry
}

/// Callback fired when the supervisor observes an ApprovalRequested
/// event for a session. The server impl uses this to trigger a Web
/// Push notification; the test impl just records the call.
pub type ApprovalHook = Arc<dyn Fn(&str, &str, bool) + Send + Sync>;

/// A `BroadcastSink` impl backed by a tokio broadcast channel. The
/// AppState in the server module wires this so cockpit events flow
/// straight into the existing WebSocket fanout, and snapshots them
/// into the per-session replay buffer used by the snapshot endpoint.
///
/// The replay buffer uses a `std::sync::Mutex` so the publish path
/// stays synchronous: ordering matters (the buffer must observe seqs
/// in publish order) and `tokio::spawn` does not preserve task
/// ordering. The lock is held only long enough to push a single
/// event, which is bounded; the REST snapshot handler also takes
/// this lock briefly.
pub struct ChannelSink {
    pub tx: broadcast::Sender<crate::server::CockpitBroadcastFrame>,
    pub on_approval: ApprovalHook,
    /// Disk-backed event log. The single source of truth for replay:
    /// the WS-on-connect drain, the `/cockpit/replay` REST endpoint,
    /// and the supervisor's startup `hydrate_seqs` all read from here.
    /// Each publish has a monotonic seq from `Supervisor::next_seqs`
    /// which is hydrated from this store at startup, so seqs survive
    /// `aoe serve` restart without coordination.
    pub event_store: Arc<crate::cockpit::event_store::EventStore>,
}

impl BroadcastSink for ChannelSink {
    fn publish(&self, session_id: &str, seq: u64, event: &Event) {
        // Persist FIRST so a disk failure can be surfaced before
        // broadcast subscribers see an event the on-disk log doesn't
        // have. If the write fails the seq is already burned (the
        // caller allocated it via next_seq), so we publish a typed
        // gap event in its place — the frontend reducer can render a
        // "history truncated at seq N" notice and the user can
        // reload to recover via the `/cockpit/replay` endpoint.
        let event_to_publish: Event;
        let event_ref: &Event = match self.event_store.record(session_id, seq, event) {
            Ok(()) => event,
            Err(e) => {
                tracing::warn!(
                    target: "cockpit.event_store",
                    session = %session_id,
                    seq,
                    "event store write failed; substituting AgentStartupError so the gap is visible: {e}"
                );
                event_to_publish = Event::AgentStartupError {
                    message: format!("event store write failed at seq {seq}: {e}"),
                };
                &event_to_publish
            }
        };

        let frame = crate::server::CockpitBroadcastFrame {
            session_id: session_id.to_string(),
            seq,
            event: Arc::new(event_ref.clone()),
        };
        let _ = self.tx.send(frame);
    }

    fn approval_requested(&self, session_id: &str, approval_title: &str, destructive: bool) {
        (self.on_approval)(session_id, approval_title, destructive);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// In-memory sink that captures published frames.
    struct VecSink {
        frames: std::sync::Mutex<Vec<(String, u64, Event)>>,
        approvals: std::sync::Mutex<Vec<(String, String, bool)>>,
    }
    impl VecSink {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                frames: std::sync::Mutex::new(Vec::new()),
                approvals: std::sync::Mutex::new(Vec::new()),
            })
        }
    }
    impl BroadcastSink for VecSink {
        fn publish(&self, session_id: &str, seq: u64, event: &Event) {
            self.frames
                .lock()
                .unwrap()
                .push((session_id.to_string(), seq, event.clone()));
        }
        fn approval_requested(&self, session: &str, title: &str, destructive: bool) {
            self.approvals.lock().unwrap().push((
                session.to_string(),
                title.to_string(),
                destructive,
            ));
        }
    }

    #[tokio::test]
    async fn spawn_unknown_agent_errors_cleanly() {
        let sink = VecSink::new();
        let sup = Supervisor::new(sink);
        let result = sup
            .spawn(SpawnRequest {
                session_id: "s-1".into(),
                agent: "no-such-agent".into(),
                cwd: std::env::temp_dir(),
                additional_dirs: vec![],
                provider_env: vec![],
                model: None,
                stored_acp_session_id: None,
            })
            .await;
        assert!(matches!(result, Err(SupervisorError::UnknownAgent(_))));
    }

    #[tokio::test]
    async fn double_spawn_returns_already_running() {
        let sink = VecSink::new();
        let sup = Supervisor::new(sink);
        // Inject a fake worker by inserting directly into the workers
        // map. We can't actually spawn without a real agent binary
        // here; this verifies the guard path.
        let mut workers = sup.workers.lock().await;
        let (client, _tx) = AcpClient::fake_for_test(CockpitSessionId("s-1".into()));
        let drain = tokio::spawn(async {});
        workers.insert(
            "s-1".into(),
            WorkerHandle {
                client: Arc::new(Mutex::new(client)),
                drain_task: drain,
                restart_history: vec![Instant::now()],
                spawn_config: None,
            },
        );
        drop(workers);

        let result = sup
            .spawn(SpawnRequest {
                session_id: "s-1".into(),
                agent: "claude-code".into(),
                cwd: std::env::temp_dir(),
                additional_dirs: vec![],
                provider_env: vec![],
                model: None,
                stored_acp_session_id: None,
            })
            .await;
        assert!(matches!(result, Err(SupervisorError::AlreadyRunning(_))));
    }

    #[tokio::test]
    async fn count_and_is_running() {
        let sink = VecSink::new();
        let sup = Supervisor::new(sink);
        assert_eq!(sup.count().await, 0);
        assert!(!sup.is_running("anything").await);
    }

    /// Watchdog: after MAX_RESPAWNS_IN_WINDOW respawn attempts inside
    /// RESTART_WINDOW, `restart_decision` returns `BudgetBurned` so the
    /// drain task parks the session instead of hot-looping.
    #[tokio::test]
    async fn restart_budget_burns_after_threshold() {
        let sink = VecSink::new();
        let sup = Supervisor::new(sink);
        // Build a worker handle with a real-looking spawn_config so the
        // budget path returns Respawn until we exhaust the window.
        let dummy_spec = AgentSpec {
            command: "/bin/true".into(),
            args: vec![],
            description: "test fixture".into(),
            env_allowlist: None,
        };
        let dummy_config = SpawnConfig {
            spec: dummy_spec,
            cwd: std::env::temp_dir(),
            additional_dirs: vec![],
            provider_env: vec![],
            socket_path: None,
            stored_acp_session_id: None,
        };
        {
            let mut workers = sup.workers.lock().await;
            let (client, _tx) = AcpClient::fake_for_test(CockpitSessionId("s-1".into()));
            let drain = tokio::spawn(async {});
            workers.insert(
                "s-1".into(),
                WorkerHandle {
                    client: Arc::new(Mutex::new(client)),
                    drain_task: drain,
                    restart_history: vec![],
                    spawn_config: Some(dummy_config),
                },
            );
        }

        for i in 0..MAX_RESPAWNS_IN_WINDOW {
            let decision = restart_decision(&sup.workers, "s-1").await;
            assert!(
                matches!(decision, RestartDecision::Respawn(_)),
                "decision #{i} should be Respawn",
            );
        }
        // One more push past the threshold should burn the budget.
        let decision = restart_decision(&sup.workers, "s-1").await;
        assert!(matches!(decision, RestartDecision::BudgetBurned));
    }

    /// `next_seq` increments per-session and is independent of the
    /// `workers` map (so `publish_startup_error` and the drain task
    /// share a counter even though the former runs while no
    /// WorkerHandle exists).
    #[tokio::test]
    async fn next_seq_is_per_session_and_persistent() {
        let sink = VecSink::new();
        let sup = Supervisor::new(sink);
        assert_eq!(next_seq(&sup.next_seqs, "s-1"), 1);
        assert_eq!(next_seq(&sup.next_seqs, "s-1"), 2);
        // Different session has its own counter.
        assert_eq!(next_seq(&sup.next_seqs, "s-2"), 1);
        // s-1 keeps incrementing.
        assert_eq!(next_seq(&sup.next_seqs, "s-1"), 3);
    }

    /// `publish_user_prompt` writes a `UserPromptSent { text }` event
    /// through the sink with a fresh seq. The handler invokes this
    /// before forwarding to the agent so the on-disk store has the
    /// user side of the conversation; if seq weren't allocated here,
    /// the agent's first reply chunk would collide on the same seq
    /// and the client-side dedupe would silently drop one of them.
    #[tokio::test]
    async fn publish_user_prompt_emits_event_and_increments_seq() {
        let sink = VecSink::new();
        let sup = Supervisor::new(sink.clone());
        sup.publish_user_prompt("s-1", "first prompt".into());
        sup.publish_user_prompt("s-1", "second prompt".into());

        let frames = sink.frames.lock().unwrap().clone();
        assert_eq!(frames.len(), 2);
        let (sid, seq, event) = &frames[0];
        assert_eq!(sid, "s-1");
        assert_eq!(*seq, 1);
        assert!(matches!(
            event,
            Event::UserPromptSent { text } if text == "first prompt"
        ));
        let (_, seq2, event2) = &frames[1];
        assert_eq!(*seq2, 2);
        assert!(matches!(
            event2,
            Event::UserPromptSent { text } if text == "second prompt"
        ));
    }

    /// After `hydrate_seqs` (called at startup with the on-disk
    /// max-seq map), the next publish for that session must return
    /// stored_max + 1, not 1. Without this, restoring from a
    /// non-empty event store would re-issue seq=1 and the INSERT OR
    /// IGNORE on the disk path would silently drop the new event.
    #[tokio::test]
    async fn hydrate_seqs_resumes_from_stored_max() {
        let sink = VecSink::new();
        let sup = Supervisor::new(sink.clone());
        // Simulate: we've persisted up to seq=42 for s-1 and seq=7 for s-2.
        sup.hydrate_seqs([("s-1".to_string(), 42), ("s-2".to_string(), 7)]);

        sup.publish_user_prompt("s-1", "after restart".into());
        sup.publish_startup_error("s-2", "retry".into());

        let frames = sink.frames.lock().unwrap().clone();
        let s1_seq = frames
            .iter()
            .find_map(|(sid, seq, _)| (sid == "s-1").then_some(*seq));
        let s2_seq = frames
            .iter()
            .find_map(|(sid, seq, _)| (sid == "s-2").then_some(*seq));
        assert_eq!(
            s1_seq,
            Some(43),
            "s-1 should resume at stored_max + 1 = 43, not 1"
        );
        assert_eq!(
            s2_seq,
            Some(8),
            "s-2 should resume at stored_max + 1 = 8, not 1"
        );
    }

    /// Regression: `publish_startup_error` and a subsequent drain-task
    /// publish must not collide on seq=1, otherwise the client-side
    /// dedupe (`frame.seq <= state.lastSeq → drop`) eats the agent's
    /// Regression: `shutdown` arriving while a spawn is mid-handshake
    /// must mark the in-flight spawn for cancellation, so the spawn's
    /// pre-insert check drops the freshly-built client instead of
    /// installing an orphaned worker. This test exercises the
    /// supervisor-side state machine without a real ACP handshake by
    /// pre-seeding `pending_spawns` and asserting `shutdown`'s effect.
    #[tokio::test]
    async fn shutdown_during_pending_spawn_marks_for_cancellation() {
        let sink = VecSink::new();
        let sup = Supervisor::new(sink);
        // Simulate "spawn in flight": session is in pending_spawns
        // but no WorkerHandle yet. This is the exact window where
        // the bug used to bite — shutdown returned UnknownSession
        // and the late spawn completion installed an orphan.
        sup.pending_spawns.lock().await.insert("s-cancel".into());
        assert!(sup.is_running("s-cancel").await);

        // The new shutdown contract: success (Ok(())), and the id is
        // recorded in cancelled_spawns so the spawn's pre-insert
        // check can bail.
        sup.shutdown("s-cancel")
            .await
            .expect("shutdown of pending spawn should succeed");
        assert!(
            sup.cancelled_spawns.lock().await.contains("s-cancel"),
            "shutdown must mark the pending spawn for cancellation"
        );

        // Sanity: a session that was never pending or running still
        // returns UnknownSession.
        match sup.shutdown("s-never").await {
            Err(SupervisorError::UnknownSession(id)) => assert_eq!(id, "s-never"),
            other => panic!("expected UnknownSession, got {other:?}"),
        }
    }

    /// first message after a retry.
    #[tokio::test]
    async fn startup_error_then_drain_publish_have_distinct_seqs() {
        let sink = VecSink::new();
        let sup = Supervisor::new(sink.clone());
        sup.publish_startup_error("s-1", "boom".into());
        // Simulate the drain task publishing the agent's first event
        // after a successful retry.
        let drained_seq = next_seq(&sup.next_seqs, "s-1");
        let frames = sink.frames.lock().unwrap();
        let startup_seq = frames
            .iter()
            .find_map(|(sid, seq, _)| if sid == "s-1" { Some(*seq) } else { None });
        assert_eq!(startup_seq, Some(1));
        assert_eq!(drained_seq, 2, "drain seq must follow startup-error seq");
    }

    /// `with_capacity` enforces the configured cap. Past the cap,
    /// new spawns return `CapacityFull` instead of starting another
    /// worker. The error must include `current` and `limit` so the
    /// REST surface can return a useful 503 body.
    #[tokio::test]
    async fn capacity_full_returns_after_limit() {
        let sink = VecSink::new();
        let sup = Supervisor::with_capacity(sink, 1);
        // Pre-load one fake worker so the cap is full.
        let mut workers = sup.workers.lock().await;
        let (client, _tx) = AcpClient::fake_for_test(CockpitSessionId("s-1".into()));
        let drain = tokio::spawn(async {});
        workers.insert(
            "s-1".into(),
            WorkerHandle {
                client: Arc::new(Mutex::new(client)),
                drain_task: drain,
                restart_history: vec![],
                spawn_config: None,
            },
        );
        drop(workers);

        let result = sup
            .spawn(SpawnRequest {
                session_id: "s-2".into(),
                agent: "claude-code".into(),
                cwd: std::env::temp_dir(),
                additional_dirs: vec![],
                provider_env: vec![],
                model: None,
                stored_acp_session_id: None,
            })
            .await;
        match result {
            Err(SupervisorError::CapacityFull { current, limit }) => {
                assert_eq!(current, 1);
                assert_eq!(limit, 1);
            }
            other => panic!("expected CapacityFull, got {other:?}"),
        }
    }

    /// `forget_session` drops the seq counter so the next conversation
    /// (e.g. cockpit_disable → cockpit_enable) starts fresh from seq=1.
    #[tokio::test]
    async fn forget_session_resets_seq_counter() {
        let sink = VecSink::new();
        let sup = Supervisor::new(sink);
        assert_eq!(next_seq(&sup.next_seqs, "s-1"), 1);
        assert_eq!(next_seq(&sup.next_seqs, "s-1"), 2);
        sup.forget_session("s-1");
        assert_eq!(next_seq(&sup.next_seqs, "s-1"), 1);
    }

    /// End-to-end: build a real `ChannelSink` (broadcast tx + on-disk
    /// EventStore) and verify a single `publish` call reaches both —
    /// broadcast subscribers AND the SQLite store. The on-disk path is
    /// the durable mirror that the WS-on-connect drain and the
    /// `/cockpit/replay` REST endpoint both serve from.
    #[tokio::test]
    async fn channel_sink_publishes_to_broadcast_and_disk() {
        use crate::cockpit::event_store::EventStore;
        use tempfile::TempDir;
        use tokio::sync::broadcast;

        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("cockpit.db");
        let event_store = Arc::new(EventStore::open(&db_path, 1000).unwrap());
        let (tx, mut rx) = broadcast::channel(16);
        let on_approval: ApprovalHook = Arc::new(|_, _, _| {});
        let sink = Arc::new(ChannelSink {
            tx,
            on_approval,
            event_store: event_store.clone(),
        });

        sink.publish(
            "s-42",
            1,
            &Event::UserPromptSent {
                text: "hello world".into(),
            },
        );
        sink.publish(
            "s-42",
            2,
            &Event::AgentMessageChunk {
                text: "agent reply".into(),
            },
        );

        // Broadcast subscribers see both frames in seq order.
        let frame1 = rx.try_recv().expect("broadcast frame 1");
        let frame2 = rx.try_recv().expect("broadcast frame 2");
        assert_eq!(frame1.session_id, "s-42");
        assert_eq!(frame1.seq, 1);
        assert_eq!(frame2.seq, 2);

        // On-disk store has the same two events.
        let stored = event_store.replay_from("s-42", 0);
        assert_eq!(stored.len(), 2);
        assert_eq!(stored[0].0, 1);
        assert!(matches!(
            stored[0].1,
            Event::UserPromptSent { ref text } if text == "hello world"
        ));
        assert_eq!(stored[1].0, 2);
        assert!(matches!(
            stored[1].1,
            Event::AgentMessageChunk { ref text } if text == "agent reply"
        ));
    }

    /// Restart simulation: publish through one Supervisor, drop it,
    /// reopen the EventStore at the same path, hydrate a fresh
    /// Supervisor's seqs from disk, and verify the next publish gets
    /// stored_max + 1 (not 1). This is exactly what `aoe serve`
    /// startup does after an unclean shutdown.
    #[tokio::test]
    async fn supervisor_resumes_seq_counter_from_disk_after_restart() {
        use crate::cockpit::event_store::EventStore;
        use tempfile::TempDir;
        use tokio::sync::broadcast;

        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("cockpit.db");

        // First "process": publish a few events, then drop everything.
        {
            let event_store = Arc::new(EventStore::open(&db_path, 1000).unwrap());
            let (tx, _rx) = broadcast::channel(16);
            let on_approval: ApprovalHook = Arc::new(|_, _, _| {});
            let sink = Arc::new(ChannelSink {
                tx,
                on_approval,
                event_store: event_store.clone(),
            });
            let sup = Supervisor::new(sink);
            sup.publish_user_prompt("s-99", "first".into());
            sup.publish_user_prompt("s-99", "second".into());
            sup.publish_user_prompt("s-99", "third".into());
            // sup, sink, and the in-memory replay ring drop here.
        }

        // Second "process": reopen the store at the same path,
        // hydrate the supervisor from disk, and publish.
        let event_store = Arc::new(EventStore::open(&db_path, 1000).unwrap());
        // Disk should still hold seqs 1..=3.
        assert_eq!(event_store.highest_seq("s-99"), 3);

        let (tx, mut rx) = broadcast::channel(16);
        let on_approval: ApprovalHook = Arc::new(|_, _, _| {});
        let sink = Arc::new(ChannelSink {
            tx,
            on_approval,
            event_store: event_store.clone(),
        });
        let sup = Supervisor::new(sink);
        sup.hydrate_seqs(event_store.all_session_seqs());
        sup.publish_user_prompt("s-99", "after restart".into());

        // The fresh publish must be seq=4, not seq=1. A seq=1
        // publish would be a no-op on disk (INSERT OR IGNORE) and
        // the client-side dedupe would silently drop it.
        let frame = rx.try_recv().expect("post-restart frame");
        assert_eq!(frame.seq, 4);

        // Disk now holds 1..=4, with the user prompt text preserved.
        let stored = event_store.replay_from("s-99", 0);
        let texts: Vec<String> = stored
            .iter()
            .filter_map(|(_, ev)| match ev {
                Event::UserPromptSent { text } => Some(text.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(texts, vec!["first", "second", "third", "after restart"]);
    }
}
