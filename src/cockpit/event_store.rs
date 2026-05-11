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
use rusqlite::{params, Connection, OptionalExtension};
use tracing::{debug, warn};

use super::state::Event;

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
    /// to the primary key — re-publishing the same seq is a no-op.
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
            // Logged at debug because the cause is usually a benign retry
            // (publish_user_prompt + replay drain re-publishing) rather
            // than a bug, but we still want a breadcrumb.
            debug!(
                target: "cockpit.event_store",
                session = %session_id,
                seq,
                kind,
                "skipped duplicate event (already on disk)"
            );
        } else {
            debug!(
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
        if self.max_events_per_session > 0 {
            match conn.execute(
                "DELETE FROM cockpit_events
                 WHERE session_id = ?1
                   AND seq <= (
                     SELECT seq FROM cockpit_events
                     WHERE session_id = ?1
                     ORDER BY seq DESC
                     LIMIT 1 OFFSET ?2
                   )",
                params![session_id, self.max_events_per_session as i64],
            ) {
                Ok(0) => {}
                Ok(pruned) => {
                    debug!(
                        target: "cockpit.event_store",
                        session = %session_id,
                        pruned,
                        cap = self.max_events_per_session,
                        "pruned oldest events past retention cap"
                    );
                }
                Err(e) => {
                    // Prune failure isn't fatal — the row is recorded,
                    // we just exceed the cap until the next prune
                    // succeeds. Log + swallow so callers don't have to
                    // distinguish "record failed" from "trim failed".
                    warn!(target: "cockpit.event_store", "prune {session_id}: {e}");
                }
            }
        }
        Ok(())
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
        debug!(
            target: "cockpit.event_store",
            session = %session_id,
            since,
            returned = out.len(),
            "replayed events"
        );
        out
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
        debug!(
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
