import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { DiffFileList } from "./diff/DiffFileList";
import { useTerminal } from "../hooks/useTerminal";
import { useMobileKeyboard } from "../hooks/useMobileKeyboard";
import { MobileTerminalToolbar } from "./MobileTerminalToolbar";
import { ensureTerminal } from "../lib/api";
import type { RichDiffFile, SessionResponse } from "../lib/types";
import "@wterm/dom/css";

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
  const { containerRef, termRef, state, manualReconnect, sendData, activate, ctrlActiveRef, clearCtrlRef } =
    useTerminal(ready ? sessionId : null, wsPath);
  const { isMobile, keyboardOpen, keyboardHeight } = useMobileKeyboard();
  const [ctrlActive, setCtrlActive] = useState(false);

  ctrlActiveRef.current = ctrlActive;
  clearCtrlRef.current = () => setCtrlActive(false);

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

  // Scroll-to-bottom on keyboard height changes (same fix as TerminalView).
  const resizeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const scrollRafRef = useRef(0);
  useLayoutEffect(() => {
    if (resizeTimerRef.current) clearTimeout(resizeTimerRef.current);
    cancelAnimationFrame(scrollRafRef.current);
    scrollRafRef.current = requestAnimationFrame(() => {
      scrollRafRef.current = requestAnimationFrame(() => {
        const el = termRef.current?.element;
        if (el) el.scrollTop = el.scrollHeight;
      });
    });
    resizeTimerRef.current = setTimeout(() => {
      resizeTimerRef.current = null;
      window.dispatchEvent(new Event("resize"));
      const el = termRef.current?.element;
      if (el) el.scrollTop = el.scrollHeight;
    }, 150);
    return () => {
      if (resizeTimerRef.current) clearTimeout(resizeTimerRef.current);
      cancelAnimationFrame(scrollRafRef.current);
    };
  }, [keyboardHeight, keyboardOpen, termRef]);

  // Auto-open keyboard on connect.
  useEffect(() => {
    if (!isMobile || !state.connected) return;
    const term = termRef.current;
    if (!term) return;
    const delays = [50, 200, 500];
    const timers = delays.map((ms) =>
      setTimeout(() => {
        const ta = term.element.querySelector("textarea");
        if (ta instanceof HTMLElement) ta.focus();
      }, ms),
    );
    return () => timers.forEach(clearTimeout);
  }, [isMobile, state.connected, termRef]);

  const toggleKeyboard = useCallback(() => {
    const term = termRef.current;
    if (!term) return;
    const ta = term.element.querySelector("textarea");
    if (keyboardOpen) {
      ta?.blur();
    } else if (ta instanceof HTMLElement) {
      ta.focus();
    }
    activate();
  }, [termRef, keyboardOpen, activate]);

  if (!ready) {
    return (
      <div className="flex-1 flex items-center justify-center bg-surface-950 text-text-dim">
        <span className="text-xs">Starting terminal...</span>
      </div>
    );
  }

  const rootStyle = {
    paddingBottom: keyboardHeight > 0 ? keyboardHeight : undefined,
  } as const;

  return (
    <div className="flex-1 flex flex-col min-h-0 overflow-hidden" style={rootStyle}>
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
      <div className="flex-1 overflow-hidden bg-surface-950 relative">
        <div
          ref={containerRef}
          className="absolute inset-0"
          onPointerDown={activate}
        />

        {isMobile && state.connected && (
          <button
            type="button"
            aria-label={keyboardOpen ? "Close keyboard" : "Open keyboard"}
            onClick={toggleKeyboard}
            className="absolute right-3 bottom-3 z-10 w-10 h-10 rounded-full bg-surface-800/90 border border-surface-700/30 text-text-secondary flex items-center justify-center shadow-lg backdrop-blur-sm active:scale-95"
          >
            {keyboardOpen ? (
              <svg
                width="18"
                height="18"
                viewBox="0 0 24 24"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.5"
                strokeLinecap="round"
                strokeLinejoin="round"
                aria-hidden="true"
              >
                <rect x="1" y="1" width="22" height="16" rx="2" />
                <line x1="5" y1="13" x2="19" y2="13" />
                <line x1="8" y1="20" x2="16" y2="20" />
                <line x1="12" y1="17" x2="12" y2="20" />
              </svg>
            ) : (
              <svg
                width="18"
                height="14"
                viewBox="0 0 24 18"
                fill="none"
                stroke="currentColor"
                strokeWidth="1.5"
                strokeLinecap="round"
                strokeLinejoin="round"
                aria-hidden="true"
              >
                <rect x="1" y="1" width="22" height="16" rx="2" />
                <line x1="5" y1="13" x2="19" y2="13" />
                <line x1="5" y1="9" x2="5.01" y2="9" />
                <line x1="9" y1="9" x2="9.01" y2="9" />
                <line x1="13" y1="9" x2="13.01" y2="9" />
                <line x1="17" y1="9" x2="17.01" y2="9" />
                <line x1="5" y1="5" x2="5.01" y2="5" />
                <line x1="9" y1="5" x2="9.01" y2="5" />
                <line x1="13" y1="5" x2="13.01" y2="5" />
                <line x1="17" y1="5" x2="17.01" y2="5" />
              </svg>
            )}
          </button>
        )}
      </div>
      {isMobile && state.connected && (
        <MobileTerminalToolbar
          sendData={sendData}
          termRef={termRef}
          keyboardHeight={keyboardHeight}
          ctrlActive={ctrlActive}
          onCtrlToggle={() => setCtrlActive((v) => !v)}
        />
      )}
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

  const handleTouchStart = useCallback((e: React.TouchEvent) => {
    e.preventDefault();
    dragging.current = true;
    document.body.style.userSelect = "none";
  }, []);

  useEffect(() => {
    const applyY = (clientY: number) => {
      if (!containerRef.current) return;
      const rect = containerRef.current.getBoundingClientRect();
      const y = clientY - rect.top;
      if (y < MIN_TOP_PX || rect.height - y < MIN_BOTTOM_PX) return;
      setTopRatio(y / rect.height);
    };
    const persistAndSettle = () => {
      if (!dragging.current) return;
      dragging.current = false;
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
      setTopRatio((r) => {
        try {
          localStorage.setItem(VSPLIT_STORAGE_KEY, String(r));
        } catch {
          // quota exceeded or private mode; non-fatal
        }
        return r;
      });
      window.dispatchEvent(new Event("resize"));
    };

    const handleMouseMove = (e: MouseEvent) => {
      if (!dragging.current) return;
      applyY(e.clientY);
    };
    const handleMouseUp = () => persistAndSettle();

    const handleTouchMove = (e: TouchEvent) => {
      if (!dragging.current) return;
      const t = e.touches[0];
      if (!t) return;
      e.preventDefault();
      applyY(t.clientY);
    };
    const handleTouchEnd = () => persistAndSettle();

    document.addEventListener("mousemove", handleMouseMove);
    document.addEventListener("mouseup", handleMouseUp);
    document.addEventListener("touchmove", handleTouchMove, { passive: false });
    document.addEventListener("touchend", handleTouchEnd);
    document.addEventListener("touchcancel", handleTouchEnd);
    return () => {
      document.removeEventListener("mousemove", handleMouseMove);
      document.removeEventListener("mouseup", handleMouseUp);
      document.removeEventListener("touchmove", handleTouchMove);
      document.removeEventListener("touchend", handleTouchEnd);
      document.removeEventListener("touchcancel", handleTouchEnd);
      // If component unmounts mid-drag, reset body styles so the cursor
      // doesn't stay in row-resize / text-select-disabled state.
      if (dragging.current) {
        dragging.current = false;
        document.body.style.cursor = "";
        document.body.style.userSelect = "";
      }
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

      {/* Drag handle: taller on mobile for easier touch targeting */}
      <div
        onMouseDown={handleMouseDown}
        onTouchStart={handleTouchStart}
        className="h-3 md:h-1 cursor-row-resize shrink-0 bg-surface-700/20 hover:bg-brand-600/50 transition-colors duration-75 touch-none flex items-center justify-center"
      >
        <div className="w-8 h-0.5 rounded-full bg-surface-500/40 md:hidden" />
      </div>

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
