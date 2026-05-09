//! Disk-backed event log for cockpit sessions.
//!
//! Every event published through `ChannelSink` is appended here so the
//! conversation survives `aoe serve` restarts. The in-memory replay
//! buffer (`replay_buffer.rs`) still serves WS-on-connect drains for
//! speed, but the snapshot endpoint and startup hydration go through
//! this store, which holds full history (subject to the per-session
//! retention cap).

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
    pub fn record(&self, session_id: &str, seq: u64, event: &Event) {
        let json = match serde_json::to_string(event) {
            Ok(s) => s,
            Err(e) => {
                warn!(target: "cockpit.event_store", "serialise event for {session_id}@{seq}: {e}");
                return;
            }
        };
        let bytes = json.len();
        let kind = event_kind(event);
        let now_ms = chrono::Utc::now().timestamp_millis();
        let conn = match self.conn.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        let inserted = match conn.execute(
            "INSERT OR IGNORE INTO cockpit_events (session_id, seq, event_json, created_at)
             VALUES (?1, ?2, ?3, ?4)",
            params![session_id, seq as i64, json, now_ms],
        ) {
            Ok(rows) => rows,
            Err(e) => {
                warn!(target: "cockpit.event_store", "insert {session_id}@{seq}: {e}");
                return;
            }
        };
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
                    warn!(target: "cockpit.event_store", "prune {session_id}: {e}");
                }
            }
        }
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
        Event::ModeChanged { .. } => "mode_changed",
        Event::ModesAvailable { .. } => "modes_available",
        Event::CurrentModeChanged { .. } => "current_mode_changed",
        Event::RawAgentUpdate { .. } => "raw_agent_update",
        Event::AgentMessageChunk { .. } => "agent_message_chunk",
        Event::Stopped { .. } => "stopped",
        Event::AgentStartupError { .. } => "agent_startup_error",
        Event::UserPromptSent { .. } => "user_prompt_sent",
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
            store.record("s-1", i, &Event::ThinkingStarted);
        }
        let replay = store.replay_from("s-1", 2);
        let seqs: Vec<u64> = replay.iter().map(|(s, _)| *s).collect();
        assert_eq!(seqs, vec![3, 4, 5]);
    }

    #[test]
    fn highest_seq_reflects_inserts() {
        let (_tmp, store) = open_store(1000);
        assert_eq!(store.highest_seq("s-1"), 0);
        store.record("s-1", 1, &Event::ThinkingStarted);
        store.record("s-1", 2, &Event::ThinkingEnded);
        assert_eq!(store.highest_seq("s-1"), 2);
    }

    #[test]
    fn duplicate_seq_is_idempotent() {
        let (_tmp, store) = open_store(1000);
        store.record("s-1", 1, &Event::UserPromptSent { text: "hi".into() });
        // Second insert at the same seq must not double-count.
        store.record("s-1", 1, &Event::ThinkingStarted);
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
            store.record("s-1", i, &Event::ThinkingStarted);
        }
        let replay = store.replay_from("s-1", 0);
        let seqs: Vec<u64> = replay.iter().map(|(s, _)| *s).collect();
        // Newest 3 survive: seqs 3, 4, 5. Oldest (1, 2) pruned.
        assert_eq!(seqs, vec![3, 4, 5]);
    }

    #[test]
    fn delete_session_clears_only_target() {
        let (_tmp, store) = open_store(1000);
        store.record("s-1", 1, &Event::ThinkingStarted);
        store.record("s-2", 1, &Event::ThinkingEnded);
        store.delete_session("s-1");
        assert_eq!(store.highest_seq("s-1"), 0);
        assert_eq!(store.highest_seq("s-2"), 1);
    }

    #[test]
    fn all_session_seqs_lists_each_session_once() {
        let (_tmp, store) = open_store(1000);
        store.record("s-1", 1, &Event::ThinkingStarted);
        store.record("s-1", 2, &Event::ThinkingEnded);
        store.record("s-2", 1, &Event::ThinkingStarted);
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
            store.record(
                "s-1",
                1,
                &Event::UserPromptSent {
                    text: "hello".into(),
                },
            );
            store.record(
                "s-1",
                2,
                &Event::AgentMessageChunk {
                    text: "hi back".into(),
                },
            );
        }
        // Drop and reopen the store; the rows should still be there.
        let store = EventStore::open(&path, 1000).unwrap();
        let replay = store.replay_from("s-1", 0);
        assert_eq!(replay.len(), 2);
        assert_eq!(store.highest_seq("s-1"), 2);
    }
}
