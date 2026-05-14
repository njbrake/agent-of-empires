import type { RichDiffHunk } from "../../../lib/types";
import type { DiffSide } from "./types";

interface Extraction {
  snippet: string;
  hunkIndex: number;
  endRowIndex: number;
}

/** Walks each hunk and pulls the contiguous side-filtered content
 *  for the requested line range. Returns null when any line in the
 *  range is missing from the diff.
 *
 *  Side semantics:
 *  - `new`: keeps rows with `new_line_num != null` (added + equal), skips pure deletes.
 *  - `old`: keeps rows with `old_line_num != null` (deleted + equal), skips pure adds.
 *
 *  The contiguous-hunk rule means a range may not span multiple hunks
 *  (the caller enforces single-hunk selection in the UI). */
export function extractSnippetFromHunks(
  hunks: RichDiffHunk[],
  side: DiffSide,
  startLine: number,
  endLine: number,
): Extraction | null {
  const lo = Math.min(startLine, endLine);
  const hi = Math.max(startLine, endLine);
  const lineKey = side === "new" ? "new_line_num" : "old_line_num";

  for (let hi_idx = 0; hi_idx < hunks.length; hi_idx++) {
    const hunk = hunks[hi_idx];
    const hunkStart = side === "new" ? hunk.new_start : hunk.old_start;
    const hunkEnd =
      hunkStart + (side === "new" ? hunk.new_lines : hunk.old_lines) - 1;
    if (hunkEnd < lo || hunkStart > hi) continue;
    if (hunkStart > lo || hunkEnd < hi) {
      // Range straddles the hunk's boundary; reject to keep extraction
      // single-hunk and avoid silently merging unrelated context.
      return null;
    }

    const seen = new Set<number>();
    const lines: string[] = [];
    let endRowIndex = -1;
    for (let row = 0; row < hunk.lines.length; row++) {
      const line = hunk.lines[row];
      const num = line[lineKey];
      if (num == null) continue;
      if (num < lo || num > hi) continue;
      seen.add(num);
      lines.push(stripTrailingNewline(line.content));
      endRowIndex = row;
    }

    for (let n = lo; n <= hi; n++) {
      if (!seen.has(n)) return null;
    }

    return {
      snippet: lines.join("\n"),
      hunkIndex: hi_idx,
      endRowIndex,
    };
  }

  return null;
}

function stripTrailingNewline(s: string): string {
  return s.replace(/\r?\n$/, "");
}
