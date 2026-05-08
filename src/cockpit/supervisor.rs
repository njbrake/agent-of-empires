//! Cockpit worker supervisor.
//!
//! Owns a per-aoe-process map of session_id -> AcpClient handles. Spawns
//! the ACP agent subprocess on demand, bridges its events into the
//! per-AppState `cockpit_events_tx` broadcast channel, and fires push
//! notifications for ApprovalRequested events.
//!
//! Watchdog: when an agent's ACP connection task ends (subprocess exit,
//! transport break) the drain task respawns it. Up to
//! `MAX_RESTARTS_IN_WINDOW` respawns are allowed inside `RESTART_WINDOW`;
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

/// Maximum number of unconditional restarts within `RESTART_WINDOW`.
/// After this many crashes the session is parked and an
/// `AgentStartupError` event is published.
const MAX_RESTARTS_IN_WINDOW: u32 = 3;
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
    /// Restart bookkeeping: timestamps of recent (re)spawns. Used by
    /// the watchdog to enforce `MAX_RESTARTS_IN_WINDOW`.
    restart_history: Vec<Instant>,
    /// Stored so the watchdog can respawn the worker when its ACP
    /// connection task exits (subprocess crash, transport break).
    /// Populated for real workers; left as `None` for fake workers
    /// inserted by tests.
    spawn_config: Option<SpawnConfig>,
    /// Last `seq` published by the drain task. Persisted across
    /// respawns so the broadcast stream stays monotonically
    /// increasing even after the agent process is replaced.
    last_seq: u64,
}

pub struct Supervisor<S: BroadcastSink> {
    sink: Arc<S>,
    registry: Arc<Mutex<AgentRegistry>>,
    workers: Arc<Mutex<HashMap<String, WorkerHandle>>>,
}

impl<S: BroadcastSink> Supervisor<S> {
    pub fn new(sink: Arc<S>) -> Self {
        Self {
            sink,
            registry: Arc::new(Mutex::new(AgentRegistry::with_defaults())),
            workers: Arc::new(Mutex::new(HashMap::new())),
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
        self.sink
            .publish(session_id, 1, &Event::AgentStartupError { message });
    }

    pub async fn upsert_agent(&self, name: String, spec: AgentSpec) {
        self.registry.lock().await.upsert(name, spec);
    }

    /// Spawn a cockpit worker for the given session. Returns Err if a
    /// worker is already running for that session.
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
                restart_history: vec![Instant::now()],
                spawn_config: Some(config),
                last_seq: 0,
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
        tokio::spawn(async move {
            let mut inbound = initial_inbound;
            loop {
                while let Some(event) = inbound.recv().await {
                    let seq = bump_seq(&workers, &session_id).await;
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
                        let seq = bump_seq(&workers, &session_id).await;
                        sink.publish(
                            &session_id,
                            seq,
                            &Event::AgentStartupError {
                                message: format!(
                                    "ACP agent crashed more than {} times in {}s; \
                                     not respawning. Use the web dashboard to retry.",
                                    MAX_RESTARTS_IN_WINDOW,
                                    RESTART_WINDOW.as_secs()
                                ),
                            },
                        );
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
                            let seq = bump_seq(&workers, &session_id).await;
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
                let new_inbound = new_client
                    .take_inbound()
                    .expect("freshly spawned AcpClient always has inbound receiver");

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
    if handle.restart_history.len() as u32 > MAX_RESTARTS_IN_WINDOW {
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

async fn bump_seq(workers: &Arc<Mutex<HashMap<String, WorkerHandle>>>, session_id: &str) -> u64 {
    let mut guard = workers.lock().await;
    let Some(handle) = guard.get_mut(session_id) else {
        return 0;
    };
    handle.last_seq = handle.last_seq.saturating_add(1);
    handle.last_seq
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
                last_seq: 0,
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

    /// Watchdog: after MAX_RESTARTS_IN_WINDOW respawn attempts inside
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
                    last_seq: 0,
                },
            );
        }

        for i in 0..MAX_RESTARTS_IN_WINDOW {
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

    /// `bump_seq` returns 0 for an unknown session (used when the
    /// drain task races a shutdown) and monotonically increments
    /// otherwise.
    #[tokio::test]
    async fn bump_seq_monotonic() {
        let sink = VecSink::new();
        let sup = Supervisor::new(sink);
        assert_eq!(bump_seq(&sup.workers, "missing").await, 0);
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
                    last_seq: 0,
                },
            );
        }
        assert_eq!(bump_seq(&sup.workers, "s-1").await, 1);
        assert_eq!(bump_seq(&sup.workers, "s-1").await, 2);
        assert_eq!(bump_seq(&sup.workers, "s-1").await, 3);
    }
}
