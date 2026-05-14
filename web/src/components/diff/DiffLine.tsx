import { memo, type ReactNode } from "react";
import type { SyntaxToken } from "../../hooks/useHighlightedLines";
import type { RichDiffLine } from "../../lib/types";

interface Props {
  line: RichDiffLine;
  tokens?: SyntaxToken[];
  /** True while Shiki is loading; hides content to avoid a flash of unstyled text. */
  highlightPending?: boolean;
  /** Hide the per-side line-number gutters. Used inside compact embedded
   *  diffs (e.g. the cockpit Edit card) where snippet line numbers add
   *  more clutter than signal. */
  hideLineNumbers?: boolean;
  /** Optional overlay rendered inside the line-number gutter (in front of
   *  the numbers). Used by the diff comments feature to surface a "+"
   *  button on hover without resizing the row. See #928. */
  leadingSlot?: ReactNode;
  /** Tint the row to indicate it's part of the active comment range. */
  isHighlighted?: boolean;
  /** Mark the row as the start/end endpoint of a range with a stronger
   *  left border. */
  isRangeEndpoint?: boolean;
}

function DiffLineImpl({
  line,
  tokens,
  highlightPending,
  hideLineNumbers,
  leadingSlot,
  isHighlighted,
  isRangeEndpoint,
}: Props) {
  let bgClass = "";
  let textClass = "text-text-secondary";
  let prefix = " ";

  if (line.type === "add") {
    bgClass = "bg-status-running/5";
    textClass = "text-status-running";
    prefix = "+";
  } else if (line.type === "delete") {
    bgClass = "bg-status-error/5";
    textClass = "text-status-error";
    prefix = "-";
  }

  const highlightOverlay = isHighlighted ? " bg-brand-600/10" : "";
  const endpointBorder = isRangeEndpoint ? " border-l-2 border-brand-600" : "";

  const content = line.content.replace(/\r?\n$/, "");

  const renderContent = () => {
    if (tokens && tokens.length > 0) {
      const opacity = line.type === "equal" ? 1 : 0.7;
      return tokens.map((tok, i) => (
        <span
          key={i}
          style={tok.color ? { color: tok.color, opacity } : { opacity }}
        >
          {tok.content}
        </span>
      ));
    }
    return content || " ";
  };

  return (
    <div
      className={`group flex ${bgClass}${highlightOverlay}${endpointBorder} hover:brightness-110 transition-[filter] duration-75`}
    >
      {!hideLineNumbers && (
        <>
          <span className="shrink-0 w-[50px] text-right pr-2 font-mono text-[11px] text-text-dim select-none border-r border-surface-700/30 relative">
            {leadingSlot}
            {line.old_line_num ?? ""}
          </span>
          <span className="shrink-0 w-[50px] text-right pr-2 font-mono text-[11px] text-text-dim select-none border-r border-surface-700/30">
            {line.new_line_num ?? ""}
          </span>
        </>
      )}
      <span
        className={`shrink-0 w-4 text-center font-mono text-[12px] ${textClass} select-none`}
      >
        {prefix}
      </span>
      <span
        className={`flex-1 font-mono text-[12px] whitespace-pre transition-opacity duration-100${tokens ? "" : ` ${textClass}`}${highlightPending ? " opacity-0" : ""}`}
      >
        {renderContent()}
      </span>
    </div>
  );
}

/** Memoized so range-selection state changes in the parent don't
 *  re-render every line. Callers should pass stable props (no inline
 *  object/function allocations per render) to keep memo effective. */
export const DiffLine = memo(DiffLineImpl);
