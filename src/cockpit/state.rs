//! CockpitState: the single-writer actor model for cockpit session state.
//!
//! All mutations flow through `apply_event`. There is exactly one writer per
//! session. Worker-side notifications (`session/update`) and client-side
//! resolutions (approval taps) both become `Event` values that go through
//! `apply_event`. This eliminates the two-writer race condition that v3's
//! sketch had.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::approvals::{Approval, ApprovalDecision, Nonce};

/// Identifier for a cockpit session. Distinct from `SessionId` in
/// `src/session/` because cockpit sessions are a separate `SessionBackend`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CockpitSessionId(pub String);

/// Which backend agent is running this session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentName(pub String);

/// One step of an agent-emitted plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub id: String,
    pub title: String,
    pub detail: Option<String>,
    pub status: PlanStepStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanStepStatus {
    Pending,
    InProgress,
    Done,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Plan {
    pub plan_id: String,
    pub version: u32,
    pub steps: Vec<PlanStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Todo {
    pub id: String,
    pub text: String,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// ACP `ToolKind` lowercased: `read` / `edit` / `delete` / `move` /
    /// `search` / `execute` / `think` / `fetch` / `switch_mode` / `other`.
    /// Lets the UI pick a per-tool renderer.
    #[serde(default)]
    pub kind: String,
    /// 16 KB cap applied at ingest, control chars stripped.
    pub args_preview: String,
    pub started_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiffPreview {
    pub path: String,
    pub old_text: Option<String>,
    pub new_text: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThinkingSignal {
    pub started_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitInfo {
    pub status: String,
    pub resets_at: DateTime<Utc>,
    pub kind: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionMode {
    Default,
    Plan,
    AcceptEdits,
    BypassPermissions,
}

/// One mode advertised by the agent. Mirrors ACP's `SessionMode`
/// shape: id is the canonical token (passed back via `set_mode`),
/// name is what the user sees, description is an optional tooltip.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModeInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CockpitState {
    pub session_id: CockpitSessionId,
    pub agent: AgentName,
    pub model: Option<String>,
    pub mode: SessionMode,

    pub current_plan: Option<Plan>,
    pub todos: Vec<Todo>,
    pub in_flight_tool: Option<ToolCall>,
    pub pending_approvals: Vec<Approval>,
    pub recent_diffs: Vec<DiffPreview>,
    pub thinking: Option<ThinkingSignal>,
    pub rate_limit: Option<RateLimitInfo>,

    pub last_seq: u64,
    pub updated_at: DateTime<Utc>,
}

impl CockpitState {
    /// Bounded ring of recent diffs. Keep the last 16 to keep state size
    /// bounded; the full diff history lives in the replay buffer.
    const MAX_RECENT_DIFFS: usize = 16;

    pub fn new(session_id: CockpitSessionId, agent: AgentName, model: Option<String>) -> Self {
        Self {
            session_id,
            agent,
            model,
            mode: SessionMode::Default,
            current_plan: None,
            todos: Vec::new(),
            in_flight_tool: None,
            pending_approvals: Vec::new(),
            recent_diffs: Vec::new(),
            thinking: None,
            rate_limit: None,
            last_seq: 0,
            updated_at: Utc::now(),
        }
    }
}

/// Single writer entry point. Every mutation goes through here so the
/// state has exactly one source of truth and `last_seq` stays monotonic.
#[derive(Debug, Error)]
pub enum StateError {
    #[error("approval nonce {0:?} did not match any pending approval")]
    UnknownApprovalNonce(Nonce),
    #[error("approval nonce {0:?} already resolved")]
    ApprovalAlreadyResolved(Nonce),
}

/// Discriminated union of state mutations. ACP `session/update`
/// notifications become specific variants; client approval taps also
/// become variants and flow through the same path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    PlanUpdated {
        plan: Plan,
    },
    TodoListUpdated {
        todos: Vec<Todo>,
    },
    ToolCallStarted {
        tool_call: ToolCall,
    },
    ToolCallCompleted {
        tool_call_id: String,
        is_error: bool,
    },
    ApprovalRequested {
        approval: Approval,
    },
    ApprovalResolved {
        nonce: Nonce,
        decision: ApprovalDecision,
    },
    DiffEmitted {
        diff: DiffPreview,
    },
    ThinkingStarted,
    ThinkingEnded,
    RateLimit {
        info: RateLimitInfo,
    },
    ModeChanged {
        mode: SessionMode,
    },
    /// Real ACP-advertised modes. Emitted once when the agent
    /// announces them (in `NewSessionResponse.modes`) so the UI can
    /// render the actual modes the agent supports rather than the
    /// hard-coded four. The id is the token that goes back via
    /// `session/set_mode`.
    ModesAvailable {
        current_mode_id: String,
        modes: Vec<ModeInfo>,
    },
    /// Agent-driven mode switch. Comes from ACP
    /// `SessionUpdate::CurrentModeUpdate`; UI swaps `current_mode_id`.
    CurrentModeChanged {
        current_mode_id: String,
    },
    /// Passthrough for an ACP `session/update` payload that we have not yet
    /// finished mapping to a typed variant. Useful while the cockpit's
    /// typed schema is still expanding to cover every ACP update kind.
    /// Carries the raw JSON so UI clients can render best-effort.
    RawAgentUpdate {
        payload: serde_json::Value,
    },
    /// An assistant message chunk (text). In ACP this comes as an
    /// `agent_message_chunk` session update.
    AgentMessageChunk {
        text: String,
    },
    /// Final stop signal from the agent. Carries an opaque reason string
    /// so the UI can render "completed" / "ended early" / "cancelled".
    Stopped {
        reason: String,
    },
    /// The agent process failed to spawn or never completed its
    /// `initialize` handshake. Surfaced through the broadcast so the
    /// React cockpit can show a remediation hint instead of staring at
    /// an empty conversation.
    AgentStartupError {
        message: String,
    },
}

impl CockpitState {
    /// Apply a single event. Returns the new `last_seq` on success.
    pub fn apply_event(&mut self, event: Event) -> Result<u64, StateError> {
        match event {
            Event::PlanUpdated { plan } => self.current_plan = Some(plan),
            Event::TodoListUpdated { todos } => self.todos = todos,
            Event::ToolCallStarted { tool_call } => self.in_flight_tool = Some(tool_call),
            Event::ToolCallCompleted { tool_call_id, .. } => {
                if self
                    .in_flight_tool
                    .as_ref()
                    .map(|t| t.id == tool_call_id)
                    .unwrap_or(false)
                {
                    self.in_flight_tool = None;
                }
            }
            Event::ApprovalRequested { approval } => self.pending_approvals.push(approval),
            Event::ApprovalResolved { ref nonce, .. } => {
                let pos = self
                    .pending_approvals
                    .iter()
                    .position(|a| a.nonce == *nonce)
                    .ok_or_else(|| StateError::UnknownApprovalNonce(nonce.clone()))?;
                let resolved = self.pending_approvals.remove(pos);
                if resolved.resolved.is_some() {
                    return Err(StateError::ApprovalAlreadyResolved(nonce.clone()));
                }
            }
            Event::DiffEmitted { diff } => {
                self.recent_diffs.push(diff);
                while self.recent_diffs.len() > Self::MAX_RECENT_DIFFS {
                    self.recent_diffs.remove(0);
                }
            }
            Event::ThinkingStarted => {
                self.thinking = Some(ThinkingSignal {
                    started_at: Utc::now(),
                });
            }
            Event::ThinkingEnded => self.thinking = None,
            Event::RateLimit { info } => self.rate_limit = Some(info),
            Event::ModeChanged { mode } => self.mode = mode,
            // ModesAvailable + CurrentModeChanged carry the real ACP-
            // advertised modes. The cockpit's persistent state doesn't
            // track them yet (the UI stores them in the broadcast
            // replay), so this is just a no-op that bumps seq.
            Event::ModesAvailable { .. } => {}
            Event::CurrentModeChanged { .. } => {}
            // The next four variants don't directly mutate persistent
            // CockpitState fields (yet); they bump seq/updated_at so
            // clients see them in the replay buffer and know the session
            // made progress.
            Event::RawAgentUpdate { .. } => {}
            Event::AgentMessageChunk { .. } => {}
            Event::Stopped { .. } => {}
            Event::AgentStartupError { .. } => {}
        }
        self.last_seq = self.last_seq.saturating_add(1);
        self.updated_at = Utc::now();
        Ok(self.last_seq)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh_state() -> CockpitState {
        CockpitState::new(
            CockpitSessionId("s-1".into()),
            AgentName("aoe-agent".into()),
            Some("claude-opus-4-7".into()),
        )
    }

    #[test]
    fn apply_event_bumps_seq_and_timestamp() {
        let mut s = fresh_state();
        let before = s.updated_at;
        let seq = s.apply_event(Event::ThinkingStarted).expect("apply ok");
        assert_eq!(seq, 1);
        assert!(s.thinking.is_some());
        assert!(s.updated_at >= before);
    }

    #[test]
    fn approval_resolved_with_unknown_nonce_errors() {
        let mut s = fresh_state();
        let result = s.apply_event(Event::ApprovalResolved {
            nonce: Nonce::new(),
            decision: ApprovalDecision::Allow,
        });
        assert!(matches!(result, Err(StateError::UnknownApprovalNonce(_))));
    }

    #[test]
    fn recent_diffs_bounded() {
        let mut s = fresh_state();
        for i in 0..(CockpitState::MAX_RECENT_DIFFS + 5) {
            s.apply_event(Event::DiffEmitted {
                diff: DiffPreview {
                    path: format!("/tmp/file{i}.txt"),
                    old_text: None,
                    new_text: Some("hi".into()),
                    created_at: Utc::now(),
                },
            })
            .unwrap();
        }
        assert_eq!(s.recent_diffs.len(), CockpitState::MAX_RECENT_DIFFS);
        // Oldest entries dropped first.
        assert!(s.recent_diffs[0].path.contains("file5"));
    }

    #[test]
    fn tool_call_lifecycle() {
        let mut s = fresh_state();
        let tc = ToolCall {
            id: "tc-1".into(),
            name: "Read".into(),
            kind: "read".into(),
            args_preview: "{\"path\":\"x\"}".into(),
            started_at: Utc::now(),
        };
        s.apply_event(Event::ToolCallStarted {
            tool_call: tc.clone(),
        })
        .unwrap();
        assert!(s.in_flight_tool.is_some());
        s.apply_event(Event::ToolCallCompleted {
            tool_call_id: "tc-1".into(),
            is_error: false,
        })
        .unwrap();
        assert!(s.in_flight_tool.is_none());
    }
}
