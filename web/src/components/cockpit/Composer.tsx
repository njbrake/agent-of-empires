// VSCode/Cursor-style composer for the cockpit.
//
// Built on assistant-ui's `<ComposerPrimitive.Root>` so message-edit
// affordances (Esc to cancel, draft persistence, send-on-Enter) come
// for free; the chrome around it (tall multi-line input, top toolbar
// with @/​/ affordances, footer strip with model placeholder + send/
// stop icon button) is ours.
//
// References we matched against:
//   - VSCode chat composer (Add Context chip, model picker, paper-plane
//     send button, Cmd+Enter hint inside the button)
//   - Cursor chat composer (subtle inner well, @ mention chip on the
//     bottom row, single arrow send icon)
//
// Icons via lucide-react (same family VSCode/Cursor visually feel like —
// Lucide is the rebrand of Feather).

import { ComposerPrimitive, ThreadPrimitive, useThreadRuntime } from "@assistant-ui/react";
import { useEffect, useMemo, useRef } from "react";
import {
  AtSign,
  CornerDownLeft,
  Paperclip,
  Slash,
  Square,
} from "lucide-react";

import {
  TriggerPopover,
  fuzzyFilter,
  useFilesIndex,
  type PickerItem,
} from "./TriggerPopover";

const SLASH_COMMANDS: ReadonlyArray<PickerItem> = [
  {
    id: "help",
    label: "/help",
    hint: "Show available commands",
    insert: "/help",
  },
  {
    id: "clear",
    label: "/clear",
    hint: "Reset conversation context",
    insert: "/clear",
  },
  {
    id: "tools",
    label: "/tools",
    hint: "List the agent's tools",
    insert: "/tools",
  },
  {
    id: "model",
    label: "/model",
    hint: "Show or switch the model",
    insert: "/model",
  },
];

interface Props {
  sessionId: string;
}

export function Composer({ sessionId }: Props) {
  const taRef = useRef<HTMLTextAreaElement | null>(null);
  const { files } = useFilesIndex(sessionId);
  const searchFiles = useMemo(
    () => (query: string) => fuzzyFilter(files, query, 30),
    [files],
  );
  const searchSlash = useMemo(
    () => (query: string) => fuzzyFilter([...SLASH_COMMANDS], query, 30),
    [],
  );

  // Auto-grow up to ~6 visible lines.
  const onInput = (e: React.FormEvent<HTMLTextAreaElement>) => {
    const el = e.currentTarget;
    el.style.height = "auto";
    el.style.height = `${Math.min(el.scrollHeight, 200)}px`;
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
    <div className="border-t border-surface-800 bg-surface-900 px-4 pt-3 pb-3">
      <div className="mx-auto max-w-3xl">
        <ComposerPrimitive.Root
          className={[
            "group relative flex flex-col gap-2 rounded-xl border border-surface-700 bg-surface-850",
            "shadow-[inset_0_1px_0_rgba(255,255,255,0.02)]",
            "focus-within:border-brand-600/70 focus-within:shadow-[inset_0_1px_0_rgba(255,255,255,0.02),0_0_0_3px_rgba(217,119,6,0.12)]",
            "transition-colors duration-150",
          ].join(" ")}
        >
          {/* @-mention file picker + /-slash command popover. Both
              hang off the same textarea ref and only one ever shows
              at a time (the trigger detection is mutually exclusive
              by leading character). */}
          <TriggerPopover
            trigger="@"
            textareaRef={taRef}
            search={searchFiles}
          />
          <TriggerPopover
            trigger="/"
            textareaRef={taRef}
            search={searchSlash}
          />

          {/* Input area — tall by default, grows up to 200px */}
          <ComposerPrimitive.Input
            ref={taRef}
            rows={2}
            placeholder="Send a message…  Type @ for files, / for commands"
            onInput={onInput}
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
              <ToolbarButton
                icon={<Paperclip className="h-3.5 w-3.5" />}
                label="Attach (coming soon)"
                disabled
              />
            </div>

            <div className="flex items-center gap-2">
              <kbd className="hidden md:inline-flex items-center gap-1 rounded border border-surface-700 bg-surface-800/80 px-1.5 py-0.5 font-mono text-[10px] text-text-dim">
                <CornerDownLeft className="h-3 w-3" />
                <span>Send</span>
              </kbd>
              <ThreadPrimitive.If running>
                <StopButton />
              </ThreadPrimitive.If>
              <ThreadPrimitive.If running={false}>
                <SendButton />
              </ThreadPrimitive.If>
            </div>
          </div>
        </ComposerPrimitive.Root>
      </div>
    </div>
  );
}

/** Insert `text` at the textarea's caret and re-focus. The toolbar
 *  buttons use this to inject `@` or `/` so the trigger popover opens
 *  without forcing the user to grab the keyboard.
 */
function insertAtCaret(
  ref: React.RefObject<HTMLTextAreaElement | null>,
  text: string,
) {
  const ta = ref.current;
  if (!ta) return;
  const start = ta.selectionStart ?? ta.value.length;
  const end = ta.selectionEnd ?? start;
  const before = ta.value.slice(0, start);
  // Trigger detection requires whitespace (or start-of-string)
  // before the trigger char; pad if we're mid-word.
  const needsSpace =
    before.length > 0 && !/[\s\n\t]$/.test(before) ? " " : "";
  const next = before + needsSpace + text + ta.value.slice(end);
  // Use the native setter so React picks the change up via input event.
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

/* ── Send / Stop ─────────────────────────────────────────────────── */

function SendButton() {
  return (
    <ComposerPrimitive.Send asChild>
      <button
        type="submit"
        aria-label="Send message"
        title="Send · Enter"
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

/**
 * Custom paper-plane glyph that points up-and-right (the ubiquitous
 * "send" icon). Sized to match lucide's stroke language so it sits
 * beside the toolbar icons cleanly.
 */
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
