import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { DiffFileList } from "./diff/DiffFileList";
import { CommentsBanner } from "./diff/comments/CommentsBanner";
import { useTerminal } from "../hooks/useTerminal";
import { useMobileKeyboard } from "../hooks/useMobileKeyboard";
import { MobileTerminalToolbar } from "./MobileTerminalToolbar";
import { BackToLiveButton } from "./BackToLiveButton";
import { KeyboardFab } from "./KeyboardFab";
import { ensureTerminal } from "../lib/api";
import { safeGetItem, safeSetItem } from "../lib/safeStorage";
import type { RepoBase, RichDiffFile, SessionResponse } from "../lib/types";
import {
  FOCUS_TERMINAL_EVENT,
  consumePendingTerminalFocus,
  setPendingTerminalFocus,
  type FocusTerminalDetail,
} from "../lib/terminalFocus";
import "@xterm/xterm/css/xterm.css";

const VSPLIT_STORAGE_KEY = "aoe-right-vsplit";
const DEFAULT_TOP_RATIO = 0.5;
const MIN_TOP_PX = 80;
const MIN_BOTTOM_PX = 120;

function loadSavedRatio(): number {
  const saved = safeGetItem(VSPLIT_STORAGE_KEY);
  if (saved) {
    const r = parseFloat(saved);
    if (r > 0 && r < 1) return r;
  }
  return DEFAULT_TOP_RATIO;
}

interface Props {
  session: SessionResponse | null;
  sessionId: string | null;
  files: RichDiffFile[];
  perRepoBases: RepoBase[];
  warning: string | null;
  filesLoading: boolean;
  selectedFilePath: string | null;
  selectedRepoName: string | undefined;
  onSelectFile: (path: string, repoName?: string) => void;
  /** Re-fetch the diff. Called after the user changes the per-session
   *  base-branch override so the file list reflects the new comparison. */
  onDiffRefresh: () => void;
  /** Diff-comments banner state (#928). Hidden on tmux sessions. */
  commentsEnabled: boolean;
  commentsCount: number;
  commentsSendEnabled: boolean;
  commentsSendDisabledReason?: string;
  onOpenSendDialog: () => void;
  onDiscardAllComments: () => void;
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
  const {
    containerRef,
    termRef,
    state,
    manualReconnect,
    sendData,
    activate,
    exitScrollback,
    ctrlActiveRef,
    clearCtrlRef,
    maxRetries,
  } = useTerminal(ready ? sessionId : null, wsPath, false);
  // PairedTerminal intentionally uses the live keyboardHeight, not the
  // sticky reservation that TerminalView uses. The slide-in only gives
  // this pane ~half a viewport tall, and applying a ~340px reservation
  // there collapses data-term="paired" to 0 height. Side-shell use is
  // sporadic so the original kb-cycle behavior is acceptable here.
  const { isMobile, keyboardOpen, keyboardHeight } = useMobileKeyboard();
  const [ctrlActive, setCtrlActive] = useState(false);
  const [termFocused, setTermFocused] = useState(false);

  // See TerminalView.tsx for why these syncs live in effects rather
  // than running during render.
  useEffect(() => {
    ctrlActiveRef.current = ctrlActive;
  });
  useEffect(() => {
    clearCtrlRef.current = () => setCtrlActive(false);
  }, [clearCtrlRef]);

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

  // Dispatch a window resize after keyboard transitions so anything else
  // watching layout is nudged; the hook's ResizeObserver already refits
  // the terminal grid automatically.
  useLayoutEffect(() => {
    const t = setTimeout(() => {
      window.dispatchEvent(new Event("resize"));
    }, 150);
    return () => clearTimeout(t);
  }, [keyboardHeight, keyboardOpen]);

  const toggleKeyboard = useCallback(() => {
    const term = termRef.current;
    if (!term?.element) return;
    const ta = term.element.querySelector("textarea");
    if (keyboardOpen) {
      ta?.blur();
    } else if (ta instanceof HTMLElement) {
      ta.focus();
    }
    activate();
  }, [termRef, keyboardOpen, activate]);

  // Returns true if focus was applied. Callers can fall back to the pending
  // latch when the textarea isn't in the DOM yet (PTY still booting).
  const focusSelf = useCallback(() => {
    const ta = termRef.current?.element?.querySelector("textarea");
    if (ta instanceof HTMLElement) {
      ta.focus();
      return true;
    }
    return false;
  }, [termRef]);

  // Cmd+` shortcut focuses this terminal when "paired" is the dispatched
  // target. The component might be mounted but its PTY not yet ready (the
  // initial ensureTerminal round-trip), in which case focusSelf() can't
  // find a textarea, so we latch the intent for the ready-effect below.
  // While the right panel is collapsed this component is unmounted entirely;
  // App.tsx sets the latch directly in that case.
  useEffect(() => {
    const onFocusEvent = (e: Event) => {
      const detail = (e as CustomEvent<FocusTerminalDetail>).detail;
      if (detail?.target !== "paired") return;
      if (!focusSelf()) setPendingTerminalFocus("paired");
    };
    window.addEventListener(FOCUS_TERMINAL_EVENT, onFocusEvent);
    return () => window.removeEventListener(FOCUS_TERMINAL_EVENT, onFocusEvent);
  }, [focusSelf]);

  useEffect(() => {
    if (!ready) return;
    if (consumePendingTerminalFocus("paired")) focusSelf();
  }, [ready, focusSelf]);

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
    <div className="flex-1 flex flex-col min-h-0 overflow-hidden md:bg-surface-800" style={rootStyle}>
      {!state.connected && state.reconnecting && (
        <div className="bg-status-waiting/15 border-b border-status-waiting/30 px-3 py-1 shrink-0">
          <span className="text-xs text-status-waiting">
            Reconnecting... ({state.retryCount}/{maxRetries})
          </span>
        </div>
      )}
      {!state.connected && !state.reconnecting && state.retryCount >= maxRetries && (
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
        data-term="paired"
        className={`flex-1 overflow-hidden bg-surface-950 relative md:rounded-lg term-panel${termFocused ? " term-focused" : ""}`}
        onFocus={() => setTermFocused(true)}
        onBlur={() => setTermFocused(false)}
      >
        <div
          ref={containerRef}
          className="absolute inset-0"
          onPointerDown={activate}
        />

        {isMobile && state.isInScrollback && (
          <BackToLiveButton onClick={exitScrollback} topOffset="top-2" />
        )}

        {isMobile && state.connected && (
          <KeyboardFab keyboardOpen={keyboardOpen} onToggle={toggleKeyboard} />
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
  perRepoBases,
  warning,
  filesLoading,
  selectedFilePath,
  selectedRepoName,
  onSelectFile,
  onDiffRefresh,
  commentsEnabled,
  commentsCount,
  commentsSendEnabled,
  commentsSendDisabledReason,
  onOpenSendDialog,
  onDiscardAllComments,
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
        safeSetItem(VSPLIT_STORAGE_KEY, String(r));
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
    <div ref={containerRef} className="flex-1 flex flex-col min-h-0 overflow-hidden md:bg-surface-800 md:pb-1.5">
      {/* Upper: file list */}
      <div
        style={{ flexBasis: `${topRatio * 100}%` }}
        className="flex flex-col min-h-0 overflow-hidden"
      >
        {commentsEnabled && commentsCount > 0 && (
          <CommentsBanner
            count={commentsCount}
            sendEnabled={commentsSendEnabled}
            sendDisabledReason={commentsSendDisabledReason}
            onSend={onOpenSendDialog}
            onDiscardAll={onDiscardAllComments}
          />
        )}
        <DiffFileList
          files={files}
          perRepoBases={perRepoBases}
          warning={warning}
          selectedPath={selectedFilePath}
          selectedRepoName={selectedRepoName}
          loading={filesLoading}
          onSelectFile={onSelectFile}
          sessionId={sessionId}
          repoPath={session?.main_repo_path ?? session?.project_path ?? null}
          baseBranchOverride={session?.base_branch_override ?? null}
          onBaseBranchChanged={onDiffRefresh}
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
