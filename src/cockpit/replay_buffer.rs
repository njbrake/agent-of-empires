//! Bounded ring buffer of cockpit events for reconnect replay.
//!
//! Bounded by event count AND total bytes (whichever hits first). When the
//! oldest is dropped, a `gap` marker stays so reconnecting clients can tell
//! they missed events instead of silently getting an inconsistent view.

use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

use super::state::Event;

/// One stored entry: either a real event or a gap marker indicating that
/// older events were dropped to stay within bounds.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BufferedEvent {
    Event { seq: u64, event: Event },
    Gap { up_to_seq: u64 },
}

#[derive(Debug)]
pub struct ReplayBuffer {
    items: VecDeque<BufferedEvent>,
    /// Approximate total bytes of stored events (serialized JSON length).
    bytes: usize,
    max_events: usize,
    max_bytes: usize,
    /// Highest seq we've ever seen, even if it's been dropped.
    highest_seen_seq: u64,
}

impl ReplayBuffer {
    pub fn new(max_events: usize, max_bytes: usize) -> Self {
        Self {
            items: VecDeque::with_capacity(max_events),
            bytes: 0,
            max_events,
            max_bytes,
            highest_seen_seq: 0,
        }
    }

    /// Record an event. The buffer drops oldest entries until both the
    /// count and byte caps are satisfied. A gap marker is inserted at the
    /// front of the buffer if any drop happened.
    pub fn push(&mut self, seq: u64, event: Event) {
        let serialized_len = serde_json::to_vec(&event).map(|v| v.len()).unwrap_or(0);
        self.items.push_back(BufferedEvent::Event { seq, event });
        self.bytes = self.bytes.saturating_add(serialized_len);
        self.highest_seen_seq = self.highest_seen_seq.max(seq);

        let mut dropped_through: Option<u64> = None;
        while self.items.len() > self.max_events || self.bytes > self.max_bytes {
            match self.items.pop_front() {
                Some(BufferedEvent::Event {
                    seq: dropped_seq,
                    event,
                }) => {
                    let dropped_len = serde_json::to_vec(&event).map(|v| v.len()).unwrap_or(0);
                    self.bytes = self.bytes.saturating_sub(dropped_len);
                    dropped_through = Some(dropped_seq);
                }
                Some(BufferedEvent::Gap { up_to_seq }) => {
                    dropped_through = Some(up_to_seq);
                }
                None => break,
            }
        }
        if let Some(seq) = dropped_through {
            // Coalesce with an existing leading gap if any.
            if let Some(BufferedEvent::Gap { up_to_seq }) = self.items.front_mut() {
                *up_to_seq = (*up_to_seq).max(seq);
            } else {
                self.items.push_front(BufferedEvent::Gap { up_to_seq: seq });
            }
        }
    }

    /// Replay everything strictly after `last_seen_seq`.
    /// Returns `None` if the requested seq is older than what the buffer
    /// holds, signalling the client should request a snapshot.
    pub fn replay_from(&self, last_seen_seq: u64) -> Option<Vec<BufferedEvent>> {
        // Find the lowest seq currently held (excluding gap markers).
        let oldest_event_seq = self.items.iter().find_map(|item| match item {
            BufferedEvent::Event { seq, .. } => Some(*seq),
            BufferedEvent::Gap { .. } => None,
        });
        if let Some(oldest) = oldest_event_seq {
            // We can answer iff every event after `last_seen_seq` is still
            // in-buffer. That's guaranteed exactly when there is no gap
            // strictly newer than last_seen_seq.
            let gap_blocks_replay = self.items.iter().any(|item| match item {
                BufferedEvent::Gap { up_to_seq } => *up_to_seq > last_seen_seq,
                _ => false,
            });
            if gap_blocks_replay {
                return None;
            }
            if last_seen_seq + 1 < oldest {
                // Buffer doesn't go far enough back even though no gap was
                // recorded (e.g., client way behind, fresh buffer).
                return None;
            }
        }

        Some(
            self.items
                .iter()
                .filter(|item| match item {
                    BufferedEvent::Event { seq, .. } => *seq > last_seen_seq,
                    BufferedEvent::Gap { up_to_seq } => *up_to_seq > last_seen_seq,
                })
                .cloned()
                .collect(),
        )
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn highest_seq(&self) -> u64 {
        self.highest_seen_seq
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pushes_and_replays() {
        let mut buf = ReplayBuffer::new(10, 1_000_000);
        for i in 1..=5 {
            buf.push(i, Event::ThinkingStarted);
        }
        let replay = buf.replay_from(2).expect("replay should succeed");
        let seqs: Vec<u64> = replay
            .iter()
            .filter_map(|e| match e {
                BufferedEvent::Event { seq, .. } => Some(*seq),
                _ => None,
            })
            .collect();
        assert_eq!(seqs, vec![3, 4, 5]);
    }

    #[test]
    fn count_bound_drops_oldest_with_gap_marker() {
        let mut buf = ReplayBuffer::new(3, 1_000_000);
        for i in 1..=5 {
            buf.push(i, Event::ThinkingStarted);
        }
        // Stored should be 4 entries (gap + 3 events) since pushing 5 with
        // max_events=3 drops 2 events and inserts a gap marker.
        let has_gap = buf
            .items
            .iter()
            .any(|i| matches!(i, BufferedEvent::Gap { .. }));
        assert!(has_gap, "expected a gap marker after dropping oldest");
    }

    #[test]
    fn replay_from_stale_seq_returns_none() {
        let mut buf = ReplayBuffer::new(3, 1_000_000);
        for i in 1..=10 {
            buf.push(i, Event::ThinkingStarted);
        }
        // Asking for seq 1 when only 8,9,10 remain (with a gap covering 1-7)
        // must return None so the client knows to snapshot.
        assert!(buf.replay_from(1).is_none());
    }

    #[test]
    fn replay_from_future_seq_returns_empty() {
        let mut buf = ReplayBuffer::new(10, 1_000_000);
        for i in 1..=3 {
            buf.push(i, Event::ThinkingStarted);
        }
        let replay = buf
            .replay_from(100)
            .expect("future seq is allowed (no gap)");
        assert!(replay.is_empty());
    }
}
