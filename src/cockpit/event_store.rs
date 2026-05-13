//! Disk-backed event log for cockpit sessions.
//!
//! Every event published through `ChannelSink` is appended here so the
//! conversation transcript survives page reloads, session switches,
//! and `aoe serve` restarts. One row per `(session_id, seq)` with a
//! per-session retention cap; older events are pruned on insert once
//! the row count exceeds the cap.
//!
//! ## How replay flows
//!
//! - **WebSocket on-connect drain.** The client passes the `lastSeq` it
//!   has cached (or 0 on first connect) as a query param to
//!   `/sessions/{id}/cockpit/ws`. The handler reads
//!   `replay_from(session_id, since)` out of this store and pushes
//!   those frames before forwarding the live broadcast, closing the
//!   subscribe-gap race that would otherwise drop the agent's first
//!   chunks on a fast page load.
//! - **Snapshot endpoint.** `GET /cockpit/replay?since=N` reads the
//!   same data path, used by the React reducer when it sees a `lagged`
//!   notice from the WS to catch up missed frames.
//! - **Startup hydration.** On boot, `next_seqs` is rehydrated from
//!   `MAX(seq) + 1` per session so post-restart writes don't collide
//!   with pre-restart rows via `INSERT OR IGNORE`.
//!
//! ## How it relates to agent-side memory
//!
//! This store only persists the *UI transcript*. The model's
//! conversation context across `aoe serve` restarts is a separate
//! mechanism in `supervisor.rs`: when the agent advertises
//! `agent_capabilities.load_session = true` on the ACP `initialize`
//! response, the supervisor stores the agent-assigned `session_id` on
//! `Instance.cockpit_acp_session_id` and uses `session/load` on
//! subsequent spawns instead of `session/new`. If `session/load`
//! fails, the stored id is cleared and a `SessionContextReset` event
//! is published; the UI renders an amber callout in the transcript so
//! the user knows prior turns are no longer in the model's context.
//!
//! The bundled `aoe-agent` does not yet advertise `load_session`, so
//! its UI transcript replays from this store on restart but the model
//! itself starts fresh each spawn (tracked in #1005).
//!
//! ## Lifecycle
//!
//! Per-session rows are dropped on session delete and on
//! `cockpit_disable` (the master switch turning off, or a per-session
//! opt-out). The connection has WAL mode enabled so the publish path
//! and the replay endpoint don't block each other under load.

use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use tracing::{debug, trace, warn};

use super::state::{Event, Plan};

/// SQLite-backed cockpit event log. One row per (session_id, seq).
pub struct EventStore {
    conn: Mutex<Connection>,
    /// Per-session retention cap. Older events are pruned on insert
    /// once the count exceeds this value. Bytes are not enforced here
    /// (the in-memory ring still has a byte cap); the row count keeps
    /// the on-disk size bounded.
    max_events_per_session: usize,
}

impl EventStore {
    /// Open or create the database at `db_path`. Creates the
    /// `cockpit_events` table if missing. The connection has WAL mode
    /// enabled so concurrent writers (publish path) and readers
    /// (replay endpoint) don't block each other.
    pub fn open(db_path: &Path, max_events_per_session: usize) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            if !parent.exists() {
                std::fs::create_dir_all(parent).with_context(|| {
                    format!("create parent dir for cockpit DB at {}", parent.display())
                })?;
            }
        }
        let conn = Connection::open(db_path)
            .with_context(|| format!("open cockpit DB at {}", db_path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .context("enable WAL mode")?;
        conn.pragma_update(None, "synchronous", "NORMAL")
            .context("set synchronous=NORMAL")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS cockpit_events (
                session_id  TEXT    NOT NULL,
                seq         INTEGER NOT NULL,
                event_json  TEXT    NOT NULL,
                created_at  INTEGER NOT NULL,
                PRIMARY KEY (session_id, seq)
            );
            CREATE INDEX IF NOT EXISTS idx_cockpit_events_session_seq
                ON cockpit_events(session_id, seq);",
        )
        .context("create cockpit_events schema")?;
        debug!(
            target: "cockpit.event_store",
            path = %db_path.display(),
            cap = max_events_per_session,
            "cockpit event store opened"
        );
        Ok(Self {
            conn: Mutex::new(conn),
            max_events_per_session,
        })
    }

    /// Append one event. Idempotent on duplicate (session_id, seq) thanks
    /// to the primary key; re-publishing the same seq is a no-op.
    /// Returns Err when the event was *not* persisted, so the caller can
    /// surface the gap (e.g. publish a `Lagged` frame on the broadcast
    /// channel) instead of letting the on-disk log silently fall behind
    /// the in-memory broadcast subscribers.
    pub fn record(&self, session_id: &str, seq: u64, event: &Event) -> Result<()> {
        let json = serde_json::to_string(event)
            .with_context(|| format!("serialise event for {session_id}@{seq}"))?;
        let bytes = json.len();
        let kind = event_kind(event);
        let now_ms = chrono::Utc::now().timestamp_millis();
        let conn = match self.conn.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let inserted = conn
            .execute(
                "INSERT OR IGNORE INTO cockpit_events (session_id, seq, event_json, created_at)
             VALUES (?1, ?2, ?3, ?4)",
                params![session_id, seq as i64, json, now_ms],
            )
            .with_context(|| format!("insert {session_id}@{seq}"))?;
        if inserted == 0 {
            // Primary-key collision: same (session_id, seq) seen before.
            // Logged at trace because the cause is usually a benign retry
            // (publish_user_prompt + replay drain re-publishing) rather
            // than a bug, but we still want a breadcrumb. Per-event lines
            // are too noisy to live at debug; they bury the lifecycle
            // signal in debug.log during an active turn.
            trace!(
                target: "cockpit.event_store",
                session = %session_id,
                seq,
                kind,
                "skipped duplicate event (already on disk)"
            );
        } else {
            trace!(
                target: "cockpit.event_store",
                session = %session_id,
                seq,
                kind,
                bytes,
                "recorded event"
            );
        }
        // Prune oldest beyond the retention cap. Cheap when below the cap
        // (the subquery returns 0 rows). We do it on every insert rather
        // than periodically so the upper bound on per-session disk usage
        // is strict rather than amortised.
        //
        // Snapshot events (the slash-command list, mode list, ACP session
        // id) are exempt from pruning: the agent only emits them once per
        // session lifecycle, near the start of the seq range, so a long
        // session blows past the cap and evicts them; leaving the
        // composer's `/` palette and the mode picker empty on reconnect.
        // See #1049. The `event_json NOT LIKE` clauses match the
        // externally-tagged JSON discriminant for each pinned variant.
        if self.max_events_per_session > 0 {
            match conn.execute(
                "DELETE FROM cockpit_events
                 WHERE session_id = ?1
                   AND seq <= (
                     SELECT seq FROM cockpit_events
                     WHERE session_id = ?1
                     ORDER BY seq DESC
                     LIMIT 1 OFFSET ?2
                   )
                   AND event_json NOT LIKE '{\"AvailableCommandsUpdated\":%'
                   AND event_json NOT LIKE '{\"ModesAvailable\":%'
                   AND event_json NOT LIKE '{\"CurrentModeChanged\":%'
                   AND event_json NOT LIKE '{\"AcpSessionAssigned\":%'",
                params![session_id, self.max_events_per_session as i64],
            ) {
                Ok(0) => {}
                Ok(pruned) => {
                    trace!(
                        target: "cockpit.event_store",
                        session = %session_id,
                        pruned,
                        cap = self.max_events_per_session,
                        "pruned oldest events past retention cap"
                    );
                }
                Err(e) => {
                    // Prune failure isn't fatal; the row is recorded,
                    // we just exceed the cap until the next prune
                    // succeeds. Log + swallow so callers don't have to
                    // distinguish "record failed" from "trim failed".
                    warn!(target: "cockpit.event_store", "prune {session_id}: {e}");
                }
            }
        }
        Ok(())
    }

    /// Return all events for `session_id` with `seq < before`, oldest
    /// first. Used by the context-primer endpoint to fetch only the
    /// transcript that precedes a `SessionContextReset` event without
    /// having to over-fetch and filter client-side. See #1004.
    pub fn replay_before(&self, session_id: &str, before: u64) -> Vec<(u64, Event)> {
        let conn = match self.conn.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let mut stmt = match conn.prepare(
            "SELECT seq, event_json FROM cockpit_events
             WHERE session_id = ?1 AND seq < ?2
             ORDER BY seq ASC",
        ) {
            Ok(s) => s,
            Err(e) => {
                warn!(target: "cockpit.event_store", "prepare replay_before for {session_id}: {e}");
                return Vec::new();
            }
        };
        let rows = match stmt.query_map(params![session_id, before as i64], |row| {
            let seq: i64 = row.get(0)?;
            let json: String = row.get(1)?;
            Ok((seq as u64, json))
        }) {
            Ok(r) => r,
            Err(e) => {
                warn!(target: "cockpit.event_store", "query replay_before for {session_id}: {e}");
                return Vec::new();
            }
        };
        let mut out = Vec::new();
        for row in rows {
            match row {
                Ok((seq, json)) => match serde_json::from_str::<Event>(&json) {
                    Ok(event) => out.push((seq, event)),
                    Err(e) => warn!(
                        target: "cockpit.event_store",
                        "deserialise event {session_id}@{seq}: {e}"
                    ),
                },
                Err(e) => warn!(target: "cockpit.event_store", "row error: {e}"),
            }
        }
        out
    }

    /// Return all events for `session_id` with `seq > since`, oldest
    /// first. An empty vec means the session has no newer events.
    pub fn replay_from(&self, session_id: &str, since: u64) -> Vec<(u64, Event)> {
        let conn = match self.conn.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let mut stmt = match conn.prepare(
            "SELECT seq, event_json FROM cockpit_events
             WHERE session_id = ?1 AND seq > ?2
             ORDER BY seq ASC",
        ) {
            Ok(s) => s,
            Err(e) => {
                warn!(target: "cockpit.event_store", "prepare replay for {session_id}: {e}");
                return Vec::new();
            }
        };
        let rows = match stmt.query_map(params![session_id, since as i64], |row| {
            let seq: i64 = row.get(0)?;
            let json: String = row.get(1)?;
            Ok((seq as u64, json))
        }) {
            Ok(r) => r,
            Err(e) => {
                warn!(target: "cockpit.event_store", "query replay for {session_id}: {e}");
                return Vec::new();
            }
        };
        let mut out = Vec::new();
        for row in rows {
            match row {
                Ok((seq, json)) => match serde_json::from_str::<Event>(&json) {
                    Ok(event) => out.push((seq, event)),
                    Err(e) => warn!(
                        target: "cockpit.event_store",
                        "deserialise event {session_id}@{seq}: {e}"
                    ),
                },
                Err(e) => warn!(target: "cockpit.event_store", "row error: {e}"),
            }
        }
        trace!(
            target: "cockpit.event_store",
            session = %session_id,
            since,
            returned = out.len(),
            "replayed events"
        );
        out
    }

    /// Return the latest `Event::PlanUpdated` stored for `session_id`,
    /// if any. Used by the REST sessions endpoint to surface
    /// plan-progress chrome (current step / completed / total) on the
    /// sidebar without subscribing to the cockpit WS for every session.
    /// See #1061.
    pub fn latest_plan(&self, session_id: &str) -> Option<Plan> {
        let conn = match self.conn.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let json: String = conn
            .query_row(
                "SELECT event_json FROM cockpit_events
                 WHERE session_id = ?1
                   AND event_json LIKE '{\"PlanUpdated\":%'
                 ORDER BY seq DESC LIMIT 1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten()?;
        let event: Event = serde_json::from_str(&json).ok()?;
        if let Event::PlanUpdated { plan } = event {
            Some(plan)
        } else {
            None
        }
    }

    /// Return the most recent unfired `WakeupScheduled` for `session_id`.
    /// A wakeup counts as pending when its seq is greater than the seq
    /// of the latest `UserPromptSent` (the /loop skill's self-firing
    /// emits a fresh prompt when the wake triggers, so a wakeup whose
    /// seq is ≤ the latest prompt has already fired). Returns the
    /// stored `at` timestamp and optional `reason`. See #1091.
    pub fn latest_pending_wakeup(
        &self,
        session_id: &str,
    ) -> Option<(DateTime<Utc>, Option<String>)> {
        let conn = match self.conn.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let json: String = conn
            .query_row(
                "SELECT event_json FROM cockpit_events
                 WHERE session_id = ?1
                   AND event_json LIKE '{\"WakeupScheduled\":%'
                   AND seq > COALESCE(
                     (SELECT MAX(seq) FROM cockpit_events
                      WHERE session_id = ?1
                        AND event_json LIKE '{\"UserPromptSent\":%'),
                     0
                   )
                 ORDER BY seq DESC LIMIT 1",
                params![session_id],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten()?;
        let event: Event = serde_json::from_str(&json).ok()?;
        if let Event::WakeupScheduled { at, reason } = event {
            Some((at, reason))
        } else {
            None
        }
    }

    /// Given the seq of a just-published `UserPromptSent`, return the
    /// `WakeupScheduled` whose timer this prompt is satisfying — i.e.
    /// the latest `WakeupScheduled` whose seq sits strictly between
    /// the previous prompt's seq and `prompt_seq`. Returns `None` when
    /// no wakeup is in that window, which is the common case (regular
    /// user-typed prompt, no scheduled wake involved). Used by the
    /// cockpit event listener to fire a one-shot push on wake fire
    /// without scanning the whole event log. See #1091.
    pub fn fired_wakeup_for_prompt(
        &self,
        session_id: &str,
        prompt_seq: u64,
    ) -> Option<(DateTime<Utc>, Option<String>)> {
        let conn = match self.conn.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let prompt_seq_i64 = prompt_seq as i64;
        let json: String = conn
            .query_row(
                "SELECT event_json FROM cockpit_events
                 WHERE session_id = ?1
                   AND seq < ?2
                   AND event_json LIKE '{\"WakeupScheduled\":%'
                   AND seq > COALESCE(
                     (SELECT MAX(seq) FROM cockpit_events
                      WHERE session_id = ?1
                        AND seq < ?2
                        AND event_json LIKE '{\"UserPromptSent\":%'),
                     0
                   )
                 ORDER BY seq DESC LIMIT 1",
                params![session_id, prompt_seq_i64],
                |row| row.get(0),
            )
            .optional()
            .ok()
            .flatten()?;
        let event: Event = serde_json::from_str(&json).ok()?;
        if let Event::WakeupScheduled { at, reason } = event {
            Some((at, reason))
        } else {
            None
        }
    }

    /// Return the highest seq stored for `session_id`, or 0 if none.
    /// Used at startup to re-seed the in-memory `next_seqs` counter so
    /// fresh publishes don't collide with restored history.
    pub fn highest_seq(&self, session_id: &str) -> u64 {
        let conn = match self.conn.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let max = match conn
            .query_row(
                "SELECT MAX(seq) FROM cockpit_events WHERE session_id = ?1",
                params![session_id],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()
        {
            Ok(Some(Some(max))) => max as u64,
            _ => 0,
        };
        trace!(
            target: "cockpit.event_store",
            session = %session_id,
            highest_seq = max,
            "highest_seq query"
        );
        max
    }

    /// Return every session_id that has at least one event stored, with
    /// its highest seq. Used at startup to pre-seed `next_seqs` in one
    /// query rather than racing per-session lookups.
    pub fn all_session_seqs(&self) -> Vec<(String, u64)> {
        let conn = match self.conn.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let mut stmt = match conn
            .prepare("SELECT session_id, MAX(seq) FROM cockpit_events GROUP BY session_id")
        {
            Ok(s) => s,
            Err(e) => {
                warn!(target: "cockpit.event_store", "prepare all_session_seqs: {e}");
                return Vec::new();
            }
        };
        let rows = match stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            let max: i64 = row.get(1)?;
            Ok((id, max as u64))
        }) {
            Ok(r) => r,
            Err(e) => {
                warn!(target: "cockpit.event_store", "query all_session_seqs: {e}");
                return Vec::new();
            }
        };
        let collected: Vec<(String, u64)> = rows.filter_map(|r| r.ok()).collect();
        debug!(
            target: "cockpit.event_store",
            sessions = collected.len(),
            "all_session_seqs hydration"
        );
        collected
    }

    /// True iff the session has a `UserPromptSent` whose turn never
    /// terminated (no later `Stopped` or `AgentStartupError`). Used at
    /// daemon startup to decide whether to synthesize a `Stopped` event
    /// for a session that was mid-turn when the previous `aoe serve`
    /// died, and on reattach to arm the resume-idle watchdog.
    ///
    /// `Stopped` and `AgentStartupError` are serialized externally-tagged
    /// (`{"Stopped":{"reason":"..."}}`) so we match on the variant key
    /// via `json_extract($.Stopped)`.
    pub fn has_in_flight_turn(&self, session_id: &str) -> bool {
        let conn = match self.conn.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let prompt_seq: Option<i64> = match conn
            .query_row(
                "SELECT MAX(seq) FROM cockpit_events
                 WHERE session_id = ?1
                   AND json_extract(event_json, '$.UserPromptSent') IS NOT NULL",
                params![session_id],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()
        {
            Ok(Some(v)) => v,
            Ok(None) => None,
            Err(e) => {
                warn!(target: "cockpit.event_store", "has_in_flight_turn prompt query {session_id}: {e}");
                return false;
            }
        };
        let Some(prompt_seq) = prompt_seq else {
            return false;
        };
        let terminator: Option<i64> = match conn
            .query_row(
                "SELECT MIN(seq) FROM cockpit_events
                 WHERE session_id = ?1
                   AND seq > ?2
                   AND (json_extract(event_json, '$.Stopped') IS NOT NULL
                     OR json_extract(event_json, '$.AgentStartupError') IS NOT NULL)",
                params![session_id, prompt_seq],
                |row| row.get::<_, Option<i64>>(0),
            )
            .optional()
        {
            Ok(Some(v)) => v,
            Ok(None) => None,
            Err(e) => {
                warn!(target: "cockpit.event_store", "has_in_flight_turn terminator query {session_id}: {e}");
                return false;
            }
        };
        terminator.is_none()
    }

    /// Drop every event for a session. Called when the session is
    /// deleted or its substrate is switched away from cockpit, so the
    /// next cockpit_enable starts fresh from seq=1.
    pub fn delete_session(&self, session_id: &str) {
        let conn = match self.conn.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        match conn.execute(
            "DELETE FROM cockpit_events WHERE session_id = ?1",
            params![session_id],
        ) {
            Ok(deleted) => {
                debug!(
                    target: "cockpit.event_store",
                    session = %session_id,
                    deleted,
                    "deleted session events"
                );
            }
            Err(e) => {
                warn!(target: "cockpit.event_store", "delete {session_id}: {e}");
            }
        }
    }
}

/// Cheap discriminant string for `Event` so debug logs don't dump the
/// full payload (assistant chunks can be a few KB each). Unknown
/// variants fall back to "other"; `event_kind` only exists for log
/// breadcrumbs and doesn't need to stay in lockstep with the enum.
fn event_kind(event: &Event) -> &'static str {
    match event {
        Event::PlanUpdated { .. } => "plan_updated",
        Event::TodoListUpdated { .. } => "todo_list_updated",
        Event::ToolCallStarted { .. } => "tool_call_started",
        Event::ToolCallCompleted { .. } => "tool_call_completed",
        Event::ToolCallContent { .. } => "tool_call_content",
        Event::ToolCallUpdated { .. } => "tool_call_updated",
        Event::ApprovalRequested { .. } => "approval_requested",
        Event::ApprovalResolved { .. } => "approval_resolved",
        Event::DiffEmitted { .. } => "diff_emitted",
        Event::ThinkingStarted => "thinking_started",
        Event::ThinkingEnded => "thinking_ended",
        Event::RateLimit { .. } => "rate_limit",
        Event::UsageUpdated { .. } => "usage_updated",
        Event::ModeChanged { .. } => "mode_changed",
        Event::ModesAvailable { .. } => "modes_available",
        Event::CurrentModeChanged { .. } => "current_mode_changed",
        Event::AvailableCommandsUpdated { .. } => "available_commands_updated",
        Event::RawAgentUpdate { .. } => "raw_agent_update",
        Event::AgentMessageChunk { .. } => "agent_message_chunk",
        Event::Stopped { .. } => "stopped",
        Event::AgentStartupError { .. } => "agent_startup_error",
        Event::UserPromptSent { .. } => "user_prompt_sent",
        Event::AcpSessionAssigned { .. } => "acp_session_assigned",
        Event::SessionContextReset { .. } => "session_context_reset",
        Event::WakeupScheduled { .. } => "wakeup_scheduled",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn open_store(max: usize) -> (TempDir, EventStore) {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("cockpit.db");
        let store = EventStore::open(&path, max).unwrap();
        (tmp, store)
    }

    #[test]
    fn record_and_replay_roundtrip() {
        let (_tmp, store) = open_store(1000);
        for i in 1..=5 {
            store.record("s-1", i, &Event::ThinkingStarted).unwrap();
        }
        let replay = store.replay_from("s-1", 2);
        let seqs: Vec<u64> = replay.iter().map(|(s, _)| *s).collect();
        assert_eq!(seqs, vec![3, 4, 5]);
    }

    #[test]
    fn highest_seq_reflects_inserts() {
        let (_tmp, store) = open_store(1000);
        assert_eq!(store.highest_seq("s-1"), 0);
        store.record("s-1", 1, &Event::ThinkingStarted).unwrap();
        store.record("s-1", 2, &Event::ThinkingEnded).unwrap();
        assert_eq!(store.highest_seq("s-1"), 2);
    }

    #[test]
    fn duplicate_seq_is_idempotent() {
        let (_tmp, store) = open_store(1000);
        store
            .record("s-1", 1, &Event::UserPromptSent { text: "hi".into() })
            .unwrap();
        // Second insert at the same seq must not double-count.
        store.record("s-1", 1, &Event::ThinkingStarted).unwrap();
        let replay = store.replay_from("s-1", 0);
        assert_eq!(replay.len(), 1);
        // The first write wins (INSERT OR IGNORE).
        if let Event::UserPromptSent { text } = &replay[0].1 {
            assert_eq!(text, "hi");
        } else {
            panic!("expected UserPromptSent");
        }
    }

    #[test]
    fn latest_plan_returns_most_recent_plan_event() {
        use super::super::state::{Plan, PlanStep, PlanStepStatus};
        let (_tmp, store) = open_store(1000);
        let plan_v1 = Plan {
            plan_id: "p-1".into(),
            version: 1,
            steps: vec![PlanStep {
                id: "s-1".into(),
                title: "Step one".into(),
                detail: None,
                status: PlanStepStatus::Pending,
            }],
        };
        let plan_v2 = Plan {
            plan_id: "p-2".into(),
            version: 2,
            steps: vec![
                PlanStep {
                    id: "s-1".into(),
                    title: "Step one".into(),
                    detail: None,
                    status: PlanStepStatus::Done,
                },
                PlanStep {
                    id: "s-2".into(),
                    title: "Step two".into(),
                    detail: None,
                    status: PlanStepStatus::Pending,
                },
            ],
        };
        store
            .record("s-1", 1, &Event::PlanUpdated { plan: plan_v1 })
            .unwrap();
        store.record("s-1", 2, &Event::ThinkingStarted).unwrap();
        store
            .record("s-1", 3, &Event::PlanUpdated { plan: plan_v2 })
            .unwrap();
        let latest = store.latest_plan("s-1").expect("plan present");
        assert_eq!(latest.steps.len(), 2);
        assert!(matches!(
            latest.steps[0].status,
            crate::cockpit::state::PlanStepStatus::Done
        ));
    }

    #[test]
    fn latest_plan_returns_none_when_no_plan_event() {
        let (_tmp, store) = open_store(1000);
        store.record("s-1", 1, &Event::ThinkingStarted).unwrap();
        assert!(store.latest_plan("s-1").is_none());
    }

    #[test]
    fn snapshot_events_survive_retention_prune() {
        // Mirrors #1049: a long session blew past max_events_per_session
        // and evicted the early `AvailableCommandsUpdated` row, leaving
        // the `/` palette empty on reconnect. Snapshot kinds are pinned
        // so they outlive the prune even when the rest of the seq tail
        // gets dropped.
        let (_tmp, store) = open_store(3);
        store
            .record(
                "s-1",
                1,
                &Event::AvailableCommandsUpdated { commands: vec![] },
            )
            .unwrap();
        store
            .record(
                "s-1",
                2,
                &Event::ModesAvailable {
                    current_mode_id: "default".into(),
                    modes: vec![],
                },
            )
            .unwrap();
        store
            .record(
                "s-1",
                3,
                &Event::AcpSessionAssigned {
                    acp_session_id: "acp-xyz".into(),
                },
            )
            .unwrap();
        // Push enough transcript events to blow past the cap several
        // times. With the old prune, seqs 1-3 would all be evicted.
        for i in 4..=20 {
            store.record("s-1", i, &Event::ThinkingStarted).unwrap();
        }
        let replay = store.replay_from("s-1", 0);
        let seqs: Vec<u64> = replay.iter().map(|(s, _)| *s).collect();
        // The three snapshot rows survive. The most recent 3 transcript
        // events also remain.
        assert!(
            seqs.contains(&1),
            "AvailableCommandsUpdated dropped: {seqs:?}"
        );
        assert!(seqs.contains(&2), "ModesAvailable dropped: {seqs:?}");
        assert!(seqs.contains(&3), "AcpSessionAssigned dropped: {seqs:?}");
        assert!(seqs.contains(&20), "newest event dropped: {seqs:?}");
        // Older transcript-only events (4 through 17) are pruned.
        assert!(
            !seqs.contains(&5),
            "stale transcript event leaked: {seqs:?}"
        );
    }

    #[test]
    fn snapshot_event_json_discriminators_match_prune_clauses() {
        // The retention prune query in `Self::record` excludes four event
        // variants via `WHERE event_json NOT LIKE '{"<Variant>":%'`. If the
        // `Event` enum is ever refactored to a different serde shape
        // (`#[serde(tag = "...")]`, a rename, or another adjacency), the
        // LIKE strings silently stop matching and snapshot pinning quietly
        // breaks. Pin the discriminator at the JSON level so any such
        // refactor trips this test instead of going unnoticed.
        let cases: &[(Event, &str)] = &[
            (
                Event::AvailableCommandsUpdated { commands: vec![] },
                "{\"AvailableCommandsUpdated\":",
            ),
            (
                Event::ModesAvailable {
                    current_mode_id: "default".into(),
                    modes: vec![],
                },
                "{\"ModesAvailable\":",
            ),
            (
                Event::CurrentModeChanged {
                    current_mode_id: "default".into(),
                },
                "{\"CurrentModeChanged\":",
            ),
            (
                Event::AcpSessionAssigned {
                    acp_session_id: "acp-xyz".into(),
                },
                "{\"AcpSessionAssigned\":",
            ),
        ];
        for (event, expected_prefix) in cases {
            let json = serde_json::to_string(event).unwrap();
            assert!(
                json.starts_with(expected_prefix),
                "snapshot variant serialised as {json}, expected to start with {expected_prefix}"
            );
        }
    }

    #[test]
    fn retention_cap_drops_oldest() {
        let (_tmp, store) = open_store(3);
        for i in 1..=5 {
            store.record("s-1", i, &Event::ThinkingStarted).unwrap();
        }
        let replay = store.replay_from("s-1", 0);
        let seqs: Vec<u64> = replay.iter().map(|(s, _)| *s).collect();
        // Newest 3 survive: seqs 3, 4, 5. Oldest (1, 2) pruned.
        assert_eq!(seqs, vec![3, 4, 5]);
    }

    #[test]
    fn delete_session_clears_only_target() {
        let (_tmp, store) = open_store(1000);
        store.record("s-1", 1, &Event::ThinkingStarted).unwrap();
        store.record("s-2", 1, &Event::ThinkingEnded).unwrap();
        store.delete_session("s-1");
        assert_eq!(store.highest_seq("s-1"), 0);
        assert_eq!(store.highest_seq("s-2"), 1);
    }

    #[test]
    fn all_session_seqs_lists_each_session_once() {
        let (_tmp, store) = open_store(1000);
        store.record("s-1", 1, &Event::ThinkingStarted).unwrap();
        store.record("s-1", 2, &Event::ThinkingEnded).unwrap();
        store.record("s-2", 1, &Event::ThinkingStarted).unwrap();
        let mut listed = store.all_session_seqs();
        listed.sort();
        assert_eq!(listed, vec![("s-1".to_string(), 2), ("s-2".to_string(), 1)]);
    }

    #[test]
    fn has_in_flight_turn_empty_store_returns_false() {
        let (_tmp, store) = open_store(1000);
        assert!(!store.has_in_flight_turn("s-1"));
    }

    #[test]
    fn has_in_flight_turn_true_when_chunks_unterminated() {
        let (_tmp, store) = open_store(1000);
        store
            .record("s-1", 1, &Event::UserPromptSent { text: "go".into() })
            .unwrap();
        store
            .record(
                "s-1",
                2,
                &Event::AgentMessageChunk {
                    text: "thinking".into(),
                },
            )
            .unwrap();
        assert!(store.has_in_flight_turn("s-1"));
    }

    #[test]
    fn has_in_flight_turn_false_when_stopped_after_prompt() {
        let (_tmp, store) = open_store(1000);
        store
            .record("s-1", 1, &Event::UserPromptSent { text: "go".into() })
            .unwrap();
        store
            .record(
                "s-1",
                2,
                &Event::AgentMessageChunk {
                    text: "done".into(),
                },
            )
            .unwrap();
        store
            .record(
                "s-1",
                3,
                &Event::Stopped {
                    reason: "prompt_complete".into(),
                },
            )
            .unwrap();
        assert!(!store.has_in_flight_turn("s-1"));
    }

    #[test]
    fn has_in_flight_turn_false_when_agent_startup_error_after_prompt() {
        let (_tmp, store) = open_store(1000);
        store
            .record("s-1", 1, &Event::UserPromptSent { text: "go".into() })
            .unwrap();
        store
            .record(
                "s-1",
                2,
                &Event::AgentStartupError {
                    message: "boom".into(),
                },
            )
            .unwrap();
        assert!(!store.has_in_flight_turn("s-1"));
    }

    #[test]
    fn has_in_flight_turn_uses_latest_prompt_only() {
        // First turn completed. Second turn in flight. Should return true.
        let (_tmp, store) = open_store(1000);
        store
            .record(
                "s-1",
                1,
                &Event::UserPromptSent {
                    text: "first".into(),
                },
            )
            .unwrap();
        store
            .record(
                "s-1",
                2,
                &Event::Stopped {
                    reason: "prompt_complete".into(),
                },
            )
            .unwrap();
        store
            .record(
                "s-1",
                3,
                &Event::UserPromptSent {
                    text: "second".into(),
                },
            )
            .unwrap();
        store
            .record("s-1", 4, &Event::AgentMessageChunk { text: "mid".into() })
            .unwrap();
        assert!(store.has_in_flight_turn("s-1"));
    }

    #[test]
    fn store_persists_across_reopen() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("cockpit.db");
        {
            let store = EventStore::open(&path, 1000).unwrap();
            store
                .record(
                    "s-1",
                    1,
                    &Event::UserPromptSent {
                        text: "hello".into(),
                    },
                )
                .unwrap();
            store
                .record(
                    "s-1",
                    2,
                    &Event::AgentMessageChunk {
                        text: "hi back".into(),
                    },
                )
                .unwrap();
        }
        // Drop and reopen the store; the rows should still be there.
        let store = EventStore::open(&path, 1000).unwrap();
        let replay = store.replay_from("s-1", 0);
        assert_eq!(replay.len(), 2);
        assert_eq!(store.highest_seq("s-1"), 2);
    }
}
