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
}: CockpitContext & { sessionId: string }) {
  return (
    <div className="flex h-full flex-col bg-surface-900 text-text-primary">
      <PlanStrip sessionId={sessionId} plan={state.plan} mode={state.mode} />

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

        <Composer sessionId={sessionId} />
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

/* ── Plan strip & mode picker ────────────────────────────────────── */

interface PlanStripProps {
  sessionId: string;
  plan: Plan | null;
  mode: CockpitState["mode"];
}

function PlanStrip({ sessionId, plan, mode }: PlanStripProps) {
  const [expanded, setExpanded] = useState(false);
  // Hide entirely on the most common case: no plan, default mode.
  if (!plan && mode === "Default") {
    // Still render a thin status bar so the mode picker is reachable.
    return (
      <div className="flex items-center justify-end gap-2 border-b border-surface-800 bg-surface-900/95 px-4 py-1.5 backdrop-blur">
        <ModePicker sessionId={sessionId} mode={mode} />
      </div>
    );
  }

  const current = plan?.steps.find((s) => s.status === "InProgress");
  const completed = plan?.steps.filter((s) => s.status === "Done").length ?? 0;
  const totalSteps = plan?.steps.length ?? 0;
  const pct = totalSteps > 0 ? Math.round((completed / totalSteps) * 100) : 0;

  return (
    <div className="border-b border-surface-800 bg-surface-900/95 backdrop-blur">
      <div className="flex items-center gap-3 px-4 py-2 text-sm">
        <button
          type="button"
          className="flex flex-1 min-w-0 items-center gap-3 text-left hover:bg-surface-800/40 -mx-2 px-2 py-1 rounded"
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
        <ModePicker sessionId={sessionId} mode={mode} />
      </div>

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

const MODE_OPTIONS: ReadonlyArray<{
  id: CockpitState["mode"];
  label: string;
  hint: string;
}> = [
  { id: "Default", label: "Default", hint: "Approve each tool individually" },
  { id: "Plan", label: "Plan", hint: "Plan first, no edits applied" },
  {
    id: "AcceptEdits",
    label: "Accept edits",
    hint: "Auto-approve safe file edits",
  },
  {
    id: "BypassPermissions",
    label: "Yolo",
    hint: "Skip all approvals (destructive)",
  },
];

function ModePicker({
  sessionId,
  mode,
}: {
  sessionId: string;
  mode: CockpitState["mode"];
}) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement | null>(null);

  // Close on outside click / Esc.
  useEffect(() => {
    if (!open) return;
    const onClick = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") setOpen(false);
    };
    document.addEventListener("mousedown", onClick);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onClick);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const select = async (id: CockpitState["mode"]) => {
    setOpen(false);
    if (id === mode) return;
    try {
      await fetch(
        `/api/sessions/${encodeURIComponent(sessionId)}/cockpit/mode`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ mode_id: id.toLowerCase() }),
        },
      );
    } catch {
      // The agent will broadcast a ModeChanged event on success; if
      // the request fails the UI stays on the current mode.
    }
  };

  const current =
    MODE_OPTIONS.find((m) => m.id === mode) ?? MODE_OPTIONS[0]!;
  const tone =
    mode === "BypassPermissions"
      ? "border-rose-700/50 bg-rose-950/30 text-rose-300 hover:border-rose-700"
      : mode === "AcceptEdits"
        ? "border-amber-700/50 bg-amber-950/30 text-amber-300 hover:border-amber-700"
        : mode === "Plan"
          ? "border-cyan-800/50 bg-cyan-950/30 text-cyan-300 hover:border-cyan-700"
          : "border-surface-700 bg-surface-800 text-text-secondary hover:border-surface-600";

  return (
    <div ref={ref} className="relative">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        title={current.hint}
        className={[
          "inline-flex items-center gap-1 rounded-md border px-2 py-0.5 text-[11px] font-medium",
          "transition-colors",
          tone,
        ].join(" ")}
      >
        <span>{current.label}</span>
        <ChevronDown className="h-3 w-3 opacity-70" />
      </button>
      {open && (
        <div
          className="absolute right-0 z-20 mt-1 w-56 rounded-md border border-surface-700 bg-surface-850 shadow-lg overflow-hidden"
          role="menu"
        >
          {MODE_OPTIONS.map((opt) => (
            <button
              key={opt.id}
              type="button"
              role="menuitem"
              onClick={() => void select(opt.id)}
              className={[
                "flex w-full items-start gap-2 px-3 py-2 text-left text-xs hover:bg-surface-800",
                opt.id === mode ? "bg-surface-800/60" : "",
              ].join(" ")}
            >
              <span
                className={[
                  "mt-0.5 inline-block h-3 w-3 shrink-0 rounded-full border",
                  opt.id === mode
                    ? "border-brand-500 bg-brand-500"
                    : "border-surface-700",
                ].join(" ")}
              />
              <span className="min-w-0 flex-1">
                <span className="block font-medium text-text-primary">
                  {opt.label}
                </span>
                <span className="block text-[11px] text-text-dim">
                  {opt.hint}
                </span>
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
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
