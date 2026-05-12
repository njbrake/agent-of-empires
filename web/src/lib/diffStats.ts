// Line-level diff counts for the Edit/Write card `+N −N` chip. Uses
// `diff` (already the engine behind `react-diff-viewer-continued`) so
// the chip and the rendered diff body agree by construction. The naive
// `oldText.split("\n").length` / `newText.split("\n").length` it
// replaces double-counted context lines as both add and del. See #1074.

import { diffLines } from "diff";

export interface DiffStats {
  adds: number;
  dels: number;
}

/** Force a single trailing newline so diffLines doesn't treat
 *  "last line without `\n`" as a distinct token from "same line with
 *  `\n`". Without this, `"a\nb\nc"` vs `"a\nb\nc\nd"` registers as
 *  remove("c") + add("c\nd\n") instead of add("d\n"). The diff viewer
 *  normalises the same way internally, so we match its grain. */
function withTrailingNewline(s: string): string {
  if (s === "") return s;
  return s.endsWith("\n") ? s : s + "\n";
}

/** Count added and removed lines between `oldText` and `newText`.
 *  Context lines (present on both sides) count as neither. Pure-empty
 *  inputs on both sides return `{ adds: 0, dels: 0 }`. */
export function lineDiffCounts(oldText: string, newText: string): DiffStats {
  if (oldText === "" && newText === "") return { adds: 0, dels: 0 };
  const parts = diffLines(
    withTrailingNewline(oldText),
    withTrailingNewline(newText),
  );
  let adds = 0;
  let dels = 0;
  for (const p of parts) {
    if (p.added) adds += p.count ?? 0;
    else if (p.removed) dels += p.count ?? 0;
  }
  return { adds, dels };
}
