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

import { useEffect, useRef, useState } from "react";
import {
  ComposerPrimitive,
  MessagePrimitive,
  ThreadPrimitive,
  useThreadRuntime,
} from "@assistant-ui/react";

import { ApprovalCard } from "./ApprovalCard";
import { CockpitRuntime, type CockpitContext } from "./CockpitRuntime";
import { Markdown } from "./Markdown";
import { ToolCard } from "./ToolCards";
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
  ToolCall,
} from "../../lib/cockpitTypes";

interface Props {
  sessionId: string;
}

const STARTER_PROMPTS = [
  "Explain this codebase",
  "Find recent changes worth reviewing",
  "What does the build pipeline do?",
];

export function CockpitView({ sessionId }: Props) {
  return (
    <CockpitRuntime sessionId={sessionId}>
      {(ctx) => <CockpitChrome {...ctx} />}
    </CockpitRuntime>
  );
}

function CockpitChrome({ state, status, resolveApproval, sendPrompt }: CockpitContext) {
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

      {state.startupError && <StartupErrorBanner message={state.startupError} />}

      <ThreadPrimitive.Root className="flex flex-1 flex-col min-h-0">
        <ThreadPrimitive.Viewport
          autoScroll
          className="flex-1 overflow-y-auto"
        >
          <div className="mx-auto max-w-3xl px-4 py-6">
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

        <Composer />
      </ThreadPrimitive.Root>
    </div>
  );
}

/* ── User & Assistant message templates ──────────────────────────── */

function UserMessage() {
  return (
    <MessagePrimitive.Root className="mt-4 flex justify-end">
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
    <MessagePrimitive.Root className="mt-4 mr-auto w-full">
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

function AssistantToolCall(props: ToolCallProps) {
  // Reconstruct the ToolCall shape our existing ToolCards.tsx
  // renderer expects. assistant-ui carries `toolName` (we set this to
  // ACP's lowercased ToolKind in CockpitRuntime) plus argsText (the
  // truncated JSON preview from the agent).
  const tool: ToolCall = {
    id: props.toolCallId,
    name: prettifyToolName(props.toolName, props.args),
    kind: props.toolName,
    args_preview: props.argsText ?? safeStringify(props.args ?? null),
    started_at: new Date().toISOString(),
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
          at: new Date().toISOString(),
        }
      : undefined;
  return <ToolCard tool={tool} result={result} />;
}

function prettifyToolName(
  kind: string,
  args?: Record<string, unknown>,
): string {
  // Pick a human-readable label for the tool card header. Falls back to
  // the path / command / query if available.
  if (args) {
    for (const key of [
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

/* ── Composer ────────────────────────────────────────────────────── */

function Composer() {
  const taRef = useRef<HTMLTextAreaElement | null>(null);

  // Auto-grow the textarea up to ~6 lines.
  const onInput = (e: React.FormEvent<HTMLTextAreaElement>) => {
    const el = e.currentTarget;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 144)}px`;
  };

  // wterm's async init() in the right pane focuses its hidden textarea
  // ~200-500ms after mount and steals focus from us. Re-claim a couple
  // of times so the agent input wins; only when focus is on body or
  // inside .wterm so an intentional click into the host shell sticks.
  useEffect(() => {
    const el = taRef.current;
    if (!el) return;
    el.focus();
    const reclaim = () => {
      const active = document.activeElement as HTMLElement | null;
      if (!active || active === document.body || active === el) {
        el.focus();
        return;
      }
      if (active.closest?.(".wterm")) {
        el.focus();
      }
    };
    const t1 = window.setTimeout(reclaim, 250);
    const t2 = window.setTimeout(reclaim, 700);
    return () => {
      window.clearTimeout(t1);
      window.clearTimeout(t2);
    };
  }, []);

  return (
    <div className="border-t border-surface-800 bg-surface-900">
      <div className="mx-auto max-w-3xl px-4 pt-3 pb-2">
        <ComposerPrimitive.Root
          className="flex items-end gap-2 rounded-xl border border-surface-700 bg-surface-800 px-3 py-2 focus-within:border-brand-600"
        >
          <ComposerPrimitive.Input
            ref={taRef}
            rows={1}
            placeholder="Send a message…"
            onInput={onInput}
            autoFocus
            className="flex-1 resize-none bg-transparent text-sm text-text-primary placeholder:text-text-dim focus:outline-none"
          />
          <ThreadPrimitive.If running>
            <StopButton />
          </ThreadPrimitive.If>
          <ThreadPrimitive.If running={false}>
            <ComposerPrimitive.Send
              className="shrink-0 rounded-md bg-brand-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-brand-500 disabled:cursor-not-allowed disabled:bg-surface-700 disabled:text-text-dim"
            >
              Send
            </ComposerPrimitive.Send>
          </ThreadPrimitive.If>
        </ComposerPrimitive.Root>
        <div className="mt-1 px-1 text-[11px] text-text-dim">
          <kbd className="font-mono">Enter</kbd> to send ·{" "}
          <kbd className="font-mono">Shift+Enter</kbd> for newline
        </div>
      </div>
    </div>
  );
}

function StopButton() {
  const runtime = useThreadRuntime();
  return (
    <button
      type="button"
      aria-label="Stop"
      title="Stop the agent"
      className="shrink-0 flex items-center justify-center rounded-md border border-surface-600 bg-surface-700 hover:bg-surface-700/70 px-2.5 py-1.5 text-text-secondary"
      onClick={() => runtime.cancelRun()}
    >
      <span className="block h-3 w-3 rounded-sm bg-text-secondary" />
    </button>
  );
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
  if (!plan && mode === "Default") return null;

  const current = plan?.steps.find((s) => s.status === "InProgress");
  const upcoming = plan?.steps.filter((s) => s.status === "Pending") ?? [];
  const completed = plan?.steps.filter((s) => s.status === "Done") ?? [];
  const totalSteps = plan?.steps.length ?? 0;

  return (
    <div className="border-b border-surface-800 bg-surface-900/95 backdrop-blur">
      <button
        type="button"
        className="flex w-full items-center gap-3 px-4 py-2 text-left text-sm hover:bg-surface-800/40"
        onClick={() => setExpanded((v) => !v)}
      >
        <span className="font-mono text-[11px] uppercase tracking-wide text-text-dim">
          plan
        </span>
        <span className="truncate text-text-primary">
          {current?.title ?? (plan ? "all steps complete" : "—")}
        </span>
        {plan && (
          <span className="ml-auto text-xs text-text-dim">
            {completed.length}/{totalSteps}
          </span>
        )}
        {mode !== "Default" && (
          <span className="rounded bg-brand-700/40 px-2 py-0.5 text-[11px] uppercase tracking-wide text-brand-400">
            {mode}
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
          {upcoming.length === 0 && current && (
            <p className="mt-2 text-xs text-text-dim">
              No upcoming steps after the current one.
            </p>
          )}
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
  return (
    <div className="mt-4 rounded-lg border border-surface-700 bg-surface-900/80 p-3 shadow-md">
      <div className="text-xs font-mono uppercase tracking-wide text-brand-500 mb-2">
        agent is asking permission
      </div>
      <ApprovalCard
        approval={approval}
        onResolve={(decision) => onResolve(approval.nonce, decision)}
      />
    </div>
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
    messages.push({ kind: "warn", text: "Cockpit connection error. Retrying…" });
  }
  if (status === "closed") {
    messages.push({ kind: "warn", text: "Cockpit disconnected." });
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

function StartupErrorBanner({ message }: { message: string }) {
  const isAuth = /authentic|login|api[_ -]?key/i.test(message);
  return (
    <div className="border-b border-rose-900/60 bg-rose-950/40 px-4 py-3 text-rose-200">
      <div className="text-sm font-medium">Cockpit agent failed to start</div>
      <pre className="mt-1 whitespace-pre-wrap text-xs text-rose-100/90">{message}</pre>
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
