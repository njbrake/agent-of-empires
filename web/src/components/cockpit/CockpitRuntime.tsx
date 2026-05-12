// Bridge between our `useCockpit` state (which subscribes to the
// cockpit WebSocket) and assistant-ui's external-store runtime. The
// runtime adapter is the seam: assistant-ui owns the chat surface
// (rendering, scrolling, accessibility, message editing affordances);
// we own the data (events from the ACP-driven supervisor) and the
// actions (sendPrompt, cancelPrompt, resolveApproval).
//
// Flow:
//   ws frame  ─►  applyEvent → CockpitState.activity (ours)
//                                      │
//                                      ▼
//                       activityToThreadMessages()  ────►  ThreadMessageLike[]
//                                      │
//                                      ▼
//                       useExternalStoreRuntime(adapter) ───►  AssistantRuntime
//                                      │
//                                      ▼
//                       <AssistantRuntimeProvider runtime>
//                                      │
//                       ▼              │              ▼
//          <ThreadPrimitive.Messages>  │   <ComposerPrimitive.Root>
//                                      │
//                       onNew: sendPrompt   onCancel: cancelPrompt
//
// We keep all of our existing renderers (Markdown, ToolCards, the
// rattle spinner, ApprovalCard) and slot them into assistant-ui's
// component-injection points.

import {
  AssistantRuntimeProvider,
  useExternalStoreRuntime,
  type ThreadMessageLike,
} from "@assistant-ui/react";
import { useMemo, type ReactNode } from "react";

import { useCockpit } from "../../hooks/useCockpit";
import type {
  ActivityRow,
  ApprovalDecision,
  CockpitState,
  ToolCall,
} from "../../lib/cockpitTypes";

interface Props {
  sessionId: string;
  children: (ctx: CockpitContext) => ReactNode;
}

export interface CockpitContext {
  state: CockpitState;
  status: ReturnType<typeof useCockpit>["status"];
  resolveApproval: (
    nonce: string,
    decision: ApprovalDecision,
  ) => Promise<void>;
  sendPrompt: (text: string) => Promise<void>;
  dismissError: () => void;
}

/**
 * Wraps children in an `<AssistantRuntimeProvider>` driven by our
 * cockpit WS state. Children get a render-prop callback with the raw
 * cockpit state + actions for things assistant-ui doesn't own
 * (approvals, plan strip, system notices).
 */
export function CockpitRuntime({ sessionId, children }: Props) {
  const cockpit = useCockpit(sessionId);
  // Memoise the activity → ThreadMessageLike conversion. The function
  // walks the entire activity array, allocates a new AssistantBuilder
  // per turn, and produces brand-new message objects. Without
  // useMemo, every parent re-render (e.g. WS heartbeat, hover state)
  // re-builds the entire transcript and assistant-ui treats every
  // message as changed. Memo on the two inputs the function reads.
  const messages = useMemo(
    () => activityToThreadMessages(cockpit.state.activity, cockpit.state.turnActive),
    [cockpit.state.activity, cockpit.state.turnActive],
  );

  const runtime = useExternalStoreRuntime<ThreadMessageLike>({
    messages,
    isRunning: cockpit.state.turnActive,
    convertMessage: (m) => m,
    onNew: async (msg) => {
      // assistant-ui hands us an AppendMessage with mixed parts. The
      // cockpit only accepts plain text prompts today, so flatten any
      // text parts into a single string. Attachments / images are not
      // supported by ACP yet.
      const text = msg.content
        .map((c) => (c.type === "text" ? c.text : ""))
        .join("")
        .trim();
      if (!text) return;
      await cockpit.sendPrompt(text);
    },
    onCancel: async () => {
      await cockpit.cancelPrompt();
    },
  });

  return (
    <AssistantRuntimeProvider runtime={runtime}>
      {children({
        state: cockpit.state,
        status: cockpit.status,
        resolveApproval: cockpit.resolveApproval,
        sendPrompt: cockpit.sendPrompt,
        dismissError: cockpit.dismissError,
      })}
    </AssistantRuntimeProvider>
  );
}

/**
 * Convert the flat `ActivityRow` log into the message tree assistant-ui
 * expects. Each `user_prompt` opens a new user message; subsequent
 * agent rows (text chunks + tool calls) collapse into one assistant
 * message until the next user_prompt or end of log.
 *
 * Tool completion rows (`tool_complete` / `tool_error`) are not their
 * own messages; they update the matching `tool-call` part in place
 * by setting `result` / `isError`, so the per-tool card renderer can
 * render running → done in one place.
 */
export function activityToThreadMessages(
  rows: readonly ActivityRow[],
  turnActive: boolean,
): ThreadMessageLike[] {
  const messages: ThreadMessageLike[] = [];
  let currentAssistant: AssistantBuilder | null = null;

  const flushAssistant = () => {
    if (!currentAssistant) return;
    messages.push(currentAssistant.build());
    currentAssistant = null;
  };

  for (const row of rows) {
    if (row.kind === "user_prompt") {
      flushAssistant();
      messages.push({
        id: row.id,
        role: "user",
        content: [{ type: "text", text: row.text }],
        createdAt: parseDate(row.at),
      });
      continue;
    }

    if (row.kind === "context_reset") {
      // Two senders share this row kind:
      //   - `session/load` fallback after an `aoe serve` restart (model's
      //     window is empty even though we replay the prior transcript)
      //   - `/compact` completion: model's window has been replaced by a
      //     summary while the rendered transcript stays put (#1050)
      // Both want the same amber-callout shape; only the header differs.
      // The Rust side sets the reason text; we sniff the compact case
      // off its leading word so the divider names what actually changed.
      flushAssistant();
      const isCompact = row.text.startsWith("Conversation compacted");
      const header = isCompact
        ? "Conversation compacted"
        : "Conversation context reset";
      const body = isCompact
        ? row.text.replace(/^Conversation compacted\s*[;,—-]?\s*/, "")
        : row.text;
      messages.push({
        id: `assistant-${row.id}`,
        role: "assistant",
        content: [
          {
            type: "text",
            text: `> ⚠️ **${header}**; ${body}`,
          },
        ],
        createdAt: parseDate(row.at),
      });
      continue;
    }

    if (!currentAssistant) {
      currentAssistant = new AssistantBuilder(row.id, row.at);
    }

    if (row.kind === "message") {
      currentAssistant.appendText(row.text);
    } else if (row.kind === "tool_start" && row.tool) {
      currentAssistant.appendToolCall(row.tool);
    } else if (row.kind === "tool_complete" || row.kind === "tool_error") {
      currentAssistant.completeToolCall(
        row.toolCallId ?? row.id.replace(/^done-/, ""),
        row.kind === "tool_error",
        row.text,
        row.at,
      );
    } else if (row.kind === "thinking") {
      // Thinking is rendered by the global rattle spinner, not the
      // message stream.
    } else if (row.kind === "empty_output") {
      // Synthesised when the agent finished a turn without emitting any
      // text or tool calls (e.g. interactive-only slash commands like
      // /usage, /status, /memory in claude-agent-acp; see upstream
      // issue agentclientprotocol/claude-agent-acp#642). Surface it as
      // a tiny muted notice instead of leaving the assistant bubble
      // empty.
      currentAssistant.appendText(`_${row.text}_`);
    } else {
      // Unknown kind: surface as a tiny text part so we don't lose
      // the data, but don't make it the whole message.
      currentAssistant.appendText(row.text);
    }
  }
  flushAssistant();

  // While the agent is still working, leave the last assistant message
  // marked as "running" so assistant-ui knows to keep its skeleton/
  // status indicators alive. The runtime's isRunning prop covers the
  // global flag; per-message status is derived from the trailing
  // message's `status`.
  if (turnActive) {
    const last = messages[messages.length - 1];
    if (last && last.role === "assistant") {
      messages[messages.length - 1] = {
        ...last,
        status: { type: "running" },
      };
    }
  }

  return messages;
}

function parseDate(iso: string): Date | undefined {
  const d = new Date(iso);
  return Number.isFinite(d.getTime()) ? d : undefined;
}

// assistant-ui's `tool-call` content part has its own (readonly,
// JSON-only) shape. We model our parts loosely here and cast at build
// time; the runtime only inspects fields it knows about and our
// per-tool renderer (ToolCards.tsx) reads the rest off `argsText`.
type DraftPart =
  | { type: "text"; text: string }
  | {
      type: "tool-call";
      toolCallId: string;
      toolName: string;
      argsText: string;
      result?: { content: string; endedAt?: string };
      isError?: boolean;
    };

/** Mutable builder for an assistant message under construction. */
class AssistantBuilder {
  private id: string;
  private createdAt?: Date;
  private parts: DraftPart[] = [];

  constructor(id: string, createdAtIso: string) {
    this.id = `assistant-${id}`;
    this.createdAt = parseDate(createdAtIso);
  }

  appendText(text: string) {
    if (!text) return;
    const last = this.parts[this.parts.length - 1];
    if (last && last.type === "text") {
      last.text += text;
    } else {
      this.parts.push({ type: "text", text });
    }
  }

  appendToolCall(tool: ToolCall) {
    // Forward the ACP tool title alongside the args so per-kind
    // renderers can show a descriptive label when raw_input is
    // empty (Claude's bash tool, for example, often emits an empty
    // raw_input on the initial tool_call frame). The `_aoe_title`
    // key is namespaced so it can't collide with real tool args.
    //
    // Also smuggle `_aoe_started_at` (the real ToolCall.started_at)
    // through assistant-ui's tool-call part shape; its primitive
    // doesn't carry timestamps and CockpitView's fallback would
    // otherwise mint one fresh per render, breaking the duration
    // label (#1060).
    let argsObj: Record<string, unknown> = {};
    try {
      const parsed = JSON.parse(tool.args_preview);
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        argsObj = parsed as Record<string, unknown>;
      }
    } catch {
      // args_preview wasn't a JSON object; keep argsObj empty.
    }
    if (tool.name) argsObj._aoe_title = tool.name;
    if (tool.started_at) argsObj._aoe_started_at = tool.started_at;
    this.parts.push({
      type: "tool-call",
      toolCallId: tool.id,
      toolName: tool.kind || "other",
      argsText: JSON.stringify(argsObj),
    });
  }

  completeToolCall(
    toolCallId: string,
    isError: boolean,
    resultText: string,
    endedAt: string,
  ) {
    for (const part of this.parts) {
      if (part.type === "tool-call" && part.toolCallId === toolCallId) {
        part.result = { content: resultText, endedAt };
        part.isError = isError || undefined;
        return;
      }
    }
  }

  build(): ThreadMessageLike {
    const grouped = collapseToolRuns(this.parts);
    return {
      id: this.id,
      role: "assistant",
      // Cast to bypass assistant-ui's strict ReadonlyJSONObject typing
      // for tool-call args. We don't carry parsed args through this
      // path; the renderer parses argsText itself; so the loose
      // shape is safe in practice.
      content: (grouped.length
        ? grouped
        : [{ type: "text", text: "" }]) as ThreadMessageLike["content"],
      createdAt: this.createdAt,
    };
  }
}

function isTodoWriteArgsText(argsText: string): boolean {
  try {
    const parsed = JSON.parse(argsText);
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      const title = (parsed as Record<string, unknown>)._aoe_title;
      if (typeof title === "string" && title.startsWith("Update TODOs")) {
        return true;
      }
      const todos = (parsed as Record<string, unknown>).todos;
      if (Array.isArray(todos)) return true;
    }
  } catch {
    // ignore
  }
  return false;
}

/** Minimum run length that triggers grouping. Two-in-a-row stays inline
 *  so a quick read-then-edit doesn't fold; three or more is the common
 *  "silent investigation" shape that benefits from one collapsible
 *  block (#1057). */
const TOOL_GROUP_MIN_RUN = 3;

/** Synthetic toolName used for the folded group card. Namespaced with
 *  the `_aoe_` prefix so it can't collide with a real ACP tool kind. */
export const TOOL_GROUP_NAME = "_aoe_tool_group";

/** Walk an assistant message's parts and collapse runs of consecutive
 *  tool-call parts (regardless of kind) into one synthetic group part
 *  when the run is ≥ TOOL_GROUP_MIN_RUN long. The grouping boundary is
 *  ANY non-tool-call part (text, callout, etc.); matching the "what
 *  did the agent do silently before its next sentence?" UX shape. The
 *  underlying tool-call data is preserved verbatim inside the group's
 *  argsText payload so the renderer can expand back to the original
 *  per-tool cards on click. */
function collapseToolRuns(parts: DraftPart[]): DraftPart[] {
  const out: DraftPart[] = [];
  let run: DraftPart[] = [];
  const flushRun = () => {
    if (run.length === 0) return;
    if (run.length < TOOL_GROUP_MIN_RUN) {
      for (const p of run) out.push(p);
    } else {
      // TodoWrite calls aren't silent tool work; they're status
      // updates the user wants to see one-by-one (#1064). Detect them
      // via the `_aoe_title` echo we stash in argsText (the adapter
      // names them "Update TODOs: …") and exempt the group entirely
      // when any child looks like a TodoWrite. Cheap: argsText is
      // already parsed JSON, just sniff the prefix.
      const hasTodoWrite = run.some(
        (p) => p.type === "tool-call" && isTodoWriteArgsText(p.argsText),
      );
      if (hasTodoWrite) {
        for (const p of run) out.push(p);
        run = [];
        return;
      }
      const childIds: string[] = [];
      const children: Array<{
        toolCallId: string;
        toolName: string;
        argsText: string;
        result?: { content: string };
        isError?: boolean;
      }> = [];
      for (const p of run) {
        if (p.type !== "tool-call") continue;
        childIds.push(p.toolCallId);
        children.push({
          toolCallId: p.toolCallId,
          toolName: p.toolName,
          argsText: p.argsText,
          result: p.result,
          isError: p.isError,
        });
      }
      out.push({
        type: "tool-call",
        toolCallId: `group-${childIds.join("-")}`,
        toolName: TOOL_GROUP_NAME,
        argsText: JSON.stringify({ children }),
      });
    }
    run = [];
  };
  for (const part of parts) {
    if (part.type === "tool-call") {
      run.push(part);
    } else {
      flushRun();
      out.push(part);
    }
  }
  flushRun();
  return out;
}
