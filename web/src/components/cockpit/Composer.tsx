// VSCode/Cursor-style composer for the cockpit.
//
// Built on assistant-ui's `<ComposerPrimitive.Root>` plus the official
// `Unstable_TriggerPopover` family for `@` mentions and `/` slash
// commands. We provide TriggerAdapters that feed categories/items
// from our own state (the workspace file listing for `@`, a static
// command list for `/`).
//
// Icons via lucide-react.

import {
  ComposerPrimitive,
  useComposerRuntime,
  useThreadRuntime,
} from "@assistant-ui/react";
import {
  unstable_defaultDirectiveFormatter as defaultDirectiveFormatter,
  type Unstable_TriggerAdapter,
  type Unstable_TriggerItem,
} from "@assistant-ui/core";
import { useEffect, useMemo, useRef, useState } from "react";
import {
  AtSign,
  ChevronUp,
  Slash,
  Square,
} from "lucide-react";

import { useFilesIndex, fuzzyFilter } from "./useFilesIndex";
import type { CockpitState } from "../../lib/cockpitTypes";
import { getDraft, setDraft } from "../../lib/cockpitDrafts";

interface Props {
  sessionId: string;
  availableModes: CockpitState["availableModes"];
  currentModeId: CockpitState["currentModeId"];
  /** Legacy enum-based mode used as fallback when the agent does not
   *  advertise modes via NewSessionResponse. */
  legacyMode: CockpitState["mode"];
  /** Latest agent-reported context-window usage. Null until the agent
   *  has emitted at least one ACP `UsageUpdate`. */
  sessionUsage: CockpitState["sessionUsage"];
  /** Slash commands the agent advertised in its most recent
   *  AvailableCommandsUpdate. Includes plugins/skills/MCP commands.
   *  Empty until the agent emits the first list. */
  availableCommands: CockpitState["availableCommands"];
  /** True when the cockpit WS is open. When false the composer
   *  refuses new submissions: prompts dispatched while disconnected
   *  would be lost (the POST /cockpit/prompt would fail with no way
   *  to retry). TODO(post-disconnect): queue locally and flush on
   *  reconnect instead of blocking. */
  connected: boolean;
  /** True while the agent is producing the current turn. When true the
   *  composer keeps its textarea editable and surfaces a queue-send
   *  button alongside Stop; sends go through the client-side queue (see
   *  #1031). When false the regular ComposerPrimitive.Send path runs as
   *  before. */
  turnActive: boolean;
  /** Number of items already enqueued for after the current turn.
   *  Drives the badge on the queue-send button. */
  queuedCount: number;
  /** Push the composer text straight onto the cockpit queue. Bypasses
   *  the ComposerPrimitive.Send path (which assistant-ui hard-disables
   *  while `thread.isRunning && !capabilities.queue`). Used by the
   *  mid-turn Send button + the Enter-while-running handler. */
  enqueuePrompt: (text: string) => void | Promise<void>;
}

export function Composer({
  sessionId,
  availableModes,
  currentModeId,
  legacyMode,
  sessionUsage,
  availableCommands,
  connected,
  turnActive,
  queuedCount,
  enqueuePrompt,
}: Props) {
  const taRef = useRef<HTMLTextAreaElement | null>(null);
  const { files } = useFilesIndex(sessionId);

  // Adapter for the @ file picker. We deliberately skip the
  // category step (return []) so the popover lands directly in
  // search-results mode — the resource short-circuits to
  // adapter.search() when there are no categories. That gives us a
  // single-pane file list instead of a "Files" category drill-down.
  const fileAdapter: Unstable_TriggerAdapter = useMemo(
    () => ({
      categories: () => [],
      categoryItems: () => [],
      search: (query) => {
        const items = files.map((path) => ({
          id: path,
          type: "file",
          label: path,
          description: extDescription(path),
        }));
        return fuzzyFilter(items, query, 30);
      },
    }),
    [files],
  );

  // Slash commands: built from the agent's AvailableCommandsUpdate.
  // Each item carries `acceptsInput` so onExecute knows whether to
  // leave the cursor parked after the name (for commands with args)
  // or to prepare for an immediate Enter-to-send.
  const slashItems: Unstable_TriggerItem[] = useMemo(
    () =>
      availableCommands.map((c) => ({
        id: c.name,
        type: "command",
        label: `/${c.name}`,
        description: c.description,
        acceptsInput: c.accepts_input,
      })),
    [availableCommands],
  );
  const slashAdapter: Unstable_TriggerAdapter = useMemo(
    () => ({
      categories: () => [],
      categoryItems: () => [],
      search: (query) => fuzzyFilter(slashItems, query, 30),
    }),
    [slashItems],
  );

  const composerRuntime = useComposerRuntime();

  // Auto-grow the textarea up to ~6 visible lines.
  const onInput = (e: React.FormEvent<HTMLTextAreaElement>) => {
    const el = e.currentTarget;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 200)}px`;
  };

  // Per-session draft persistence: keep an unsent prompt across
  // sidebar navigation / route changes by mirroring composer text into
  // localStorage. The CockpitView unmounts when the user switches to
  // another session, so without this the draft is gone on return.
  // Keyed by sessionId; cleared when the text goes empty (user deleted
  // it, or the runtime cleared after a successful send).
  useEffect(() => {
    const saved = getDraft(sessionId);
    if (saved && composerRuntime.getState().text === "") {
      composerRuntime.setText(saved);
      // setText doesn't fire the textarea's onInput, so the auto-grow
      // never runs for the restored value. Resize manually once the DOM
      // has the seeded text.
      requestAnimationFrame(() => {
        const el = taRef.current;
        if (el) {
          el.style.height = "auto";
          el.style.height = `${Math.min(el.scrollHeight, 200)}px`;
        }
      });
    }

    let writeTimer: number | null = null;
    const flush = () => {
      writeTimer = null;
      setDraft(sessionId, composerRuntime.getState().text);
    };
    const unsub = composerRuntime.subscribe(() => {
      if (writeTimer !== null) window.clearTimeout(writeTimer);
      writeTimer = window.setTimeout(flush, 250);
    });
    return () => {
      unsub();
      if (writeTimer !== null) {
        window.clearTimeout(writeTimer);
        flush();
      }
    };
  }, [composerRuntime, sessionId]);

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
    <div className="border-t border-surface-800 bg-surface-900 px-4 pt-3 pb-3">
      <div className="mx-auto max-w-3xl xl:max-w-4xl 2xl:max-w-5xl">
        <ComposerPrimitive.Unstable_TriggerPopoverRoot>
          <ComposerPrimitive.Root
            className={[
              "group relative flex flex-col gap-2 rounded-xl border border-surface-700 bg-surface-850",
              "shadow-[inset_0_1px_0_rgba(255,255,255,0.02)]",
              "focus-within:border-brand-600/70 focus-within:shadow-[inset_0_1px_0_rgba(255,255,255,0.02),0_0_0_3px_rgba(217,119,6,0.12)]",
              "transition-colors duration-150",
            ].join(" ")}
          >
            {/* @ file picker — Directive behavior chips the path into
                the prompt text using the default formatter. */}
            <ComposerPrimitive.Unstable_TriggerPopover
              char="@"
              adapter={fileAdapter}
              className="absolute bottom-full left-0 right-0 mb-2 z-30 overflow-hidden rounded-lg border border-surface-700 bg-surface-850 shadow-xl"
            >
              <ComposerPrimitive.Unstable_TriggerPopover.Directive
                formatter={defaultDirectiveFormatter}
              />
              <PopoverItems trigger="@" />
            </ComposerPrimitive.Unstable_TriggerPopover>

            {/* / slash commands — Action behavior fires a handler and
                strips the `/cmd` text from the input. */}
            <ComposerPrimitive.Unstable_TriggerPopover
              char="/"
              adapter={slashAdapter}
              className="absolute bottom-full left-0 right-0 mb-2 z-30 overflow-hidden rounded-lg border border-surface-700 bg-surface-850 shadow-xl"
            >
              <ComposerPrimitive.Unstable_TriggerPopover.Action
                onExecute={(item) => insertSlashCommand(composerRuntime, item)}
                removeOnExecute
              />
              <PopoverItems trigger="/" />
            </ComposerPrimitive.Unstable_TriggerPopover>

            {/* Input area — tall by default, grows up to 200px */}
            <ComposerPrimitive.Input
              ref={taRef}
              rows={2}
              placeholder={
                turnActive
                  ? "Queue a follow-up… (sent when current turn ends)"
                  : "Send a message…  Type @ for files, / for commands"
              }
              onInput={onInput}
              onKeyDown={(e) => {
                // Intercept Enter while the agent is running so the
                // textarea can queue follow-ups; ComposerPrimitive.Input
                // hard-blocks Enter when `thread.isRunning && !queue`
                // (see ComposerInput.js in @assistant-ui/react). Without
                // this the textarea would do nothing on Enter mid-turn.
                // Capture-phase so our handler wins over the primitive's
                // bubble-phase listener.
                if (
                  !turnActive ||
                  e.key !== "Enter" ||
                  e.shiftKey ||
                  e.ctrlKey ||
                  e.metaKey ||
                  e.nativeEvent.isComposing
                ) {
                  return;
                }
                e.preventDefault();
                e.stopPropagation();
                void sendFromTextarea(taRef, composerRuntime, enqueuePrompt);
              }}
              autoFocus
              className={[
                "min-h-[56px] max-h-[200px] resize-none bg-transparent",
                "px-4 pt-3 pb-1 text-sm leading-6 text-text-primary",
                "placeholder:text-text-dim focus:outline-none",
              ].join(" ")}
            />

            {/* Footer strip — affordances on the left, send/stop on the right */}
            <div className="flex items-center justify-between gap-2 border-t border-surface-800/60 px-2 pb-2 pt-1.5">
              <div className="flex items-center gap-0.5">
                <ToolbarButton
                  icon={<AtSign className="h-3.5 w-3.5" />}
                  label="Add file context (@)"
                  hint="@"
                  onClick={() => insertAtCaret(taRef, "@")}
                />
                <ToolbarButton
                  icon={<Slash className="h-3.5 w-3.5" />}
                  label="Slash command (/)"
                  hint="/"
                  onClick={() => insertAtCaret(taRef, "/")}
                />
                <span className="mx-1 h-4 w-px bg-surface-700" aria-hidden />
                <ModePicker
                  sessionId={sessionId}
                  availableModes={availableModes}
                  currentModeId={currentModeId}
                  legacyMode={legacyMode}
                />
              </div>

              <div className="flex items-center gap-2">
                <UsageHint usage={sessionUsage} />
                {turnActive ? (
                  <>
                    <StopButton />
                    <QueueSendButton
                      disabled={!connected}
                      queuedCount={queuedCount}
                      onSend={() => sendFromTextarea(taRef, composerRuntime, enqueuePrompt)}
                    />
                  </>
                ) : (
                  <SendButton disabled={!connected} />
                )}
              </div>
            </div>
          </ComposerPrimitive.Root>
        </ComposerPrimitive.Unstable_TriggerPopoverRoot>
      </div>
    </div>
  );
}

/** Popover items list — same render shape for @ and / since both
 *  have a single category and we surface a flat list. */
function PopoverItems({ trigger }: { trigger: string }) {
  return (
    <ComposerPrimitive.Unstable_TriggerPopoverItems className="max-h-64 overflow-y-auto">
      {(items) =>
        items.length === 0 ? (
          <div className="px-3 py-2 text-xs italic text-text-dim">
            No matches
          </div>
        ) : (
          items.map((item, i) => (
            <ComposerPrimitive.Unstable_TriggerPopoverItem
              key={item.id}
              item={item}
              index={i}
              className={[
                "flex w-full items-start gap-2 px-3 py-2 text-left text-xs",
                "hover:bg-surface-800/60",
                "data-[highlighted=true]:bg-surface-800",
              ].join(" ")}
            >
              <span className="font-mono text-text-dim">{trigger}</span>
              <span className="min-w-0 flex-1">
                <span className="block truncate font-medium text-text-primary">
                  {item.label}
                </span>
                {item.description && (
                  <span className="block truncate text-[11px] text-text-dim">
                    {item.description}
                  </span>
                )}
              </span>
            </ComposerPrimitive.Unstable_TriggerPopoverItem>
          ))
        )
      }
    </ComposerPrimitive.Unstable_TriggerPopoverItems>
  );
}

/** Insert the picked slash command into the composer text. The Action
 *  popover already stripped the user's `/<typed>` from the input via
 *  `removeOnExecute`, so we set the canonical `/<name>` form and add
 *  a trailing space when the agent advertised that the command takes
 *  free-form arguments. The user is then free to type args and hit
 *  Enter to send, or hit Enter immediately for a no-arg command. */
function insertSlashCommand(
  runtime: ReturnType<typeof useComposerRuntime>,
  item: Unstable_TriggerItem,
) {
  if (!runtime) return;
  const accepts = (item as { acceptsInput?: boolean }).acceptsInput === true;
  const current = runtime.getState().text;
  const suffix = accepts ? " " : "";
  // Preserve any text that was already in the buffer (e.g. user typed
  // a long prompt then ran `/foo` mid-message). We just append the
  // command at the end; the typed `/typed` token has already been
  // removed by removeOnExecute, so trailing whitespace is rare.
  const sep = current.length > 0 && !current.endsWith(" ") ? " " : "";
  runtime.setText(`${current}${sep}/${item.id}${suffix}`);
}

/** Insert `text` at the textarea's caret and re-focus. The toolbar
 *  buttons use this to inject `@` or `/` so the trigger popover opens
 *  without forcing the user to grab the keyboard. */
function insertAtCaret(
  ref: React.RefObject<HTMLTextAreaElement | null>,
  text: string,
) {
  const ta = ref.current;
  if (!ta) return;
  const start = ta.selectionStart ?? ta.value.length;
  const end = ta.selectionEnd ?? start;
  const before = ta.value.slice(0, start);
  // Trigger detection requires whitespace (or start-of-string) before
  // the trigger char; pad if we're mid-word.
  const needsSpace =
    before.length > 0 && !/[\s\n\t]$/.test(before) ? " " : "";
  const next = before + needsSpace + text + ta.value.slice(end);
  const setter = Object.getOwnPropertyDescriptor(
    HTMLTextAreaElement.prototype,
    "value",
  )?.set;
  setter?.call(ta, next);
  ta.dispatchEvent(new Event("input", { bubbles: true }));
  const pos = before.length + needsSpace.length + text.length;
  ta.focus();
  ta.setSelectionRange(pos, pos);
}

function extDescription(path: string): string | undefined {
  const m = path.match(/\.([a-z0-9]+)$/i);
  return m?.[1]?.toLowerCase();
}

/* ── Toolbar buttons ─────────────────────────────────────────────── */

function ToolbarButton({
  icon,
  label,
  hint,
  disabled,
  onClick,
}: {
  icon: React.ReactNode;
  label: string;
  hint?: string;
  disabled?: boolean;
  onClick?: () => void;
}) {
  return (
    <button
      type="button"
      title={label}
      aria-label={label}
      disabled={disabled}
      onClick={onClick}
      className={[
        "inline-flex items-center gap-1 rounded-md px-2 py-1 text-[11px] text-text-dim",
        "hover:bg-surface-800 hover:text-text-secondary",
        "disabled:cursor-not-allowed disabled:opacity-60 disabled:hover:bg-transparent disabled:hover:text-text-dim",
        "transition-colors",
      ].join(" ")}
    >
      {icon}
      {hint && <span className="font-mono">{hint}</span>}
    </button>
  );
}

/* ── Mode picker ─────────────────────────────────────────────────── */

const LEGACY_MODES: ReadonlyArray<{
  id: string;
  legacyId: CockpitState["mode"];
  name: string;
  description: string;
}> = [
  { id: "default", legacyId: "Default", name: "Default", description: "Approve each tool individually" },
  { id: "plan", legacyId: "Plan", name: "Plan", description: "Plan first, no edits applied" },
  { id: "accept_edits", legacyId: "AcceptEdits", name: "Accept edits", description: "Auto-approve safe file edits" },
  { id: "bypass_permissions", legacyId: "BypassPermissions", name: "Yolo", description: "Skip all approvals (destructive)" },
];

interface ModePickerProps {
  sessionId: string;
  availableModes: CockpitState["availableModes"];
  currentModeId: string | null;
  legacyMode: CockpitState["mode"];
}

function ModePicker({
  sessionId,
  availableModes,
  currentModeId,
  legacyMode,
}: ModePickerProps) {
  const [open, setOpen] = useState(false);
  const ref = useRef<HTMLDivElement | null>(null);

  // Use real agent-advertised modes when available, otherwise fall
  // back to the four-mode taxonomy. Even with agent modes, we still
  // tint by id pattern (default/plan/accept/bypass) because Claude's
  // adapter happens to use those tokens.
  const usingAgentModes = availableModes.length > 0;
  const modes = usingAgentModes
    ? availableModes.map((m) => ({
        id: m.id,
        name: m.name,
        description: m.description ?? "",
      }))
    : LEGACY_MODES.map((m) => ({
        id: m.id,
        name: m.name,
        description: m.description,
      }));

  // Pick "current": agent-reported id wins; else map legacyMode → id.
  const fallbackId =
    LEGACY_MODES.find((m) => m.legacyId === legacyMode)?.id ?? "default";
  const activeId = currentModeId ?? fallbackId;
  const current = modes.find((m) => m.id === activeId) ?? modes[0]!;

  // Tint the chip by id pattern so destructive modes are visually loud.
  const tone = toneForId(activeId);

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

  const select = async (id: string) => {
    setOpen(false);
    if (id === activeId) return;
    try {
      await fetch(
        `/api/sessions/${encodeURIComponent(sessionId)}/cockpit/mode`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ mode_id: id }),
        },
      );
    } catch {
      // The agent broadcasts CurrentModeChanged on success; if the
      // request fails the UI stays on the current mode.
    }
  };

  return (
    <div ref={ref} className="relative">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        title={current.description || `Mode: ${current.name}`}
        className={[
          "inline-flex items-center gap-1 rounded-md border px-2 py-1 text-[11px] font-medium",
          "transition-colors",
          tone,
        ].join(" ")}
      >
        <span>{current.name}</span>
        <ChevronUp className="h-3 w-3 opacity-70" />
      </button>
      {open && (
        <div
          className="absolute bottom-full left-0 z-30 mb-1 w-56 overflow-hidden rounded-md border border-surface-700 bg-surface-850 shadow-xl"
          role="menu"
        >
          <div className="border-b border-surface-800 px-3 py-1.5 text-[10px] uppercase tracking-wider text-text-dim">
            {usingAgentModes ? "Agent modes" : "Modes"}
          </div>
          {modes.map((opt) => (
            <button
              key={opt.id}
              type="button"
              role="menuitem"
              onClick={() => void select(opt.id)}
              className={[
                "flex w-full items-start gap-2 px-3 py-2 text-left text-xs hover:bg-surface-800",
                opt.id === activeId ? "bg-surface-800/60" : "",
              ].join(" ")}
            >
              <span
                className={[
                  "mt-0.5 inline-block h-3 w-3 shrink-0 rounded-full border",
                  opt.id === activeId
                    ? "border-brand-500 bg-brand-500"
                    : "border-surface-700",
                ].join(" ")}
              />
              <span className="min-w-0 flex-1">
                <span className="block font-medium text-text-primary">
                  {opt.name}
                </span>
                {opt.description && (
                  <span className="block text-[11px] text-text-dim">
                    {opt.description}
                  </span>
                )}
              </span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function toneForId(id: string): string {
  if (/bypass|yolo/i.test(id))
    return "border-rose-700/50 bg-rose-950/30 text-rose-300 hover:border-rose-700";
  if (/accept/i.test(id))
    return "border-amber-700/50 bg-amber-950/30 text-amber-300 hover:border-amber-700";
  if (/plan/i.test(id))
    return "border-cyan-800/50 bg-cyan-950/30 text-cyan-300 hover:border-cyan-700";
  return "border-surface-700 bg-surface-800 text-text-secondary hover:border-surface-600";
}

/* ── Usage hint ──────────────────────────────────────────────────── */

function UsageHint({ usage }: { usage: CockpitState["sessionUsage"] }) {
  if (!usage || usage.size <= 0) return null;
  const pct = Math.min(100, Math.round((usage.used / usage.size) * 100));
  const tone =
    pct >= 90
      ? "text-rose-400"
      : pct >= 75
        ? "text-amber-400"
        : "text-text-dim";
  const usedLabel = formatTokens(usage.used);
  const sizeLabel = formatTokens(usage.size);
  const cost = usage.cost
    ? formatCost(usage.cost.amount, usage.cost.currency)
    : null;
  const title =
    `Context: ${usage.used.toLocaleString()} / ${usage.size.toLocaleString()} tokens (${pct}%)` +
    (cost ? ` · session cost ${cost}` : "");
  return (
    <span
      className={`hidden sm:inline-flex items-center gap-1 text-[11px] tabular-nums ${tone}`}
      title={title}
      aria-label={title}
    >
      <span>
        {usedLabel}/{sizeLabel}
      </span>
      <span className="opacity-70">({pct}%)</span>
      {cost ? <span className="opacity-70">· {cost}</span> : null}
    </span>
  );
}

function formatTokens(n: number): string {
  if (n < 1_000) return String(n);
  if (n < 1_000_000) return `${(n / 1_000).toFixed(n < 10_000 ? 1 : 0)}k`;
  return `${(n / 1_000_000).toFixed(n < 10_000_000 ? 2 : 1)}M`;
}

function formatCost(amount: number, currency: string): string {
  try {
    return new Intl.NumberFormat(undefined, {
      style: "currency",
      currency,
      maximumFractionDigits: amount < 1 ? 4 : 2,
    }).format(amount);
  } catch {
    return `${amount.toFixed(amount < 1 ? 4 : 2)} ${currency}`;
  }
}

/* ── Send / Stop ─────────────────────────────────────────────────── */

function SendButton({ disabled = false }: { disabled?: boolean }) {
  // When the WS is closed we surface the offline state via `disabled`
  // and a swapped tooltip; ComposerPrimitive.Send would still try to
  // dispatch otherwise (it only knows about thread-runtime state, not
  // our connection status). TODO: queue prompts locally and flush on
  // reconnect instead of dropping them.
  return (
    <ComposerPrimitive.Send asChild>
      <button
        type="submit"
        aria-label="Send message"
        title={disabled ? "Disconnected — reconnect to send" : "Send · Enter"}
        disabled={disabled}
        className={[
          "group/send inline-flex items-center justify-center gap-1",
          "rounded-lg bg-brand-600 px-2.5 py-1.5 text-white shadow-sm",
          "hover:bg-brand-500 active:scale-[0.98]",
          "disabled:cursor-not-allowed disabled:bg-surface-700 disabled:text-text-dim disabled:shadow-none",
          "transition-all duration-100",
        ].join(" ")}
      >
        <PaperPlaneIcon />
      </button>
    </ComposerPrimitive.Send>
  );
}

function StopButton() {
  const runtime = useThreadRuntime();
  return (
    <button
      type="button"
      aria-label="Stop"
      title="Stop the agent · Esc"
      onClick={() => runtime.cancelRun()}
      className={[
        "inline-flex items-center justify-center gap-1.5",
        "rounded-lg border border-surface-600 bg-surface-800",
        "px-2.5 py-1.5 text-[12px] font-medium text-text-secondary",
        "hover:border-rose-700/60 hover:bg-rose-950/30 hover:text-rose-300",
        "active:scale-[0.98] transition-all duration-100",
      ].join(" ")}
    >
      <Square className="h-3.5 w-3.5 fill-current" strokeWidth={0} />
      <span>Stop</span>
    </button>
  );
}

/** Send button rendered alongside Stop while a turn is in flight.
 *  Bypasses ComposerPrimitive.Send (which is disabled by the SDK when
 *  the thread is running). Shows a small badge with the current queue
 *  length so users can see at a glance how many follow-ups are stacked
 *  up. See #1031. */
function QueueSendButton({
  disabled,
  queuedCount,
  onSend,
}: {
  disabled: boolean;
  queuedCount: number;
  onSend: () => void;
}) {
  const title = disabled
    ? "Disconnected, reconnect to send"
    : queuedCount > 0
      ? `Queue follow-up (${queuedCount} pending) · Enter`
      : "Queue follow-up (sent when current turn ends) · Enter";
  return (
    <button
      type="button"
      aria-label="Queue follow-up message"
      title={title}
      disabled={disabled}
      onClick={() => {
        if (!disabled) onSend();
      }}
      className={[
        "group/send relative inline-flex items-center justify-center gap-1",
        "rounded-lg bg-brand-600 px-2.5 py-1.5 text-white shadow-sm",
        "hover:bg-brand-500 active:scale-[0.98]",
        "disabled:cursor-not-allowed disabled:bg-surface-700 disabled:text-text-dim disabled:shadow-none",
        "transition-all duration-100",
      ].join(" ")}
    >
      <PaperPlaneIcon />
      {queuedCount > 0 && (
        <span
          aria-hidden
          className={[
            "absolute -right-1.5 -top-1.5 inline-flex h-4 min-w-[16px] items-center justify-center",
            "rounded-full bg-amber-500 px-1 text-[10px] font-semibold text-surface-900",
            "ring-2 ring-surface-900",
          ].join(" ")}
        >
          {queuedCount}
        </span>
      )}
    </button>
  );
}

/** Pulls current composer text, hands it to the cockpit queue, then
 *  clears the textarea + persisted draft. Shared by the mid-turn Send
 *  button and the Enter-while-running keyboard handler. */
function sendFromTextarea(
  taRef: React.RefObject<HTMLTextAreaElement | null>,
  composerRuntime: ReturnType<typeof useComposerRuntime>,
  enqueuePrompt: (text: string) => void | Promise<void>,
): void {
  if (!composerRuntime) return;
  const text = composerRuntime.getState().text.trim();
  if (!text) return;
  void enqueuePrompt(text);
  composerRuntime.setText("");
  // Manually reset the textarea height; auto-grow runs on input events
  // and we cleared the value without firing one.
  const el = taRef.current;
  if (el) el.style.height = "auto";
}

function PaperPlaneIcon() {
  return (
    <svg
      viewBox="0 0 24 24"
      width="14"
      height="14"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <path d="M22 2 11 13" />
      <path d="M22 2 15 22l-4-9-9-4 20-7Z" />
    </svg>
  );
}
