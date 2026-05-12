// Embedded inline diff renderer for an `(old_string, new_string)`
// pair. Used inside the cockpit Edit/Write card; reuses the same
// `DiffLine` row and `useHighlightedLines` token grid as the branch
// diff viewer so the two surfaces look the same. See #1073.

import { useMemo } from "react";
import { useHighlightedLines } from "../../hooks/useHighlightedLines";
import { diffPair } from "../../lib/diffPair";
import type { RichDiffHunk } from "../../lib/types";
import { DiffLine } from "./DiffLine";

interface Props {
  oldText: string;
  newText: string;
  /** File path used for Shiki language detection. Pass even when the
   *  file is virtual (`(unknown file)`); the hook safely no-ops when
   *  no grammar matches. */
  filePath: string;
}

export function StringDiff({ oldText, newText, filePath }: Props) {
  const hunk: RichDiffHunk = useMemo(
    () => diffPair(oldText, newText).hunk,
    [oldText, newText],
  );
  const hunks = useMemo(() => [hunk], [hunk]);
  const { tokens, loading } = useHighlightedLines(hunks, filePath);

  if (hunk.lines.length === 0) return null;

  const lineTokens = tokens?.[0];

  return (
    <div className="leading-[1.6]">
      {hunk.lines.map((line, i) => (
        <DiffLine
          key={`${line.old_line_num ?? "_"}-${line.new_line_num ?? "_"}-${i}`}
          line={line}
          tokens={lineTokens?.[i]}
          highlightPending={loading}
          hideLineNumbers
        />
      ))}
    </div>
  );
}
