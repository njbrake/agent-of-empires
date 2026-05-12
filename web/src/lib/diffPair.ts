// Convert an `(old_string, new_string)` pair into a `RichDiffHunk`
// plus add/del counts, so the cockpit Edit/Write card can drive its
// body and its `+N −N` chip off a single line-diff pass. Same
// `diff` engine used elsewhere in the tree. See #1073 / #1074.

import { diffLines } from "diff";
import type { RichDiffHunk, RichDiffLine } from "./types";

export interface DiffPairResult {
  hunk: RichDiffHunk;
  adds: number;
  dels: number;
}

/** Force a single trailing newline so diffLines doesn't treat
 *  "last line without `\n`" as a distinct token from "same line with
 *  `\n`". Without this, `"a\nb\nc"` vs `"a\nb\nc\nd"` registers as
 *  remove("c") + add("c\nd\n") instead of add("d\n"). */
function withTrailingNewline(s: string): string {
  if (s === "") return s;
  return s.endsWith("\n") ? s : s + "\n";
}

/** Run a line-level diff over the pair and emit a `RichDiffHunk`
 *  shaped the same way the file-diff endpoint does, plus the running
 *  add/del tallies. Snippet line numbers start at 1 on each side. */
export function diffPair(oldText: string, newText: string): DiffPairResult {
  if (oldText === "" && newText === "") {
    return {
      hunk: {
        old_start: 0,
        old_lines: 0,
        new_start: 0,
        new_lines: 0,
        lines: [],
      },
      adds: 0,
      dels: 0,
    };
  }

  const parts = diffLines(
    withTrailingNewline(oldText),
    withTrailingNewline(newText),
  );

  const lines: RichDiffLine[] = [];
  let oldNum = 1;
  let newNum = 1;
  let adds = 0;
  let dels = 0;

  for (const part of parts) {
    const trimmed = part.value.endsWith("\n")
      ? part.value.slice(0, -1)
      : part.value;
    if (trimmed === "") continue;
    for (const content of trimmed.split("\n")) {
      if (part.added) {
        lines.push({
          type: "add",
          old_line_num: null,
          new_line_num: newNum++,
          content,
        });
        adds += 1;
      } else if (part.removed) {
        lines.push({
          type: "delete",
          old_line_num: oldNum++,
          new_line_num: null,
          content,
        });
        dels += 1;
      } else {
        lines.push({
          type: "equal",
          old_line_num: oldNum++,
          new_line_num: newNum++,
          content,
        });
      }
    }
  }

  const oldLines = oldNum - 1;
  const newLines = newNum - 1;

  return {
    hunk: {
      old_start: oldLines > 0 ? 1 : 0,
      old_lines: oldLines,
      new_start: newLines > 0 ? 1 : 0,
      new_lines: newLines,
      lines,
    },
    adds,
    dels,
  };
}
