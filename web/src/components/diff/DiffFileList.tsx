import type { RichDiffFile } from "../../lib/types";

interface Props {
  files: RichDiffFile[];
  baseBranch: string;
  warning: string | null;
  selectedPath: string | null;
  loading: boolean;
  onSelectFile: (path: string) => void;
}

const STATUS_COLORS: Record<string, string> = {
  added: "text-status-running",
  modified: "text-status-waiting",
  deleted: "text-status-error",
  renamed: "text-accent-600",
  copied: "text-accent-600",
  untracked: "text-text-muted",
  conflicted: "text-status-waiting",
};

const STATUS_LETTERS: Record<string, string> = {
  added: "A",
  modified: "M",
  deleted: "D",
  renamed: "R",
  copied: "C",
  untracked: "?",
  conflicted: "U",
};

export function DiffFileList({
  files,
  baseBranch,
  warning,
  selectedPath,
  loading,
  onSelectFile,
}: Props) {
  const totalAdditions = files.reduce((sum, f) => sum + f.additions, 0);
  const totalDeletions = files.reduce((sum, f) => sum + f.deletions, 0);

  return (
    <div className="flex flex-col h-full bg-surface-900 overflow-hidden">
      {/* Header */}
      <div className="px-3 py-2 border-b border-surface-700/20 shrink-0">
        <div className="flex items-center gap-2 flex-wrap">
          <span className="font-mono text-[11px] uppercase tracking-wider text-text-dim">
            Changes
          </span>
          <span className="font-mono text-[10px] px-1.5 py-px rounded bg-surface-800 text-text-muted">
            vs {baseBranch}
          </span>
          {files.length > 0 && (
            <>
              <span className="font-mono text-[11px] text-text-muted">
                {files.length} file{files.length !== 1 ? "s" : ""}
              </span>
              <span className="font-mono text-[11px]">
                <span className="text-status-running">+{totalAdditions}</span>
                {" "}
                <span className="text-status-error">-{totalDeletions}</span>
              </span>
            </>
          )}
        </div>
        {warning && (
          <p className="text-[11px] text-status-waiting mt-1">{warning}</p>
        )}
      </div>

      {/* File list */}
      <div className="flex-1 overflow-y-auto">
        {loading && files.length === 0 ? (
          <div className="flex items-center justify-center h-full text-text-dim">
            <span className="text-xs">Loading files...</span>
          </div>
        ) : files.length === 0 ? (
          <div className="flex items-center justify-center h-full text-text-dim">
            <div className="text-center px-4">
              <div className="font-mono text-xl text-surface-700 mb-1">0</div>
              <p className="text-xs">No changes yet</p>
            </div>
          </div>
        ) : (
          files.map((file) => {
            const parts = file.path.split("/");
            const fileName = parts.pop() || file.path;
            const dirPath = parts.length > 0 ? parts.join("/") + "/" : "";
            const isSelected = file.path === selectedPath;

            return (
              <button
                key={file.path}
                onClick={() => onSelectFile(file.path)}
                className={`w-full text-left px-3 py-1.5 cursor-pointer transition-colors flex items-center gap-2 ${
                  isSelected
                    ? "bg-surface-850 text-text-primary"
                    : "text-text-secondary hover:bg-surface-800/50"
                }`}
              >
                <span
                  className={`shrink-0 font-mono text-[12px] w-3 text-center ${STATUS_COLORS[file.status] ?? "text-text-muted"}`}
                >
                  {STATUS_LETTERS[file.status] ?? "?"}
                </span>
                <span className="truncate min-w-0 flex-1">
                  {dirPath && (
                    <span className="font-mono text-[11px] text-text-dim">
                      {dirPath}
                    </span>
                  )}
                  <span className="font-mono text-[12px]">{fileName}</span>
                </span>
                <span className="shrink-0 font-mono text-[11px] flex items-center gap-1">
                  {file.additions > 0 && (
                    <span className="text-status-running">+{file.additions}</span>
                  )}
                  {file.deletions > 0 && (
                    <span className="text-status-error">-{file.deletions}</span>
                  )}
                </span>
              </button>
            );
          })
        )}
      </div>
    </div>
  );
}
