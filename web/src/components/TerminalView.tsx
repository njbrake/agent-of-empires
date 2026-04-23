import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
} from "react";
import { useTerminal } from "../hooks/useTerminal";
import { useMobileKeyboard } from "../hooks/useMobileKeyboard";
import { MobileTerminalToolbar } from "./MobileTerminalToolbar";
import { BackToLiveButton } from "./BackToLiveButton";
import { KeyboardFab } from "./KeyboardFab";
import { ensureSession } from "../lib/api";
import type { SessionResponse } from "../lib/types";
import "@wterm/dom/css";

interface Props {
  session: SessionResponse;
}

const SCROLL_HINT_SEEN_KEY = "aoe-mobile-scroll-hint-seen";
const SCROLL_HINT_TIMEOUT_MS = 8000;

export function TerminalView({ session }: Props) {
  const [ensureState, setEnsureState] = useState<"pending" | "ready" | "error">(
    "pending",
  );
  const [ensureError, setEnsureError] = useState<string | null>(null);
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
  } = useTerminal(ensureState === "ready" ? session.id : null);
  const { isMobile, keyboardOpen, keyboardHeight } = useMobileKeyboard();
  const [ctrlActive, setCtrlActive] = useState(false);

  // Sync React state → hook ref in an effect. The mobile toolbar toggles
  // `ctrlActive` but the wterm native onData callback reads the ref to
  // decide whether to transform the next keystroke. Writing refs during
  // render trips react-hooks/refs; a commit-phase effect does the same
  // work without tripping the lint.
  useEffect(() => {
    ctrlActiveRef.current = ctrlActive;
  });
  useEffect(() => {
    clearCtrlRef.current = () => setCtrlActive(false);
  }, [clearCtrlRef]);

  useEffect(() => {
    const controller = new AbortController();
    setEnsureState("pending");
    setEnsureError(null);
    ensureSession(session.id, controller.signal).then((res) => {
      if (controller.signal.aborted) return;
      if (res.ok) {
        setEnsureState("ready");
      } else {
        setEnsureState("error");
        setEnsureError(res.message ?? "Could not start session.");
      }
    });
    return () => controller.abort();
  }, [session.id]);

  const retryEnsure = useCallback(() => {
    setEnsureState((prev) => {
      if (prev === "pending") return prev;
      setEnsureError(null);
      const controller = new AbortController();
      ensureSession(session.id, controller.signal).then((res) => {
        if (controller.signal.aborted) return;
        if (res.ok) {
          setEnsureState("ready");
        } else {
          setEnsureState("error");
          setEnsureError(res.message ?? "Could not start session.");
        }
      });
      return "pending";
    });
  }, [session.id]);

  const [hintDismissed, setHintDismissed] = useState(() => {
    try {
      return localStorage.getItem(SCROLL_HINT_SEEN_KEY) === "1";
    } catch {
      return true;
    }
  });
  const showScrollHint = isMobile && state.connected && !hintDismissed;

  // When the keyboard opens (keyboardHeight goes from 0 → positive), the
  // terminal container shrinks. wterm's ResizeObserver fires and checks
  // _isScrolledToBottom() BEFORE the DOM has reflowed, sees the reduced
  // clientHeight while scrollTop/scrollHeight are stale, and concludes "not
  // at bottom." This makes it skip _scrollToBottom() after the resize,
  // leaving the cursor off-screen.
  //
  // Fix: force a scroll-to-bottom via double-rAF (fires after wterm's own
  // rAF render) on every keyboardHeight change, plus a debounced final
  // scroll after the animation settles.
  const resizeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const scrollRafRef = useRef(0);
  useLayoutEffect(() => {
    if (resizeTimerRef.current) clearTimeout(resizeTimerRef.current);

    // Immediate: double-rAF ensures we fire AFTER wterm's scheduled render
    // (which also uses rAF). This keeps the cursor visible during the
    // keyboard animation, not just after it settles.
    cancelAnimationFrame(scrollRafRef.current);
    scrollRafRef.current = requestAnimationFrame(() => {
      scrollRafRef.current = requestAnimationFrame(() => {
        const el = termRef.current?.element;
        if (el) el.scrollTop = el.scrollHeight;
      });
    });

    // Debounced: final correction after the keyboard animation fully settles.
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

  // On initial connect, auto-open the keyboard.
  useEffect(() => {
    if (!isMobile || !state.connected) return;
    const term = termRef.current;
    if (!term) return;
    // Retry a few times: wterm's textarea may not exist immediately.
    const delays = [50, 200, 500];
    const timers = delays.map((ms) =>
      setTimeout(() => {
        const ta = term.element.querySelector("textarea");
        if (ta instanceof HTMLElement) ta.focus();
      }, ms),
    );
    return () => timers.forEach(clearTimeout);
  }, [isMobile, state.connected, termRef]);

  // Toggle keyboard: focus/blur MUST be the first thing in this handler
  // so iOS considers it part of the user-gesture chain. Anything before
  // focus() (even a synchronous ws.send) can break iOS keyboard display.
  // Claim primary after the focus so the PTY resizes to this viewport.
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

  // Dismiss scroll hint on first touch or timeout.
  useEffect(() => {
    if (!showScrollHint) return;
    const markSeen = () => {
      setHintDismissed(true);
      try {
        localStorage.setItem(SCROLL_HINT_SEEN_KEY, "1");
      } catch {
        // ignore
      }
    };
    const t = setTimeout(markSeen, SCROLL_HINT_TIMEOUT_MS);
    const c = containerRef.current;
    c?.addEventListener("touchmove", markSeen, { once: true });
    return () => {
      clearTimeout(t);
      c?.removeEventListener("touchmove", markSeen);
    };
  }, [showScrollHint, containerRef]);

  if (ensureState === "pending") {
    return (
      <div className="flex-1 flex items-center justify-center bg-surface-950 text-text-dim">
        <span className="text-xs">Starting session...</span>
      </div>
    );
  }

  if (ensureState === "error") {
    return (
      <div className="flex-1 flex flex-col items-center justify-center bg-surface-950 gap-2 px-4 text-center">
        <span className="text-xs text-status-error max-w-md break-words">
          {ensureError ?? "Could not start session."}
        </span>
        <button
          onClick={retryEnsure}
          className="text-xs text-brand-500 hover:text-brand-400 cursor-pointer underline"
        >
          Retry
        </button>
      </div>
    );
  }

  const rootStyle = {
    paddingBottom: keyboardHeight > 0 ? keyboardHeight : undefined,
  } as const;
  return (
    <div
      className="flex-1 flex flex-col overflow-hidden relative"
      style={rootStyle}
    >
      {!state.connected && state.reconnecting && (
        <div className="bg-status-waiting/15 border-b border-status-waiting/30 px-4 py-1.5 flex items-center gap-2 shrink-0">
          <span className="text-xs text-status-waiting">
            Reconnecting in {state.retryCountdown}s... ({state.retryCount}/3)
          </span>
        </div>
      )}
      {!state.connected && !state.reconnecting && state.retryCount >= 3 && (
        <div className="bg-status-error/10 border-b border-status-error/30 px-4 py-1.5 flex items-center gap-2 shrink-0">
          <span className="text-xs text-status-error">Connection lost</span>
          <button
            onClick={manualReconnect}
            className="text-xs text-brand-500 hover:text-brand-400 cursor-pointer underline"
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

        {state.connected && !state.isPrimary && (
          <div
            aria-hidden="true"
            className="absolute left-0 right-0 top-3 flex justify-center pointer-events-none z-10"
          >
            <span className="font-mono text-[11px] text-text-dim bg-surface-800/80 border border-surface-700/50 rounded-md px-2.5 py-1 backdrop-blur-sm">
              Viewing from another device. Tap to take over.
            </span>
          </div>
        )}

        {showScrollHint && (
          <div
            aria-hidden="true"
            className="absolute left-0 right-0 top-3 flex justify-center pointer-events-none motion-safe:animate-[fadeIn_300ms_ease-out]"
          >
            <span className="flex items-center gap-2 font-mono text-[13px] text-text-primary bg-surface-800/95 border border-surface-700 rounded-md px-3 py-2 shadow-lg backdrop-blur-sm">
              <span aria-hidden="true" className="text-base leading-none">
                {"\u21C5"}
              </span>
              Swipe to scroll
            </span>
          </div>
        )}

        {isMobile && state.isInScrollback && (
          <BackToLiveButton onClick={exitScrollback} topOffset="top-3" />
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
