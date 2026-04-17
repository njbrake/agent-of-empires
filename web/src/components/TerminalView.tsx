import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { ClipboardEvent as ReactClipboardEvent, FormEvent, KeyboardEvent as ReactKeyboardEvent } from "react";
import { useTerminal } from "../hooks/useTerminal";
import { useMobileKeyboard } from "../hooks/useMobileKeyboard";
import { useWebSettings } from "../hooks/useWebSettings";
import { MobileTerminalToolbar } from "./MobileTerminalToolbar";
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
  const { containerRef, termRef, state, manualReconnect, sendData, ctrlActiveRef, clearCtrlRef } =
    useTerminal(ensureState === "ready" ? session.id : null);
  const { isMobile, keyboardOpen, keyboardHeight } = useMobileKeyboard();
  const { settings } = useWebSettings();
  const proxyRef = useRef<HTMLInputElement>(null);
  const [proxyFocused, setProxyFocused] = useState(false);
  const [ctrlActive, setCtrlActive] = useState(false);

  // Keep the shared ref in sync so wterm's onData callback can read it.
  ctrlActiveRef.current = ctrlActive;
  clearCtrlRef.current = () => setCtrlActive(false);

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
    // Abort the in-flight ensure when the selected session changes or the
    // component unmounts. Without this, switching sessions mid-ensure would
    // let the server restart the previous session after the user moved on.
    return () => controller.abort();
  }, [session.id]);

  const retryEnsure = useCallback(() => {
    // Re-entrancy guard: ignore clicks while a retry is already in flight.
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
      return true; // localStorage unavailable — treat as already seen
    }
  });
  const showScrollHint = isMobile && state.connected && !hintDismissed;
  // Treat the keyboard as "up" whenever the proxy input has focus, not only
  // when visualViewport reports a shrunk viewport. On iOS PWA standalone
  // mode and iPadOS floating keyboards, visualViewport can lag or never
  // fire; proxy focus flips instantly.
  const keyboardVisible = keyboardOpen || proxyFocused;

  // Re-layout the terminal and scroll to the cursor when the keyboard settles.
  // Debounce the resize dispatch so it fires once after the iOS keyboard
  // animation (~300ms) instead of on every intermediate height change.
  const resizeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  useLayoutEffect(() => {
    if (resizeTimerRef.current) clearTimeout(resizeTimerRef.current);
    resizeTimerRef.current = setTimeout(() => {
      resizeTimerRef.current = null;
      window.dispatchEvent(new Event("resize"));
      if (keyboardOpen && termRef.current) {
        const el = termRef.current.element;
        el.scrollTop = el.scrollHeight;
      }
    }, 150);
    return () => {
      if (resizeTimerRef.current) clearTimeout(resizeTimerRef.current);
    };
  }, [keyboardOpen, keyboardHeight, termRef]);

  // Auto-open soft keyboard when a session is selected, if the user wants it.
  // iOS can delay or skip showing the keyboard when focus() is called from a
  // timeout (broken user-gesture chain). Retry a few times with increasing
  // delay to cover the WS-connect + terminal-mount + keyboard-animation window.
  useEffect(() => {
    if (!isMobile || !state.connected) return;
    if (!settings.autoOpenKeyboard) return;
    const delays = [50, 200, 500];
    const timers = delays.map((ms) =>
      setTimeout(() => {
        if (!proxyRef.current) return;
        proxyRef.current.focus();
      }, ms),
    );
    return () => timers.forEach(clearTimeout);
  }, [isMobile, state.connected, session.id, settings.autoOpenKeyboard]);

  // The proxy input is the keyboard bridge: soft keyboard types into it,
  // we relay each input to the PTY and clear. Mobile browsers don't
  // reliably expose the terminal's own helper textarea for the soft keyboard.
  const onProxyInput = useCallback(
    (e: FormEvent<HTMLInputElement>) => {
      const value = e.currentTarget.value;
      if (!value) return;
      if (ctrlActive) {
        // Transform each character to its Ctrl equivalent.
        // Ctrl+A = \x01, Ctrl+Z = \x1a, etc.
        for (const ch of value) {
          const code = ch.toUpperCase().charCodeAt(0);
          if (code >= 65 && code <= 90) {
            sendData(String.fromCharCode(code - 64));
          }
        }
        setCtrlActive(false);
      } else {
        sendData(value);
      }
      e.currentTarget.value = "";
    },
    [sendData, ctrlActive],
  );

  // Catch paste events on the proxy input and relay the text to the PTY.
  // On iOS the proxy input is the only focused element, so paste events land
  // here rather than on wterm's hidden textarea.
  const onProxyPaste = useCallback(
    (e: ReactClipboardEvent<HTMLInputElement>) => {
      e.preventDefault();
      const text = e.clipboardData.getData("text");
      if (text) sendData(text);
    },
    [sendData],
  );

  // iOS soft keyboards don't fire 'input' for Enter/Backspace/Tab/Arrows — they
  // only fire keydown. Without this handler, hitting Return on iOS silently
  // dropped the keystroke (Enter never reached the PTY). Translate the common
  // non-printing keys into the byte sequences the shell expects. Printable
  // keys stay on the 'input' path so composition/autocorrect still works.
  const onProxyKeyDown = useCallback(
    (e: ReactKeyboardEvent<HTMLInputElement>) => {
      // Single printable character with Ctrl armed: transform to control char.
      if (ctrlActive && e.key.length === 1) {
        const code = e.key.toUpperCase().charCodeAt(0);
        if (code >= 65 && code <= 90) {
          e.preventDefault();
          sendData(String.fromCharCode(code - 64));
          setCtrlActive(false);
          if (proxyRef.current) proxyRef.current.value = "";
          return;
        }
      }

      const seq = (() => {
        switch (e.key) {
          case "Enter":
            return "\r";
          case "Backspace":
            return "\x7f";
          case "Tab":
            return "\t";
          case "Escape":
            return "\x1b";
          case "ArrowUp":
            return "\x1b[A";
          case "ArrowDown":
            return "\x1b[B";
          case "ArrowRight":
            return "\x1b[C";
          case "ArrowLeft":
            return "\x1b[D";
          default:
            return null;
        }
      })();
      if (seq === null) return;
      e.preventDefault();
      sendData(seq);
      if (ctrlActive) setCtrlActive(false);
      if (proxyRef.current) proxyRef.current.value = "";
    },
    [sendData, ctrlActive],
  );

  // Tap the terminal pane to reopen the keyboard. Skip when text is
  // selected (preserves native long-press-to-select behavior).
  const onContainerClick = useCallback(() => {
    if (!isMobile) return;
    const selection = window.getSelection()?.toString() ?? "";
    if (selection.length > 0) return;
    proxyRef.current?.focus();
  }, [isMobile]);

  const focusProxy = useCallback(() => {
    proxyRef.current?.focus();
  }, []);

  // Dismiss the one-time scroll hint on first touchmove or after a timeout.
  // Persisted to localStorage so it's shown only once per device.
  useEffect(() => {
    if (!showScrollHint) return;
    const markSeen = () => {
      setHintDismissed(true);
      try {
        localStorage.setItem(SCROLL_HINT_SEEN_KEY, "1");
      } catch {
        // ignore quota / disabled-storage errors
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
          <span className="text-xs text-status-error">
            Connection lost
          </span>
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
          onClick={onContainerClick}
          className="absolute inset-0"
        />

        {isMobile && state.connected && (
          <>
            <input
              ref={proxyRef}
              type="text"
              autoComplete="off"
              autoCorrect="off"
              autoCapitalize="none"
              inputMode="none"
              enterKeyHint="send"
              spellCheck={false}
              onInput={onProxyInput}
              onKeyDown={onProxyKeyDown}
              onPaste={onProxyPaste}
              onFocus={() => setProxyFocused(true)}
              onBlur={() => setProxyFocused(false)}
              aria-hidden="true"
              tabIndex={-1}
              className="absolute opacity-0 pointer-events-none w-px h-px -z-10"
              style={{ left: 0, top: 0 }}
            />

            {!keyboardVisible && (
              <button
                type="button"
                aria-label="Open keyboard"
                onClick={focusProxy}
                className="absolute right-3 bottom-3 w-9 h-9 rounded-full bg-surface-800 border border-surface-700/30 text-text-secondary flex items-center justify-center motion-safe:transition-opacity motion-safe:duration-150 hover:bg-surface-700 active:scale-95"
              >
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
              </button>
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
          </>
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
