// Cockpit wire types. Mirror the shapes emitted by the Rust
// `CockpitBroadcastFrame` serializer + the `Event` enum in
// `src/cockpit/state.rs`. These are intentionally permissive: the Rust
// side can add new variants without breaking the UI as long as the
// component renders unknown frames gracefully.

export type ApprovalDecision = "Allow" | "AllowAlways" | "Deny";

export type SessionMode =
  | "Default"
  | "Plan"
  | "AcceptEdits"
  | "BypassPermissions";

export type PlanStepStatus = "Pending" | "InProgress" | "Done" | "Cancelled";

export interface PlanStep {
  id: string;
  title: string;
  detail?: string | null;
  status: PlanStepStatus;
}

export interface Plan {
  plan_id: string;
  version: number;
  steps: PlanStep[];
}

export interface ToolCall {
  id: string;
  name: string;
  args_preview: string;
  started_at: string; // ISO-8601 from chrono
}

export interface DiffPreview {
  path: string;
  old_text?: string | null;
  new_text?: string | null;
  created_at: string;
}

export interface RateLimitInfo {
  status: string;
  resets_at: string;
  kind: string;
}

export interface Approval {
  nonce: string;
  tool_call: ToolCall;
  destructive: boolean;
  requested_at: string;
  resolved?: {
    decision: ApprovalDecision;
    message?: string | null;
    resolved_at: string;
  } | null;
}

// One variant per Event::* in src/cockpit/state.rs. All variants carry
// a discriminant key matching the serde representation: serde defaults
// to externally-tagged JSON for an enum, e.g.
// { "ApprovalRequested": { "approval": ... } }.
export type CockpitEvent =
  | { PlanUpdated: { plan: Plan } }
  | { TodoListUpdated: { todos: Array<{ id: string; text: string; completed: boolean }> } }
  | { ToolCallStarted: { tool_call: ToolCall } }
  | { ToolCallCompleted: { tool_call_id: string; is_error: boolean } }
  | { ApprovalRequested: { approval: Approval } }
  | { ApprovalResolved: { nonce: string; decision: ApprovalDecision } }
  | { DiffEmitted: { diff: DiffPreview } }
  | "ThinkingStarted"
  | "ThinkingEnded"
  | { RateLimit: { info: RateLimitInfo } }
  | { ModeChanged: { mode: SessionMode } }
  | { RawAgentUpdate: { payload: unknown } }
  | { AgentMessageChunk: { text: string } }
  | { Stopped: { reason: string } }
  | { AgentStartupError: { message: string } };

export interface CockpitFrame {
  session_id: string;
  seq: number;
  event: CockpitEvent;
}

// Special control frame the server emits when the broadcast lagged.
// Surfaced separately so the UI can request a snapshot.
export interface LaggedFrame {
  kind: "lagged";
  skipped: number;
}

export interface CockpitState {
  agent: string | null;
  model: string | null;
  mode: SessionMode;
  plan: Plan | null;
  inFlightTool: ToolCall | null;
  pendingApprovals: Approval[];
  recentDiffs: DiffPreview[];
  thinking: boolean;
  rateLimit: RateLimitInfo | null;
  /** Most recent assistant message chunks accumulated as a single
   *  text body. Cleared each time a new prompt is sent. */
  assistantMessage: string;
  /** Activity rows (tool starts + completions + agent messages),
   *  oldest first. Bounded for memory. */
  activity: ActivityRow[];
  /** Last seen seq, for reconnect requests. */
  lastSeq: number;
  /** True if the most recent broadcast told us we lagged. */
  lagged: boolean;
  /** Latest agent startup failure message, if any. Cleared when a new
   *  prompt is sent or the worker successfully connects. */
  startupError: string | null;
}

export interface ActivityRow {
  id: string;
  kind:
    | "tool_start"
    | "tool_complete"
    | "tool_error"
    | "message"
    | "thinking"
    | "user_prompt";
  text: string;
  toolCallId?: string;
  at: string; // ISO-8601
}

const ACTIVITY_LIMIT = 200;

export function emptyCockpitState(): CockpitState {
  return {
    agent: null,
    model: null,
    mode: "Default",
    plan: null,
    inFlightTool: null,
    pendingApprovals: [],
    recentDiffs: [],
    thinking: false,
    rateLimit: null,
    assistantMessage: "",
    activity: [],
    lastSeq: 0,
    lagged: false,
    startupError: null,
  };
}

/** Pure reducer. Returns a new state; never mutates the input. */
export function applyEvent(
  state: CockpitState,
  frame: CockpitFrame,
): CockpitState {
  const next = { ...state, lastSeq: Math.max(state.lastSeq, frame.seq) };
  const event = frame.event;
  if (typeof event === "string") {
    if (event === "ThinkingStarted") {
      next.thinking = true;
    } else if (event === "ThinkingEnded") {
      next.thinking = false;
    }
    return next;
  }
  if ("PlanUpdated" in event) {
    next.plan = event.PlanUpdated.plan;
    return next;
  }
  if ("ToolCallStarted" in event) {
    const tc = event.ToolCallStarted.tool_call;
    next.inFlightTool = tc;
    next.activity = pushActivity(next.activity, {
      id: `start-${tc.id}`,
      kind: "tool_start",
      text: tc.name,
      toolCallId: tc.id,
      at: tc.started_at,
    });
    return next;
  }
  if ("ToolCallCompleted" in event) {
    const { tool_call_id, is_error } = event.ToolCallCompleted;
    if (next.inFlightTool && next.inFlightTool.id === tool_call_id) {
      next.inFlightTool = null;
    }
    next.activity = pushActivity(next.activity, {
      id: `done-${tool_call_id}`,
      kind: is_error ? "tool_error" : "tool_complete",
      text: is_error ? "tool failed" : "completed",
      toolCallId: tool_call_id,
      at: new Date().toISOString(),
    });
    return next;
  }
  if ("ApprovalRequested" in event) {
    const a = event.ApprovalRequested.approval;
    next.pendingApprovals = [...next.pendingApprovals, a];
    return next;
  }
  if ("ApprovalResolved" in event) {
    const { nonce } = event.ApprovalResolved;
    next.pendingApprovals = next.pendingApprovals.filter(
      (a) => a.nonce !== nonce,
    );
    return next;
  }
  if ("DiffEmitted" in event) {
    next.recentDiffs = [...next.recentDiffs, event.DiffEmitted.diff].slice(-16);
    return next;
  }
  if ("RateLimit" in event) {
    next.rateLimit = event.RateLimit.info;
    return next;
  }
  if ("ModeChanged" in event) {
    next.mode = event.ModeChanged.mode;
    return next;
  }
  if ("AgentMessageChunk" in event) {
    next.assistantMessage = next.assistantMessage + event.AgentMessageChunk.text;
    next.activity = pushActivity(next.activity, {
      id: `msg-${frame.seq}`,
      kind: "message",
      text: event.AgentMessageChunk.text,
      at: new Date().toISOString(),
    });
    return next;
  }
  if ("Stopped" in event) {
    // Final marker; nothing to mutate, but reset the inflight tool just
    // in case the agent forgot to emit a completion.
    next.inFlightTool = null;
    return next;
  }
  if ("AgentStartupError" in event) {
    next.startupError = event.AgentStartupError.message;
    next.inFlightTool = null;
    return next;
  }
  // RawAgentUpdate, TodoListUpdated, anything else: pass through with
  // no state mutation. The activity feed shows the raw text where
  // useful via the catch-all branch in the UI.
  return next;
}

function pushActivity(rows: ActivityRow[], row: ActivityRow): ActivityRow[] {
  const next = rows.concat(row);
  if (next.length > ACTIVITY_LIMIT) {
    return next.slice(next.length - ACTIVITY_LIMIT);
  }
  return next;
}
