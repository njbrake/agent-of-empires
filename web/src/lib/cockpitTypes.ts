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
  /** ACP ToolKind lowercased: read | edit | delete | move | search |
   *  execute | think | fetch | switch_mode | other. Drives the per-tool
   *  renderer in CockpitView. */
  kind: string;
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
  | {
      ModesAvailable: {
        current_mode_id: string;
        modes: Array<{ id: string; name: string; description?: string | null }>;
      };
    }
  | { CurrentModeChanged: { current_mode_id: string } }
  | { RawAgentUpdate: { payload: unknown } }
  | { AgentMessageChunk: { text: string } }
  | { Stopped: { reason: string } }
  | { AgentStartupError: { message: string } };

export interface CockpitFrame {
  session_id: string;
  seq: number;
  event: CockpitEvent;
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
  /** Last seen seq, for reconnect requests. Frames whose `seq` is
   *  not strictly greater than this are dropped by the reducer so
   *  reconnect-replay can deliver the same frames again without
   *  double-applying them to state. */
  lastSeq: number;
  /** True if the most recent broadcast told us we lagged. Cleared
   *  the next time the client successfully resyncs via the snapshot
   *  endpoint. */
  lagged: boolean;
  /** Latest agent startup failure message, if any. Cleared when a new
   *  prompt is sent or the worker successfully connects. */
  startupError: string | null;
  /** Latest interaction error (failed sendPrompt / resolveApproval /
   *  cancel POST). Surfaces as a dismissible banner so users don't
   *  silently lose actions to a network blip. Cleared on the next
   *  successful interaction. */
  lastError: string | null;
  /** True between sending a user prompt and receiving the
   *  `Stopped { reason: "prompt_complete" }` event. Drives the global
   *  "working" spinner so the UI feels alive even when the agent
   *  isn't streaming text or running a tool yet. */
  turnActive: boolean;
  /** Real ACP-advertised modes from the agent's NewSessionResponse,
   *  plus the agent's currently-active mode id. Empty until the
   *  agent reports them; the picker falls back to the hard-coded
   *  four-mode taxonomy in that case. */
  availableModes: Array<{ id: string; name: string; description?: string | null }>;
  currentModeId: string | null;
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
  /** Full ToolCall payload, present on tool_start rows so the UI can
   *  pick a per-kind renderer without needing to look the call up by
   *  toolCallId. */
  tool?: ToolCall;
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
    lastError: null,
    turnActive: false,
    availableModes: [],
    currentModeId: null,
  };
}

/** Pure reducer. Returns a new state; never mutates the input.
 *  Drops frames whose seq is not strictly greater than `state.lastSeq`
 *  so reconnect/replay can re-deliver buffered frames without
 *  double-applying them (duplicate tool cards, doubled message
 *  chunks, etc.). */
export function applyEvent(
  state: CockpitState,
  frame: CockpitFrame,
): CockpitState {
  if (frame.seq <= state.lastSeq) {
    return state;
  }
  const next = { ...state, lastSeq: frame.seq };
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
      tool: tc,
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
  if ("ModesAvailable" in event) {
    next.availableModes = event.ModesAvailable.modes.map((m) => ({
      id: m.id,
      name: m.name,
      description: m.description ?? null,
    }));
    next.currentModeId = event.ModesAvailable.current_mode_id;
    return next;
  }
  if ("CurrentModeChanged" in event) {
    next.currentModeId = event.CurrentModeChanged.current_mode_id;
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
    // in case the agent forgot to emit a completion. Also clears the
    // turn-active flag so the global "working" spinner stops.
    next.inFlightTool = null;
    next.turnActive = false;
    return next;
  }
  if ("AgentStartupError" in event) {
    next.startupError = event.AgentStartupError.message;
    next.inFlightTool = null;
    next.turnActive = false;
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
