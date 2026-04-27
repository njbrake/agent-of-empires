import { useCallback, useEffect, useRef, useState } from "react";
import { WTerm } from "@wterm/dom";
import type {
  ActivateMessage,
  PauseOutputMessage,
  PrimaryStatusMessage,
  ResizeMessage,
  ResumeOutputMessage,
} from "../lib/types";
import { getToken } from "../lib/token";
import { useWebSettings } from "./useWebSettings";

// Exponential backoff: 1s, 2s, 4s, 8s, 16s, 30s, 30s (cap). Seven attempts
// cover typical tunnel restarts and transient WiFi drops without flooding
// the server or burning the user's battery on a truly dead backend.
const MAX_RETRIES = 7;
const RETRY_BASE_MS = 1000;
const RETRY_CAP_MS = 30000;
const retryDelayMs = (attempt: number) =>
  Math.min(RETRY_CAP_MS, RETRY_BASE_MS * 2 ** (attempt - 1));
const MIN_FONT_SIZE = 6;
const MAX_FONT_SIZE = 28;
const DEFAULT_FONT_SIZE = 14;
const MOBILE_BREAKPOINT_PX = 768;
const WHEEL_ZOOM_SENSITIVITY = 0.05;
const WHEEL_PERSIST_DEBOUNCE_MS = 400;
const RESIZE_DEBOUNCE_MS = 50;
// First-resize debounce: longer than the steady-state value so the
// initial layout transition (sidebar mount, splitter snap, font swap)
// settles into a single PTY resize instead of one per stable point.
// CSS transitions in the dashboard run ~200ms; 250ms covers them with
// a small margin. After the first resize lands the debounce drops to
// RESIZE_DEBOUNCE_MS so live splitter drags still feel responsive.
const INITIAL_SETTLE_MS = 250;

export interface TerminalState {
  connected: boolean;
  reconnecting: boolean;
  retryCount: number;
  retryCountdown: number;
  isPrimary: boolean;
  /**
   * True when the user has scrolled up and tmux is (likely) in copy-mode.
   * Set when the first wheel-up byte goes out after being false; cleared
   * by an explicit call to `exitScrollback()` from the "Back to live" UI.
   * We use the client-side send as the signal rather than a server-sent
   * notification because tmux copy-mode state is not exposed on the PTY.
   */
  isInScrollback: boolean;
}

/**
 * Manages a wterm terminal connected to a PTY-relayed WebSocket.
 * Returns a ref to attach to a container div, plus connection state.
 */
export function useTerminal(
  sessionId: string | null,
  wsPath: string = "ws",
  autoFocus: boolean = true,
) {
  const { settings, update } = useWebSettings();
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<WTerm | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const retryTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const countdownRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const retryCountRef = useRef(0);
  // Shared ref so the wterm onData callback can read the virtual Ctrl state
  // set by MobileTerminalToolbar. This bridges React state with the native
  // event handler without requiring focus on the proxy input.
  const ctrlActiveRef = useRef(false);
  // Stable callback set by the component to clear React's ctrlActive state
  // when onData consumes the Ctrl modifier.
  const clearCtrlRef = useRef<(() => void) | null>(null);
  // Populated inside the effect; `exitScrollback()` uses it to reset the
  // mobile scroll-depth counter when the user escapes copy-mode.
  const resetScrollbackDepthRef = useRef<(() => void) | null>(null);
  // Mirror of state.isInScrollback so the resize callback (which lives
  // inside the WTerm options closure) can read the latest value without
  // re-creating the terminal. Updated by an effect below.
  const isInScrollbackRef = useRef(false);
  // Latest pending resize that was deferred because the user was reading
  // scrollback. Drained when scrollback exits.
  const pendingResizeRef = useRef<{ cols: number; rows: number } | null>(null);
  // Most recent size measured by wterm's ResizeObserver. Until this is
  // populated, term.cols/term.rows hold wterm's hardcoded 80x24 default,
  // and sending those over WS makes the server resize the PTY before
  // wterm has measured the container. Each spurious resize fires
  // SIGWINCH at the regular-screen TUI (opencode, Claude Code) and the
  // previous frame ends up stacked into tmux scrollback as garbled
  // output. Holding off until first measurement collapses the init
  // storm to a single resize at the correct size.
  const lastMeasuredRef = useRef<{ cols: number; rows: number } | null>(null);
  // Set inside the effect; the scrollback-watch effect calls it to flush
  // a deferred resize without poking React state.
  const flushPendingResizeRef = useRef<(() => void) | null>(null);
  const [state, setState] = useState<TerminalState>({
    connected: false,
    reconnecting: false,
    retryCount: 0,
    retryCountdown: 0,
    isPrimary: true,
    isInScrollback: false,
  });

  useEffect(() => {
    if (!sessionId || !containerRef.current) return;

    // Clean up previous instance
    wsRef.current?.close();
    termRef.current?.destroy();
    if (retryTimerRef.current) clearTimeout(retryTimerRef.current);
    if (countdownRef.current) clearInterval(countdownRef.current);
    retryCountRef.current = 0;

    const container = containerRef.current;
    container.innerHTML = "";

    const isMobileViewport = () => window.innerWidth < MOBILE_BREAKPOINT_PX;
    const readFontSize = () =>
      isMobileViewport() ? settings.mobileFontSize : settings.desktopFontSize;
    const persistFontSize = (size: number) => {
      if (isMobileViewport()) update({ mobileFontSize: size });
      else update({ desktopFontSize: size });
    };
    const fontSize = readFontSize();

    // Create a child element for WTerm so the container div keeps its own
    // layout (absolute inset-0 in TerminalView, flex-1 in RightPanel).
    // WTerm adds .wterm with position:relative which would override the
    // container's positioning if we used the container directly.
    const termEl = document.createElement("div");
    termEl.style.width = "100%";
    termEl.style.height = "100%";
    container.appendChild(termEl);

    // Apply custom theme colors via CSS custom properties.
    // wterm uses --term-* variables for theming instead of a JS theme object.
    termEl.style.setProperty("--term-bg", "#141416");
    termEl.style.setProperty("--term-fg", "#e4e4e7");
    termEl.style.setProperty("--term-cursor", "#d97706");
    termEl.style.setProperty("--term-color-0", "#1c1c1f");
    termEl.style.setProperty("--term-color-1", "#ef4444");
    termEl.style.setProperty("--term-color-2", "#22c55e");
    termEl.style.setProperty("--term-color-3", "#fbbf24");
    termEl.style.setProperty("--term-color-4", "#60a5fa");
    termEl.style.setProperty("--term-color-5", "#a78bfa");
    termEl.style.setProperty("--term-color-6", "#22d3ee");
    termEl.style.setProperty("--term-color-7", "#e4e4e7");
    termEl.style.setProperty("--term-color-8", "#52525b");
    termEl.style.setProperty("--term-color-9", "#f87171");
    termEl.style.setProperty("--term-color-10", "#4ade80");
    termEl.style.setProperty("--term-color-11", "#fde68a");
    termEl.style.setProperty("--term-color-12", "#93c5fd");
    termEl.style.setProperty("--term-color-13", "#c4b5fd");
    termEl.style.setProperty("--term-color-14", "#67e8f9");
    termEl.style.setProperty("--term-color-15", "#fafafa");
    termEl.style.setProperty(
      "--term-font-family",
      "'Geist Mono', ui-monospace, 'SFMono-Regular', monospace",
    );
    termEl.style.setProperty("--term-font-size", `${fontSize}px`);

    // wterm's autoResize uses ResizeObserver to fit the terminal element.
    // Debounce resize messages to avoid SIGWINCH storms during keyboard
    // animation (ResizeObserver fires multiple times with intermediate
    // sizes). 50ms settles after the animation ends.
    let resizeDebounceTimer: ReturnType<typeof setTimeout> | null = null;
    // Flips true once the first measured resize has been sent. Until
    // then, onResize uses INITIAL_SETTLE_MS so the dashboard's mount-
    // time layout transitions coalesce into one PTY resize instead of
    // one per stable point along the way.
    let hasSentInitialResize = false;

    // All client-initiated resize sends route through this helper so the
    // scrollback gate is impossible to bypass. While the user is reading
    // scrollback, hold the latest size and drain it on exit. Without the
    // gate, claude redraws on every SIGWINCH and stacks banners into
    // tmux scrollback while the user is trying to read it.
    //
    // Also dedupes consecutive identical sizes. The ws.onopen path and
    // the rAF re-send both read from lastMeasuredRef, so back-to-back
    // calls with the same cols/rows are common; sending both would
    // produce two SIGWINCHes for one effective resize and re-introduce
    // banner stacking that the rest of this fix is trying to avoid.
    let lastSentCols = -1;
    let lastSentRows = -1;
    const sendResize = (cols: number, rows: number) => {
      if (isInScrollbackRef.current) {
        pendingResizeRef.current = { cols, rows };
        return;
      }
      if (cols === lastSentCols && rows === lastSentRows) return;
      const ws = wsRef.current;
      if (ws?.readyState !== WebSocket.OPEN) return;
      ws.send(JSON.stringify({ type: "resize", cols, rows } as ResizeMessage));
      lastSentCols = cols;
      lastSentRows = rows;
    };

    // Pre-measure cell size and seed the WTerm constructor with the
    // computed cols/rows so the WASM bridge initializes at the right
    // size. wterm's own ResizeObserver also runs and will correct any
    // drift, but seeding from this measurement guarantees that the very
    // first byte processed lives in the correct grid (not 80x24). The
    // .wterm class is added here so font-family / line-height resolve
    // from the CSS variables we just set; WTerm's constructor will
    // re-add it as a no-op.
    //
    // The measurement uses whatever font is currently rendering. With a
    // cold cache, Geist Mono may not have loaded yet and the probe gets
    // fallback-font metrics. wterm's ResizeObserver only re-measures the
    // cell on outer-size changes, so a font swap that doesn't move the
    // outer rect can leave wterm stuck with stale metrics. We schedule a
    // post-fonts.ready re-measure below to dispatch term.resize() if the
    // corrected size differs.
    termEl.classList.add("wterm");
    const computeSeedSize = (): { cols: number; rows: number } | null => {
      const probe = document.createElement("span");
      probe.textContent = "W";
      probe.style.position = "absolute";
      probe.style.visibility = "hidden";
      probe.style.whiteSpace = "pre";
      termEl.appendChild(probe);
      const cellRect = probe.getBoundingClientRect();
      probe.remove();
      const containerRect = termEl.getBoundingClientRect();
      if (
        cellRect.width <= 0 ||
        cellRect.height <= 0 ||
        containerRect.width <= 0 ||
        containerRect.height <= 0
      ) {
        return null;
      }
      // wterm's ResizeObserver uses entry.contentRect (padding-excluded).
      // Match that so the seeded size won't immediately conflict with the
      // observer's first measurement.
      const cs = getComputedStyle(termEl);
      const padX =
        (parseFloat(cs.paddingLeft) || 0) +
        (parseFloat(cs.paddingRight) || 0);
      const padY =
        (parseFloat(cs.paddingTop) || 0) +
        (parseFloat(cs.paddingBottom) || 0);
      const contentW = Math.max(0, containerRect.width - padX);
      const contentH = Math.max(0, containerRect.height - padY);
      return {
        cols: Math.max(1, Math.floor(contentW / cellRect.width)),
        rows: Math.max(1, Math.floor(contentH / cellRect.height)),
      };
    };
    const seedSize = computeSeedSize();

    const term = new WTerm(termEl, {
      cols: seedSize?.cols,
      rows: seedSize?.rows,
      autoResize: true,
      cursorBlink: true,
      onResize: (cols: number, rows: number) => {
        lastMeasuredRef.current = { cols, rows };
        if (resizeDebounceTimer) clearTimeout(resizeDebounceTimer);
        const delay = hasSentInitialResize
          ? RESIZE_DEBOUNCE_MS
          : INITIAL_SETTLE_MS;
        resizeDebounceTimer = setTimeout(() => {
          resizeDebounceTimer = null;
          hasSentInitialResize = true;
          sendResize(cols, rows);
        }, delay);
      },
    });
    if (seedSize) {
      lastMeasuredRef.current = { cols: seedSize.cols, rows: seedSize.rows };
    }

    // Once webfonts have fully loaded, re-measure and dispatch a resize
    // if the cell metrics shifted. This catches the cold-cache case where
    // the synchronous seed above used fallback-font metrics. term.resize
    // routes through onResize so the same debounce + dedup applies; if
    // the corrected size matches what was already sent, nothing goes out.
    const fontsApi = (
      document as Document & { fonts?: { ready: Promise<unknown> } }
    ).fonts;
    if (fontsApi?.ready) {
      fontsApi.ready
        .then(() => {
          if (termRef.current !== term) return;
          const remeasured = computeSeedSize();
          if (
            remeasured &&
            (remeasured.cols !== term.cols || remeasured.rows !== term.rows)
          ) {
            term.resize(remeasured.cols, remeasured.rows);
          }
        })
        .catch(() => {
          // fonts.ready can reject in headless environments where no
          // FontFaceSet is wired up; treat as no-op.
        });
    }

    // Drain handler: sends the latest deferred size when the user
    // exits scrollback. Routes through sendResize, but by this point
    // isInScrollbackRef is false so it takes the live-send path.
    flushPendingResizeRef.current = () => {
      const pending = pendingResizeRef.current;
      pendingResizeRef.current = null;
      if (!pending) return;
      sendResize(pending.cols, pending.rows);
    };

    termRef.current = term;

    // Two iOS patches for wterm's textarea:
    // 1. Move from -9999px to 0,0 so iOS shows the soft keyboard on focus.
    // 2. Fix backspace repeat: wterm calls preventDefault() on all keydown
    //    events, which prevents iOS from entering its key-repeat loop.
    //    We intercept Backspace in capture phase, skip wterm's handler,
    //    and let the native deletion happen. iOS repeat fires "input"
    //    events with inputType "deleteContentBackward" (not keydown),
    //    so we detect those and send \x7f for each one.
    //    A ZWS seed keeps the textarea non-empty so iOS always has
    //    something to delete on each repeat tick.
    // Paste: wterm's textarea has pointerEvents:none and is 1x1px, so
    // iOS can't show a paste popup on it. Use the toolbar Paste button.
    const BACKSPACE_SEED = "\u200B";
    let wtermTextarea: HTMLTextAreaElement | null = null;
    const setupMobileTextarea = () => {
      if (!isMobileViewport()) return;
      wtermTextarea = termEl.querySelector("textarea");
      if (!wtermTextarea) return;

      // Move wterm's textarea from -9999px into the viewport so iOS
      // opens the soft keyboard when it receives focus.
      wtermTextarea.style.left = "0";
      wtermTextarea.style.top = "0";
      // wterm sets opacity:0; override so the textarea is technically
      // "visible" to iOS (needed for future keyboard/paste improvements).
      wtermTextarea.style.opacity = "0.01";

      const seedTextarea = () => {
        if (wtermTextarea && !wtermTextarea.value) {
          wtermTextarea.value = BACKSPACE_SEED;
          wtermTextarea.setSelectionRange(1, 1);
        }
      };
      wtermTextarea.addEventListener("focus", seedTextarea);
      seedTextarea();

      // Capture-phase: block wterm's preventDefault on Backspace so iOS
      // can enter its key-repeat loop. Don't send \x7f here; the native
      // deletion fires a deleteContentBackward input event which handles it.
      wtermTextarea.addEventListener(
        "keydown",
        (e: KeyboardEvent) => {
          if (e.key !== "Backspace") return;
          e.stopImmediatePropagation();
        },
        true,
      );

      // All backspace handling (first press + iOS repeat) comes through
      // here as deleteContentBackward input events. Send \x7f and re-seed.
      const ta = wtermTextarea;
      ta.addEventListener("input", (e: Event) => {
        const ie = e as InputEvent;
        if (ie.inputType === "deleteContentBackward") {
          const ws = wsRef.current;
          if (ws?.readyState === WebSocket.OPEN) {
            ws.send(new TextEncoder().encode("\x7f"));
          }
        }
        queueMicrotask(seedTextarea);
      });
    };

    // Initialize the WASM bridge, then connect to the PTY.
    let connectOnReady = true;
    term
      .init()
      .then(() => {
        if (!connectOnReady) return;
        setupMobileTextarea();
        connect();
      })
      .catch((err: unknown) => {
        console.error("wterm init failed:", err);
      });

    function connect() {
      const proto = location.protocol === "https:" ? "wss:" : "ws:";
      // Pass the auth token via the WebSocket subprotocol list instead of
      // the URL query string. URLs land in access logs (axum, cloudflared,
      // Tailscale, any reverse proxy); subprotocol headers don't.
      const token = getToken();
      const url = `${proto}//${location.host}/sessions/${sessionId}/${wsPath}`;
      const ws = token
        ? new WebSocket(url, ["aoe-auth", token])
        : new WebSocket(url);
      ws.binaryType = "arraybuffer";
      wsRef.current = ws;

      ws.onopen = () => {
        retryCountRef.current = 0;
        // Reset the dedup baseline so the first resize on a fresh
        // connection always reaches the server, even if it matches
        // the size we last sent on the previous (now-closed) socket.
        // The new server-side handler may not share state with the
        // old one (think tunnel restarts) and needs to learn the
        // current PTY size from scratch.
        lastSentCols = -1;
        lastSentRows = -1;
        // Preserve isInScrollback across reconnects. Tmux's copy-mode
        // state is stored on the pane and survives client disconnects,
        // so the client-side flag should too — otherwise a WiFi blip
        // mid-scroll would hide the "Back to live" button while tmux
        // is still in copy-mode, leaving the user with no way out.
        setState((prev) => ({
          ...prev,
          connected: true,
          reconnecting: false,
          retryCount: 0,
          retryCountdown: 0,
          isPrimary: true,
        }));
        if (autoFocus) term.focus();
        // Claim primary immediately so this client's resize is applied.
        // Without this, the first resize lands in "vacant" state (which
        // works) but a race with focus/visibility events could delay it.
        ws.send(JSON.stringify({ type: "activate" } as ActivateMessage));
        // Send initial PTY dimensions only if wterm has actually measured
        // the container. Reading term.cols/term.rows directly would yield
        // wterm's 80x24 default before ResizeObserver fires, and pushing
        // that ahead of the real measurement causes a stale-default ->
        // real-size resize storm at session open. The onResize callback
        // (already wired through sendResize) delivers the correct size
        // after the first measurement, so on the very first connect this
        // branch is intentionally a no-op. On reconnect lastMeasuredRef
        // is populated and we send immediately so the new server-side
        // handler picks up the right size.
        const measured = lastMeasuredRef.current;
        if (measured) {
          sendResize(measured.cols, measured.rows);
        }
        // Re-send after layout settles. Same gate; on first connect this
        // still no-ops because ResizeObserver fires async.
        requestAnimationFrame(() => {
          const m = lastMeasuredRef.current;
          if (m) {
            sendResize(m.cols, m.rows);
          }
        });
      };

      ws.onmessage = (event: MessageEvent) => {
        if (event.data instanceof ArrayBuffer) {
          term.write(new Uint8Array(event.data));
        } else if (typeof event.data === "string") {
          // Check for server control messages before writing to terminal
          try {
            const msg = JSON.parse(event.data) as { type?: string };
            if (msg.type === "primary_status") {
              const status = msg as PrimaryStatusMessage;
              setState((prev) => ({ ...prev, isPrimary: status.is_primary }));
              return;
            }
          } catch {
            // Not JSON, treat as terminal text
          }
          term.write(event.data);
        }
      };

      ws.onclose = () => {
        setState((prev) => ({ ...prev, connected: false }));
        if (retryCountRef.current < MAX_RETRIES) {
          retryCountRef.current += 1;
          const count = retryCountRef.current;
          const delayMs = retryDelayMs(count);
          let countdown = Math.ceil(delayMs / 1000);

          setState((prev) => ({
            ...prev,
            connected: false,
            reconnecting: true,
            retryCount: count,
            retryCountdown: countdown,
          }));

          term.write(
            `\r\n\x1b[33m[Disconnected, reconnecting in ${countdown}s... (${count}/${MAX_RETRIES})]\x1b[0m\r\n`,
          );

          countdownRef.current = setInterval(() => {
            countdown -= 1;
            if (countdown > 0) {
              setState((prev) => ({ ...prev, retryCountdown: countdown }));
            }
          }, 1000);

          retryTimerRef.current = setTimeout(() => {
            if (countdownRef.current) clearInterval(countdownRef.current);
            connect();
          }, delayMs);
        } else {
          term.write(
            "\r\n\x1b[31m[Connection lost. Click retry or press Enter to reconnect.]\x1b[0m\r\n",
          );
          setState((prev) => ({
            ...prev,
            connected: false,
            reconnecting: false,
            retryCount: retryCountRef.current,
            retryCountdown: 0,
          }));
        }
      };

      ws.onerror = () => {
        // onclose will fire after onerror
      };

      // Relay keystrokes as binary. When the virtual Ctrl button is armed,
      // intercept single printable characters and transform them to their
      // Ctrl equivalents (Ctrl+A = 0x01, Ctrl+U = 0x15, etc.).
      term.onData = (data: string) => {
        if (ws.readyState !== WebSocket.OPEN) return;
        // Strip the backspace-seed ZWS so it never reaches the PTY.
        const cleaned = data.replace(/\u200B/g, "");
        if (!cleaned) return;
        data = cleaned;
        if (ctrlActiveRef.current && data.length === 1) {
          const code = data.toUpperCase().charCodeAt(0);
          if (code >= 65 && code <= 90) {
            ws.send(new TextEncoder().encode(String.fromCharCode(code - 64)));
            ctrlActiveRef.current = false;
            clearCtrlRef.current?.();
            return;
          }
        }
        ws.send(new TextEncoder().encode(data));
      };
    }

    // Touch swipe emits SGR mouse-wheel escape sequences to the PTY
    // so tmux mouse-mode enters copy-mode and scrolls.
    //
    // Track net wheel-UP depth so the client knows whether tmux is in
    // copy-mode and can pause/resume the pane's process accordingly.
    // Tmux doesn't signal copy-mode state over the PTY, so the client
    // infers it from scroll direction: depth goes 0 → 1 on first
    // wheel-UP (copy-mode entered), back to 0 when balanced (copy-mode
    // auto-exited via tmux's `-e` flag on desktop, or manually exited
    // via the "Back to live" button on mobile).
    //
    // Mobile-only: clamp wheel-DOWN emissions so depth floors at 1,
    // preventing tmux's `-e` auto-exit. On mobile the down-swipe
    // overshoots easily and the snap-to-live discards the scroll
    // position. Desktop keeps the unclamped behavior — scroll-down-past-
    // bottom auto-exits, as users expect there.
    //
    // Pause/resume apply to BOTH platforms: claude's continued output
    // shifts scrollback under the reader regardless of client size.
    const WHEEL_UP_SEQ = "\x1b[<64;1;1M";
    const WHEEL_DOWN_SEQ = "\x1b[<65;1;1M";
    let scrollbackDepth = 0;
    const sendWheel = (dir: "up" | "down", count: number) => {
      let sendCount = count;
      const clampForMobile = isMobileViewport();
      if (dir === "up") {
        scrollbackDepth += sendCount;
      } else if (clampForMobile) {
        const maxDown = Math.max(0, scrollbackDepth - 1);
        sendCount = Math.min(sendCount, maxDown);
        if (sendCount === 0) return;
        scrollbackDepth -= sendCount;
      } else {
        // Desktop: emit freely, let tmux's -e handle exit. Track depth
        // so the resume transition fires when the user scrolls back.
        scrollbackDepth = Math.max(0, scrollbackDepth - sendCount);
      }
      const seq = dir === "up" ? WHEEL_UP_SEQ : WHEEL_DOWN_SEQ;
      const ws = wsRef.current;
      if (ws?.readyState !== WebSocket.OPEN) return;
      for (let i = 0; i < sendCount; i++) {
        ws.send(new TextEncoder().encode(seq));
      }
      // Transition into scrollback on first wheel-up (desktop + mobile).
      if (dir === "up") {
        setState((prev) => {
          if (prev.isInScrollback) return prev;
          if (ws.readyState === WebSocket.OPEN) {
            ws.send(
              JSON.stringify({ type: "pause_output" } as PauseOutputMessage),
            );
          }
          return { ...prev, isInScrollback: true };
        });
      } else if (scrollbackDepth === 0) {
        // Back at live on desktop (tmux auto-exited copy-mode via -e);
        // resume the pane's process. On mobile this branch never fires
        // because the clamp keeps depth >= 1; mobile exits via the
        // explicit "Back to live" button (see exitScrollback).
        setState((prev) => {
          if (!prev.isInScrollback) return prev;
          if (ws.readyState === WebSocket.OPEN) {
            ws.send(
              JSON.stringify({
                type: "resume_output",
              } as ResumeOutputMessage),
            );
          }
          return { ...prev, isInScrollback: false };
        });
      }
    };
    // Expose so exitScrollback can reset the depth in sync with the
    // Escape sent to tmux.
    const resetScrollbackDepth = () => {
      scrollbackDepth = 0;
    };
    resetScrollbackDepthRef.current = resetScrollbackDepth;

    let touchMidY = 0;
    let touchAccum = 0;
    let lastMoveTs = 0;
    let velocity = 0;
    let momentumRaf: number | null = null;
    let gestureMode: "single-scroll" | "pinch" | "scroll" | null = null;
    let pinchStartDist = 0;
    let pinchStartSize = DEFAULT_FONT_SIZE;
    let pinchStartMidY = 0;
    let singleStartY = 0;
    let singleStartTs = 0;
    let singleY = 0;
    let singleAccum = 0;
    let singleLastTs = 0;
    let suppressNextClick = false;
    const GESTURE_LOCK_PX = 12;
    const LINES_PER_WHEEL = 2;
    const MAX_VELOCITY = 2.0;
    const MAX_WHEELS_PER_FRAME = 6;
    const clampV = (v: number) =>
      Math.max(-MAX_VELOCITY, Math.min(MAX_VELOCITY, v));
    const cellHeight = () => {
      const cs = getComputedStyle(term.element);
      return (
        parseFloat(cs.getPropertyValue("--term-font-size")) || DEFAULT_FONT_SIZE
      );
    };
    const pxPerWheel = () => cellHeight() * LINES_PER_WHEEL;
    const prefersReducedMotion = () =>
      window.matchMedia?.("(prefers-reduced-motion: reduce)").matches ?? false;

    const midpointY = (e: TouchEvent) => {
      const a = e.touches[0];
      const b = e.touches[1];
      if (!a || !b) return 0;
      return (a.clientY + b.clientY) / 2;
    };

    const touchDistance = (e: TouchEvent) => {
      const a = e.touches[0];
      const b = e.touches[1];
      if (!a || !b) return 0;
      return Math.hypot(a.clientX - b.clientX, a.clientY - b.clientY);
    };

    const clampFont = (n: number) =>
      Math.max(MIN_FONT_SIZE, Math.min(MAX_FONT_SIZE, n));

    // Font size updates via CSS custom property. Coalesce to one per frame.
    let pendingFontSize: number | null = null;
    let fontSizeRaf: number | null = null;
    const currentFontSize = (): number => {
      const cs = getComputedStyle(term.element);
      return (
        parseFloat(cs.getPropertyValue("--term-font-size")) || DEFAULT_FONT_SIZE
      );
    };
    const applyFontSize = (size: number) => {
      const next = clampFont(Math.round(size));
      const current = currentFontSize();
      if (next !== current) {
        term.element.style.setProperty("--term-font-size", `${next}px`);
        // wterm's ResizeObserver will detect the size change and call resize()
      }
      return next;
    };
    const scheduleFontSize = (size: number) => {
      pendingFontSize = clampFont(Math.round(size));
      if (fontSizeRaf !== null) return;
      fontSizeRaf = requestAnimationFrame(() => {
        fontSizeRaf = null;
        if (pendingFontSize !== null) {
          applyFontSize(pendingFontSize);
          pendingFontSize = null;
        }
      });
    };
    const flushFontSize = () => {
      if (fontSizeRaf !== null) {
        cancelAnimationFrame(fontSizeRaf);
        fontSizeRaf = null;
      }
      if (pendingFontSize !== null) {
        applyFontSize(pendingFontSize);
        pendingFontSize = null;
      }
    };
    const currentPendingOrLiveSize = () => pendingFontSize ?? currentFontSize();

    const cancelMomentum = () => {
      if (momentumRaf !== null) {
        cancelAnimationFrame(momentumRaf);
        momentumRaf = null;
      }
    };

    const onTouchStart = (e: TouchEvent) => {
      cancelMomentum();
      suppressNextClick = false;

      if (e.touches.length === 1) {
        const t = e.touches[0]!;
        singleStartY = t.clientY;
        singleStartTs = performance.now();
        singleY = t.clientY;
        singleAccum = 0;
        singleLastTs = singleStartTs;
        velocity = 0;
        gestureMode = null;
        return;
      }

      if (e.touches.length === 2) {
        gestureMode = null;
        touchMidY = midpointY(e);
        touchAccum = 0;
        velocity = 0;
        lastMoveTs = performance.now();
        pinchStartDist = touchDistance(e);
        pinchStartSize = currentFontSize();
        pinchStartMidY = touchMidY;
      }
    };

    const onTouchMove = (e: TouchEvent) => {
      // Single-finger scroll
      if (
        e.touches.length === 1 &&
        (gestureMode === null || gestureMode === "single-scroll")
      ) {
        const t = e.touches[0]!;
        const y = t.clientY;
        const now = performance.now();

        if (gestureMode === null) {
          if (Math.abs(y - singleStartY) < GESTURE_LOCK_PX) {
            singleLastTs = now;
            return;
          }
          gestureMode = "single-scroll";
          singleY = y;
        }

        e.preventDefault();

        const dy = singleY - y;
        singleY = y;
        singleAccum += dy;
        const step = pxPerWheel();
        const rawWheels = Math.trunc(singleAccum / step);
        const wheels = Math.max(
          -MAX_WHEELS_PER_FRAME,
          Math.min(MAX_WHEELS_PER_FRAME, rawWheels),
        );
        if (wheels !== 0) {
          sendWheel(wheels > 0 ? "up" : "down", Math.abs(wheels));
          singleAccum -= wheels * step;
          const dt = Math.max(1, now - singleLastTs);
          velocity = clampV(dy / dt);
        }
        singleLastTs = now;
        return;
      }

      // Two-finger gesture (scroll or pinch)
      if (e.touches.length !== 2) return;
      e.preventDefault();
      const y = midpointY(e);
      const now = performance.now();
      const dist = touchDistance(e);

      if (gestureMode === null || gestureMode === "single-scroll") {
        const distDelta = Math.abs(dist - pinchStartDist);
        const panDelta = Math.abs(y - pinchStartMidY);
        if (Math.max(distDelta, panDelta) < GESTURE_LOCK_PX) {
          lastMoveTs = now;
          return;
        }
        gestureMode = distDelta > panDelta ? "pinch" : "scroll";
        touchMidY = y;
      }

      if (gestureMode === "pinch") {
        if (pinchStartDist > 0) {
          scheduleFontSize(pinchStartSize * (dist / pinchStartDist));
        }
        lastMoveTs = now;
        return;
      }

      const dy = touchMidY - y;
      touchMidY = y;
      touchAccum += dy;
      const step = pxPerWheel();
      const rawWheels = Math.trunc(touchAccum / step);
      const wheels = Math.max(
        -MAX_WHEELS_PER_FRAME,
        Math.min(MAX_WHEELS_PER_FRAME, rawWheels),
      );
      if (wheels !== 0) {
        sendWheel(wheels > 0 ? "up" : "down", Math.abs(wheels));
        touchAccum -= wheels * step;
        const dt = Math.max(1, now - lastMoveTs);
        velocity = clampV(dy / dt);
      }
      lastMoveTs = now;
    };

    const onTouchEnd = (e: TouchEvent) => {
      if (e.touches.length > 0) return;
      if (gestureMode === "pinch") {
        flushFontSize();
        persistFontSize(currentFontSize());
        gestureMode = null;
        velocity = 0;
        return;
      }
      const wasScrolling =
        gestureMode === "single-scroll" || gestureMode === "scroll";
      gestureMode = null;
      if (wasScrolling) suppressNextClick = true;
      if (prefersReducedMotion() || Math.abs(velocity) < 0.05) {
        velocity = 0;
        return;
      }
      let v = velocity;
      let last = performance.now();
      let carry = 0;
      const decay = () => {
        const now = performance.now();
        const dt = now - last;
        last = now;
        v *= Math.pow(0.92, dt / 16);
        carry += v * dt;
        const step = pxPerWheel();
        const rawW = Math.trunc(carry / step);
        const w = Math.max(
          -MAX_WHEELS_PER_FRAME,
          Math.min(MAX_WHEELS_PER_FRAME, rawW),
        );
        if (w !== 0) {
          sendWheel(w > 0 ? "up" : "down", Math.abs(w));
          carry -= w * step;
        }
        if (Math.abs(v) > 0.05) {
          momentumRaf = requestAnimationFrame(decay);
        } else {
          momentumRaf = null;
        }
      };
      momentumRaf = requestAnimationFrame(decay);
    };

    // Attach touch handlers to the .wterm element. `touch-action: none`
    // tells the browser we own all touch behavior here, so iOS Safari
    // won't engage native scroll/rubber-band on the dead-zone frames
    // before our handler decides whether to preventDefault.
    const viewport = term.element;
    viewport.style.touchAction = "none";
    const touchOpts = { passive: false, capture: true } as const;
    viewport.addEventListener("touchstart", onTouchStart, touchOpts);
    viewport.addEventListener("touchmove", onTouchMove, touchOpts);
    viewport.addEventListener("touchend", onTouchEnd, touchOpts);
    viewport.addEventListener("touchcancel", onTouchEnd, touchOpts);

    // On mobile, suppress ALL click-to-focus so the keyboard is only
    // controlled via the FAB button. On desktop, only suppress after a
    // scroll gesture.
    const onClickCapture = (e: MouseEvent) => {
      const wasScroll = suppressNextClick;
      suppressNextClick = false;
      if (isMobileViewport() || wasScroll) e.stopPropagation();
    };
    viewport.addEventListener("click", onClickCapture, true);

    // Mouse wheel: Ctrl+wheel = zoom (trackpad pinch), plain wheel = scroll.
    // wterm has no built-in wheel handling and tmux manages its own scrollback,
    // so we convert wheel events to SGR mouse-wheel escape sequences (same
    // mechanism the touch handler uses).
    let wheelAccum = 0;
    let scrollWheelAccum = 0;
    let wheelPersistTimer: ReturnType<typeof setTimeout> | null = null;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();

      if (e.ctrlKey) {
        // Trackpad pinch fires wheel events with ctrlKey=true
        wheelAccum -= e.deltaY * WHEEL_ZOOM_SENSITIVITY;
        if (Math.abs(wheelAccum) < 1) return;
        const delta = Math.trunc(wheelAccum);
        wheelAccum -= delta;
        const base = currentPendingOrLiveSize();
        const next = clampFont(Math.round(base + delta));
        if (next === base) return;
        scheduleFontSize(next);
        if (wheelPersistTimer) clearTimeout(wheelPersistTimer);
        wheelPersistTimer = setTimeout(() => {
          flushFontSize();
          persistFontSize(currentFontSize());
          wheelPersistTimer = null;
        }, WHEEL_PERSIST_DEBOUNCE_MS);
        return;
      }

      // Plain scroll: convert to SGR mouse-wheel sequences for tmux
      scrollWheelAccum += e.deltaY;
      const step = pxPerWheel();
      const rawWheels = Math.trunc(scrollWheelAccum / step);
      const wheels = Math.max(
        -MAX_WHEELS_PER_FRAME,
        Math.min(MAX_WHEELS_PER_FRAME, rawWheels),
      );
      if (wheels !== 0) {
        sendWheel(wheels > 0 ? "down" : "up", Math.abs(wheels));
        scrollWheelAccum -= wheels * step;
      }
    };
    viewport.addEventListener("wheel", onWheel, { passive: false });

    // When the user switches to this tab/window, tell the server so it
    // can claim primary and resize the PTY to match this viewport.
    const sendActivate = () => {
      const ws = wsRef.current;
      if (ws?.readyState === WebSocket.OPEN) {
        const msg: ActivateMessage = { type: "activate" };
        ws.send(JSON.stringify(msg));
      }
    };
    const onVisibilityChange = () => {
      if (document.visibilityState === "visible") sendActivate();
    };
    document.addEventListener("visibilitychange", onVisibilityChange);
    window.addEventListener("focus", sendActivate);

    return () => {
      connectOnReady = false;
      cancelMomentum();
      document.removeEventListener("visibilitychange", onVisibilityChange);
      window.removeEventListener("focus", sendActivate);
      viewport.removeEventListener("touchstart", onTouchStart, touchOpts);
      viewport.removeEventListener("touchmove", onTouchMove, touchOpts);
      viewport.removeEventListener("touchend", onTouchEnd, touchOpts);
      viewport.removeEventListener("touchcancel", onTouchEnd, touchOpts);
      viewport.removeEventListener("click", onClickCapture, true);
      viewport.removeEventListener("wheel", onWheel);
      if (wheelPersistTimer) clearTimeout(wheelPersistTimer);
      if (resizeDebounceTimer) clearTimeout(resizeDebounceTimer);
      if (fontSizeRaf !== null) cancelAnimationFrame(fontSizeRaf);
      wsRef.current?.close();
      term.destroy();
      if (retryTimerRef.current) clearTimeout(retryTimerRef.current);
      if (countdownRef.current) clearInterval(countdownRef.current);
      termRef.current = null;
      wsRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, wsPath]);

  // Apply font size changes from settings UI to the live terminal.
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    const size =
      window.innerWidth < MOBILE_BREAKPOINT_PX
        ? settings.mobileFontSize
        : settings.desktopFontSize;
    const current =
      parseFloat(
        getComputedStyle(term.element).getPropertyValue("--term-font-size"),
      ) || DEFAULT_FONT_SIZE;
    if (current !== size) {
      term.element.style.setProperty("--term-font-size", `${size}px`);
    }
  }, [settings.mobileFontSize, settings.desktopFontSize]);

  // Mirror state.isInScrollback into a ref so the resize callback can read
  // the latest value, and drain any pending deferred resize when the user
  // exits scrollback (so claude redraws once at the final size).
  useEffect(() => {
    const wasInScrollback = isInScrollbackRef.current;
    isInScrollbackRef.current = state.isInScrollback;
    if (wasInScrollback && !state.isInScrollback) {
      flushPendingResizeRef.current?.();
    }
  }, [state.isInScrollback]);

  const manualReconnect = () => {
    retryCountRef.current = 0;
    setState((prev) => ({
      ...prev,
      connected: false,
      reconnecting: true,
      retryCount: 0,
      retryCountdown: 0,
    }));
    wsRef.current?.close();
  };

  const sendData = useCallback((data: string) => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      wsRef.current.send(new TextEncoder().encode(data));
    }
  }, []);

  const activate = useCallback(() => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      wsRef.current.send(
        JSON.stringify({ type: "activate" } as ActivateMessage),
      );
    }
  }, []);

  // Mobile-only: sends ESC to force tmux out of copy-mode. On mobile we
  // clamp scroll-down so tmux never reaches the bottom on its own; the
  // button is the only way back to live.
  //
  // Also sends `resume_output` so the server SIGCONTs the pane's
  // process tree (which was paused on entry to scrollback). The server
  // auto-resumes on disconnect as a safety net, so forgetting this is
  // annoying but not permanent.
  const exitScrollback = useCallback(() => {
    const ws = wsRef.current;
    if (ws?.readyState === WebSocket.OPEN) {
      ws.send(
        JSON.stringify({ type: "resume_output" } as ResumeOutputMessage),
      );
      ws.send(new TextEncoder().encode("\x1b"));
    }
    resetScrollbackDepthRef.current?.();
    setState((prev) =>
      prev.isInScrollback ? { ...prev, isInScrollback: false } : prev,
    );
  }, []);

  return {
    containerRef,
    termRef,
    state,
    manualReconnect,
    sendData,
    activate,
    exitScrollback,
    ctrlActiveRef,
    clearCtrlRef,
  };
}
