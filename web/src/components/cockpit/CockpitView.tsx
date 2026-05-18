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

import { useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  MessagePrimitive,
  ThreadPrimitive,
  useMessage,
} from "@assistant-ui/react";
import {
  AlertTriangle,
  Check,
  ChevronDown,
  Clock,
  ListChecks,
  RotateCcw,
  X,
} from "lucide-react";

import { ApprovalCard } from "./ApprovalCard";
import {
  CockpitRuntime,
  SUBAGENT_TASK_NAME,
  TOOL_GROUP_NAME,
  type CockpitContext,
} from "./CockpitRuntime";
import { Composer } from "./Composer";
import { ContextPrimerBanner } from "./ContextPrimerBanner";
import { Markdown } from "./Markdown";
import { SubagentCard, ToolCard, ToolGroupCard } from "./ToolCards";
import { DiffCommentsUserCard } from "../diff/comments/DiffCommentsUserCard";
import { parseDiffCommentsSentinel } from "../diff/comments/buildPrompt";
import {
  SPINNER_FRAMES,
  SPINNER_INTERVAL_MS,
  VERB_INTERVAL_MS,
  chooseVerb,
} from "../../lib/cockpitRattle";
import { useCockpitPrefs } from "../../lib/cockpitPrefs";
import { useApprovalSound } from "../../hooks/useApprovalSound";
import type {
  Approval,
  ApprovalDecision,
  CockpitState,
  Plan,
  QueuedPrompt,
  RejectedPrompt,
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
  // Folds rows above the most recent `/clear` divider out of the
  // thread by default; the disclosure banner toggles this. Lives on
  // the view (not the reducer) because it's a UI preference, not
  // event-log state. See #1101.
  const [showClearedTurns, setShowClearedTurns] = useState(false);
  return (
    <CockpitRuntime
      sessionId={sessionId}
      cockpitWorkerState={cockpitWorkerState}
      showClearedTurns={showClearedTurns}
    >
      {(ctx) => (
        <CockpitChrome
          sessionId={sessionId}
          cockpitWorkerState={cockpitWorkerState}
          showClearedTurns={showClearedTurns}
          onToggleClearedTurns={() => setShowClearedTurns((v) => !v)}
          {...ctx}
        />
      )}
    </CockpitRuntime>
  );
}

function CockpitChrome({
  sessionId,
  cockpitWorkerState,
  showClearedTurns,
  onToggleClearedTurns,
  state,
  status,
  hasEverOpened,
  reconnecting,
  retryCount,
  retryCountdown,
  maxRetries,
  manualReconnect,
  resolveApproval,
  sendPrompt,
  forceEndTurn,
  lastActivityRef,
  dismissError,
  dismissPrimer,
  removeQueuedPrompt,
  editQueuedPrompt,
  clearQueue,
  dismissRejectedPrompt,
}: CockpitContext & {
  sessionId: string;
  cockpitWorkerState: "absent" | "resuming" | "running";
  showClearedTurns: boolean;
  onToggleClearedTurns: () => void;
}) {
  // Count how many activity rows precede the latest `session_cleared`
  // divider so the banner can say "12 earlier turns hidden". The
  // reducer always appends the divider as the last row at clear time,
  // so the count is `lastClearIndex` (rows before it are the cleared
  // history). See #1101.
  const clearedSummary = (() => {
    let lastClearIndex = -1;
    for (let i = state.activity.length - 1; i >= 0; i -= 1) {
      if (state.activity[i]!.kind === "session_cleared") {
        lastClearIndex = i;
        break;
      }
    }
    if (lastClearIndex < 0) return null;
    return { hiddenCount: lastClearIndex };
  })();
  // Composer prefill keyed for re-fires; set by the
  // ContextPrimerBanner on click. Local rather than on CockpitState
  // because it's a one-shot UI action, not part of the event log.
  const [primerPrefill, setPrimerPrefill] = useState<
    { id: string; text: string } | null
  >(null);

  // Browser-side approval chime. Fires once on the 0 -> >=1 edge of
  // pendingApprovals; complements the OS push (delivered via the SW
  // when the dashboard is backgrounded) and the in-app toast (when
  // foregrounded). See #1038.
  useApprovalSound(state.pendingApprovals.length);

  // Re-pin the chat viewport to the bottom when the composer (or any
  // sibling below it: queued strip, primer banner) grows. assistant-ui's
  // `autoScroll` only re-pins on message updates, not on viewport
  // height shrinks; without this, typing multi-line prompts slides the
  // visible bottom of the chat up by the height the composer just
  // grew. See #1104.
  //
  // We sample "is the viewport pinned to the bottom?" on every scroll
  // event into a ref. By the time the ResizeObserver fires the layout
  // has already settled at the smaller viewport height, so reading
  // pinned-ness after the fact would always be false for the very
  // case we want to catch (composer grew, scroll content now overflows
  // the shrunk viewport by exactly the grow amount). The scroll-time
  // sample captures the pre-resize state; the RO callback consumes it.
  const viewportRef = useRef<HTMLDivElement | null>(null);
  const belowViewportRef = useRef<HTMLDivElement | null>(null);
  const wasAtBottomRef = useRef<boolean>(true);
  useLayoutEffect(() => {
    const vp = viewportRef.current;
    const below = belowViewportRef.current;
    if (!vp || !below) return;
    // Treat "within 16px of the bottom" as pinned. assistant-ui's
    // own stick-to-bottom uses a similar slop; sub-pixel rounding
    // and momentary content reflows otherwise drop us out of the
    // pinned state for one frame.
    const sampleAtBottom = () => {
      wasAtBottomRef.current =
        vp.scrollTop + vp.clientHeight >= vp.scrollHeight - 16;
    };
    sampleAtBottom();
    vp.addEventListener("scroll", sampleAtBottom, { passive: true });
    let prevHeight = below.offsetHeight;
    const ro = new ResizeObserver(() => {
      const nextHeight = below.offsetHeight;
      if (nextHeight === prevHeight) return;
      prevHeight = nextHeight;
      if (wasAtBottomRef.current) {
        vp.scrollTop = vp.scrollHeight;
      }
    });
    ro.observe(below);
    return () => {
      ro.disconnect();
      vp.removeEventListener("scroll", sampleAtBottom);
    };
  }, []);
  return (
    <div className="flex h-full flex-col bg-surface-900 text-text-primary">
      <PlanStrip plan={state.plan} />

      {(status !== "open" || state.lagged || state.rateLimit || reconnecting) && (
        <SystemNotices
          status={status}
          lagged={state.lagged}
          rateLimit={state.rateLimit}
          hasEverOpened={hasEverOpened}
          reconnecting={reconnecting}
          retryCount={retryCount}
          retryCountdown={retryCountdown}
          maxRetries={maxRetries}
          manualReconnect={manualReconnect}
        />
      )}

      {state.startupError && (
        <StartupErrorBanner sessionId={sessionId} message={state.startupError} />
      )}
      {state.workerStopped && !state.startupError && (
        <WorkerStoppedBanner sessionId={sessionId} />
      )}
      {state.workerRestarting && !state.startupError && !state.workerStopped && (
        <WorkerRestartingBanner agentUnresponsive={state.agentUnresponsive} />
      )}
      {cockpitWorkerState === "resuming" &&
        !state.startupError &&
        !state.workerStopped &&
        !state.workerRestarting &&
        (state.lastSeq === 0 ? <SpawningBanner /> : <WorkerResumingBanner />)}
      {state.nextWakeupAt &&
        !state.turnActive &&
        !state.startupError &&
        !state.workerStopped &&
        !state.workerRestarting && (
          <ScheduledWakeupBanner
            wakeAt={state.nextWakeupAt}
            reason={state.nextWakeupReason}
          />
        )}
      {state.lastError && (
        <InteractionErrorBanner
          message={state.lastError}
          onDismiss={dismissError}
        />
      )}

      <ThreadPrimitive.Root className="flex flex-1 flex-col min-h-0">
        <ThreadPrimitive.Viewport
          autoScroll
          ref={viewportRef}
          className="flex-1 overflow-y-auto"
        >
          <div className="mx-auto max-w-3xl xl:max-w-4xl 2xl:max-w-5xl px-4 py-6">
            <ThreadPrimitive.Empty>
              <EmptyState onPick={sendPrompt} />
            </ThreadPrimitive.Empty>

            {clearedSummary && clearedSummary.hiddenCount > 0 && (
              <ClearedTurnsBanner
                hiddenCount={clearedSummary.hiddenCount}
                expanded={showClearedTurns}
                onToggle={onToggleClearedTurns}
              />
            )}

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
                  lastActivityRef={lastActivityRef}
                  onForceEndTurn={forceEndTurn}
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

        <div ref={belowViewportRef}>
          <QueuedPromptsStrip
            queued={state.queuedPrompts}
            onRemove={removeQueuedPrompt}
            onEdit={editQueuedPrompt}
            onClear={clearQueue}
          />

          <RejectedPromptsStrip
            rejected={state.rejectedPrompts}
            onRetry={sendPrompt}
            onDismiss={dismissRejectedPrompt}
            disabled={
              state.workerRestarting ||
              state.workerStopped ||
              Boolean(state.startupError)
            }
          />

          <ContextPrimerBanner
            sessionId={sessionId}
            available={state.contextPrimerAvailable}
            onInsertPrimer={(text) =>
              setPrimerPrefill({
                id: `primer-${state.contextPrimerAvailable?.resetSeq ?? 0}-${Date.now()}`,
                text,
              })
            }
            onDismiss={dismissPrimer}
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
            primerPrefill={primerPrefill}
          />
        </div>
      </ThreadPrimitive.Root>
    </div>
  );
}

/* ── User & Assistant message templates ──────────────────────────── */

function UserMessage() {
  return (
    <MessagePrimitive.Root className="group mt-4 flex flex-col items-end gap-1">
      <MessagePrimitive.Parts
        components={{
          Text: UserText,
        }}
      />
    </MessagePrimitive.Root>
  );
}

/** Text-part renderer for user messages. Detects the diff-comments
 *  sentinel header (prepended by `buildFullPrompt`) and swaps in the
 *  structured `DiffCommentsUserCard`; falls back to the classic chat
 *  bubble otherwise. */
function UserText({ text }: { text: string }) {
  const payload = parseDiffCommentsSentinel(text);
  if (payload) {
    return <DiffCommentsUserCard payload={payload} />;
  }
  // User prompts get the same markdown pipeline as agent messages so
  // fenced code blocks render with syntax highlighting instead of
  // literal backticks. Smooth-reveal is off because user prompts arrive
  // complete; the pacing only matters for streamed agent tokens. See #1108.
  return (
    <div className="max-w-[80%] min-w-0 rounded-2xl rounded-br-sm border border-surface-700 bg-surface-800/70 px-3 py-1.5 text-sm">
      <Markdown text={text} smooth={false} />
    </div>
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
  // Smooth-reveal only the live streaming tail: an assistant message
  // whose runtime status is `running` is the one the agent is
  // actively chunking text into. Historical messages (loaded from
  // the localStorage cache on reload, or replayed from the server on
  // session switch) render with the Markdown default `smooth={false}`
  // so the user doesn't watch the entire transcript type itself out
  // again. See #1132.
  const isRunning = useMessage((m) => m.status?.type === "running");
  if (!text) return null;
  return <Markdown text={text} smooth={isRunning} />;
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

/* ── Cleared turns disclosure ────────────────────────────────────── */

function ClearedTurnsBanner({
  hiddenCount,
  expanded,
  onToggle,
}: {
  hiddenCount: number;
  expanded: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onToggle}
      className="mb-4 w-full flex items-center gap-2 px-3 py-2 rounded-md border border-surface-700 bg-surface-800 text-text-secondary hover:bg-surface-700 hover:text-text-primary cursor-pointer text-sm"
      aria-expanded={expanded}
    >
      <ChevronDown
        size={14}
        className={`shrink-0 transition-transform ${expanded ? "" : "-rotate-90"}`}
        aria-hidden="true"
      />
      <span className="flex-1 text-left">
        {expanded ? "Hide" : "Show"} {hiddenCount} earlier turn
        {hiddenCount === 1 ? "" : "s"}
        <span className="text-text-dim"> (cleared, not in the model's memory)</span>
      </span>
    </button>
  );
}

/** Render a "Xm Ys" / "Ys" elapsed-time string for the
 *  WorkingSpinner's "waiting on model" badge. Seconds-only under one
 *  minute, minutes + seconds otherwise. Single-digit seconds zero-pad
 *  in the minute case so "1m 09s" doesn't visually jump to "1m 10s"
 *  width-wise during the live tick. */
function formatElapsed(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  const minutes = Math.floor(seconds / 60);
  const rem = seconds % 60;
  return `${minutes}m ${rem.toString().padStart(2, "0")}s`;
}

/* ── Working spinner (rattle) ────────────────────────────────────── */

function WorkingSpinner({
  thinking,
  tool,
  lastActivityRef,
  onForceEndTurn,
}: {
  thinking: boolean;
  tool: string | null;
  lastActivityRef: React.RefObject<number>;
  onForceEndTurn: () => Promise<void>;
}) {
  const [frame, setFrame] = useState(0);
  const [seed, setSeed] = useState(() => Math.floor(Math.random() * 0xffffffff));
  // 1s-tick clock for the force-end-turn watchdog. We compare against
  // `lastActivityRef.current` (a ref bumped on every incoming frame)
  // and surface the escape hatch when the gap exceeds the configured
  // threshold. Polling here, not on every event, so the rest of the
  // tree isn't perturbed by activity bookkeeping. See #1100.
  const [stalledSecs, setStalledSecs] = useState(0);
  const { forceEndTurnThresholdSecs } = useCockpitPrefs();

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

  useEffect(() => {
    // The hook starts the ref at 0 to avoid a render-time `Date.now()`
    // (react-hooks/purity). If we land here while it's still the
    // sentinel (e.g. mounting against a cached `turnActive=true`
    // state without yet receiving a fresh frame), pin it to now so
    // the watchdog clock isn't instantly tripped.
    if (lastActivityRef.current === 0) {
      lastActivityRef.current = Date.now();
    }
    const t = window.setInterval(() => {
      const last = lastActivityRef.current;
      setStalledSecs(Math.floor((Date.now() - last) / 1000));
    }, 1000);
    return () => window.clearInterval(t);
  }, [lastActivityRef]);

  const state: "thinking" | "tool" | "working" = thinking
    ? "thinking"
    : tool
      ? "tool"
      : "working";
  // Swap the rattle verb for an explicit "waiting on model" badge
  // with a live elapsed counter once the inactivity gap is clearly
  // longer than normal TTFT. The user can then distinguish "model
  // is taking a while" from "everything is wedged" without watching
  // logs. Threshold reuses the force-end-turn config so users who
  // want a more sensitive signal lower one knob and get both. See
  // #1112.
  const showStalled = stalledSecs >= forceEndTurnThresholdSecs;
  const label = showStalled
    ? `Waiting on model… ${formatElapsed(stalledSecs)}`
    : chooseVerb(state, seed, tool);
  const showForceEnd = showStalled;

  return (
    <div className="flex flex-col gap-2 text-sm italic text-text-muted">
      <div className="flex items-center gap-2">
        <span
          className="inline-block w-3 text-center font-mono text-brand-500"
          aria-hidden="true"
        >
          {SPINNER_FRAMES[frame]}
        </span>
        <span>{label}</span>
      </div>
      {showForceEnd ? (
        <button
          type="button"
          onClick={() => {
            void onForceEndTurn();
          }}
          className="self-start text-xs not-italic px-2 py-1 rounded-md border border-surface-700 bg-surface-800 text-text-secondary hover:bg-surface-700 hover:text-text-primary cursor-pointer"
          title={`No streaming activity for ${stalledSecs}s. Clears the spinner and sends a best-effort cancel to the agent.`}
        >
          Force end turn
        </button>
      ) : null}
    </div>
  );
}

/* ── Plan strip ──────────────────────────────────────────────────── */

interface PlanStripProps {
  plan: Plan | null;
}

function PlanStrip({ plan }: PlanStripProps) {
  const [expanded, setExpanded] = useState(false);
  // Hide entirely when there are no steps to show. The mode picker
  // now lives in the composer footer, so the strip only earns its
  // pixels when there's a plan with at least one step. An agent that
  // emits an empty `plan.steps` array otherwise leaves a clickable
  // banner reading "0/0" with nothing under the disclosure.
  if (!plan || plan.steps.length === 0) return null;

  // Pick the active step: prefer an explicit `InProgress` (Claude's
  // ExitPlanMode bridge sets this), otherwise fall back to the first
  // non-Done / non-Cancelled step (TodoWrite-produced plans typically
  // arrive with all entries Pending). Mirrors the server-side
  // `plan_summary_from_plan` logic so the strip and sidebar agree.
  const current =
    plan.steps.find((s) => s.status === "InProgress") ??
    plan.steps.find((s) => s.status !== "Done" && s.status !== "Cancelled");
  const completed = plan.steps.filter((s) => s.status === "Done").length;
  const totalSteps = plan.steps.length;
  const pct = Math.round((completed / totalSteps) * 100);
  const allDone = completed === totalSteps;

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
      </button>

      {expanded && (
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
  hasEverOpened,
  reconnecting,
  retryCount,
  retryCountdown,
  maxRetries,
  manualReconnect,
}: {
  status: CockpitContext["status"];
  lagged: boolean;
  rateLimit: CockpitState["rateLimit"];
  hasEverOpened: boolean;
  reconnecting: boolean;
  retryCount: number;
  retryCountdown: number;
  maxRetries: number;
  manualReconnect: () => void;
}) {
  const messages: { kind: string; text: string }[] = [];
  // Retry envelope exhausted: the auto-reconnect chain stopped after
  // `maxRetries` and we're sitting on a dead WS. Surface the manual
  // affordance instead of a status line so the user has a clear path
  // back to live. See #1130.
  const retriesExhausted =
    status !== "open" &&
    hasEverOpened &&
    !reconnecting &&
    retryCount >= maxRetries;
  if (reconnecting && status !== "open") {
    // Auto-retry banner: "Reconnecting (3/7) in 4s". Replaces the bare
    // "Reconnecting…" copy with concrete progress so the user knows
    // the tab isn't frozen and roughly how long until the next dial.
    const countdownPart =
      retryCountdown > 0 ? ` in ${retryCountdown}s` : "";
    messages.push({
      kind: "warn",
      text: `Cockpit disconnected. Reconnecting (${retryCount}/${maxRetries})${countdownPart}…`,
    });
  } else if (status === "connecting") {
    messages.push({
      kind: "info",
      text: hasEverOpened ? "Reconnecting to cockpit…" : "Starting cockpit…",
    });
  } else if (status === "error") {
    messages.push({
      kind: "warn",
      text: hasEverOpened
        ? "Cockpit reconnecting… showing cached transcript; new messages disabled."
        : "Starting cockpit worker… this can take a few seconds for new sessions.",
    });
  } else if (status === "closed" && !retriesExhausted) {
    messages.push({
      kind: "warn",
      text: hasEverOpened
        ? "Cockpit disconnected. Showing cached transcript; new messages disabled."
        : "Cockpit not ready yet. Retrying…",
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
  if (messages.length === 0 && !retriesExhausted) return null;
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
      {retriesExhausted && (
        <div className="flex items-center justify-between gap-3 text-xs text-brand-400">
          <span>Connection lost. Auto-retry stopped.</span>
          <button
            type="button"
            onClick={manualReconnect}
            className="shrink-0 rounded-md border border-brand-700 bg-brand-900/40 px-2 py-1 text-[10px] font-mono uppercase tracking-wide text-brand-100 hover:bg-brand-900/60"
          >
            Reconnect
          </button>
        </div>
      )}
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

function WorkerRestartingBanner({
  agentUnresponsive,
}: {
  agentUnresponsive: boolean;
}) {
  // Two reasons land here:
  //   - `aoe cockpit restart` (deletes registry, daemon's reaper
  //     publishes Stopped{reason:"restart_pending"}, reconciler spawns
  //     a fresh worker with the cached acp_session_id).
  //   - Cancel-escalation watchdog fired: claude-agent-acp ignored
  //     `session/cancel` for the grace window, the supervisor SIGTERMed
  //     the wedged runner and is respawning via `session/load`.
  // Both end with `AcpSessionAssigned` clearing the banner.
  // See #1196 for the agent_unresponsive variant.
  const message = agentUnresponsive
    ? "Agent stopped responding to cancel. Restarting worker; your transcript will be preserved."
    : "Restarting cockpit worker… the daemon will respawn the agent with your existing transcript shortly.";
  return (
    <div className="flex items-center gap-2 border-b border-sky-900/60 bg-sky-950/40 px-4 py-2 text-xs text-sky-200">
      <span
        className="inline-block h-2 w-2 animate-pulse rounded-full bg-sky-400"
        aria-hidden
      />
      <span>{message}</span>
    </div>
  );
}

/** First-spawn variant of `WorkerResumingBanner` shown when the
 *  session has no prior transcript (`lastSeq === 0`). The "cached
 *  transcript still available" copy is wrong there since there's
 *  nothing to be still-available. See #1106. */
function SpawningBanner() {
  return (
    <div className="flex items-center gap-2 border-b border-amber-900/60 bg-amber-950/40 px-4 py-2 text-xs text-amber-200">
      <span
        className="inline-block h-2 w-2 animate-pulse rounded-full bg-amber-400"
        aria-hidden
      />
      <span>
        Starting cockpit worker for new session… this can take a few
        seconds.
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

/** Top-of-cockpit chip shown while the agent's `ScheduleWakeup` is
 *  pending. Visible only when no turn is in flight (turns produce their
 *  own busy chrome) and no other recovery banner is up. 1Hz local tick
 *  for the countdown; once the wake fires the next UserPromptSent
 *  clears `state.nextWakeupAt` on the reducer side and this unmounts.
 *  See #1091. */
function ScheduledWakeupBanner({
  wakeAt,
  reason,
}: {
  wakeAt: string;
  reason: string | null;
}) {
  const targetMs = Date.parse(wakeAt);
  const [now, setNow] = useState(() => Date.now());
  const elapsed = !Number.isFinite(targetMs) || targetMs <= now;
  useEffect(() => {
    if (elapsed) return;
    const id = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(id);
  }, [elapsed]);
  if (!Number.isFinite(targetMs)) return null;
  const remaining = Math.max(0, Math.floor((targetMs - now) / 1000));
  const wakeDate = new Date(targetMs);
  const clock = `${String(wakeDate.getHours()).padStart(2, "0")}:${String(
    wakeDate.getMinutes(),
  ).padStart(2, "0")}`;
  let label: string;
  if (elapsed) {
    label = "Waking…";
  } else if (remaining < 60) {
    label = `Asleep until ${clock} (in ${remaining}s)`;
  } else if (remaining < 3600) {
    const m = Math.floor(remaining / 60);
    const s = remaining % 60;
    label = `Asleep until ${clock} (in ${m}m ${String(s).padStart(2, "0")}s)`;
  } else {
    const h = Math.floor(remaining / 3600);
    const m = Math.floor((remaining % 3600) / 60);
    label = `Asleep until ${clock} (in ${h}h ${m}m)`;
  }
  return (
    <div className="flex items-center gap-2 border-b border-sky-900/60 bg-sky-950/40 px-4 py-2 text-xs text-sky-200">
      <span aria-hidden className="text-base leading-none">
        ⏰
      </span>
      <span className="truncate">
        {label}
        {reason ? (
          <span className="text-sky-300/70">: {reason}</span>
        ) : null}
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
function RejectedPromptsStrip({
  rejected,
  onRetry,
  onDismiss,
  disabled,
}: {
  rejected: RejectedPrompt[];
  onRetry: (text: string) => void;
  /** Drop a single pill without resending. Local-only; the daemon has
   *  no record of pending rejections so this never goes over the wire. */
  onDismiss: (id: string) => void;
  /** True while the worker is restarting/stopped/in startup error.
   *  Retry must be gated then: `sendPrompt` would clear
   *  `workerRestarting` / `agentUnresponsive` and the rejected pills
   *  before the respawn has produced a new `AcpSessionAssigned`,
   *  leaving the UI claiming the agent is ready while the daemon
   *  hasn't reconnected yet. Dismiss stays available so the user can
   *  clear stale pills during the respawn. See #1196. */
  disabled: boolean;
}) {
  // Pills for prompts the daemon refused while another `session/prompt`
  // was already in flight. The user sees the rejection and can re-fire
  // via the Retry button instead of having their message vanish. The
  // reducer caps the list at 5 entries (oldest dropped) and clears on
  // the next UserPromptSent. See #1196.
  if (rejected.length === 0) return null;
  return (
    <div className="border-t border-amber-900/40 bg-amber-950/20 px-4 py-2">
      <div className="mx-auto max-w-3xl xl:max-w-4xl 2xl:max-w-5xl">
        <div className="pb-1.5 text-[11px] uppercase tracking-wider text-amber-300">
          <span className="inline-flex items-center gap-1">
            <AlertTriangle className="h-3 w-3" />
            Rejected ({rejected.length})
          </span>
        </div>
        <ul className="flex flex-col gap-1.5">
          {rejected.map((r) => (
            <li
              key={r.id}
              className="group flex items-start gap-2 rounded-lg border border-amber-700/30 bg-amber-950/15 px-2.5 py-1.5"
            >
              <span className="mt-0.5 inline-flex h-4 w-4 shrink-0 items-center justify-center rounded-full bg-amber-500/20 text-[10px] font-semibold text-amber-300">
                !
              </span>
              <div className="min-w-0 flex-1">
                <p className="truncate whitespace-pre-wrap break-words text-xs text-amber-100">
                  {r.text}
                </p>
                <p className="mt-0.5 text-[10px] text-amber-400/80">
                  Agent was busy; prompt was not sent.
                </p>
              </div>
              <button
                type="button"
                onClick={() => onRetry(r.text)}
                disabled={disabled}
                className="inline-flex shrink-0 items-center gap-1 rounded-md border border-amber-700/60 bg-amber-900/30 px-2 py-1 text-[10px] font-mono uppercase tracking-wide text-amber-100 hover:bg-amber-900/60 disabled:cursor-not-allowed disabled:opacity-40 disabled:hover:bg-amber-900/30"
                aria-label="Retry rejected prompt"
              >
                <RotateCcw className="h-3 w-3" />
                Retry
              </button>
              <button
                type="button"
                onClick={() => onDismiss(r.id)}
                className="inline-flex shrink-0 items-center justify-center rounded-md border border-amber-700/40 bg-amber-900/20 p-1 text-amber-200 hover:bg-amber-900/60"
                aria-label="Dismiss rejected prompt"
              >
                <X className="h-3 w-3" />
              </button>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

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
              className="text-text-dim hover:text-text-secondary transition-colors"
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
    <li className="group flex items-start gap-2 rounded-lg border border-sky-700/30 bg-sky-950/15 px-2.5 py-1.5">
      <span className="mt-0.5 inline-flex h-4 w-4 shrink-0 items-center justify-center rounded-full bg-sky-500/20 text-[10px] font-semibold text-sky-300">
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
        className="shrink-0 rounded p-1 text-text-dim hover:bg-surface-800 hover:text-text-secondary"
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
