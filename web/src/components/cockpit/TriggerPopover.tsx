// Generic trigger-popover for the composer.
//
// When the user types `@` or `/` at a word boundary, we open a popover
// listing matching items (files for `@`, slash commands for `/`).
// Arrow keys move the selection; Enter/Tab inserts; Esc closes.
//
// The picker reads the textarea's value via useComposerRuntime() and
// tracks the active trigger via the caret position. On select, we
// replace `@xxx` (or `/yyy`) with the rendered token (e.g. `@path/to/
// file.ts`) and let the agent see plain text.
//
// We deliberately do NOT use a portal — the popover is positioned
// `absolute` above the composer to keep stacking simple. The composer
// is at the bottom of the viewport, so anchoring the popover to the
// top edge of the composer card looks like an overlay menu.

import { useComposerRuntime } from "@assistant-ui/react";
import { useEffect, useMemo, useRef, useState } from "react";

interface PickerItem {
  /** Stable id for keyed rendering. */
  id: string;
  /** What the user sees in the list. */
  label: string;
  /** Optional secondary line (file extension, command description). */
  hint?: string;
  /** Inserted into the prompt when chosen, replacing `@query` /
   *  `/query`. The trigger char is included (so an `@` item should
   *  return e.g. `@path/to/file.ts`). */
  insert: string;
}

interface Props {
  /** Trigger character that opens this popover (`@` or `/`). */
  trigger: string;
  /** Anchor element — usually the composer textarea. */
  textareaRef: React.RefObject<HTMLTextAreaElement | null>;
  /** Provide items for a given query string. May be async. */
  search: (query: string) => Promise<PickerItem[]> | PickerItem[];
  /** Optional null to show before the user types (e.g. recent files). */
  initialItems?: PickerItem[];
}

interface ActiveTrigger {
  /** Position of the trigger character in the textarea value. */
  start: number;
  /** Caret position (end of query). */
  end: number;
  /** The query (text after the trigger up to the caret). */
  query: string;
}

export function TriggerPopover({
  trigger,
  textareaRef,
  search,
  initialItems,
}: Props) {
  const composer = useComposerRuntime();
  const [active, setActive] = useState<ActiveTrigger | null>(null);
  const [items, setItems] = useState<PickerItem[]>(initialItems ?? []);
  const [selected, setSelected] = useState(0);
  const reqId = useRef(0);

  // Track the textarea's value + selection on every input/keyup.
  useEffect(() => {
    const ta = textareaRef.current;
    if (!ta) return;
    const detect = () => {
      const value = ta.value;
      const caret = ta.selectionStart ?? value.length;
      // Walk back from the caret to find a trigger char that's
      // preceded by whitespace or start-of-string; abort on
      // whitespace so multi-word queries close the popover.
      let i = caret;
      while (i > 0) {
        const ch = value[i - 1];
        if (ch === trigger) {
          const before = i >= 2 ? value[i - 2] : "";
          if (i === 1 || before === " " || before === "\n" || before === "\t") {
            const query = value.slice(i, caret);
            // Allow word chars + `/` + `.` + `-` + `_` in queries.
            if (/^[\w./-]*$/.test(query)) {
              setActive({ start: i - 1, end: caret, query });
              return;
            }
          }
          break;
        }
        if (ch === " " || ch === "\n" || ch === "\t") break;
        i--;
      }
      setActive(null);
    };
    ta.addEventListener("input", detect);
    ta.addEventListener("keyup", detect);
    ta.addEventListener("click", detect);
    ta.addEventListener("focus", detect);
    return () => {
      ta.removeEventListener("input", detect);
      ta.removeEventListener("keyup", detect);
      ta.removeEventListener("click", detect);
      ta.removeEventListener("focus", detect);
    };
  }, [textareaRef, trigger]);

  // Refresh items when the query changes.
  useEffect(() => {
    if (!active) return;
    const my = ++reqId.current;
    Promise.resolve(search(active.query)).then((next) => {
      if (reqId.current !== my) return;
      setItems(next);
      setSelected(0);
    });
  }, [active, search]);

  // Keyboard navigation: ↑/↓/Enter/Tab/Esc on the textarea while open.
  useEffect(() => {
    const ta = textareaRef.current;
    if (!ta || !active) return;
    const onKey = (e: KeyboardEvent) => {
      if (!active) return;
      if (e.key === "ArrowDown") {
        e.preventDefault();
        setSelected((s) => Math.min(s + 1, Math.max(items.length - 1, 0)));
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        setSelected((s) => Math.max(s - 1, 0));
      } else if (e.key === "Enter" || e.key === "Tab") {
        const pick = items[selected];
        if (pick) {
          e.preventDefault();
          insert(pick);
        }
      } else if (e.key === "Escape") {
        e.preventDefault();
        setActive(null);
      }
    };
    ta.addEventListener("keydown", onKey);
    return () => ta.removeEventListener("keydown", onKey);
  }, [active, items, selected, textareaRef]);

  const insert = (item: PickerItem) => {
    const ta = textareaRef.current;
    if (!ta || !active) return;
    const value = ta.value;
    const before = value.slice(0, active.start);
    const after = value.slice(active.end);
    const trailing = after.startsWith(" ") || after === "" ? "" : " ";
    const next = `${before}${item.insert}${trailing}${after}`;
    // Push through assistant-ui's composer state so isEmpty / draft
    // tracking stays consistent.
    composer.setText(next);
    // Restore caret immediately after the inserted token + space.
    queueMicrotask(() => {
      const pos = (before + item.insert + trailing).length;
      ta.focus();
      ta.setSelectionRange(pos, pos);
    });
    setActive(null);
  };

  if (!active) return null;
  const list = items.length > 0 ? items : null;

  return (
    <div
      className={[
        "absolute bottom-full left-0 right-0 mb-2 z-30",
        "max-h-64 overflow-y-auto rounded-lg border border-surface-700",
        "bg-surface-850 shadow-xl",
      ].join(" ")}
      role="listbox"
      aria-label={`${trigger}-completions`}
    >
      {list ? (
        list.map((it, i) => (
          <button
            key={it.id}
            type="button"
            role="option"
            aria-selected={i === selected}
            onMouseEnter={() => setSelected(i)}
            onMouseDown={(e) => {
              // Use mousedown so the textarea doesn't lose focus to
              // the click before our insert runs.
              e.preventDefault();
              insert(it);
            }}
            className={[
              "flex w-full items-start gap-2 px-3 py-2 text-left text-xs",
              i === selected ? "bg-surface-800" : "hover:bg-surface-800/60",
            ].join(" ")}
          >
            <span className="font-mono text-text-dim">{trigger}</span>
            <span className="min-w-0 flex-1">
              <span className="block truncate font-medium text-text-primary">
                {it.label}
              </span>
              {it.hint && (
                <span className="block truncate text-[11px] text-text-dim">
                  {it.hint}
                </span>
              )}
            </span>
          </button>
        ))
      ) : (
        <div className="px-3 py-2 text-xs italic text-text-dim">
          {`No matches for ${trigger}${active.query}`}
        </div>
      )}
    </div>
  );
}

/* ── Useful shared search helpers ────────────────────────────────── */

/** Lightweight fuzzy filter: prefer prefix matches, then substring. */
export function fuzzyFilter<T extends { label: string; hint?: string }>(
  items: T[],
  query: string,
  cap = 30,
): T[] {
  const q = query.toLowerCase();
  if (!q) return items.slice(0, cap);
  return items
    .map((it) => {
      const label = it.label.toLowerCase();
      const hint = it.hint?.toLowerCase() ?? "";
      if (label.startsWith(q)) return { it, score: 0 };
      if (label.includes(q)) return { it, score: 1 };
      if (hint.includes(q)) return { it, score: 2 };
      return { it, score: 99 };
    })
    .filter((x) => x.score < 99)
    .sort((a, b) => a.score - b.score || a.it.label.length - b.it.label.length)
    .slice(0, cap)
    .map((x) => x.it);
}

/** Memoize a one-shot async fetch for the lifetime of a component. */
export function useFilesIndex(sessionId: string): {
  files: PickerItem[];
  loading: boolean;
} {
  const [files, setFiles] = useState<PickerItem[]>([]);
  const [loading, setLoading] = useState(true);
  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    fetch(
      `/api/sessions/${encodeURIComponent(sessionId)}/cockpit/files`,
    )
      .then((r) => (r.ok ? r.json() : { files: [] }))
      .then((data: { files?: string[] }) => {
        if (cancelled) return;
        setFiles(
          (data.files ?? []).map((path) => ({
            id: path,
            label: path,
            hint: extDescription(path),
            insert: `@${path}`,
          })),
        );
      })
      .catch(() => {
        if (!cancelled) setFiles([]);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [sessionId]);
  return useMemo(() => ({ files, loading }), [files, loading]);
}

function extDescription(path: string): string | undefined {
  const m = path.match(/\.([a-z0-9]+)$/i);
  return m?.[1]?.toLowerCase();
}

export type { PickerItem };
