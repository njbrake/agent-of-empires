// Cockpit conversation surface.
//
// Layout matches the rest of the app shell: <ContentSplit> handles the
// outer columns; this component owns the middle pane (chat feed +
// composer + plan/system strips). It is intentionally chat-shaped, not
// terminal-shaped, since the cockpit's whole point is to render
// structured agent state instead of a tmux pane.
//
// Design references: Cursor agent chat, VSCode Copilot Chat, Claude Code
// CLI. User turns are right-aligned chips; agent turns are full-width
// markdown; tool calls render as per-kind cards inline (see ToolCards.tsx).

import { useEffect, useMemo, useRef, useState } from "react";
import { useCockpit, type ConnectionStatus } from "../../hooks/useCockpit";
import { ApprovalCard } from "./ApprovalCard";
import { Markdown } from "./Markdown";
import { ToolCard } from "./ToolCards";
import {
  SPINNER_FRAMES,
  SPINNER_INTERVAL_MS,
  VERB_INTERVAL_MS,
  chooseVerb,
} from "../../lib/cockpitRattle";
import type {
  ActivityRow,
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
  const { state, status, resolveApproval, sendPrompt, cancelPrompt } =
    useCockpit(sessionId);

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

      <ConversationFeed
        state={state}
        onResolve={resolveApproval}
        onStarterPrompt={sendPrompt}
      />

      <Composer
        sendPrompt={sendPrompt}
        cancelPrompt={cancelPrompt}
        thinking={state.thinking}
        inFlight={!!state.inFlightTool}
        turnActive={state.turnActive}
      />
    </div>
  );
}

/* ── Conversation feed ───────────────────────────────────────────── */

interface ConversationFeedProps {
  state: CockpitState;
  onResolve: (nonce: string, decision: ApprovalDecision) => Promise<void>;
  onStarterPrompt: (text: string) => Promise<void>;
}

function ConversationFeed({ state, onResolve, onStarterPrompt }: ConversationFeedProps) {
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

  const empty = cells.length === 0 && state.pendingApprovals.length === 0;

  return (
    <div ref={scroller} className="flex-1 overflow-y-auto">
      <div className="mx-auto max-w-3xl px-4 py-6">
        {empty && <EmptyState onPick={onStarterPrompt} />}

        {cells.map((cell, i) => (
          <Turn key={cell.id} cell={cell} prev={cells[i - 1]} />
        ))}

        {state.turnActive && (
          <div className="mt-3 ml-1">
            <WorkingSpinner
              thinking={state.thinking}
              tool={state.inFlightTool?.name ?? null}
            />
          </div>
        )}

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
  | { id: string; kind: "tool"; tool: ToolCall; result?: ActivityRow }
  | { id: string; kind: "system"; text: string };

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
    } else if (row.kind === "tool_start" && row.tool) {
      out.push({ id: row.id, kind: "tool", tool: row.tool });
    } else if (row.kind === "tool_complete" || row.kind === "tool_error") {
      const target = [...out]
        .reverse()
        .find(
          (c) =>
            c.kind === "tool" &&
            (c.tool.id === row.toolCallId ||
              `start-${c.tool.id}` === row.id.replace(/^done-/, "start-")),
        );
      if (target && target.kind === "tool") {
        target.result = row;
      } else {
        out.push({ id: row.id, kind: "system", text: row.text });
      }
    } else if (row.kind === "thinking") {
      // Suppressed; live state shown by ThinkingBubble below the feed.
    } else {
      out.push({ id: row.id, kind: "system", text: row.text });
    }
  }
  return out;
}

/* ── Turn renderer ───────────────────────────────────────────────── */

function Turn({ cell, prev }: { cell: Cell; prev?: Cell }) {
  // Boundary divider above each user turn (except the first), so user→
  // agent→user reads as discrete chunks instead of one long stream.
  const showDivider = cell.kind === "user" && !!prev;
  return (
    <>
      {showDivider && <div className="my-5 border-t border-surface-800/70" />}
      <div className={cellSpacing(cell, prev)}>
        <CellContent cell={cell} />
      </div>
    </>
  );
}

function cellSpacing(cell: Cell, prev?: Cell): string {
  // Tool cards hug the agent message above; agent text gets its own
  // breathing room. User chips are right-aligned with a top margin.
  if (cell.kind === "user") return "flex justify-end mt-2";
  if (cell.kind === "tool") {
    return prev?.kind === "tool" || prev?.kind === "agent" ? "mt-1" : "mt-3";
  }
  if (cell.kind === "agent") return "mt-3";
  return "mt-2";
}

function CellContent({ cell }: { cell: Cell }) {
  if (cell.kind === "user") {
    return (
      <div className="max-w-[80%] rounded-2xl rounded-br-sm border border-surface-700 bg-surface-800/70 px-3 py-1.5 text-sm whitespace-pre-wrap">
        {cell.text}
      </div>
    );
  }
  if (cell.kind === "agent") {
    return (
      <div className="text-sm text-text-primary leading-relaxed">
        <Markdown text={cell.text} />
      </div>
    );
  }
  if (cell.kind === "tool") {
    return <ToolCard tool={cell.tool} result={cell.result} />;
  }
  return <div className="text-xs italic text-text-dim">{cell.text}</div>;
}

/* ── Empty state ─────────────────────────────────────────────────── */

function EmptyState({ onPick }: { onPick: (text: string) => Promise<void> }) {
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

/* ── Working spinner ─────────────────────────────────────────────── */

/**
 * "Agent is working" indicator with AOE-themed verbs and a braille
 * rattle. Visible from prompt-sent until the agent emits `Stopped`.
 *
 * Two animations layered:
 *  - The glyph rattles through SPINNER_FRAMES at SPINNER_INTERVAL_MS,
 *    same vibe as the ratatui `rattles` spinners on the TUI side.
 *  - The verb cycles through WORKING_VERBS / THINKING_VERBS every
 *    VERB_INTERVAL_MS so long turns get variety. Tool runs override
 *    the verb with the tool's actual name dressed up with an empire
 *    verb ("Dispatching read", "Marshalling write"…).
 */
function WorkingSpinner({
  thinking,
  tool,
}: {
  thinking: boolean;
  tool: string | null;
}) {
  const [frame, setFrame] = useState(0);
  const [seed, setSeed] = useState(() => Math.floor(Math.random() * 0xffffffff));

  // Rattle the glyph.
  useEffect(() => {
    const t = window.setInterval(() => {
      setFrame((f) => (f + 1) % SPINNER_FRAMES.length);
    }, SPINNER_INTERVAL_MS);
    return () => window.clearInterval(t);
  }, []);

  // Re-pick the verb every few seconds for variety on long turns.
  // Tool changes (different tool name) implicitly bump the verb
  // because chooseVerb hashes seed+context.
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
              <li
                key={step.id}
                className="flex items-start gap-2 text-text-secondary"
              >
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

/* ── Composer ────────────────────────────────────────────────────── */

interface ComposerProps {
  sendPrompt: (text: string) => Promise<void>;
  cancelPrompt: () => Promise<void>;
  thinking: boolean;
  inFlight: boolean;
  turnActive: boolean;
}

function Composer({
  sendPrompt,
  cancelPrompt,
  thinking,
  inFlight,
  turnActive,
}: ComposerProps) {
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

  // Focus the composer when the cockpit mounts so the user can type
  // immediately without an extra click. The right-pane wterm calls
  // `input.focus()` unconditionally at the end of its async WASM init
  // (see node_modules/@wterm/dom/dist/wterm.js init()), which fires
  // ~200-500ms after we mount and steals focus. Re-claim with a couple
  // of staggered timeouts. The reclaim only fires while focus is on
  // body or on a non-input element (i.e. wterm's internal textarea
  // capture), so an intentional click into the host shell sticks.
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
      // wterm's input is a textarea inside .wterm; treat it as
      // "not yet a deliberate user choice" during the initial race.
      if (active.closest?.(".wterm")) {
        el.focus();
      }
    };
    const t1 = setTimeout(reclaim, 250);
    const t2 = setTimeout(reclaim, 700);
    return () => {
      clearTimeout(t1);
      clearTimeout(t2);
    };
  }, []);

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

  // The agent is "busy" any time it owes the user a Stopped event or
  // is mid-tool / mid-thought. Drives the Send → Stop swap.
  const busy = thinking || inFlight || turnActive;
  const placeholder = busy
    ? "Steer the agent…  (Enter to send)"
    : "Send a message…";

  return (
    <div className="border-t border-surface-800 bg-surface-900">
      <div className="mx-auto max-w-3xl px-4 pt-3 pb-2">
        <div className="flex items-end gap-2 rounded-xl border border-surface-700 bg-surface-800 px-3 py-2 focus-within:border-brand-600">
          <textarea
            ref={taRef}
            className="flex-1 resize-none bg-transparent text-sm text-text-primary placeholder:text-text-dim focus:outline-none"
            rows={1}
            placeholder={placeholder}
            value={text}
            onChange={(event) => setText(event.target.value)}
            onKeyDown={onKeyDown}
            disabled={sending}
          />
          {busy ? (
            <button
              type="button"
              aria-label="Stop"
              title="Stop the agent"
              className="shrink-0 flex items-center justify-center rounded-md border border-surface-600 bg-surface-700 hover:bg-surface-700/70 px-2.5 py-1.5 text-text-secondary"
              onClick={() => void cancelPrompt()}
            >
              <span className="block h-3 w-3 rounded-sm bg-text-secondary" />
            </button>
          ) : (
            <button
              type="button"
              className={`shrink-0 rounded-md px-3 py-1.5 text-sm font-medium ${
                sending || !text.trim()
                  ? "bg-surface-700 text-text-dim cursor-not-allowed"
                  : "bg-brand-600 text-white hover:bg-brand-500"
              }`}
              disabled={sending || !text.trim()}
              onClick={() => void submit()}
            >
              {sending ? "…" : "Send"}
            </button>
          )}
        </div>
        <div className="mt-1 px-1 text-[11px] text-text-dim">
          <kbd className="font-mono">Enter</kbd> to send ·{" "}
          <kbd className="font-mono">Shift+Enter</kbd> for newline
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
    <div className="border-b border-surface-800 px-4 py-2 space-y-1">
      {messages.map((m, i) => (
        <div
          key={i}
          className={`text-xs ${
            m.kind === "warn" ? "text-brand-400" : "text-text-muted"
          }`}
        >
          {m.text}
        </div>
      ))}
    </div>
  );
}

function StartupErrorBanner({ message }: { message: string }) {
  // Auth-required is a different remediation than missing-binary. The
  // adapter throws "Authentication required" when the binary IS installed
  // but no creds are reachable.
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
