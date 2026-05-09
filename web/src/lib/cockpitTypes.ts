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

export interface SessionUsage {
  /** Tokens currently in context. */
  used: number;
  /** Total context window size in tokens. */
  size: number;
  /** Cumulative session cost; undefined if the agent doesn't report it. */
  cost?: { amount: number; currency: string } | null;
}

/** One slash command advertised by the agent (mirrors ACP's
 *  `AvailableCommand`). The composer's `/` picker renders these so
 *  users see plugin/skill/MCP commands the agent actually has loaded
 *  rather than a hard-coded placeholder list. */
export interface AvailableCommand {
  name: string;
  description: string;
  /** True when ACP reported an `Unstructured` input spec — i.e. the
   *  command takes free-form arguments after the name. The composer
   *  inserts a trailing space and leaves the cursor in place when
   *  this is true so the user can keep typing. */
  accepts_input: boolean;
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
  | {
      ToolCallCompleted: {
        tool_call_id: string;
        is_error: boolean;
        /** Final textual content extracted from
         *  ACP `ToolCallUpdate.fields.content`. Empty when the agent
         *  emitted no content blocks on completion. */
        content: string;
      };
    }
  | {
      /** Streaming output for a still-running tool call. Carries the
       *  latest full content snapshot (per ACP, content is a
       *  replacement, not append). The reducer buffers it keyed by
       *  tool_call_id and uses it on completion if the final
       *  ToolCallCompleted carries no content of its own. */
      ToolCallContent: { tool_call_id: string; content: string };
    }
  | {
      /** Late-arriving inputs/title for an already-started tool call.
       *  Claude's claude-agent-acp emits the initial tool_call with an
       *  empty `raw_input` and only fills in the actual command in a
       *  follow-up ToolCallUpdate. Without this, bash cards display
       *  `$ Terminal` (the title) rather than the command. */
      ToolCallUpdated: {
        tool_call_id: string;
        title: string | null;
        args_preview: string | null;
      };
    }
  | { ApprovalRequested: { approval: Approval } }
  | { ApprovalResolved: { nonce: string; decision: ApprovalDecision } }
  | { DiffEmitted: { diff: DiffPreview } }
  | "ThinkingStarted"
  | "ThinkingEnded"
  | { RateLimit: { info: RateLimitInfo } }
  | { UsageUpdated: { usage: SessionUsage } }
  | { ModeChanged: { mode: SessionMode } }
  | {
      ModesAvailable: {
        current_mode_id: string;
        modes: Array<{ id: string; name: string; description?: string | null }>;
      };
    }
  | { CurrentModeChanged: { current_mode_id: string } }
  | { AvailableCommandsUpdated: { commands: AvailableCommand[] } }
  | { RawAgentUpdate: { payload: unknown } }
  | { AgentMessageChunk: { text: string } }
  | { Stopped: { reason: string } }
  | { AgentStartupError: { message: string } }
  | { UserPromptSent: { text: string } }
  | { AcpSessionAssigned: { acp_session_id: string } }
  | { SessionContextReset: { reason: string } };

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
  /** Latest agent-reported context-window usage. Null until the agent
   *  emits its first ACP `UsageUpdate`. */
  sessionUsage: SessionUsage | null;
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
  /** Slash commands the agent advertised in its most recent
   *  `AvailableCommandsUpdate`. Empty until the agent emits one; the
   *  composer's `/` picker reads from here. */
  availableCommands: AvailableCommand[];
  /** Streaming output buffer keyed by tool_call_id. Populated by
   *  ToolCallContent frames while the call is still running, drained
   *  on ToolCallCompleted (used as a fallback when the completion
   *  carries no content of its own). */
  toolOutputs: Record<string, string>;
}

export interface ActivityRow {
  id: string;
  kind:
    | "tool_start"
    | "tool_complete"
    | "tool_error"
    | "message"
    | "thinking"
    | "user_prompt"
    | "empty_output"
    | "context_reset";
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
    sessionUsage: null,
    assistantMessage: "",
    activity: [],
    lastSeq: 0,
    lagged: false,
    startupError: null,
    lastError: null,
    turnActive: false,
    availableModes: [],
    currentModeId: null,
    availableCommands: [],
    toolOutputs: {},
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
    const { tool_call_id, is_error, content } = event.ToolCallCompleted;
    if (next.inFlightTool && next.inFlightTool.id === tool_call_id) {
      next.inFlightTool = null;
    }
    // Prefer content shipped with the completion event itself; fall
    // back to whatever streamed earlier via ToolCallContent. Only use
    // the status word when neither carried text.
    const buffered = next.toolOutputs[tool_call_id] ?? "";
    const text =
      content && content.length > 0
        ? content
        : buffered.length > 0
          ? buffered
          : is_error
            ? "tool failed"
            : "completed";
    if (buffered) {
      const { [tool_call_id]: _drop, ...rest } = next.toolOutputs;
      void _drop;
      next.toolOutputs = rest;
    }
    next.activity = pushActivity(next.activity, {
      id: `done-${tool_call_id}`,
      kind: is_error ? "tool_error" : "tool_complete",
      text,
      toolCallId: tool_call_id,
      at: new Date().toISOString(),
    });
    return next;
  }
  if ("ToolCallContent" in event) {
    const { tool_call_id, content } = event.ToolCallContent;
    next.toolOutputs = { ...next.toolOutputs, [tool_call_id]: content };
    return next;
  }
  if ("ToolCallUpdated" in event) {
    const { tool_call_id, title, args_preview } = event.ToolCallUpdated;
    if (next.inFlightTool && next.inFlightTool.id === tool_call_id) {
      next.inFlightTool = {
        ...next.inFlightTool,
        name: title ?? next.inFlightTool.name,
        args_preview: args_preview ?? next.inFlightTool.args_preview,
      };
    }
    // Walk activity backwards to find the matching tool_start row and
    // patch its `tool` payload in place. AssistantBuilder reads from
    // here at render time, so updating the row is enough to refresh
    // the per-tool card.
    let patched = false;
    const updated = next.activity.map((row) => {
      if (
        !patched &&
        row.kind === "tool_start" &&
        row.toolCallId === tool_call_id &&
        row.tool
      ) {
        patched = true;
        return {
          ...row,
          text: title ?? row.text,
          tool: {
            ...row.tool,
            name: title ?? row.tool.name,
            args_preview: args_preview ?? row.tool.args_preview,
          },
        };
      }
      return row;
    });
    if (patched) next.activity = updated;
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
  if ("UsageUpdated" in event) {
    next.sessionUsage = event.UsageUpdated.usage;
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
  if ("AvailableCommandsUpdated" in event) {
    next.availableCommands = event.AvailableCommandsUpdated.commands;
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
    // Some upstream slash commands (e.g. /usage, /status, /memory in
    // claude-agent-acp) advertise via available_commands_update but
    // produce no agent_message_chunk and no tool calls when invoked —
    // see https://github.com/agentclientprotocol/claude-agent-acp/issues/642.
    // Detect that case and append a notice row so the user sees the turn
    // ran rather than silent dead air. Walk back to the last user_prompt;
    // if no message/tool/thinking row appeared between it and now, the
    // turn was a no-op.
    let sawOutput = false;
    let sawPrompt = false;
    for (let i = next.activity.length - 1; i >= 0; i--) {
      const r = next.activity[i];
      if (!r) continue;
      if (r.kind === "user_prompt") {
        sawPrompt = true;
        break;
      }
      if (
        r.kind === "message" ||
        r.kind === "tool_start" ||
        r.kind === "tool_complete" ||
        r.kind === "tool_error" ||
        r.kind === "thinking" ||
        r.kind === "empty_output"
      ) {
        sawOutput = true;
        break;
      }
    }
    if (sawPrompt && !sawOutput) {
      next.activity = pushActivity(next.activity, {
        id: `empty-${frame.seq}`,
        kind: "empty_output",
        text: "Command produced no output.",
        at: new Date().toISOString(),
      });
    }
    return next;
  }
  if ("AgentStartupError" in event) {
    next.startupError = event.AgentStartupError.message;
    next.inFlightTool = null;
    next.turnActive = false;
    return next;
  }
  if ("UserPromptSent" in event) {
    const text = event.UserPromptSent.text;
    // Dedupe against the optimistic row that useCockpit's sendPrompt
    // dispatched a moment ago: if the most recent activity row is a
    // matching user_prompt with no server-assigned id yet (id starts
    // with "user-" + a timestamp, not "user-seq-"), promote it to the
    // authoritative seq-based id rather than appending a duplicate.
    const lastIdx = next.activity.length - 1;
    const last = lastIdx >= 0 ? next.activity[lastIdx] : null;
    if (
      last &&
      last.kind === "user_prompt" &&
      last.text === text &&
      !last.id.startsWith("user-seq-")
    ) {
      const updated = next.activity.slice();
      updated[lastIdx] = { ...last, id: `user-seq-${frame.seq}` };
      next.activity = updated;
      return next;
    }
    next.activity = pushActivity(next.activity, {
      id: `user-seq-${frame.seq}`,
      kind: "user_prompt",
      text,
      at: new Date().toISOString(),
    });
    next.assistantMessage = "";
    next.startupError = null;
    next.lastError = null;
    next.turnActive = true;
    return next;
  }
  if ("AcpSessionAssigned" in event) {
    // Persistence breadcrumb only; the server-side listener writes the
    // id to sessions.json so a subsequent restart can call session/load.
    // Nothing to render in the conversation surface.
    return next;
  }
  if ("SessionContextReset" in event) {
    // session/load failed and the agent fell back to session/new — its
    // context window is empty. Show a muted notice in the transcript
    // and clear the now-stale token-usage hint so the composer footer
    // doesn't keep showing the previous run's "75k / 200k" until the
    // next UsageUpdate arrives.
    next.sessionUsage = null;
    next.activity = pushActivity(next.activity, {
      id: `reset-${frame.seq}`,
      kind: "context_reset",
      text:
        event.SessionContextReset.reason ||
        "Conversation context reset; agent transcript was unavailable.",
      at: new Date().toISOString(),
    });
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
