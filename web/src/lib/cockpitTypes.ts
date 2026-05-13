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
  /** When the agent launches a sub-agent (Claude's Task tool), the
   *  adapter rides `_meta.claudeCode.parentToolUseId` along on the
   *  child tool calls. Threaded through here so the cockpit can group
   *  sub-tools under their parent Task. Undefined for top-level
   *  calls. See #1041. */
  parent_tool_call_id?: string;
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
  /** True when ACP reported an `Unstructured` input spec; i.e. the
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
        /** Server-side ISO-8601 wall clock at which the completion
         *  was minted. Used to stamp the activity row's `at` so the
         *  duration label survives page reload; without it, the
         *  reducer would assign `new Date()` at replay time and the
         *  measured duration would count from "now". Optional for
         *  backward compatibility with events persisted before this
         *  field landed. */
        completed_at?: string;
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
        /** Re-stamped start time when the agent reports the tool's
         *  status transitioned to InProgress. See acp_client.rs;
         *  reused so the duration label measures real tool runtime
         *  rather than adapter scheduling time. Null for non-status
         *  updates. */
        started_at?: string | null;
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
  /** True iff the current turn has produced at least one piece of
   *  visible output (assistant chunk, tool call, thinking signal).
   *  Reset to false on every UserPromptSent. Used by the Stopped
   *  handler to detect "no-op turn" without walking the full
   *  activity array. */
  turnHasOutput: boolean;
  /** Set true when the daemon publishes `Stopped { reason: "user_stopped" }`,
   *  meaning `aoe cockpit stop|kill` (or an equivalent external
   *  teardown) terminated the runner. The composer disables itself and
   *  shows a reconnect banner; cleared on the next UserPromptSent or
   *  AcpSessionAssigned (a fresh worker is online). */
  workerStopped: boolean;
  /** Set true when the daemon publishes `Stopped { reason: "restart_pending" }`,
   *  meaning `aoe cockpit restart` ran and the reconciler will respawn
   *  the worker on its next 2s tick with the cached `acp_session_id`
   *  (transcript continuity). The composer disables itself and a
   *  transient "Restarting…" banner appears without a reconnect button;
   *  cleared on AcpSessionAssigned or UserPromptSent. */
  workerRestarting: boolean;
  /** Follow-up prompts the user typed and submitted while a turn was
   *  already running. The composer enqueues them client-side instead
   *  of racing the agent (claude-agent-acp serialises session/prompt
   *  internally, but client-side queueing gives us a visible "queued"
   *  badge and lets the user edit / drop entries before they fire).
   *  On `Stopped` (when the worker is healthy) the head is popped and
   *  dispatched via the regular sendPrompt path. See #1031. */
  queuedPrompts: QueuedPrompt[];
}

export interface QueuedPrompt {
  /** Client-minted id; survives edits. Used by the composer strip to
   *  key the list and by the edit / delete actions to target a row. */
  id: string;
  text: string;
  /** ISO-8601 client wall clock at enqueue time. Displayed as a
   *  relative age in the strip. */
  queuedAt: string;
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
    turnHasOutput: false,
    workerStopped: false,
    workerRestarting: false,
    queuedPrompts: [],
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
      next.turnHasOutput = true;
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
    // Skip duplicate tool_start rows. SQLite stores accumulated from
    // pre-fix runs (where post-load history-replay leaked through) can
    // contain the same tool_call_id twice; rendering both makes
    // assistant-ui's tapResources throw "Duplicate key" and crash the
    // panel. Patch the existing row in place instead.
    const existing = next.activity.findIndex(
      (r) => r.kind === "tool_start" && r.toolCallId === tc.id,
    );
    if (existing >= 0) {
      const prev = next.activity[existing];
      if (prev) {
        const copy = next.activity.slice();
        copy[existing] = { ...prev, tool: tc, text: tc.name };
        next.activity = copy;
      }
      return next;
    }
    next.activity = pushActivity(next.activity, {
      id: `start-${tc.id}`,
      kind: "tool_start",
      text: tc.name,
      toolCallId: tc.id,
      tool: tc,
      at: tc.started_at,
    });
    next.turnHasOutput = true;
    return next;
  }
  if ("ToolCallCompleted" in event) {
    const { tool_call_id, is_error, content, completed_at } =
      event.ToolCallCompleted;
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
    // Use the server-side completion timestamp when present so the
    // duration label survives page reload. Events persisted before
    // `completed_at` landed fall back to "now" (same bug as before for
    // those specific rows only).
    next.activity = pushActivity(next.activity, {
      id: `done-${tool_call_id}`,
      kind: is_error ? "tool_error" : "tool_complete",
      text,
      toolCallId: tool_call_id,
      at: completed_at ?? new Date().toISOString(),
    });
    return next;
  }
  if ("ToolCallContent" in event) {
    const { tool_call_id, content } = event.ToolCallContent;
    next.toolOutputs = { ...next.toolOutputs, [tool_call_id]: content };
    return next;
  }
  if ("ToolCallUpdated" in event) {
    const { tool_call_id, title, args_preview, started_at } =
      event.ToolCallUpdated;
    if (next.inFlightTool && next.inFlightTool.id === tool_call_id) {
      next.inFlightTool = {
        ...next.inFlightTool,
        name: title ?? next.inFlightTool.name,
        args_preview: args_preview ?? next.inFlightTool.args_preview,
        started_at: started_at ?? next.inFlightTool.started_at,
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
            started_at: started_at ?? row.tool.started_at,
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
    next.turnHasOutput = true;
    return next;
  }
  if ("Stopped" in event) {
    // Final marker; nothing to mutate, but reset the inflight tool just
    // in case the agent forgot to emit a completion. Also clears the
    // turn-active flag so the global "working" spinner stops.
    next.inFlightTool = null;
    next.turnActive = false;
    // The "user_stopped" / "restart_pending" reasons are published by
    // the supervisor's reap_user_stopped pass when it detects an
    // out-of-band CLI teardown. Surface a distinct UI state for each:
    //   - user_stopped: persistent "Stopped" banner with a Reconnect
    //     button; the daemon will NOT auto-respawn.
    //   - restart_pending: transient "Restarting…" banner without a
    //     reconnect affordance; the reconciler will respawn within ~2s
    //     and AcpSessionAssigned clears the flag.
    if (event.Stopped.reason === "user_stopped") {
      next.workerStopped = true;
      next.workerRestarting = false;
    } else if (event.Stopped.reason === "restart_pending") {
      next.workerRestarting = true;
      next.workerStopped = false;
    }
    // Some upstream slash commands (e.g. /usage, /status, /memory in
    // claude-agent-acp) advertise via available_commands_update but
    // produce no agent_message_chunk and no tool calls when invoked;
    // see https://github.com/agentclientprotocol/claude-agent-acp/issues/642.
    // Detect that case and append a notice row. The `turnHasOutput`
    // flag is flipped by every output-producing handler and reset by
    // UserPromptSent, so this check is O(1) instead of walking the
    // full activity array on every Stopped.
    if (state.turnActive && !state.turnHasOutput) {
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
    // dispatched a moment ago: find the OLDEST matching un-promoted
    // user_prompt with the same text and promote it to the
    // authoritative seq-based id. Walking oldest-first matters when
    // the user submits the same text twice in quick succession; the
    // first server echo must promote the first optimistic row, not
    // the second, so the seq order matches the submission order.
    const matchIdx = next.activity.findIndex(
      (r) =>
        r.kind === "user_prompt" &&
        r.text === text &&
        !r.id.startsWith("user-seq-"),
    );
    if (matchIdx >= 0) {
      const match = next.activity[matchIdx];
      if (match) {
        const updated = next.activity.slice();
        updated[matchIdx] = { ...match, id: `user-seq-${frame.seq}` };
        next.activity = updated;
        return next;
      }
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
    // New turn; reset the no-output detector so Stopped fires the
    // empty-output notice if the agent produces nothing.
    next.turnHasOutput = false;
    // A fresh prompt means the worker is alive again; clear the
    // user_stopped banner without waiting for AcpSessionAssigned.
    next.workerStopped = false;
    next.workerRestarting = false;
    return next;
  }
  if ("AcpSessionAssigned" in event) {
    // Primary purpose: persistence breadcrumb so the server-side
    // listener can write the id to sessions.json for a subsequent
    // session/load.
    //
    // Secondary purpose: signal that the agent connection is alive
    // again. After a crash + respawn (e.g. the agent process was killed
    // and the supervisor restarted it), the prior turn's
    // AgentStartupError sat in SQLite and kept `startupError` set even
    // though the agent had since recovered. Clear sticky error flags
    // here so the red "Cockpit agent failed to start" banner heals on
    // its own once the respawn completes the handshake.
    next.startupError = null;
    next.lastError = null;
    // A fresh agent (via POST /cockpit/spawn after `aoe cockpit stop`
    // or via the reconciler's auto-respawn after `aoe cockpit restart`)
    // is online; clear both transient worker banners.
    next.workerStopped = false;
    next.workerRestarting = false;
    return next;
  }
  if ("SessionContextReset" in event) {
    // session/load failed and the agent fell back to session/new; its
    // context window is empty. Clear the now-stale token-usage hint so
    // the composer footer doesn't keep showing the previous run's
    // "75k / 200k" until the next UsageUpdate arrives.
    next.sessionUsage = null;
    // Suppress the visible notice on a session that never saw a user
    // prompt: claude-agent-acp doesn't persist a 0-prompt session, so
    // session/load failing on the next spawn is expected, not an
    // incident the user needs to know about. Events arrive in seq
    // order, so checking `activity` here captures "any prompt with a
    // lower seq than this reset"; later prompts won't retroactively
    // surface the suppressed row.
    const hasPriorPrompt = next.activity.some((r) => r.kind === "user_prompt");
    if (!hasPriorPrompt) {
      return next;
    }
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
