import { useCallback, useEffect, useRef, useState } from "react";
import { WTerm } from "@wterm/dom";
import type { ResizeMessage } from "../lib/types";
import { getToken } from "../lib/token";
import { useWebSettings } from "./useWebSettings";

const MAX_RETRIES = 3;
const RETRY_DELAY = 5000;
const MIN_FONT_SIZE = 6;
const MAX_FONT_SIZE = 28;
const DEFAULT_FONT_SIZE = 14;
const MOBILE_BREAKPOINT_PX = 768;
const WHEEL_ZOOM_SENSITIVITY = 0.05;
const WHEEL_PERSIST_DEBOUNCE_MS = 400;

export interface TerminalState {
  connected: boolean;
  reconnecting: boolean;
  retryCount: number;
  retryCountdown: number;
}

/**
 * Manages a wterm terminal connected to a PTY-relayed WebSocket.
 * Returns a ref to attach to a container div, plus connection state.
 */
export function useTerminal(
  sessionId: string | null,
  wsPath: string = "ws",
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
  const [state, setState] = useState<TerminalState>({
    connected: false,
    reconnecting: false,
    retryCount: 0,
    retryCountdown: 0,
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
    const term = new WTerm(termEl, {
      autoResize: true,
      cursorBlink: true,
      onResize: (cols: number, rows: number) => {
        // Relay resize to the PTY backend
        const ws = wsRef.current;
        if (ws?.readyState === WebSocket.OPEN) {
          const msg: ResizeMessage = { type: "resize", cols, rows };
          ws.send(JSON.stringify(msg));
        }
      },
    });

    termRef.current = term;

    // Initialize the WASM bridge, then connect to the PTY.
    let connectOnReady = true;
    term
      .init()
      .then(() => {
        if (!connectOnReady) return;
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
        setState({
          connected: true,
          reconnecting: false,
          retryCount: 0,
          retryCountdown: 0,
        });
        term.focus();
        // Send initial PTY dimensions from the already-autoresized terminal.
        if (
          term.cols > 0 &&
          term.rows > 0 &&
          ws.readyState === WebSocket.OPEN
        ) {
          const msg: ResizeMessage = {
            type: "resize",
            cols: term.cols,
            rows: term.rows,
          };
          ws.send(JSON.stringify(msg));
        }
        // Re-send after layout settles
        requestAnimationFrame(() => {
          if (
            term.cols > 0 &&
            term.rows > 0 &&
            ws.readyState === WebSocket.OPEN
          ) {
            const msg: ResizeMessage = {
              type: "resize",
              cols: term.cols,
              rows: term.rows,
            };
            ws.send(JSON.stringify(msg));
          }
        });
      };

      ws.onmessage = (event: MessageEvent) => {
        if (event.data instanceof ArrayBuffer) {
          term.write(new Uint8Array(event.data));
        } else {
          term.write(event.data as string);
        }
      };

      ws.onclose = () => {
        setState((prev) => ({ ...prev, connected: false }));
        if (retryCountRef.current < MAX_RETRIES) {
          retryCountRef.current += 1;
          const count = retryCountRef.current;
          let countdown = RETRY_DELAY / 1000;

          setState({
            connected: false,
            reconnecting: true,
            retryCount: count,
            retryCountdown: countdown,
          });

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
          }, RETRY_DELAY);
        } else {
          term.write(
            "\r\n\x1b[31m[Connection lost. Click retry or press Enter to reconnect.]\x1b[0m\r\n",
          );
          setState({
            connected: false,
            reconnecting: false,
            retryCount: retryCountRef.current,
            retryCountdown: 0,
          });
        }
      };

      ws.onerror = () => {
        // onclose will fire after onerror
      };

      // Relay keystrokes as binary. When the virtual Ctrl button is armed,
      // intercept single printable characters and transform them to their
      // Ctrl equivalents (Ctrl+A = 0x01, Ctrl+U = 0x15, etc.). This works
      // regardless of whether the keystroke came from our mobile proxy input
      // or from wterm's own hidden textarea.
      term.onData = (data: string) => {
        if (ws.readyState !== WebSocket.OPEN) return;
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

    // Two-finger swipe emits SGR mouse-wheel escape sequences to the PTY,
    // so tmux mouse-mode enters copy-mode and scrolls.
    const WHEEL_UP_SEQ = "\x1b[<64;1;1M";
    const WHEEL_DOWN_SEQ = "\x1b[<65;1;1M";
    const sendWheel = (dir: "up" | "down", count: number) => {
      const seq = dir === "up" ? WHEEL_UP_SEQ : WHEEL_DOWN_SEQ;
      const ws = wsRef.current;
      if (ws?.readyState !== WebSocket.OPEN) return;
      for (let i = 0; i < count; i++) {
        ws.send(new TextEncoder().encode(seq));
      }
    };

    let touchMidY = 0;
    let touchAccum = 0;
    let lastMoveTs = 0;
    let velocity = 0;
    let momentumRaf: number | null = null;
    let gestureMode: "pinch" | "scroll" | null = null;
    let pinchStartDist = 0;
    let pinchStartSize = DEFAULT_FONT_SIZE;
    let pinchStartMidY = 0;
    const GESTURE_LOCK_PX = 12;
    const LINES_PER_WHEEL = 2;
    const MAX_VELOCITY = 2.0;
    const MAX_WHEELS_PER_FRAME = 6;
    const clampV = (v: number) =>
      Math.max(-MAX_VELOCITY, Math.min(MAX_VELOCITY, v));
    const cellHeight = () => {
      const cs = getComputedStyle(term.element);
      return parseFloat(cs.getPropertyValue("--term-font-size")) || DEFAULT_FONT_SIZE;
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
      return parseFloat(cs.getPropertyValue("--term-font-size")) || DEFAULT_FONT_SIZE;
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
    const currentPendingOrLiveSize = () =>
      pendingFontSize ?? currentFontSize();

    const cancelMomentum = () => {
      if (momentumRaf !== null) {
        cancelAnimationFrame(momentumRaf);
        momentumRaf = null;
      }
    };

    const onTouchStart = (e: TouchEvent) => {
      cancelMomentum();
      if (e.touches.length !== 2) return;
      touchMidY = midpointY(e);
      touchAccum = 0;
      velocity = 0;
      lastMoveTs = performance.now();
      gestureMode = null;
      pinchStartDist = touchDistance(e);
      pinchStartSize = currentFontSize();
      pinchStartMidY = touchMidY;
    };

    const onTouchMove = (e: TouchEvent) => {
      if (e.touches.length !== 2) return;
      e.preventDefault();
      const y = midpointY(e);
      const now = performance.now();
      const dist = touchDistance(e);

      if (gestureMode === null) {
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
      gestureMode = null;
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

    // Attach touch handlers to the .wterm element. wterm adds this class to
    // the container automatically during construction.
    const viewport = term.element;
    viewport.style.touchAction = "none";
    const touchOpts = { passive: false, capture: true } as const;
    viewport.addEventListener("touchstart", onTouchStart, touchOpts);
    viewport.addEventListener("touchmove", onTouchMove, touchOpts);
    viewport.addEventListener("touchend", onTouchEnd, touchOpts);
    viewport.addEventListener("touchcancel", onTouchEnd, touchOpts);

    // Trackpad pinch fires wheel events with ctrlKey=true
    let wheelAccum = 0;
    let wheelPersistTimer: ReturnType<typeof setTimeout> | null = null;
    const onWheel = (e: WheelEvent) => {
      if (!e.ctrlKey) return;
      e.preventDefault();
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
    };
    viewport.addEventListener("wheel", onWheel, { passive: false });

    return () => {
      connectOnReady = false;
      cancelMomentum();
      viewport.removeEventListener("touchstart", onTouchStart, touchOpts);
      viewport.removeEventListener("touchmove", onTouchMove, touchOpts);
      viewport.removeEventListener("touchend", onTouchEnd, touchOpts);
      viewport.removeEventListener("touchcancel", onTouchEnd, touchOpts);
      viewport.removeEventListener("wheel", onWheel);
      if (wheelPersistTimer) clearTimeout(wheelPersistTimer);
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

  const manualReconnect = () => {
    retryCountRef.current = 0;
    setState({
      connected: false,
      reconnecting: true,
      retryCount: 0,
      retryCountdown: 0,
    });
    wsRef.current?.close();
  };

  const sendData = useCallback((data: string) => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      wsRef.current.send(new TextEncoder().encode(data));
    }
  }, []);

  return { containerRef, termRef, state, manualReconnect, sendData, ctrlActiveRef, clearCtrlRef };
}
