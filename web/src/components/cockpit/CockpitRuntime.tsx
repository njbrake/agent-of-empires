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
 * own messages — they update the matching `tool-call` part in place
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
      // session/load failed and the agent fell back to session/new on
      // an `aoe serve` restart, so the model's context window is empty
      // even though our UI still replays the prior transcript. Render
      // as a dedicated assistant bubble so it doesn't run on from any
      // prior message, and use a blockquote with a ⚠️ prefix so the
      // custom Markdown blockquote component can style it as an amber
      // callout (see Markdown.tsx).
      flushAssistant();
      messages.push({
        id: `assistant-${row.id}`,
        role: "assistant",
        content: [
          {
            type: "text",
            text: `> ⚠️ **Conversation context reset** — ${row.text}`,
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
      );
    } else if (row.kind === "thinking") {
      // Thinking is rendered by the global rattle spinner, not the
      // message stream.
    } else if (row.kind === "empty_output") {
      // Synthesised when the agent finished a turn without emitting any
      // text or tool calls (e.g. interactive-only slash commands like
      // /usage, /status, /memory in claude-agent-acp — see upstream
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
      result?: { content: string };
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
    let argsObj: Record<string, unknown> = {};
    try {
      const parsed = JSON.parse(tool.args_preview);
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        argsObj = parsed as Record<string, unknown>;
      }
    } catch {
      // args_preview wasn't a JSON object — keep argsObj empty.
    }
    if (tool.name) argsObj._aoe_title = tool.name;
    this.parts.push({
      type: "tool-call",
      toolCallId: tool.id,
      toolName: tool.kind || "other",
      argsText: JSON.stringify(argsObj),
    });
  }

  completeToolCall(toolCallId: string, isError: boolean, resultText: string) {
    for (const part of this.parts) {
      if (part.type === "tool-call" && part.toolCallId === toolCallId) {
        part.result = { content: resultText };
        part.isError = isError || undefined;
        return;
      }
    }
  }

  build(): ThreadMessageLike {
    return {
      id: this.id,
      role: "assistant",
      // Cast to bypass assistant-ui's strict ReadonlyJSONObject typing
      // for tool-call args. We don't carry parsed args through this
      // path — the renderer parses argsText itself — so the loose
      // shape is safe in practice.
      content: (this.parts.length
        ? this.parts
        : [{ type: "text", text: "" }]) as ThreadMessageLike["content"],
      createdAt: this.createdAt,
    };
  }
}
