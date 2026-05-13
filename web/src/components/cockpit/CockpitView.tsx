// Cockpit conversation surface, built on @assistant-ui/react primitives.
//
// The chat shell (scroll viewport, message list, message editing, keyboard
// shortcuts, accessibility) is delegated to assistant-ui. We slot our own
// renderers into its component injection points:
//   - Markdown.tsx for text parts (with shiki code blocks)
//   - ToolCards.tsx for tool-call parts (per-kind dispatch)
//   - ApprovalCard for ACP permission requests (pinned below messages)
//   - WorkingSpinner with the empire-themed rattle
//
// State lives in `useCockpit` (subscribes to /sessions/:id/cockpit/ws)
// and reaches assistant-ui via `useExternalStoreRuntime` in
// CockpitRuntime.tsx. We never let assistant-ui own the chat state; it
// only renders what we feed it and surfaces user actions back.

import { useEffect, useState } from "react";
import {
  MessagePrimitive,
  ThreadPrimitive,
} from "@assistant-ui/react";
import { Check, ChevronDown, Clock, ListChecks, X } from "lucide-react";

import { ApprovalCard } from "./ApprovalCard";
import {
  CockpitRuntime,
  SUBAGENT_TASK_NAME,
  TOOL_GROUP_NAME,
  type CockpitContext,
} from "./CockpitRuntime";
import { Composer } from "./Composer";
import { Markdown } from "./Markdown";
import { SubagentCard, ToolCard, ToolGroupCard } from "./ToolCards";
import {
  SPINNER_FRAMES,
  SPINNER_INTERVAL_MS,
  VERB_INTERVAL_MS,
  chooseVerb,
} from "../../lib/cockpitRattle";
import type {
  Approval,
  ApprovalDecision,
  CockpitState,
  Plan,
  QueuedPrompt,
  ToolCall,
} from "../../lib/cockpitTypes";

interface Props {
  sessionId: string;
  /** Cockpit worker lifecycle pulled from `SessionResponse.cockpit_worker_state`
   *  (REST-poll-driven, ~3s cadence). Drives the `WorkerResumingBanner`
   *  while the reconciler is mid-spawn/attach. See #1088. */
  cockpitWorkerState: "absent" | "resuming" | "running";
}

const STARTER_PROMPTS = [
  "Explain this codebase",
  "Find recent changes worth reviewing",
  "What does the build pipeline do?",
];

export function CockpitView({ sessionId, cockpitWorkerState }: Props) {
  return (
    <CockpitRuntime
      sessionId={sessionId}
      cockpitWorkerState={cockpitWorkerState}
    >
      {(ctx) => (
        <CockpitChrome
          sessionId={sessionId}
          cockpitWorkerState={cockpitWorkerState}
          {...ctx}
        />
      )}
    </CockpitRuntime>
  );
}

function CockpitChrome({
  sessionId,
  cockpitWorkerState,
  state,
  status,
  resolveApproval,
  sendPrompt,
  dismissError,
  removeQueuedPrompt,
  editQueuedPrompt,
  clearQueue,
}: CockpitContext & {
  sessionId: string;
  cockpitWorkerState: "absent" | "resuming" | "running";
}) {
  return (
    <div className="flex h-full flex-col bg-surface-900 text-text-primary">
      <PlanStrip plan={state.plan} mode={state.mode} />

      {(status !== "open" || state.lagged || state.rateLimit) && (
        <SystemNotices
          status={status}
          lagged={state.lagged}
          rateLimit={state.rateLimit}
        />
      )}

      {state.startupError && (
        <StartupErrorBanner sessionId={sessionId} message={state.startupError} />
      )}
      {state.workerStopped && !state.startupError && (
        <WorkerStoppedBanner sessionId={sessionId} />
      )}
      {state.workerRestarting && !state.startupError && !state.workerStopped && (
        <WorkerRestartingBanner />
      )}
      {cockpitWorkerState === "resuming" &&
        !state.startupError &&
        !state.workerStopped &&
        !state.workerRestarting && <WorkerResumingBanner />}
      {state.lastError && (
        <InteractionErrorBanner
          message={state.lastError}
          onDismiss={dismissError}
        />
      )}

      <ThreadPrimitive.Root className="flex flex-1 flex-col min-h-0">
        <ThreadPrimitive.Viewport
          autoScroll
          className="flex-1 overflow-y-auto"
        >
          <div className="mx-auto max-w-3xl xl:max-w-4xl 2xl:max-w-5xl px-4 py-6">
            <ThreadPrimitive.Empty>
              <EmptyState onPick={sendPrompt} />
            </ThreadPrimitive.Empty>

            <ThreadPrimitive.Messages
              components={{
                UserMessage,
                AssistantMessage,
              }}
            />

            <ThreadPrimitive.If running>
              <div className="mt-3 ml-1">
                <WorkingSpinner
                  thinking={state.thinking}
                  tool={state.inFlightTool?.name ?? null}
                />
              </div>
            </ThreadPrimitive.If>

            {state.pendingApprovals.map((approval) => (
              <PendingApproval
                key={approval.nonce}
                approval={approval}
                onResolve={resolveApproval}
              />
            ))}
          </div>
        </ThreadPrimitive.Viewport>

        <QueuedPromptsStrip
          queued={state.queuedPrompts}
          onRemove={removeQueuedPrompt}
          onEdit={editQueuedPrompt}
          onClear={clearQueue}
        />

        <Composer
          sessionId={sessionId}
          availableModes={state.availableModes}
          currentModeId={state.currentModeId}
          legacyMode={state.mode}
          sessionUsage={state.sessionUsage}
          availableCommands={state.availableCommands}
          connected={status === "open" && !state.workerStopped && !state.workerRestarting}
          turnActive={state.turnActive}
          queuedCount={state.queuedPrompts.length}
          enqueuePrompt={sendPrompt}
        />
      </ThreadPrimitive.Root>
    </div>
  );
}

/* ── User & Assistant message templates ──────────────────────────── */

function UserMessage() {
  return (
    <MessagePrimitive.Root className="group mt-4 flex flex-col items-end gap-1">
      <div className="max-w-[80%] rounded-2xl rounded-br-sm border border-surface-700 bg-surface-800/70 px-3 py-1.5 text-sm whitespace-pre-wrap">
        <MessagePrimitive.Parts
          components={{
            Text: ({ text }) => <>{text}</>,
          }}
        />
      </div>
    </MessagePrimitive.Root>
  );
}

function AssistantMessage() {
  return (
    <MessagePrimitive.Root className="group mt-4 mr-auto w-full">
      <div className="text-sm text-text-primary leading-relaxed">
        <MessagePrimitive.Parts
          components={{
            Text: AssistantText,
            tools: {
              Override: AssistantToolCall,
            },
          }}
        />
      </div>
    </MessagePrimitive.Root>
  );
}

function AssistantText({ text }: { text: string }) {
  if (!text) return null;
  // MarkdownTextPrimitive (in Markdown.tsx) handles smooth
  // streaming via its built-in `smooth` prop, so we don't need the
  // hand-rolled char-budget reveal anymore.
  return <Markdown text={text} />;
}

// assistant-ui's tool-call props are typed as JSON-only; in our app the
// `result` payload is set in CockpitRuntime to `{ content: string }`,
// so we cast a narrow read of it here.
interface ToolCallProps {
  toolName: string;
  toolCallId: string;
  args?: Record<string, unknown>;
  argsText?: string;
  result?: unknown;
  isError?: boolean;
}

// Stable per-tool-call timestamp. assistant-ui doesn't carry the
// original started_at through (we only get the call id + name), so
// once we mint a date for a tool call we reuse it across renders
// rather than producing a fresh ISO string every time. Without this
// the ToolCard's `started_at` reference changes every render, which
// invalidates downstream memoization.
const TOOL_CALL_TIMES = new Map<string, string>();

function toolCallTimestamp(id: string): string {
  let t = TOOL_CALL_TIMES.get(id);
  if (t === undefined) {
    t = new Date().toISOString();
    TOOL_CALL_TIMES.set(id, t);
  }
  return t;
}

function AssistantToolCall(props: ToolCallProps) {
  // Synthetic group-of-tool-calls part. CockpitRuntime's build pass
  // folds runs of ≥3 consecutive tool-call parts (between agent text)
  // into one collapsible block (#1057). The children payload carries
  // the original per-tool parts verbatim so the group card can render
  // each one with its normal per-kind card on expand.
  if (props.toolName === TOOL_GROUP_NAME) {
    return <AssistantToolGroup argsText={props.argsText} />;
  }

  if (props.toolName === SUBAGENT_TASK_NAME) {
    return <AssistantSubagentTask argsText={props.argsText} />;
  }

  // Reconstruct the ToolCall shape our existing ToolCards.tsx
  // renderer expects. assistant-ui carries `toolName` (we set this to
  // ACP's lowercased ToolKind in CockpitRuntime) plus argsText (the
  // truncated JSON preview from the agent). The real `started_at` and
  // completion `endedAt` are smuggled through argsText/result by
  // CockpitRuntime's AssistantBuilder so the duration label (#1060)
  // reflects actual tool runtime instead of "time between renders".
  const fallbackAt = toolCallTimestamp(props.toolCallId);
  const startedAt = pickStartedAt(props.args, props.argsText) ?? fallbackAt;
  const endedAt = pickEndedAt(props.result) ?? fallbackAt;
  const tool: ToolCall = {
    id: props.toolCallId,
    name: prettifyToolName(props.toolName, props.args),
    kind: props.toolName,
    args_preview: props.argsText ?? safeStringify(props.args ?? null),
    started_at: startedAt,
  };
  const resultContent =
    props.result &&
    typeof props.result === "object" &&
    "content" in (props.result as Record<string, unknown>)
      ? String((props.result as { content?: unknown }).content ?? "")
      : "";
  const result =
    props.result !== undefined
      ? {
          id: `done-${props.toolCallId}`,
          kind: props.isError
            ? ("tool_error" as const)
            : ("tool_complete" as const),
          text: resultContent,
          toolCallId: props.toolCallId,
          at: endedAt,
        }
      : undefined;
  return <ToolCard tool={tool} result={result} />;
}

/** Read the real `_aoe_started_at` ISO timestamp out of the
 *  tool-call args. Returns null when neither the parsed `args` object
 *  nor the raw `argsText` carries it; caller falls back to a minted
 *  client time. */
function pickStartedAt(
  args: Record<string, unknown> | undefined,
  argsText: string | undefined,
): string | null {
  if (args && typeof args._aoe_started_at === "string") {
    return args._aoe_started_at;
  }
  if (argsText) {
    try {
      const parsed = JSON.parse(argsText);
      if (
        parsed &&
        typeof parsed === "object" &&
        !Array.isArray(parsed) &&
        typeof (parsed as Record<string, unknown>)._aoe_started_at === "string"
      ) {
        return (parsed as Record<string, string>)._aoe_started_at ?? null;
      }
    } catch {
      // ignore
    }
  }
  return null;
}

/** Read the smuggled `endedAt` field set by AssistantBuilder.completeToolCall. */
function pickEndedAt(result: unknown): string | null {
  if (
    result &&
    typeof result === "object" &&
    "endedAt" in (result as Record<string, unknown>)
  ) {
    const v = (result as { endedAt?: unknown }).endedAt;
    if (typeof v === "string") return v;
  }
  return null;
}

interface GroupChild {
  toolCallId: string;
  toolName: string;
  argsText: string;
  result?: { content: string; endedAt?: string };
  isError?: boolean;
}

function AssistantToolGroup({ argsText }: { argsText?: string }) {
  let children: GroupChild[] = [];
  if (argsText) {
    try {
      const parsed = JSON.parse(argsText);
      if (parsed && Array.isArray(parsed.children)) {
        children = parsed.children as GroupChild[];
      }
    } catch {
      // Malformed payload; fall through to an empty group rather than
      // crashing the assistant-ui render.
    }
  }
  const items = children.map((c) => {
    const fallbackAt = toolCallTimestamp(c.toolCallId);
    let parsedArgs: Record<string, unknown> = {};
    try {
      const p = JSON.parse(c.argsText);
      if (p && typeof p === "object" && !Array.isArray(p)) {
        parsedArgs = p as Record<string, unknown>;
      }
    } catch {
      // ignore
    }
    const startedAt = pickStartedAt(parsedArgs, c.argsText) ?? fallbackAt;
    const endedAt = pickEndedAt(c.result) ?? fallbackAt;
    const tool: ToolCall = {
      id: c.toolCallId,
      name: prettifyToolName(c.toolName, parsedArgs),
      kind: c.toolName,
      args_preview: c.argsText,
      started_at: startedAt,
    };
    const result =
      c.result !== undefined
        ? {
            id: `done-${c.toolCallId}`,
            kind: c.isError
              ? ("tool_error" as const)
              : ("tool_complete" as const),
            text: c.result.content,
            toolCallId: c.toolCallId,
            at: endedAt,
          }
        : undefined;
    return { tool, result, kind: c.toolName };
  });
  return <ToolGroupCard items={items} />;
}

interface SubagentPayload {
  parent: GroupChild;
  children: GroupChild[];
}

/** Reconstructs the parent Task tool plus its sub-agent children from
 *  the synthetic `_aoe_subagent_task` part CockpitRuntime emits, then
 *  hands them to SubagentCard. See #1041 layer B. */
function AssistantSubagentTask({ argsText }: { argsText?: string }) {
  let payload: SubagentPayload | null = null;
  if (argsText) {
    try {
      const parsed = JSON.parse(argsText);
      if (
        parsed &&
        typeof parsed === "object" &&
        parsed.parent &&
        Array.isArray(parsed.children)
      ) {
        payload = parsed as SubagentPayload;
      }
    } catch {
      // Malformed; render nothing rather than crashing.
    }
  }
  if (!payload) return null;

  const reconstruct = (c: GroupChild) => {
    const fallbackAt = toolCallTimestamp(c.toolCallId);
    let parsedArgs: Record<string, unknown> = {};
    try {
      const p = JSON.parse(c.argsText);
      if (p && typeof p === "object" && !Array.isArray(p)) {
        parsedArgs = p as Record<string, unknown>;
      }
    } catch {
      // ignore
    }
    const startedAt = pickStartedAt(parsedArgs, c.argsText) ?? fallbackAt;
    const endedAt = pickEndedAt(c.result) ?? fallbackAt;
    const tool: ToolCall = {
      id: c.toolCallId,
      name: prettifyToolName(c.toolName, parsedArgs),
      kind: c.toolName,
      args_preview: c.argsText,
      started_at: startedAt,
    };
    const result =
      c.result !== undefined
        ? {
            id: `done-${c.toolCallId}`,
            kind: c.isError
              ? ("tool_error" as const)
              : ("tool_complete" as const),
            text: c.result.content,
            toolCallId: c.toolCallId,
            at: endedAt,
          }
        : undefined;
    return { tool, result };
  };

  const parent = reconstruct(payload.parent);
  const children = payload.children.map(reconstruct);
  return (
    <SubagentCard
      tool={parent.tool}
      result={parent.result}
      children={children}
    />
  );
}

function prettifyToolName(
  kind: string,
  args?: Record<string, unknown>,
): string {
  // Pick a human-readable label for the tool card header. Prefer the
  // ACP title we forward via _aoe_title, then any well-known input
  // field, then the bare kind.
  if (args) {
    for (const key of [
      "_aoe_title",
      "path",
      "file_path",
      "filePath",
      "command",
      "cmd",
      "query",
      "url",
    ]) {
      const v = (args as Record<string, unknown>)[key];
      if (typeof v === "string" && v.length > 0) {
        return v;
      }
    }
  }
  return kind || "tool";
}

function safeStringify(v: unknown): string {
  try {
    return JSON.stringify(v ?? null);
  } catch {
    return "";
  }
}

/* ── Empty state ─────────────────────────────────────────────────── */

function EmptyState({
  onPick,
}: {
  onPick: (text: string) => Promise<void>;
}) {
  return (
    <div className="mt-12 flex flex-col items-center gap-4 text-center">
      <div className="text-sm text-text-muted">
        Ask the agent anything about this workspace.
      </div>
      <div className="flex flex-wrap justify-center gap-2">
        {STARTER_PROMPTS.map((p) => (
          <button
            key={p}
            type="button"
            onClick={() => void onPick(p)}
            className="rounded-full border border-surface-700 bg-surface-800/60 px-3 py-1 text-xs text-text-secondary hover:border-brand-600/60 hover:bg-surface-800 hover:text-text-primary"
          >
            {p}
          </button>
        ))}
      </div>
    </div>
  );
}

/* ── Working spinner (rattle) ────────────────────────────────────── */

function WorkingSpinner({
  thinking,
  tool,
}: {
  thinking: boolean;
  tool: string | null;
}) {
  const [frame, setFrame] = useState(0);
  const [seed, setSeed] = useState(() => Math.floor(Math.random() * 0xffffffff));

  useEffect(() => {
    const t = window.setInterval(() => {
      setFrame((f) => (f + 1) % SPINNER_FRAMES.length);
    }, SPINNER_INTERVAL_MS);
    return () => window.clearInterval(t);
  }, []);

  useEffect(() => {
    const t = window.setInterval(() => {
      setSeed((s) => (s + 0x9e3779b9) | 0);
    }, VERB_INTERVAL_MS);
    return () => window.clearInterval(t);
  }, []);

  const state: "thinking" | "tool" | "working" = thinking
    ? "thinking"
    : tool
      ? "tool"
      : "working";
  const label = chooseVerb(state, seed, tool);

  return (
    <div className="flex items-center gap-2 text-sm italic text-text-muted">
      <span
        className="inline-block w-3 text-center font-mono text-brand-500"
        aria-hidden="true"
      >
        {SPINNER_FRAMES[frame]}
      </span>
      <span>{label}</span>
    </div>
  );
}

/* ── Plan strip ──────────────────────────────────────────────────── */

interface PlanStripProps {
  plan: Plan | null;
  mode: CockpitState["mode"];
}

function PlanStrip({ plan, mode }: PlanStripProps) {
  const [expanded, setExpanded] = useState(false);
  // Hide entirely on the most common case: no plan, default mode.
  // The mode picker now lives in the composer footer.
  if (!plan && mode === "Default") return null;

  // Pick the active step: prefer an explicit `InProgress` (Claude's
  // ExitPlanMode bridge sets this), otherwise fall back to the first
  // non-Done / non-Cancelled step (TodoWrite-produced plans typically
  // arrive with all entries Pending). Mirrors the server-side
  // `plan_summary_from_plan` logic so the strip and sidebar agree.
  const current =
    plan?.steps.find((s) => s.status === "InProgress") ??
    plan?.steps.find(
      (s) => s.status !== "Done" && s.status !== "Cancelled",
    );
  const completed = plan?.steps.filter((s) => s.status === "Done").length ?? 0;
  const totalSteps = plan?.steps.length ?? 0;
  const pct = totalSteps > 0 ? Math.round((completed / totalSteps) * 100) : 0;
  const allDone = totalSteps > 0 && completed === totalSteps;

  return (
    <div className="border-b border-surface-800 bg-surface-900/95 backdrop-blur">
      <button
        type="button"
        className="flex w-full items-center gap-3 px-4 py-2 text-left text-sm hover:bg-surface-800/40"
        onClick={() => setExpanded((v) => !v)}
      >
        <ListChecks className="h-3.5 w-3.5 shrink-0 text-text-dim" />
        <span className="truncate text-text-primary">
          {current?.title ?? (allDone ? "all steps complete" : "…")}
        </span>
        {plan && (
          <span className="ml-auto flex items-center gap-2">
            <span className="text-[11px] tabular-nums text-text-dim">
              {completed}/{totalSteps}
            </span>
            <span className="hidden sm:block h-1 w-16 overflow-hidden rounded-full bg-surface-800">
              <span
                className="block h-full bg-brand-500 transition-[width] duration-300"
                style={{ width: `${pct}%` }}
              />
            </span>
            <ChevronDown
              className={[
                "h-3.5 w-3.5 text-text-dim transition-transform",
                expanded ? "rotate-180" : "",
              ].join(" ")}
            />
          </span>
        )}
      </button>

      {expanded && plan && (
        <div className="max-h-64 overflow-y-auto border-t border-surface-800 px-4 py-2 text-sm">
          <ul className="space-y-1">
            {plan.steps.map((step) => (
              <li key={step.id} className="flex items-start gap-2 text-text-secondary">
                <StepGlyph status={step.status} />
                <span
                  className={
                    step.status === "Done"
                      ? "text-text-dim line-through"
                      : step.status === "InProgress"
                        ? "text-text-primary font-medium"
                        : "text-text-secondary"
                  }
                >
                  {step.title}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}

function StepGlyph({ status }: { status: Plan["steps"][number]["status"] }) {
  switch (status) {
    case "Done":
      return <span className="text-status-running">✓</span>;
    case "InProgress":
      return <span className="text-brand-500">●</span>;
    case "Cancelled":
      return <span className="text-text-dim">⊘</span>;
    case "Pending":
    default:
      return <span className="text-text-dim">○</span>;
  }
}


/* ── Approvals ───────────────────────────────────────────────────── */

function PendingApproval({
  approval,
  onResolve,
}: {
  approval: Approval;
  onResolve: (nonce: string, decision: ApprovalDecision) => Promise<void>;
}) {
  // ApprovalCard owns its own chrome (matches the tool-card style).
  return (
    <ApprovalCard
      approval={approval}
      onResolve={(decision) => onResolve(approval.nonce, decision)}
    />
  );
}

/* ── System notices ──────────────────────────────────────────────── */

function SystemNotices({
  status,
  lagged,
  rateLimit,
}: {
  status: CockpitContext["status"];
  lagged: boolean;
  rateLimit: CockpitState["rateLimit"];
}) {
  const messages: { kind: string; text: string }[] = [];
  if (status === "connecting") {
    messages.push({ kind: "info", text: "Connecting to cockpit…" });
  }
  if (status === "error") {
    messages.push({
      kind: "warn",
      text: "Cockpit reconnecting… showing cached transcript; new messages disabled.",
    });
  }
  if (status === "closed") {
    messages.push({
      kind: "warn",
      text: "Cockpit disconnected. Showing cached transcript; new messages disabled.",
    });
  }
  if (lagged) {
    messages.push({ kind: "warn", text: "Some events were missed during reconnect." });
  }
  if (rateLimit) {
    const reset = new Date(rateLimit.resets_at).toLocaleTimeString();
    messages.push({
      kind: "warn",
      text: `Rate-limited (${rateLimit.kind}); resets at ${reset}.`,
    });
  }
  if (messages.length === 0) return null;
  return (
    <div className="border-b border-surface-800 px-4 py-2 space-y-1">
      {messages.map((m, i) => (
        <div
          key={i}
          className={`text-xs ${m.kind === "warn" ? "text-brand-400" : "text-text-muted"}`}
        >
          {m.text}
        </div>
      ))}
    </div>
  );
}

function InteractionErrorBanner({
  message,
  onDismiss,
}: {
  message: string;
  onDismiss: () => void;
}) {
  return (
    <div className="flex items-start justify-between gap-3 border-b border-amber-900/60 bg-amber-950/40 px-4 py-2 text-amber-200">
      <div className="flex-1 min-w-0">
        <div className="text-xs font-medium">Action did not complete</div>
        <div className="mt-0.5 text-xs text-amber-100/90 break-words">{message}</div>
      </div>
      <button
        type="button"
        onClick={onDismiss}
        className="shrink-0 rounded-md border border-amber-800/60 bg-amber-900/40 px-2 py-1 text-[10px] font-mono uppercase tracking-wide text-amber-100 hover:bg-amber-900/60"
      >
        Dismiss
      </button>
    </div>
  );
}

function WorkerRestartingBanner() {
  // `aoe cockpit restart` deletes the registry + writes a sentinel; the
  // daemon's reaper publishes Stopped{reason:"restart_pending"} and the
  // reconciler clears its `attempted` set so the next 2s tick spawns a
  // fresh worker (with the cached acp_session_id for transcript
  // continuity). AcpSessionAssigned then clears `workerRestarting` and
  // this banner unmounts. No reconnect button because the daemon is
  // already handling it.
  return (
    <div className="flex items-center gap-2 border-b border-sky-900/60 bg-sky-950/40 px-4 py-2 text-xs text-sky-200">
      <span
        className="inline-block h-2 w-2 animate-pulse rounded-full bg-sky-400"
        aria-hidden
      />
      <span>
        Restarting cockpit worker… the daemon will respawn the agent with
        your existing transcript shortly.
      </span>
    </div>
  );
}

function WorkerResumingBanner() {
  // Shown while `SessionResponse.cockpit_worker_state === "resuming"`:
  // the reconciler is mid-spawn or mid-attach. The cached transcript
  // stays scrollable and the composer keeps queuing prompts; the banner
  // clears as soon as the next session-list poll sees the worker in
  // `running` state (typically within a few hundred ms of completion).
  // See #1088.
  return (
    <div className="flex items-center gap-2 border-b border-amber-900/60 bg-amber-950/40 px-4 py-2 text-xs text-amber-200">
      <span
        className="inline-block h-2 w-2 animate-pulse rounded-full bg-amber-400"
        aria-hidden
      />
      <span>
        Resuming cockpit worker… cached transcript still available. Queued
        prompts will send once the agent is back online.
      </span>
    </div>
  );
}

function WorkerStoppedBanner({ sessionId }: { sessionId: string }) {
  const [retryState, setRetryState] = useState<
    "idle" | "retrying" | "ok" | "failed"
  >("idle");
  const [retryError, setRetryError] = useState<string | null>(null);

  const handleReconnect = async () => {
    setRetryState("retrying");
    setRetryError(null);
    try {
      const res = await fetch(
        `/api/sessions/${encodeURIComponent(sessionId)}/cockpit/spawn`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({}),
        },
      );
      if (res.ok) {
        // The next AcpSessionAssigned (or UserPromptSent) clears
        // workerStopped on the reducer side and this banner unmounts.
        setRetryState("ok");
      } else {
        const detail = (await res.text().catch(() => "")).slice(0, 200);
        setRetryState("failed");
        setRetryError(`Server returned ${res.status}. ${detail}`.trim());
      }
    } catch (e) {
      setRetryState("failed");
      setRetryError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="border-b border-amber-900/60 bg-amber-950/40 px-4 py-3 text-amber-200">
      <div className="flex items-start justify-between gap-3">
        <div className="flex-1 min-w-0">
          <div className="text-sm font-medium">Cockpit worker stopped</div>
          <div className="mt-1 text-xs text-amber-100/90">
            The agent was terminated via{" "}
            <code className="rounded bg-amber-900/60 px-1">aoe cockpit stop</code>{" "}
            or an equivalent external teardown. New prompts are disabled until
            you reconnect.
          </div>
        </div>
        <button
          type="button"
          onClick={handleReconnect}
          disabled={retryState === "retrying"}
          className="shrink-0 rounded-md border border-amber-800/60 bg-amber-900/40 px-3 py-1 text-xs font-medium text-amber-100 hover:bg-amber-900/60 disabled:cursor-not-allowed disabled:opacity-60"
        >
          {retryState === "retrying" ? "Reconnecting…" : "Reconnect"}
        </button>
      </div>
      {retryState === "ok" && (
        <div className="mt-2 text-xs text-emerald-200/90">
          Spawn requested. The composer will re-enable when the agent is back
          online.
        </div>
      )}
      {retryState === "failed" && retryError && (
        <div className="mt-2 text-xs text-amber-100/90">
          Reconnect failed: {retryError}
        </div>
      )}
    </div>
  );
}

function StartupErrorBanner({
  sessionId,
  message,
}: {
  sessionId: string;
  message: string;
}) {
  const isAuth = /authentic|login|api[_ -]?key/i.test(message);
  const isCapacity = /capacity full|max_concurrent_workers/i.test(message);
  // Match the exact `Display` of `AcpError::ProjectPathMissing`.
  // Capture the path so the banner can echo it back to the user; the
  // path lets them spot whether a rename or a delete is the cause and
  // jump straight to the right fix. See #1089.
  const projectPathMissingMatch = /project path no longer exists:\s*(\S.*)$/im.exec(
    message,
  );
  const isProjectPathMissing = projectPathMissingMatch !== null;
  const missingPath = projectPathMissingMatch?.[1]?.trim() ?? null;
  const [retryState, setRetryState] = useState<
    "idle" | "retrying" | "ok" | "failed"
  >("idle");
  const [retryError, setRetryError] = useState<string | null>(null);

  const handleRetry = async () => {
    setRetryState("retrying");
    setRetryError(null);
    try {
      const res = await fetch(
        `/api/sessions/${encodeURIComponent(sessionId)}/cockpit/spawn`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({}),
        },
      );
      if (res.ok) {
        // The supervisor's drain task will start emitting events
        // shortly; the banner will disappear when the next user
        // prompt clears `startupError`.
        setRetryState("ok");
      } else {
        const detail = (await res.text().catch(() => "")).slice(0, 200);
        setRetryState("failed");
        setRetryError(`Server returned ${res.status}. ${detail}`.trim());
      }
    } catch (e) {
      setRetryState("failed");
      setRetryError(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <div className="border-b border-rose-900/60 bg-rose-950/40 px-4 py-3 text-rose-200">
      <div className="flex items-start justify-between gap-3">
        <div className="flex-1 min-w-0">
          <div className="text-sm font-medium">Cockpit agent failed to start</div>
          <pre className="mt-1 whitespace-pre-wrap text-xs text-rose-100/90">
            {message}
          </pre>
        </div>
        <button
          type="button"
          onClick={handleRetry}
          disabled={retryState === "retrying"}
          className="shrink-0 rounded-md border border-rose-800/60 bg-rose-900/40 px-3 py-1 text-xs font-medium text-rose-100 hover:bg-rose-900/60 disabled:cursor-not-allowed disabled:opacity-60"
        >
          {retryState === "retrying" ? "Retrying…" : "Retry"}
        </button>
      </div>
      {retryState === "ok" && (
        <div className="mt-2 text-xs text-emerald-200/90">
          Spawn requested. New events should start streaming in shortly.
        </div>
      )}
      {retryState === "failed" && retryError && (
        <div className="mt-2 text-xs text-rose-100/90">
          Retry failed: {retryError}
        </div>
      )}
      <div className="mt-2 text-xs text-rose-200/80">
        {isAuth ? (
          <>
            The adapter is installed but has no Claude credentials. Either set{" "}
            <code className="rounded bg-rose-900/60 px-1">ANTHROPIC_API_KEY</code>{" "}
            in the env that runs <code className="rounded bg-rose-900/60 px-1">aoe serve</code>,
            or run <code className="rounded bg-rose-900/60 px-1">claude /login</code>{" "}
            in a terminal to write credentials to{" "}
            <code className="rounded bg-rose-900/60 px-1">~/.claude</code>,
            then restart aoe.
          </>
        ) : isCapacity ? (
          <>
            All cockpit worker slots are in use. Either raise{" "}
            <code className="rounded bg-rose-900/60 px-1">[cockpit] max_concurrent_workers</code>{" "}
            in <code className="rounded bg-rose-900/60 px-1">config.toml</code>{" "}
            and restart <code className="rounded bg-rose-900/60 px-1">aoe serve</code>,
            or free a slot by deleting an existing cockpit session
            or switching one to the tmux substrate. Reinstalling the adapter
            won't help; the adapter is fine, the cap is the limit.
          </>
        ) : isProjectPathMissing ? (
          <>
            The session's working directory no longer exists on disk:
            {missingPath && (
              <pre className="mt-1 whitespace-pre-wrap break-all rounded bg-rose-900/40 p-2 text-xs">
                {missingPath}
              </pre>
            )}
            Reinstalling the adapter won't help; the adapter is fine, the cwd
            is gone. Two paths forward:
            <ol className="mt-1 list-decimal space-y-0.5 pl-5">
              <li>
                Restore the directory at the path above (e.g.{" "}
                <code className="rounded bg-rose-900/60 px-1">git worktree move</code>{" "}
                it back, or recreate it), then click <strong>Retry</strong>.
              </li>
              <li>
                Stop <code className="rounded bg-rose-900/60 px-1">aoe serve</code>,
                edit{" "}
                <code className="rounded bg-rose-900/60 px-1">project_path</code>{" "}
                for this session in{" "}
                <code className="rounded bg-rose-900/60 px-1">
                  ~/.agent-of-empires/profiles/&lt;profile&gt;/sessions.json
                </code>
                {" "}to point at the new location, then start{" "}
                <code className="rounded bg-rose-900/60 px-1">aoe serve</code>{" "}
                again.
              </li>
            </ol>
          </>
        ) : (
          <>
            Run <code className="rounded bg-rose-900/60 px-1">aoe cockpit doctor --fix</code>{" "}
            from a terminal, or install the adapter manually:
            <pre className="mt-1 whitespace-pre-wrap rounded bg-rose-900/40 p-2 text-xs">
              npm install -g @agentclientprotocol/claude-agent-acp
            </pre>
          </>
        )}
      </div>
    </div>
  );
}

/* ── Queued prompts strip ─────────────────────────────────────────── */

interface QueuedPromptsStripProps {
  queued: QueuedPrompt[];
  onRemove: (id: string) => void;
  onEdit: (id: string, text: string) => void;
  onClear: () => void;
}

/** Strip rendered above the composer listing prompts the user has
 *  queued mid-turn. Each row is editable in place (click to edit, save
 *  on Enter or blur, cancel on Escape) and removable via the X button.
 *  Hidden when the queue is empty. See #1031. */
function QueuedPromptsStrip({
  queued,
  onRemove,
  onEdit,
  onClear,
}: QueuedPromptsStripProps) {
  if (queued.length === 0) return null;
  return (
    <div className="border-t border-surface-800 bg-surface-900/60 px-4 py-2">
      <div className="mx-auto max-w-3xl xl:max-w-4xl 2xl:max-w-5xl">
        <div className="flex items-center justify-between pb-1.5 text-[11px] uppercase tracking-wider text-text-dim">
          <span className="inline-flex items-center gap-1">
            <Clock className="h-3 w-3" />
            Queued ({queued.length})
          </span>
          {queued.length > 1 && (
            <button
              type="button"
              onClick={onClear}
              className="text-text-dim hover:text-rose-300 transition-colors"
            >
              Clear all
            </button>
          )}
        </div>
        <ul className="flex flex-col gap-1.5">
          {queued.map((q) => (
            <QueuedPromptRow
              key={q.id}
              prompt={q}
              onRemove={() => onRemove(q.id)}
              onEdit={(text) => onEdit(q.id, text)}
            />
          ))}
        </ul>
      </div>
    </div>
  );
}

function QueuedPromptRow({
  prompt,
  onRemove,
  onEdit,
}: {
  prompt: QueuedPrompt;
  onRemove: () => void;
  onEdit: (text: string) => void;
}) {
  // Editor state co-mounts with the textarea: when `editing` flips on
  // we re-key <QueuedPromptEditor> so it initialises `draft` from the
  // current prompt.text. This avoids a setState-in-effect to keep the
  // draft synced with external edits (lint: react-hooks/set-state-in-effect).
  const [editing, setEditing] = useState(false);

  return (
    <li className="group flex items-start gap-2 rounded-lg border border-amber-700/30 bg-amber-950/15 px-2.5 py-1.5">
      <span className="mt-0.5 inline-flex h-4 w-4 shrink-0 items-center justify-center rounded-full bg-amber-500/20 text-[10px] font-semibold text-amber-300">
        ⏱
      </span>
      {editing ? (
        <QueuedPromptEditor
          key={prompt.id}
          initial={prompt.text}
          onCancel={() => setEditing(false)}
          onSave={(text) => {
            const trimmed = text.trim();
            if (trimmed && trimmed !== prompt.text) onEdit(trimmed);
            setEditing(false);
          }}
        />
      ) : (
        <button
          type="button"
          onClick={() => setEditing(true)}
          title="Click to edit"
          className="min-w-0 flex-1 text-left text-xs leading-5 text-text-secondary whitespace-pre-wrap break-words hover:text-text-primary"
        >
          {prompt.text}
        </button>
      )}
      <button
        type="button"
        onClick={onRemove}
        title="Drop this queued message"
        className="shrink-0 rounded p-1 text-text-dim hover:bg-surface-800 hover:text-rose-300"
      >
        <X className="h-3.5 w-3.5" />
      </button>
    </li>
  );
}

function QueuedPromptEditor({
  initial,
  onCancel,
  onSave,
}: {
  initial: string;
  onCancel: () => void;
  onSave: (text: string) => void;
}) {
  const [draft, setDraft] = useState(initial);
  return (
    <>
      <textarea
        autoFocus
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => onSave(draft)}
        onKeyDown={(e) => {
          if (e.key === "Enter" && !e.shiftKey) {
            e.preventDefault();
            onSave(draft);
          } else if (e.key === "Escape") {
            e.preventDefault();
            onCancel();
          }
        }}
        rows={Math.min(6, Math.max(1, draft.split("\n").length))}
        className={[
          "min-w-0 flex-1 resize-none bg-transparent text-xs leading-5",
          "text-text-primary outline-none placeholder:text-text-dim",
        ].join(" ")}
      />
      <button
        type="button"
        onMouseDown={(e) => e.preventDefault()}
        onClick={() => onSave(draft)}
        title="Save (Enter)"
        className="shrink-0 rounded p-1 text-text-dim hover:bg-surface-800 hover:text-emerald-300"
      >
        <Check className="h-3.5 w-3.5" />
      </button>
    </>
  );
}
