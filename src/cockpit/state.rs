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
    /// When the agent launches a sub-agent (Claude's Task tool) the
    /// adapter rides `_meta.claudeCode.parentToolUseId` along on the
    /// child tool calls. We thread it through here so the cockpit can
    /// render sub-tasks under their parent Task instead of as a flat
    /// stream. None for top-level tool calls. See #1041.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tool_call_id: Option<String>,
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

/// Snapshot of the agent's last-reported context-window usage and
/// (optionally) cumulative session cost. Mirrors the ACP
/// `UsageUpdate` notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionUsage {
    /// Tokens currently in context.
    pub used: u64,
    /// Total context window size in tokens.
    pub size: u64,
    /// Cumulative cost since session start, when the agent reports it.
    #[serde(default)]
    pub cost: Option<UsageCost>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageCost {
    pub amount: f64,
    /// ISO 4217 code (USD/EUR/...).
    pub currency: String,
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

/// One slash command advertised by the agent. Mirrors ACP's
/// `AvailableCommand` shape. `name` is the canonical token (sent back
/// to the agent as `/<name> <args>`); `description` is the human label
/// for the picker; `accepts_input` is true when the agent reports an
/// `Unstructured` input spec, signalling the command takes free-form
/// arguments after the name.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvailableCommand {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub accepts_input: bool,
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
    /// Last-known context-window usage from the agent's most recent
    /// `UsageUpdate`. None until the agent emits one.
    #[serde(default)]
    pub usage: Option<SessionUsage>,
    /// Slash commands the agent advertised in its most recent
    /// `AvailableCommandsUpdate`. Empty until the agent emits one. Used
    /// by the composer's `/` picker so users see real plugin/skill/MCP
    /// commands instead of a hard-coded placeholder list.
    #[serde(default)]
    pub available_commands: Vec<AvailableCommand>,

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
            usage: None,
            available_commands: Vec::new(),
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
        /// Final textual output extracted from ACP `ToolCallUpdate.fields.content`
        /// (concat of all `ToolCallContent::Content(Text(_))` blocks). Empty
        /// when the agent emits no content blocks on completion. Renderers
        /// fall back to a status word ("completed" / "tool failed") when this
        /// is empty so cards still convey state.
        #[serde(default)]
        content: String,
        /// Server-side wall-clock time the completion frame was minted.
        /// Carried on the event so the frontend reducer can stamp the
        /// matching `tool_complete` activity row with the REAL
        /// completion time rather than `new Date()` at replay time;
        /// without this, page-reload after a long delay made every
        /// completed tool's duration count from "now", inflating the
        /// label from seconds to minutes/hours. Events persisted
        /// before this field landed default to "now" on deserialise
        /// (serde calls the function), so the durations of pre-fix
        /// events stay imprecise; new events are accurate end-to-end.
        #[serde(default = "chrono::Utc::now")]
        completed_at: DateTime<Utc>,
    },
    /// Streaming tool output. Some agents emit `ToolCallUpdate` notifications
    /// with `status != Completed` but populated `fields.content` to stream
    /// stdout/stderr while the call is still running. Each event carries the
    /// LATEST full content snapshot for that call (per ACP, the content
    /// field is a replacement, not an append). Reducer buffers it keyed by
    /// tool_call_id; on completion the buffer is used if the final update
    /// shipped no content of its own.
    ToolCallContent {
        tool_call_id: String,
        content: String,
    },
    /// Late-arriving title or raw_input for a tool call. Some agents
    /// (Claude's claude-agent-acp among them) emit the initial
    /// `tool_call` notification with an empty `raw_input` and only fill
    /// in the actual inputs in a follow-up `ToolCallUpdate`. Without
    /// this, bash cards render `$ Terminal` instead of the command and
    /// edit cards lose their target path. The reducer locates the
    /// matching tool_start row by id and overwrites its name/args.
    ToolCallUpdated {
        tool_call_id: String,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        args_preview: Option<String>,
        /// Re-stamps the tool's start time. Set when the agent reports
        /// `ToolCallStatus::InProgress`; claude-agent-acp emits the
        /// initial `tool_call` notification eagerly (often well before
        /// the underlying command actually starts running), so the
        /// duration label (#1060) would otherwise count adapter
        /// scheduling time as part of the tool's runtime. Treating
        /// "InProgress" as the real start gives an accurate elapsed
        /// time on completion.
        #[serde(default)]
        started_at: Option<DateTime<Utc>>,
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
    /// Agent-reported context-window usage. Comes from ACP
    /// `SessionUpdate::UsageUpdate` (gated on the
    /// `unstable_session_usage` schema feature). Latest snapshot wins;
    /// the agent typically resends after each turn.
    UsageUpdated {
        usage: SessionUsage,
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
    /// `session/set_mode` round-trip rejected by the adapter. Fired when
    /// the cockpit asked for a mode the adapter does not advertise
    /// (claude-agent-acp gates `bypassPermissions` on `ALLOW_BYPASS`, so
    /// a YOLO-driven post-spawn `set_mode("bypassPermissions")` lands
    /// here when the env var is unset). UI renders a non-blocking notice
    /// so the user knows their requested mode did not take effect; the
    /// session keeps whatever mode the adapter last reported. See #1233.
    ModeSwitchFailed {
        mode_id: String,
        reason: String,
    },
    /// Full snapshot of the slash commands the agent advertises. Comes
    /// from ACP `SessionUpdate::AvailableCommandsUpdate`. Replaces the
    /// previous list (the agent re-broadcasts the full set whenever it
    /// changes; e.g. after plugin enable/disable).
    AvailableCommandsUpdated {
        commands: Vec<AvailableCommand>,
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
    /// Echo of a user-submitted prompt. Published synchronously by the
    /// `POST /cockpit/prompt` handler before the text is forwarded to
    /// the agent, so the replay buffer (and the on-disk event store)
    /// captures the user's side of the conversation. Without this,
    /// reload/session-switch reconstructs only the agent's chunks and
    /// every turn collapses into one assistant blob.
    UserPromptSent {
        text: String,
    },
    /// A user prompt arrived at the daemon while another `session/prompt`
    /// was still in flight. The daemon refused to forward it (claude-agent-acp
    /// serializes prompts internally and a second concurrent prompt would
    /// race the pending one). Carries the rejected text so the UI can
    /// render a Retry pill near the composer. The text was already
    /// persisted as `UserPromptSent` upstream of this rejection by the
    /// `/cockpit/prompt` handler, so this event does not introduce new
    /// PII exposure relative to the existing transcript. Reason is an
    /// opaque tag for forward extensibility; today only `"agent_busy"`
    /// is used. See #1196.
    PromptRejected {
        reason: String,
        text: String,
    },
    /// Agent-assigned ACP session id from a successful `session/new`.
    /// Server-side listener catches this and persists the id on
    /// `Instance.cockpit_acp_session_id` so the next spawn can call
    /// `session/load` and the model retains context across `aoe serve`
    /// restarts. Not emitted on `session/load` success (id unchanged).
    AcpSessionAssigned {
        acp_session_id: String,
    },
    /// `session/load` failed and we fell back to `session/new`. The
    /// agent's stored transcript is gone (or the id was never valid),
    /// so the model starts with no context. UI uses this to render a
    /// muted notice and clear the now-stale token-usage hint; the
    /// server-side listener clears `Instance.cockpit_acp_session_id`
    /// before the new id arrives via `AcpSessionAssigned`.
    SessionContextReset {
        reason: String,
    },
    /// The agent invoked the Claude SDK's `ScheduleWakeup` tool. The
    /// session will sit idle until `at`, then a new turn fires. Emitted
    /// from `acp_client::map_update_to_events` on `ToolCallStarted` for
    /// `ScheduleWakeup` so the sidebar can flip to a "scheduled" badge
    /// plus countdown without subscribing to the cockpit WS. Considered
    /// pending until the next `UserPromptSent` lands, which is what
    /// /loop's self-firing emits when the wake actually triggers. See
    /// #1091.
    WakeupScheduled {
        at: DateTime<Utc>,
        reason: Option<String>,
    },
    /// User invoked `/clear` (claude-agent-acp's reset-conversation
    /// slash command). The adapter rotates its internal session so the
    /// model truly forgets earlier turns; aoe's transcript is now a
    /// stale historical artifact. Reducer drops session-scoped
    /// capabilities (`availableCommands`, `availableModes`, `plan`,
    /// `mode`) and cancels any open approvals; UI collapses rows above
    /// the divider behind a disclosure. Distinct from
    /// `SessionContextReset` (which fires only on `session/load`
    /// failure now) and `ConversationCompacted` because the
    /// user-experience contract differs: cleared is "the model has
    /// forgotten", reset is "the model has empty context", compacted
    /// is "the model has a summary". See #1101.
    SessionCleared,
    /// `/compact` cycle completed: the model's context window has been
    /// replaced with a summary of the prior turns. The model still
    /// has continuity through the summary, so unlike
    /// `SessionContextReset` there is no recovery to offer; the
    /// reducer drops the now-stale usage snapshot and the UI renders
    /// an inline divider but does NOT surface the context-primer
    /// banner. See #1109.
    ConversationCompacted,
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
            Event::ToolCallContent { .. } => {}
            Event::ToolCallUpdated {
                tool_call_id,
                title,
                args_preview,
                started_at,
            } => {
                if let Some(tool) = self.in_flight_tool.as_mut() {
                    if tool.id == tool_call_id {
                        if let Some(t) = title {
                            tool.name = t;
                        }
                        if let Some(a) = args_preview {
                            tool.args_preview = a;
                        }
                        if let Some(t) = started_at {
                            tool.started_at = t;
                        }
                    }
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
            Event::UsageUpdated { usage } => self.usage = Some(usage),
            Event::ModeChanged { mode } => self.mode = mode,
            // ModesAvailable + CurrentModeChanged carry the real ACP-
            // advertised modes. The cockpit's persistent state doesn't
            // track them yet (the UI stores them in the broadcast
            // replay), so this is just a no-op that bumps seq.
            Event::ModesAvailable { .. } => {}
            Event::CurrentModeChanged { .. } => {}
            Event::ModeSwitchFailed { .. } => {}
            Event::AvailableCommandsUpdated { commands } => {
                self.available_commands = commands;
            }
            // The next four variants don't directly mutate persistent
            // CockpitState fields (yet); they bump seq/updated_at so
            // clients see them in the replay buffer and know the session
            // made progress.
            Event::RawAgentUpdate { .. } => {}
            Event::AgentMessageChunk { .. } => {}
            Event::Stopped { .. } => {}
            Event::AgentStartupError { .. } => {}
            Event::UserPromptSent { .. } => {}
            Event::AcpSessionAssigned { .. } => {}
            Event::SessionContextReset { .. } => {
                // Agent's stored context is gone; clear the cached
                // usage snapshot so the composer footer doesn't keep
                // showing the old "75k / 200k" until the new session
                // emits its first UsageUpdate.
                self.usage = None;
            }
            Event::SessionCleared => {
                // /clear truly wipes the model's memory. Drop
                // session-scoped capability caches and the usage
                // snapshot so the UI doesn't keep showing stale data
                // referencing a conversation the model has forgotten.
                self.usage = None;
                self.available_commands = Vec::new();
                self.current_plan = None;
                self.mode = SessionMode::Default;
                self.pending_approvals = Vec::new();
            }
            Event::ConversationCompacted => {
                // /compact replaces the model's context with a summary
                // of the prior turns. The usage snapshot for the old
                // raw turns no longer matches what the model holds;
                // clear it so the next UsageUpdate seeds the new
                // (compacted) value. Plan/mode/commands persist:
                // unlike /clear, the model still has continuity here.
                self.usage = None;
            }
            // Persistent state for "scheduled wakeup" lives in the
            // event log (queried by the REST endpoint per #1091); no
            // in-memory mirror needed yet. Bumps seq so the WS replay
            // surfaces it to live clients.
            Event::WakeupScheduled { .. } => {}
            // Rejected follow-up prompt while another prompt was in flight.
            // No durable in-memory mutation; the reducer surfaces a Retry
            // pill from the broadcast frame and the event_store entry
            // carries the historical record. See #1196.
            Event::PromptRejected { .. } => {}
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
    fn mode_switch_failed_bumps_seq_without_mutating_mode() {
        let mut s = fresh_state();
        let before_mode = s.mode;
        let seq = s
            .apply_event(Event::ModeSwitchFailed {
                mode_id: "bypassPermissions".into(),
                reason: "Mode bypassPermissions is not available.".into(),
            })
            .expect("apply ok");
        assert_eq!(seq, 1);
        assert_eq!(s.mode, before_mode);
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
            parent_tool_call_id: None,
        };
        s.apply_event(Event::ToolCallStarted {
            tool_call: tc.clone(),
        })
        .unwrap();
        assert!(s.in_flight_tool.is_some());
        s.apply_event(Event::ToolCallCompleted {
            tool_call_id: "tc-1".into(),
            is_error: false,
            content: String::new(),
            completed_at: Utc::now(),
        })
        .unwrap();
        assert!(s.in_flight_tool.is_none());
    }

    #[test]
    fn available_commands_updated_replaces_previous_list() {
        let mut s = fresh_state();
        assert!(s.available_commands.is_empty());
        s.apply_event(Event::AvailableCommandsUpdated {
            commands: vec![AvailableCommand {
                name: "help".into(),
                description: "Show help".into(),
                accepts_input: false,
            }],
        })
        .unwrap();
        assert_eq!(s.available_commands.len(), 1);
        s.apply_event(Event::AvailableCommandsUpdated {
            commands: vec![
                AvailableCommand {
                    name: "review".into(),
                    description: "Review PR".into(),
                    accepts_input: true,
                },
                AvailableCommand {
                    name: "clear".into(),
                    description: "Clear context".into(),
                    accepts_input: false,
                },
            ],
        })
        .unwrap();
        assert_eq!(s.available_commands.len(), 2);
        assert_eq!(s.available_commands[0].name, "review");
        assert!(s.available_commands[0].accepts_input);
    }

    #[test]
    fn usage_updated_replaces_previous_snapshot() {
        let mut s = fresh_state();
        assert!(s.usage.is_none());
        s.apply_event(Event::UsageUpdated {
            usage: SessionUsage {
                used: 1_000,
                size: 200_000,
                cost: None,
            },
        })
        .unwrap();
        assert_eq!(s.usage.as_ref().map(|u| u.used), Some(1_000));
        s.apply_event(Event::UsageUpdated {
            usage: SessionUsage {
                used: 5_000,
                size: 200_000,
                cost: Some(UsageCost {
                    amount: 0.12,
                    currency: "USD".into(),
                }),
            },
        })
        .unwrap();
        let u = s.usage.as_ref().unwrap();
        assert_eq!(u.used, 5_000);
        assert_eq!(u.cost.as_ref().unwrap().currency, "USD");
    }
}
