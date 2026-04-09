import { useEffect, useState } from "react";
import { getSessionDiff } from "../lib/api";
import type { DiffResponse } from "../lib/types";

interface Props {
  sessionId: string;
  onClose: () => void;
}

export function DiffView({ sessionId, onClose }: Props) {
  const [diff, setDiff] = useState<DiffResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [selectedFile, setSelectedFile] = useState<number>(0);

  useEffect(() => {
    setLoading(true);
    getSessionDiff(sessionId).then((d) => {
      setDiff(d);
      setLoading(false);
    });
  }, [sessionId]);

  if (loading) {
    return (
      <div className="flex-1 flex items-center justify-center bg-surface-900 text-text-muted font-mono text-sm">
        Loading diff...
      </div>
    );
  }

  if (!diff || diff.files.length === 0) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center bg-surface-900 text-text-muted">
        <div className="font-mono text-2xl text-surface-700 mb-3">0</div>
        <p className="font-body text-sm">No changes detected</p>
        <button
          onClick={onClose}
          className="mt-4 px-3 py-1.5 font-body text-xs rounded-md text-brand-500 border border-brand-600/30 hover:bg-brand-600/10 cursor-pointer"
        >
          Back to terminal
        </button>
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col overflow-hidden bg-surface-900">
      {/* Header */}
      <div className="h-10 bg-surface-850 border-b border-surface-700 flex items-center px-4 shrink-0">
        <button
          onClick={onClose}
          className="text-brand-500 mr-3 cursor-pointer font-body text-sm"
        >
          &larr; Terminal
        </button>
        <span className="font-mono text-label uppercase tracking-wider text-text-muted">
          Diff
        </span>
        <span className="font-mono text-label text-text-dim ml-2">
          {diff.files.length} file{diff.files.length !== 1 ? "s" : ""} changed
        </span>
      </div>

      <div className="flex flex-1 overflow-hidden">
        {/* File list */}
        <div className="w-sidebar-sm min-w-sidebar-sm border-r border-surface-700 overflow-y-auto">
          {diff.files.map((file, i) => (
            <button
              key={file.path}
              onClick={() => setSelectedFile(i)}
              className={`w-full text-left px-3 py-1.5 font-mono text-code truncate cursor-pointer transition-colors ${
                i === selectedFile
                  ? "bg-surface-800 text-text-primary border-l-2 border-brand-600 pl-2.5"
                  : "text-text-secondary hover:bg-surface-800/50"
              }`}
            >
              <span
                className={`inline-block w-3 mr-1.5 text-center ${
                  file.status === "M"
                    ? "text-status-waiting"
                    : file.status === "A"
                      ? "text-status-running"
                      : file.status === "D"
                        ? "text-status-error"
                        : "text-text-muted"
                }`}
              >
                {file.status}
              </span>
              {file.path.split("/").pop()}
            </button>
          ))}
        </div>

        {/* Diff content */}
        <div className="flex-1 overflow-auto">
          <pre className="font-mono text-ui leading-[1.5] p-4 text-text-secondary">
            {diff.raw.split("\n").map((line, i) => {
              let color = "text-text-secondary";
              if (line.startsWith("+") && !line.startsWith("+++"))
                color = "text-status-running";
              if (line.startsWith("-") && !line.startsWith("---"))
                color = "text-status-error";
              if (line.startsWith("@@")) color = "text-accent-600";
              if (line.startsWith("diff ")) color = "text-text-primary font-semibold";
              return (
                <div
                  key={i}
                  className={`${color} ${
                    line.startsWith("+") && !line.startsWith("+++")
                      ? "bg-status-running/5"
                      : line.startsWith("-") && !line.startsWith("---")
                        ? "bg-status-error/5"
                        : ""
                  }`}
                >
                  {line || "\u00a0"}
                </div>
              );
            })}
          </pre>
        </div>
      </div>
    </div>
  );
}
