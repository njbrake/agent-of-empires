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
  ActionBarPrimitive,
  MessagePrimitive,
  ThreadPrimitive,
} from "@assistant-ui/react";
import {
  ChevronDown,
  Copy as CopyIcon,
  ListChecks,
  Pencil,
  RefreshCcw,
} from "lucide-react";

import { ApprovalCard } from "./ApprovalCard";
import { CockpitRuntime, type CockpitContext } from "./CockpitRuntime";
import { Composer } from "./Composer";
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
      {(ctx) => <CockpitChrome sessionId={sessionId} {...ctx} />}
    </CockpitRuntime>
  );
}

function CockpitChrome({
  sessionId,
  state,
  status,
  resolveApproval,
  sendPrompt,
  dismissError,
}: CockpitContext & { sessionId: string }) {
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

        <Composer
          sessionId={sessionId}
          availableModes={state.availableModes}
          currentModeId={state.currentModeId}
          legacyMode={state.mode}
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
      <ActionBarPrimitive.Root
        autohide="not-last"
        className="opacity-0 transition-opacity duration-100 group-hover:opacity-100 focus-within:opacity-100"
      >
        <div className="flex items-center gap-0.5 rounded-md border border-surface-700/60 bg-surface-850 p-0.5">
          <ActionBarPrimitive.Edit asChild>
            <ActionIconButton label="Edit" icon={<Pencil className="h-3 w-3" />} />
          </ActionBarPrimitive.Edit>
          <ActionBarPrimitive.Copy asChild>
            <ActionIconButton label="Copy" icon={<CopyIcon className="h-3 w-3" />} />
          </ActionBarPrimitive.Copy>
        </div>
      </ActionBarPrimitive.Root>
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
      <ActionBarPrimitive.Root
        autohide="not-last"
        className="mt-1 opacity-0 transition-opacity duration-100 group-hover:opacity-100 focus-within:opacity-100"
      >
        <div className="flex items-center gap-0.5 rounded-md border border-surface-700/60 bg-surface-850 p-0.5">
          <ActionBarPrimitive.Copy asChild>
            <ActionIconButton label="Copy" icon={<CopyIcon className="h-3 w-3" />} />
          </ActionBarPrimitive.Copy>
          <ActionBarPrimitive.Reload asChild>
            <ActionIconButton
              label="Regenerate"
              icon={<RefreshCcw className="h-3 w-3" />}
            />
          </ActionBarPrimitive.Reload>
        </div>
      </ActionBarPrimitive.Root>
    </MessagePrimitive.Root>
  );
}

function ActionIconButton({
  label,
  icon,
  ...rest
}: {
  label: string;
  icon: React.ReactNode;
} & React.ButtonHTMLAttributes<HTMLButtonElement>) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      {...rest}
      className={[
        "inline-flex h-6 w-6 items-center justify-center rounded text-text-dim",
        "hover:bg-surface-800 hover:text-text-secondary",
        "transition-colors",
        rest.className ?? "",
      ].join(" ")}
    >
      {icon}
    </button>
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

  const current = plan?.steps.find((s) => s.status === "InProgress");
  const completed = plan?.steps.filter((s) => s.status === "Done").length ?? 0;
  const totalSteps = plan?.steps.length ?? 0;
  const pct = totalSteps > 0 ? Math.round((completed / totalSteps) * 100) : 0;

  return (
    <div className="border-b border-surface-800 bg-surface-900/95 backdrop-blur">
      <button
        type="button"
        className="flex w-full items-center gap-3 px-4 py-2 text-left text-sm hover:bg-surface-800/40"
        onClick={() => setExpanded((v) => !v)}
      >
        <ListChecks className="h-3.5 w-3.5 shrink-0 text-text-dim" />
        <span className="truncate text-text-primary">
          {current?.title ?? (plan ? "all steps complete" : "—")}
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

function StartupErrorBanner({
  sessionId,
  message,
}: {
  sessionId: string;
  message: string;
}) {
  const isAuth = /authentic|login|api[_ -]?key/i.test(message);
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
