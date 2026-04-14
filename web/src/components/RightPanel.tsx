import { useCallback, useEffect, useRef, useState } from "react";
import { DiffFileList } from "./diff/DiffFileList";
import { useTerminal } from "../hooks/useTerminal";
import { ensureTerminal } from "../lib/api";
import type { RichDiffFile, SessionResponse } from "../lib/types";
import "@xterm/xterm/css/xterm.css";

const VSPLIT_STORAGE_KEY = "aoe-right-vsplit";
const DEFAULT_TOP_RATIO = 0.5;
const MIN_TOP_PX = 80;
const MIN_BOTTOM_PX = 120;

function loadSavedRatio(): number {
  try {
    const saved = localStorage.getItem(VSPLIT_STORAGE_KEY);
    if (saved) {
      const r = parseFloat(saved);
      if (r > 0 && r < 1) return r;
    }
  } catch {
    // ignore
  }
  return DEFAULT_TOP_RATIO;
}

interface Props {
  session: SessionResponse | null;
  sessionId: string | null;
  files: RichDiffFile[];
  baseBranch: string;
  warning: string | null;
  filesLoading: boolean;
  selectedFilePath: string | null;
  onSelectFile: (path: string) => void;
}

type ShellMode = "host" | "container";

function PairedTerminal({
  sessionId,
  mode,
}: {
  sessionId: string;
  mode: ShellMode;
}) {
  const [ready, setReady] = useState(false);
  const wsPath =
    mode === "container" ? "container-terminal/ws" : "terminal/ws";
  const { containerRef, state, manualReconnect } = useTerminal(
    ready ? sessionId : null,
    wsPath,
  );

  useEffect(() => {
    let cancelled = false;
    setReady(false);
    ensureTerminal(sessionId, mode === "container").then((ok) => {
      if (!cancelled && ok) setReady(true);
    });
    return () => {
      cancelled = true;
    };
  }, [sessionId, mode]);

  if (!ready) {
    return (
      <div className="flex-1 flex items-center justify-center bg-surface-950 text-text-dim">
        <span className="text-xs">Starting terminal...</span>
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col min-h-0 overflow-hidden">
      {!state.connected && state.reconnecting && (
        <div className="bg-status-waiting/15 border-b border-status-waiting/30 px-3 py-1 shrink-0">
          <span className="text-xs text-status-waiting">
            Reconnecting... ({state.retryCount}/3)
          </span>
        </div>
      )}
      {!state.connected && !state.reconnecting && state.retryCount >= 3 && (
        <div className="bg-status-error/10 border-b border-status-error/30 px-3 py-1 flex items-center gap-2 shrink-0">
          <span className="text-xs text-status-error">Disconnected</span>
          <button
            onClick={manualReconnect}
            className="text-xs text-brand-500 cursor-pointer underline"
          >
            Retry
          </button>
        </div>
      )}
      <div
        ref={containerRef}
        className="flex-1 overflow-hidden bg-surface-950"
      />
    </div>
  );
}

export function RightPanel({
  session,
  sessionId,
  files,
  baseBranch,
  warning,
  filesLoading,
  selectedFilePath,
  onSelectFile,
}: Props) {
  const [shellMode, setShellMode] = useState<ShellMode>("host");
  const isSandboxed = session?.is_sandboxed ?? false;

  const [topRatio, setTopRatio] = useState(loadSavedRatio);
  const containerRef = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    dragging.current = true;
    document.body.style.cursor = "row-resize";
    document.body.style.userSelect = "none";
  }, []);

  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      if (!dragging.current || !containerRef.current) return;
      const rect = containerRef.current.getBoundingClientRect();
      const y = e.clientY - rect.top;
      if (y < MIN_TOP_PX || rect.height - y < MIN_BOTTOM_PX) return;
      setTopRatio(y / rect.height);
    };
    const handleMouseUp = () => {
      if (!dragging.current) return;
      dragging.current = false;
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
      setTopRatio((r) => {
        localStorage.setItem(VSPLIT_STORAGE_KEY, String(r));
        return r;
      });
      window.dispatchEvent(new Event("resize"));
    };
    document.addEventListener("mousemove", handleMouseMove);
    document.addEventListener("mouseup", handleMouseUp);
    return () => {
      document.removeEventListener("mousemove", handleMouseMove);
      document.removeEventListener("mouseup", handleMouseUp);
    };
  }, []);

  return (
    <div ref={containerRef} className="flex-1 flex flex-col min-h-0 overflow-hidden">
      {/* Upper: file list */}
      <div
        style={{ flexBasis: `${topRatio * 100}%` }}
        className="flex flex-col min-h-0 overflow-hidden"
      >
        <DiffFileList
          files={files}
          baseBranch={baseBranch}
          warning={warning}
          selectedPath={selectedFilePath}
          loading={filesLoading}
          onSelectFile={onSelectFile}
        />
      </div>

      {/* Drag handle */}
      <div
        onMouseDown={handleMouseDown}
        className="h-1 cursor-row-resize shrink-0 bg-surface-700/20 hover:bg-brand-600/50 transition-colors duration-75"
      />

      {/* Lower: paired terminal */}
      <div
        style={{ flexBasis: `${(1 - topRatio) * 100}%` }}
        className="flex flex-col min-h-0">
        <div className="flex items-center gap-1 px-2 py-1 bg-surface-900 border-b border-surface-700/20 shrink-0">
          <span className="text-xs text-text-dim mr-1">Shell</span>
          <button
            onClick={() => setShellMode("host")}
            className={`text-[12px] px-2 py-0.5 rounded cursor-pointer transition-colors ${
              shellMode === "host"
                ? "text-brand-500 bg-brand-600/10"
                : "text-text-dim hover:text-text-muted"
            }`}
          >
            Host
          </button>
          {isSandboxed && (
            <button
              onClick={() => setShellMode("container")}
              className={`text-[12px] px-2 py-0.5 rounded cursor-pointer transition-colors ${
                shellMode === "container"
                  ? "text-brand-500 bg-brand-600/10"
                  : "text-text-dim hover:text-text-muted"
              }`}
            >
              Container
            </button>
          )}
        </div>

        {sessionId ? (
          <PairedTerminal
            key={`${sessionId}-${shellMode}`}
            sessionId={sessionId}
            mode={shellMode}
          />
        ) : (
          <div className="flex-1 flex items-center justify-center bg-surface-950 text-text-dim">
            <p className="text-xs">Select a session</p>
          </div>
        )}
      </div>
    </div>
  );
}
