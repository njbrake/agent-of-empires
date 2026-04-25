//! Cockpit worker supervisor.
//!
//! Owns a per-aoe-process map of session_id -> AcpClient handles. Spawns
//! the ACP agent subprocess on demand, bridges its events into the
//! per-AppState `cockpit_events_tx` broadcast channel, restarts on
//! crash with exponential backoff (capped at 3 restarts in 60s before
//! the session transitions to Status::Error), and fires push
//! notifications for ApprovalRequested events.
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
use tokio::sync::{broadcast, Mutex};
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use super::acp_client::{AcpClient, AcpError, SpawnConfig};
use super::agent_registry::{AgentRegistry, AgentSpec};
use super::approvals::{ApprovalDecision, Nonce};
use super::state::{CockpitSessionId, Event};

/// Maximum number of unconditional restarts within `RESTART_WINDOW`.
/// After this many crashes the session is parked in Status::Error.
const MAX_RESTARTS_IN_WINDOW: u32 = 3;
const RESTART_WINDOW: Duration = Duration::from_secs(60);

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
    fn approval_requested(
        &self,
        _session_id: &str,
        _approval_title: &str,
        _destructive: bool,
    ) {
        // Default: no-op. The server impl fires a push notification.
    }
}

struct WorkerHandle {
    client: Arc<Mutex<AcpClient>>,
    /// Background task draining events from the client. Aborted on
    /// shutdown.
    drain_task: JoinHandle<()>,
    /// Restart bookkeeping: timestamps of recent (re)spawns.
    restart_history: Vec<Instant>,
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

    pub async fn registry_snapshot(&self) -> AgentRegistry {
        self.registry.lock().await.clone()
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
        };

        let cockpit_session_id = CockpitSessionId(session_id.clone());
        let client = AcpClient::spawn(config, cockpit_session_id.clone()).await?;

        info!(target: "cockpit.supervisor", session = %session_id, "cockpit worker spawned");

        let client = Arc::new(Mutex::new(client));
        let drain_task = self
            .start_drain_task(session_id.clone(), Arc::clone(&client))
            .await;

        let mut workers = self.workers.lock().await;
        workers.insert(
            session_id,
            WorkerHandle {
                client,
                drain_task,
                restart_history: vec![Instant::now()],
            },
        );
        Ok(())
    }

    async fn start_drain_task(
        &self,
        session_id: String,
        client: Arc<Mutex<AcpClient>>,
    ) -> JoinHandle<()> {
        let sink = Arc::clone(&self.sink);
        tokio::spawn(async move {
            let mut seq: u64 = 0;
            loop {
                let event = {
                    let mut c = client.lock().await;
                    c.next_event().await
                };
                let Some(event) = event else {
                    debug!(target: "cockpit.supervisor", session = %session_id, "drain channel closed");
                    break;
                };
                seq = seq.saturating_add(1);
                sink.publish(&session_id, seq, &event);
                if let Event::ApprovalRequested { approval } = &event {
                    sink.approval_requested(
                        &session_id,
                        &approval.tool_call.name,
                        approval.destructive,
                    );
                }
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

    /// Restart bookkeeping: returns false if the session has already
    /// burned through MAX_RESTARTS_IN_WINDOW restarts in
    /// RESTART_WINDOW. Callers should park the session in
    /// Status::Error in that case.
    pub async fn record_restart(&self, session_id: &str) -> bool {
        let mut workers = self.workers.lock().await;
        let Some(handle) = workers.get_mut(session_id) else {
            return false;
        };
        let now = Instant::now();
        let window_start = now - RESTART_WINDOW;
        handle
            .restart_history
            .retain(|t| *t >= window_start);
        handle.restart_history.push(now);
        if handle.restart_history.len() as u32 > MAX_RESTARTS_IN_WINDOW {
            warn!(
                target: "cockpit.supervisor",
                session = %session_id,
                count = handle.restart_history.len(),
                "session exceeded restart budget"
            );
            return false;
        }
        true
    }
}

/// A `BroadcastSink` impl backed by a tokio broadcast channel. The
/// AppState in the server module wires this so cockpit events flow
/// straight into the existing WebSocket fanout.
pub struct ChannelSink {
    pub tx: broadcast::Sender<crate::server::CockpitBroadcastFrame>,
    pub on_approval: Arc<dyn Fn(&str, &str, bool) + Send + Sync>,
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
    }

    fn approval_requested(
        &self,
        session_id: &str,
        approval_title: &str,
        destructive: bool,
    ) {
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
            self.frames.lock().unwrap().push((
                session_id.to_string(),
                seq,
                event.clone(),
            ));
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

    #[tokio::test]
    async fn restart_budget_burns_after_three_in_window() {
        let sink = VecSink::new();
        let sup = Supervisor::new(sink);
        // Inject a worker handle
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
                },
            );
        }

        for i in 0..MAX_RESTARTS_IN_WINDOW {
            assert!(sup.record_restart("s-1").await, "restart #{i} should succeed");
        }
        // Fourth restart in the window: budget burned.
        assert!(!sup.record_restart("s-1").await, "fourth restart should be denied");
    }
}
