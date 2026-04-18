//! Web Push notifications for the dashboard PWA.
//!
//! Sends VAPID-signed pushes to subscribed browsers when session status
//! transitions require user attention (currently: Running -> Waiting).
//! Consumed via a broadcast channel off `AppState.status_tx`, so the
//! transition-detection logic is decoupled from tmux polling and can be
//! unit-tested by feeding events directly.
//!
//! Wire format for subscriptions and the security model (per-token hash
//! ownership, rotate-invalidation) are documented in
//! `docs/plans/web-push-notifications.md`.

use crate::session::Status;
use chrono::{DateTime, Utc};

/// Emitted when an instance's status changes. The broadcast channel on
/// `AppState.status_tx` carries these; `push.rs` is the only consumer in
/// v1, but future features (UI realtime, webhooks) can subscribe too.
#[derive(Clone, Debug)]
pub struct StatusChange {
    pub instance_id: String,
    pub instance_title: String,
    pub old: Status,
    pub new: Status,
    pub at: DateTime<Utc>,
}

/// Capacity of the broadcast channel. Large enough that short bursts of
/// concurrent transitions (e.g., `/api/sessions` bulk refresh) don't drop
/// events even if the consumer is momentarily behind. If a receiver lags
/// past this, broadcast surfaces `RecvError::Lagged` and the consumer can
/// log and continue; push delivery is best-effort anyway.
pub const STATUS_CHANNEL_CAPACITY: usize = 64;
