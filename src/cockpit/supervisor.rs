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

use std::collections::HashMap;
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
use super::replay_buffer::ReplayBuffer;
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
    /// Cap on concurrently-running workers, snapshotted from
    /// `[cockpit] max_concurrent_workers` at startup. Enforced in
    /// `spawn`; new workers past the cap return `CapacityFull`.
    /// Tests use `Supervisor::new` (effectively unbounded); production
    /// uses `Supervisor::with_capacity`.
    max_concurrent_workers: u32,
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

    /// Drop per-session bookkeeping (replay seq counter). Called when
    /// the session is deleted or its substrate is switched away from
    /// cockpit, so the next cockpit_enable starts a fresh conversation
    /// from seq=1 with a clean replay buffer.
    pub fn forget_session(&self, session_id: &str) {
        if let Ok(mut guard) = self.next_seqs.lock() {
            guard.remove(session_id);
        }
    }

    pub async fn upsert_agent(&self, name: String, spec: AgentSpec) {
        self.registry.lock().await.upsert(name, spec);
    }

    /// Spawn a cockpit worker for the given session. Returns Err if a
    /// worker is already running for that session, or if the
    /// `max_concurrent_workers` cap is full.
    ///
    /// Concurrency note: the AlreadyRunning + CapacityFull check
    /// releases the workers lock before `AcpClient::spawn`, then
    /// re-acquires it for the insert. Two concurrent `spawn(same_id)`
    /// calls could in principle both pass the check and race to
    /// insert. Today's callers don't trigger this — the auto-spawn
    /// reconciler dedupes via its `attempted` set, REST handlers
    /// gate on `is_running()`, and CLI add is single-process — so we
    /// rely on the contract rather than holding the lock across the
    /// (possibly seconds-long) `AcpClient::spawn().await`.
    pub async fn spawn(
        &self,
        session_id: String,
        agent: &str,
        cwd: PathBuf,
        additional_dirs: Vec<PathBuf>,
        provider_env: Vec<(String, String)>,
        model: Option<String>,
    ) -> Result<(), SupervisorError> {
        {
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
        }

        let mut spec = self.resolve_agent(agent).await?;
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
        };

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
                debug!(
                    target: "cockpit.supervisor",
                    session = %session_id,
                    "drain channel closed; checking restart budget"
                );
                let respawn_config = match restart_decision(&workers, &session_id).await {
                    RestartDecision::Respawn(cfg) => cfg,
                    RestartDecision::BudgetBurned => {
                        warn!(
                            target: "cockpit.supervisor",
                            session = %session_id,
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

    /// Send a user prompt to a running cockpit worker.
    pub async fn send_prompt(&self, session_id: &str, text: &str) -> Result<(), SupervisorError> {
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
        let mut workers = self.workers.lock().await;
        let handle = workers
            .remove(session_id)
            .ok_or_else(|| SupervisorError::UnknownSession(session_id.into()))?;
        {
            let client = handle.client.lock().await;
            let _ = client.shutdown().await;
        }
        handle.drain_task.abort();
        Ok(())
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

    /// Whether this session has a running cockpit worker.
    pub async fn is_running(&self, session_id: &str) -> bool {
        self.workers.lock().await.contains_key(session_id)
    }

    /// Return the number of running workers (for the doctor + stats).
    pub async fn count(&self) -> usize {
        self.workers.lock().await.len()
    }
}

enum RestartDecision {
    Respawn(SpawnConfig),
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
        return RestartDecision::Gone;
    };
    let now = Instant::now();
    let window_start = now - RESTART_WINDOW;
    handle.restart_history.retain(|t| *t >= window_start);
    handle.restart_history.push(now);
    if handle.restart_history.len() as u32 > MAX_RESPAWNS_IN_WINDOW {
        return RestartDecision::BudgetBurned;
    }
    match handle.spawn_config.clone() {
        Some(cfg) => RestartDecision::Respawn(cfg),
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
    /// Per-session replay buffer. Frames are appended on each publish
    /// so a reconnecting client can resync via
    /// `GET /api/sessions/{id}/cockpit/replay?since={seq}`.
    pub replay: Arc<std::sync::Mutex<HashMap<String, ReplayBuffer>>>,
    /// Per-session caps `(max_events, max_bytes)`. Pulled from
    /// `[cockpit]` config at startup; applied on lazy buffer init.
    pub replay_caps: (usize, usize),
}

impl BroadcastSink for ChannelSink {
    fn publish(&self, session_id: &str, seq: u64, event: &Event) {
        let payload = serde_json::to_value(event).unwrap_or(serde_json::Value::Null);
        let frame = crate::server::CockpitBroadcastFrame {
            session_id: session_id.to_string(),
            seq,
            event: payload,
        };
        let _ = self.tx.send(frame);

        if let Ok(mut guard) = self.replay.lock() {
            let (max_events, max_bytes) = self.replay_caps;
            let buf = guard
                .entry(session_id.to_string())
                .or_insert_with(|| ReplayBuffer::new(max_events, max_bytes));
            buf.push(seq, event.clone());
        }
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
            .spawn(
                "s-1".into(),
                "no-such-agent",
                std::env::temp_dir(),
                vec![],
                vec![],
                None,
            )
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
            .spawn(
                "s-1".into(),
                "claude-code",
                std::env::temp_dir(),
                vec![],
                vec![],
                None,
            )
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

    /// Regression: `publish_startup_error` and a subsequent drain-task
    /// publish must not collide on seq=1, otherwise the client-side
    /// dedupe (`frame.seq <= state.lastSeq → drop`) eats the agent's
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
            .spawn(
                "s-2".into(),
                "claude-code",
                std::env::temp_dir(),
                vec![],
                vec![],
                None,
            )
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
}
