// Cockpit view: a single-column chat conversation that fills the
// middle pane of the app shell.
//
// The layout is the responsibility of <ContentSplit> in App.tsx (left
// = workspace sidebar, right = terminal/diff). This component should
// NOT introduce another layout — it's just the conversation feed,
// pinned input, and a small sticky plan strip when the agent has one.

import { useEffect, useMemo, useRef, useState } from "react";
import {
  useCockpit,
  type ConnectionStatus,
} from "../../hooks/useCockpit";
import { ApprovalCard } from "./ApprovalCard";
import type {
  ActivityRow,
  Approval,
  ApprovalDecision,
  CockpitState,
  Plan,
} from "../../lib/cockpitTypes";

interface Props {
  sessionId: string;
}

export function CockpitView({ sessionId }: Props) {
  const { state, status, resolveApproval, sendPrompt } = useCockpit(sessionId);

  return (
    <div className="flex h-full flex-col bg-slate-900 text-slate-100">
      <PlanStrip plan={state.plan} mode={state.mode} />

      {(status !== "open" || state.lagged || state.rateLimit) && (
        <SystemNotices
          status={status}
          lagged={state.lagged}
          rateLimit={state.rateLimit}
        />
      )}

      {state.startupError && <StartupErrorBanner message={state.startupError} />}

      <ConversationFeed
        state={state}
        onResolve={resolveApproval}
      />

      <Composer
        sessionId={sessionId}
        sendPrompt={sendPrompt}
        thinking={state.thinking}
        inFlight={!!state.inFlightTool}
      />
    </div>
  );
}

/* ──────────────────────────────────────────────────────────────────── */

interface ConversationFeedProps {
  state: CockpitState;
  onResolve: (nonce: string, decision: ApprovalDecision) => Promise<void>;
}

/** Single scrollable column of message-style cells. Groups consecutive
 *  agent text chunks into one bubble so streaming reads as one
 *  paragraph; tool calls appear inline as cards between paragraphs;
 *  approvals are pinned at the bottom of the feed. */
function ConversationFeed({ state, onResolve }: ConversationFeedProps) {
  const cells = useMemo(() => groupActivity(state.activity), [state.activity]);
  const scroller = useRef<HTMLDivElement | null>(null);

  // Auto-stick to bottom unless the user has scrolled up. Cheap heuristic:
  // re-scroll on every state mutation iff the scroller is within ~80px of
  // the bottom.
  useEffect(() => {
    const el = scroller.current;
    if (!el) return;
    const distance = el.scrollHeight - el.scrollTop - el.clientHeight;
    if (distance < 80) {
      el.scrollTop = el.scrollHeight;
    }
  }, [cells, state.pendingApprovals.length, state.thinking]);

  return (
    <div
      ref={scroller}
      className="flex-1 overflow-y-auto"
    >
      <div className="mx-auto max-w-3xl px-4 py-6 space-y-4">
        {cells.length === 0 && state.pendingApprovals.length === 0 && (
          <EmptyState />
        )}
        {cells.map((cell) => (
          <Cell key={cell.id} cell={cell} />
        ))}

        {state.thinking && <ThinkingBubble />}

        {state.pendingApprovals.map((approval) => (
          <PendingApproval
            key={approval.nonce}
            approval={approval}
            onResolve={onResolve}
          />
        ))}
      </div>
    </div>
  );
}

/* ── Cell types ──────────────────────────────────────────────────── */

type Cell =
  | { id: string; kind: "user"; text: string }
  | { id: string; kind: "agent"; text: string }
  | { id: string; kind: "tool"; row: ActivityRow; result?: ActivityRow }
  | { id: string; kind: "system"; text: string };

/** Compact the raw activity stream into the message-cells the UI
 *  renders. Consecutive agent_message_chunk rows fuse into one bubble.
 *  tool_start/tool_complete pairs collapse into a single Cell. */
function groupActivity(rows: ActivityRow[]): Cell[] {
  const out: Cell[] = [];
  for (const row of rows) {
    if (row.kind === "message") {
      const last = out[out.length - 1];
      if (last && last.kind === "agent") {
        last.text += row.text;
        continue;
      }
      out.push({ id: row.id, kind: "agent", text: row.text });
    } else if (row.kind === "user_prompt") {
      out.push({ id: row.id, kind: "user", text: row.text });
    } else if (row.kind === "tool_start") {
      out.push({ id: row.id, kind: "tool", row });
    } else if (row.kind === "tool_complete" || row.kind === "tool_error") {
      // Attach to the most recent tool cell with the matching id.
      const target = [...out]
        .reverse()
        .find(
          (c) =>
            c.kind === "tool" &&
            (c.row.toolCallId === row.toolCallId ||
              c.row.id === row.id.replace(/^done-/, "start-")),
        );
      if (target && target.kind === "tool") {
        target.result = row;
      } else {
        out.push({ id: row.id, kind: "system", text: row.text });
      }
    } else if (row.kind === "thinking") {
      // Suppressed; the bubble at the bottom of the feed handles the
      // live state.
    } else {
      out.push({ id: row.id, kind: "system", text: row.text });
    }
  }
  return out;
}

function Cell({ cell }: { cell: Cell }) {
  if (cell.kind === "user") {
    return (
      <div className="flex justify-end">
        <div className="max-w-full rounded-lg bg-slate-700 px-3 py-2 text-sm text-slate-100 whitespace-pre-wrap">
          {cell.text}
        </div>
      </div>
    );
  }
  if (cell.kind === "agent") {
    return (
      <div className="text-slate-100 leading-relaxed whitespace-pre-wrap">
        {cell.text}
      </div>
    );
  }
  if (cell.kind === "tool") {
    return <ToolCallCell row={cell.row} result={cell.result} />;
  }
  return (
    <div className="text-xs text-slate-500 italic">{cell.text}</div>
  );
}

function ToolCallCell({
  row,
  result,
}: {
  row: ActivityRow;
  result?: ActivityRow;
}) {
  const [expanded, setExpanded] = useState(false);
  const status: "running" | "ok" | "err" = !result
    ? "running"
    : result.kind === "tool_error"
      ? "err"
      : "ok";

  const dot =
    status === "running"
      ? "bg-amber-400 animate-pulse"
      : status === "ok"
        ? "bg-emerald-500"
        : "bg-red-500";

  return (
    <div className="rounded-md border border-slate-700 bg-slate-800/60 text-sm">
      <button
        type="button"
        className="flex w-full items-center gap-2 px-3 py-2 text-left hover:bg-slate-800"
        onClick={() => setExpanded((v) => !v)}
      >
        <span className={`h-2 w-2 rounded-full ${dot}`} />
        <span className="font-mono text-xs text-slate-300">{row.text}</span>
        <span className="ml-auto text-xs text-slate-500">
          {status === "running" ? "running" : status === "ok" ? "✓" : "failed"}
        </span>
      </button>
      {expanded && result?.text && (
        <pre className="border-t border-slate-700 bg-slate-900 px-3 py-2 text-xs text-slate-400 whitespace-pre-wrap break-all">
          {result.text}
        </pre>
      )}
    </div>
  );
}

function ThinkingBubble() {
  return (
    <div className="flex items-center gap-2 text-sm italic text-slate-400">
      <span className="flex gap-1" aria-hidden="true">
        <span className="h-1.5 w-1.5 rounded-full bg-slate-500 animate-pulse" />
        <span className="h-1.5 w-1.5 rounded-full bg-slate-500 animate-pulse [animation-delay:120ms]" />
        <span className="h-1.5 w-1.5 rounded-full bg-slate-500 animate-pulse [animation-delay:240ms]" />
      </span>
      <span>Thinking…</span>
    </div>
  );
}

function EmptyState() {
  return (
    <div className="text-center text-slate-500 italic mt-12">
      Type a prompt below to start the conversation.
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
    <div className="border-b border-slate-800 bg-slate-900/95 backdrop-blur">
      <button
        type="button"
        className="flex w-full items-center gap-3 px-4 py-2 text-left text-sm hover:bg-slate-800/40"
        onClick={() => setExpanded((v) => !v)}
      >
        <span className="font-mono text-[11px] uppercase tracking-wide text-slate-500">
          plan
        </span>
        <span className="truncate text-slate-200">
          {current?.title ?? (plan ? "all steps complete" : "—")}
        </span>
        {plan && (
          <span className="ml-auto text-xs text-slate-500">
            {completed.length}/{totalSteps}
          </span>
        )}
        {mode !== "Default" && (
          <span className="rounded bg-amber-900/40 px-2 py-0.5 text-[11px] uppercase tracking-wide text-amber-300">
            {mode}
          </span>
        )}
      </button>

      {expanded && plan && (
        <div className="max-h-64 overflow-y-auto border-t border-slate-800 px-4 py-2 text-sm">
          <ul className="space-y-1">
            {plan.steps.map((step) => (
              <li
                key={step.id}
                className="flex items-start gap-2 text-slate-300"
              >
                <StepGlyph status={step.status} />
                <span
                  className={
                    step.status === "Done"
                      ? "text-slate-500 line-through"
                      : step.status === "InProgress"
                        ? "text-slate-100 font-medium"
                        : "text-slate-300"
                  }
                >
                  {step.title}
                </span>
              </li>
            ))}
          </ul>
          {upcoming.length === 0 && current && (
            <p className="mt-2 text-xs text-slate-500">
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
      return <span className="text-emerald-500">✓</span>;
    case "InProgress":
      return <span className="text-amber-400">●</span>;
    case "Cancelled":
      return <span className="text-slate-600">⊘</span>;
    case "Pending":
    default:
      return <span className="text-slate-600">○</span>;
  }
}

/* ── Composer (input area) ───────────────────────────────────────── */

interface ComposerProps {
  sessionId: string;
  sendPrompt: (text: string) => Promise<void>;
  thinking: boolean;
  inFlight: boolean;
}

function Composer({ sendPrompt, thinking, inFlight }: ComposerProps) {
  const [text, setText] = useState("");
  const [sending, setSending] = useState(false);
  const taRef = useRef<HTMLTextAreaElement | null>(null);

  // Auto-grow up to ~6 lines.
  useEffect(() => {
    const el = taRef.current;
    if (!el) return;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 144)}px`;
  }, [text]);

  const submit = async () => {
    const trimmed = text.trim();
    if (!trimmed || sending) return;
    setSending(true);
    try {
      await sendPrompt(trimmed);
      setText("");
    } finally {
      setSending(false);
    }
  };

  const onKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      void submit();
    }
  };

  const placeholder = thinking
    ? "Steer the agent…  (Enter to send)"
    : inFlight
      ? "Tool running. Your message will queue."
      : "Send a message…  (Enter to send, Shift+Enter for newline)";

  return (
    <div className="border-t border-slate-800 bg-slate-900">
      <div className="mx-auto max-w-3xl px-4 py-3">
        <div className="flex items-end gap-2 rounded-lg border border-slate-700 bg-slate-800 px-3 py-2 focus-within:border-amber-600">
          <textarea
            ref={taRef}
            className="flex-1 resize-none bg-transparent text-sm text-slate-100 placeholder:text-slate-500 focus:outline-none"
            rows={1}
            placeholder={placeholder}
            value={text}
            onChange={(event) => setText(event.target.value)}
            onKeyDown={onKeyDown}
            disabled={sending}
          />
          <button
            type="button"
            className={`shrink-0 rounded px-3 py-1.5 text-sm font-medium ${
              sending || !text.trim()
                ? "bg-slate-700 text-slate-500 cursor-not-allowed"
                : "bg-amber-600 text-white hover:bg-amber-500"
            }`}
            disabled={sending || !text.trim()}
            onClick={() => void submit()}
          >
            {sending ? "…" : "Send"}
          </button>
        </div>
      </div>
    </div>
  );
}

/* ── Pending approval card ───────────────────────────────────────── */

function PendingApproval({
  approval,
  onResolve,
}: {
  approval: Approval;
  onResolve: (nonce: string, decision: ApprovalDecision) => Promise<void>;
}) {
  return (
    <div className="rounded-lg border border-slate-700 bg-slate-900/80 p-3 shadow-md">
      <div className="text-xs font-mono uppercase tracking-wide text-amber-400 mb-2">
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
  status: ConnectionStatus;
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
    messages.push({
      kind: "warn",
      text: "Some events were missed during reconnect.",
    });
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
    <div className="border-b border-slate-800 px-4 py-2 space-y-1">
      {messages.map((m, i) => (
        <div
          key={i}
          className={`text-xs ${
            m.kind === "warn" ? "text-amber-300" : "text-slate-400"
          }`}
        >
          {m.text}
        </div>
      ))}
    </div>
  );
}

function StartupErrorBanner({ message }: { message: string }) {
  return (
    <div className="border-b border-rose-900/60 bg-rose-950/40 px-4 py-3 text-rose-200">
      <div className="text-sm font-medium">Cockpit agent failed to start</div>
      <pre className="mt-1 whitespace-pre-wrap text-xs text-rose-100/90">{message}</pre>
      <div className="mt-2 text-xs text-rose-200/80">
        Run <code className="rounded bg-rose-900/60 px-1">aoe cockpit doctor --fix</code> from a terminal,
        or install the adapter manually:
        <pre className="mt-1 whitespace-pre-wrap rounded bg-rose-900/40 p-2 text-xs">
          npm install -g @agentclientprotocol/claude-agent-acp
        </pre>
      </div>
    </div>
  );
}
