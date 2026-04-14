import { useFileDiff } from "../../hooks/useFileDiff";
import type { RichDiffHunk, RichDiffLine } from "../../lib/types";

interface Props {
  sessionId: string;
  filePath: string;
  /** Triggers a re-fetch when the file list changes. */
  revision?: number;
  /** Called when the user wants to return to the terminal view. */
  onClose?: () => void;
}

const STATUS_LABELS: Record<string, string> = {
  added: "Added",
  modified: "Modified",
  deleted: "Deleted",
  renamed: "Renamed",
  copied: "Copied",
  untracked: "Untracked",
};

const STATUS_COLORS: Record<string, string> = {
  added: "text-status-running",
  modified: "text-status-waiting",
  deleted: "text-status-error",
  renamed: "text-accent-600",
  copied: "text-accent-600",
  untracked: "text-text-muted",
};

function DiffLine({ line }: { line: RichDiffLine }) {
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

  // Strip trailing newline (handles both \n and \r\n) so CRLF files
  // don't render a stray carriage-return glyph.
  const content = line.content.replace(/\r?\n$/, "");

  return (
    <div className={`flex ${bgClass} hover:brightness-110 transition-[filter] duration-75`}>
      <span className="shrink-0 w-[50px] text-right pr-2 font-mono text-[11px] text-text-dim select-none border-r border-surface-700/30">
        {line.old_line_num ?? ""}
      </span>
      <span className="shrink-0 w-[50px] text-right pr-2 font-mono text-[11px] text-text-dim select-none border-r border-surface-700/30">
        {line.new_line_num ?? ""}
      </span>
      <span className={`shrink-0 w-4 text-center font-mono text-[12px] ${textClass} select-none`}>
        {prefix}
      </span>
      <span className={`flex-1 font-mono text-[12px] ${textClass} whitespace-pre`}>
        {content || "\u00a0"}
      </span>
    </div>
  );
}

function HunkView({ hunk }: { hunk: RichDiffHunk }) {
  return (
    <div>
      <div className="flex bg-surface-850 border-y border-surface-700/20 sticky top-0 z-[1]">
        <span className="shrink-0 w-[50px] border-r border-surface-700/30" />
        <span className="shrink-0 w-[50px] border-r border-surface-700/30" />
        <span className="shrink-0 w-4" />
        <span className="flex-1 font-mono text-[11px] text-accent-600 py-0.5 px-1">
          @@ -{hunk.old_start},{hunk.old_lines} +{hunk.new_start},{hunk.new_lines} @@
        </span>
      </div>
      {hunk.lines.map((line, i) => (
        <DiffLine
          key={`${line.old_line_num ?? "_"}-${line.new_line_num ?? "_"}-${i}`}
          line={line}
        />
      ))}
    </div>
  );
}

export function DiffFileViewer({ sessionId, filePath, revision, onClose }: Props) {
  const { diff, loading, error } = useFileDiff(sessionId, filePath, revision);

  if (loading && !diff) {
    return (
      <div className="flex-1 flex items-center justify-center bg-surface-900 text-text-dim">
        <span className="text-sm">Loading diff...</span>
      </div>
    );
  }

  if (error) {
    return (
      <div className="flex-1 flex items-center justify-center bg-surface-900 text-status-error">
        <span className="text-sm">{error}</span>
      </div>
    );
  }

  if (!diff) {
    return (
      <div className="flex-1 flex items-center justify-center bg-surface-900 text-text-dim">
        <span className="text-sm">Select a file to view changes</span>
      </div>
    );
  }

  const statusColor = STATUS_COLORS[diff.file.status] ?? "text-text-muted";
  const statusLabel = STATUS_LABELS[diff.file.status] ?? diff.file.status;

  return (
    <div className="flex-1 flex flex-col bg-surface-900 overflow-hidden">
      {/* File header */}
      <div className="px-3 py-2 border-b border-surface-700/20 flex items-center gap-2 shrink-0 flex-wrap">
        {onClose && (
          <button
            onClick={onClose}
            className="text-text-dim hover:text-text-secondary cursor-pointer transition-colors flex items-center gap-1 text-[11px]"
            title="Back to terminal"
            aria-label="Back to terminal"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.75" strokeLinecap="round" strokeLinejoin="round">
              <path d="M15 18l-6-6 6-6" />
            </svg>
            <span className="hidden sm:inline">Terminal</span>
          </button>
        )}
        <span className={`font-mono text-[11px] font-semibold ${statusColor}`}>
          {statusLabel}
        </span>
        <span className="font-mono text-[12px] text-text-primary truncate">
          {diff.file.old_path
            ? `${diff.file.old_path} \u2192 ${diff.file.path}`
            : diff.file.path}
        </span>
        <span className="font-mono text-[11px] flex items-center gap-1">
          {diff.file.additions > 0 && (
            <span className="text-status-running">+{diff.file.additions}</span>
          )}
          {diff.file.deletions > 0 && (
            <span className="text-status-error">-{diff.file.deletions}</span>
          )}
        </span>
      </div>

      {/* Diff content */}
      <div className="flex-1 overflow-auto">
        {diff.is_binary ? (
          <div className="flex items-center justify-center h-full text-text-dim">
            <span className="text-sm">Binary file changed</span>
          </div>
        ) : diff.truncated ? (
          <div className="flex items-center justify-center h-full text-text-dim">
            <div className="text-center px-4">
              <p className="text-sm mb-1">File too large to diff inline</p>
              <p className="text-xs">
                Open it in your editor to review the changes.
              </p>
            </div>
          </div>
        ) : diff.hunks.length === 0 ? (
          <div className="flex items-center justify-center h-full text-text-dim">
            <span className="text-sm">No changes in this file</span>
          </div>
        ) : (
          <div className="leading-[1.6]">
            {diff.hunks.map((hunk) => (
              <HunkView
                key={`${hunk.old_start}-${hunk.new_start}`}
                hunk={hunk}
              />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
