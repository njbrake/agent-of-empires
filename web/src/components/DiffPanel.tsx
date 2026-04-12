import { useCallback, useEffect, useRef, useState } from "react";
import { getSessionDiff } from "../lib/api";
import type { DiffResponse } from "../lib/types";

const POLL_INTERVAL = 10_000;

interface Props {
  sessionId: string | null;
  expanded: boolean;
  onFileCountChange?: (count: number) => void;
}

export function DiffPanel({ sessionId, expanded, onFileCountChange }: Props) {
  const [diff, setDiff] = useState<DiffResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [selectedFile, setSelectedFile] = useState<number>(0);
  const lastRawRef = useRef<string>("");
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const fetchDiff = useCallback(async () => {
    if (!sessionId) return;
    const d = await getSessionDiff(sessionId);
    if (d) {
      if (d.raw !== lastRawRef.current) {
        lastRawRef.current = d.raw;
        setDiff(d);
        onFileCountChange?.(d.files.length);
      }
    }
    setLoading(false);
  }, [sessionId, onFileCountChange]);

  // Fetch on session change
  useEffect(() => {
    if (!sessionId) {
      setDiff(null);
      lastRawRef.current = "";
      return;
    }
    setLoading(true);
    setSelectedFile(0);
    lastRawRef.current = "";
    void fetchDiff();
  }, [sessionId, fetchDiff]);

  // Poll only when expanded
  useEffect(() => {
    if (intervalRef.current) {
      clearInterval(intervalRef.current);
      intervalRef.current = null;
    }
    if (expanded && sessionId) {
      intervalRef.current = setInterval(() => {
        void fetchDiff();
      }, POLL_INTERVAL);
    }
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, [expanded, sessionId, fetchDiff]);

  if (!sessionId) {
    return (
      <div className="flex-1 flex items-center justify-center bg-surface-900 text-text-dim">
        <p className="text-sm">Select a session to see changes</p>
      </div>
    );
  }

  if (loading) {
    return (
      <div className="flex-1 flex flex-col bg-surface-900">
        <div className="px-3 py-2 border-b border-surface-700 flex items-center gap-2">
          <span className="font-mono text-[11px] uppercase tracking-wider text-text-dim">
            Changes
          </span>
        </div>
        <div className="flex-1 flex items-center justify-center text-text-dim">
          <span className="text-sm">Loading changes...</span>
        </div>
      </div>
    );
  }

  if (!diff || diff.files.length === 0) {
    return (
      <div className="flex-1 flex flex-col bg-surface-900">
        <div className="px-3 py-2 border-b border-surface-700 flex items-center gap-2">
          <span className="font-mono text-[11px] uppercase tracking-wider text-text-dim">
            Changes
          </span>
        </div>
        <div className="flex-1 flex items-center justify-center text-text-dim">
          <div className="text-center">
            <div className="font-mono text-xl text-surface-700 mb-1">0</div>
            <p className="text-xs">No changes yet</p>
          </div>
        </div>
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col bg-surface-900 overflow-hidden">
      {/* Header */}
      <div className="px-3 py-2 border-b border-surface-700 flex items-center gap-2 shrink-0">
        <span className="font-mono text-[11px] uppercase tracking-wider text-text-dim">
          Changes
        </span>
        <span className="font-mono text-[11px] text-text-muted bg-surface-800 px-1.5 py-px rounded-full">
          {diff.files.length}
        </span>
        <div className="flex-1" />
        <button
          onClick={() => {
            setLoading(true);
            void fetchDiff();
          }}
          className="text-text-dim hover:text-text-muted cursor-pointer transition-colors"
          title="Refresh diff"
          aria-label="Refresh diff"
        >
          <svg
            width="14"
            height="14"
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            strokeWidth="2"
            strokeLinecap="round"
            strokeLinejoin="round"
          >
            <path d="M21 12a9 9 0 0 0-9-9 9.75 9.75 0 0 0-6.74 2.74L3 8" />
            <path d="M3 3v5h5" />
            <path d="M3 12a9 9 0 0 0 9 9 9.75 9.75 0 0 0 6.74-2.74L21 16" />
            <path d="M16 16h5v5" />
          </svg>
        </button>
      </div>

      {/* File list */}
      <div className="border-b border-surface-700 shrink-0 max-h-32 overflow-y-auto">
        {diff.files.map((file, i) => (
          <button
            key={file.path}
            onClick={() => setSelectedFile(i)}
            className={`w-full text-left px-3 py-1 font-mono text-[12px] truncate cursor-pointer transition-colors flex items-center gap-2 ${
              i === selectedFile
                ? "bg-surface-850 text-text-primary"
                : "text-text-secondary hover:bg-surface-800/50"
            }`}
          >
            <span
              className={`shrink-0 ${
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
            <span className="truncate">{file.path.split("/").pop()}</span>
          </button>
        ))}
      </div>

      {/* Diff content */}
      <div className="flex-1 overflow-auto">
        <pre className="font-mono text-[12px] leading-[1.6] px-3 py-2 text-text-secondary">
          {diff.raw.split("\n").map((line, i) => {
            let color = "text-text-secondary";
            let bg = "";
            if (line.startsWith("+") && !line.startsWith("+++")) {
              color = "text-status-running";
              bg = "bg-status-running/5";
            }
            if (line.startsWith("-") && !line.startsWith("---")) {
              color = "text-status-error";
              bg = "bg-status-error/5";
            }
            if (line.startsWith("@@")) color = "text-accent-600";
            if (line.startsWith("diff "))
              color = "text-text-primary font-semibold";
            return (
              <div key={i} className={`${color} ${bg}`}>
                {line || "\u00a0"}
              </div>
            );
          })}
        </pre>
      </div>
    </div>
  );
}
