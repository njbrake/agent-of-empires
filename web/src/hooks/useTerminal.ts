import { useCallback, useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import type { ResizeMessage } from "../lib/types";
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
 * Manages an xterm.js terminal connected to a PTY-relayed WebSocket.
 * Returns a ref to attach to a container div, plus connection state.
 */
export function useTerminal(
  sessionId: string | null,
  wsPath: string = "ws",
) {
  const { settings, update } = useWebSettings();
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const retryTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const countdownRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const retryCountRef = useRef(0);
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
    termRef.current?.dispose();
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

    const term = new Terminal({
      cursorBlink: true,
      fontSize,
      fontFamily: "'Geist Mono', ui-monospace, 'SFMono-Regular', monospace",
      theme: {
        background: "#141416",
        foreground: "#e4e4e7",
        cursor: "#d97706",
        cursorAccent: "#141416",
        selectionBackground: "rgba(161, 161, 170, 0.2)",
        black: "#1c1c1f",
        red: "#ef4444",
        green: "#22c55e",
        yellow: "#fbbf24",
        blue: "#60a5fa",
        magenta: "#a78bfa",
        cyan: "#22d3ee",
        white: "#e4e4e7",
        brightBlack: "#52525b",
        brightRed: "#f87171",
        brightGreen: "#4ade80",
        brightYellow: "#fde68a",
        brightBlue: "#93c5fd",
        brightMagenta: "#c4b5fd",
        brightCyan: "#67e8f9",
        brightWhite: "#fafafa",
      },
    });

    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(container);

    termRef.current = term;
    fitRef.current = fitAddon;

    requestAnimationFrame(() => fitAddon.fit());

    let dataDisposable: { dispose: () => void } | null = null;
    let resizeDisposable: { dispose: () => void } | null = null;

    function connect() {
      const proto = location.protocol === "https:" ? "wss:" : "ws:";
      const ws = new WebSocket(
        `${proto}//${location.host}/sessions/${sessionId}/${wsPath}`,
      );
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
        const dims = fitAddon.proposeDimensions();
        if (
          dims &&
          Number.isFinite(dims.cols) &&
          Number.isFinite(dims.rows) &&
          dims.cols > 0 &&
          dims.rows > 0
        ) {
          const msg: ResizeMessage = {
            type: "resize",
            cols: Math.round(dims.cols),
            rows: Math.round(dims.rows),
          };
          ws.send(JSON.stringify(msg));
        }
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

      // Relay keystrokes as binary
      dataDisposable?.dispose();
      dataDisposable = term.onData((data: string) => {
        if (ws.readyState === WebSocket.OPEN) {
          ws.send(new TextEncoder().encode(data));
        }
      });

      // Relay resize
      resizeDisposable?.dispose();
      resizeDisposable = term.onResize(({ cols, rows }) => {
        if (ws.readyState === WebSocket.OPEN) {
          const msg: ResizeMessage = { type: "resize", cols, rows };
          ws.send(JSON.stringify(msg));
        }
      });
    }

    connect();

    // Window resize -> fit terminal
    const handleResize = () => fitAddon.fit();
    window.addEventListener("resize", handleResize);

    // Two-finger swipe emits SGR mouse-wheel escape sequences to the PTY,
    // so tmux mouse-mode enters copy-mode and scrolls (and apps that read
    // wheel events handle their own scrolling). We intentionally do NOT
    // call term.scrollLines() — under tmux/alt-screen the local xterm
    // scrollback is empty, so scrollLines would no-op. One-finger touches
    // are left to xterm's native handling (text selection, taps). Calling
    // preventDefault on one-finger moves breaks selection — never do it.
    //
    // Why we attach to .xterm and not the outer container: xterm.js
    // registers document-level touch handlers that dispatch custom gesture
    // events. Our listener sits on xterm's root element with capture:true
    // so we fire before xterm's internal handlers. touch-action:none
    // prevents the browser's native pan from competing for the gesture.
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
    let velocity = 0; // pixels per ms
    let momentumRaf: number | null = null;
    // Pinch state: once a two-finger gesture exceeds a small deadzone, we
    // lock into either 'pinch' (zoom) or 'scroll' for the rest of the gesture.
    let gestureMode: "pinch" | "scroll" | null = null;
    let pinchStartDist = 0;
    let pinchStartSize = DEFAULT_FONT_SIZE;
    let pinchStartMidY = 0;
    const GESTURE_LOCK_PX = 12;
    const LINES_PER_WHEEL = 2; // swipe pixels-per-wheel-event = cellHeight * 2
    const MAX_VELOCITY = 2.0; // px/ms — a genuinely fast finger is ~1–2 px/ms
    const MAX_WHEELS_PER_FRAME = 6; // cap runaway bursts
    const clampV = (v: number) =>
      Math.max(-MAX_VELOCITY, Math.min(MAX_VELOCITY, v));
    const cellHeight = () => term.options.fontSize ?? DEFAULT_FONT_SIZE;
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

    // Pinch/wheel events fire faster than the frame rate; coalesce font-size
    // updates to at most one fitAddon.fit() per animation frame to avoid
    // layout thrash and spamming PTY resize messages over the WebSocket.
    let pendingFontSize: number | null = null;
    let fontSizeRaf: number | null = null;
    const applyFontSize = (size: number) => {
      const next = clampFont(Math.round(size));
      if (next !== term.options.fontSize) {
        term.options.fontSize = next;
        fitAddon.fit();
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
      pendingFontSize ?? term.options.fontSize ?? DEFAULT_FONT_SIZE;

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
      pinchStartSize = term.options.fontSize ?? DEFAULT_FONT_SIZE;
      pinchStartMidY = touchMidY;
    };

    const onTouchMove = (e: TouchEvent) => {
      if (e.touches.length !== 2) return; // Single-finger = xterm handles it.
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
        // Reset scroll baseline so we don't replay the deadzone travel.
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
        // Positive wheels means scrolled up (dy positive = finger moved up =
        // content should scroll up to reveal lines above = wheel-up).
        sendWheel(wheels > 0 ? "up" : "down", Math.abs(wheels));
        touchAccum -= wheels * step;
        const dt = Math.max(1, now - lastMoveTs);
        velocity = clampV(dy / dt);
      }
      lastMoveTs = now;
    };

    const onTouchEnd = (e: TouchEvent) => {
      // Fires whenever the touch count changes; only decay when all fingers lift.
      if (e.touches.length > 0) return;
      if (gestureMode === "pinch") {
        flushFontSize();
        persistFontSize(term.options.fontSize ?? DEFAULT_FONT_SIZE);
        gestureMode = null;
        velocity = 0;
        return;
      }
      gestureMode = null;
      if (prefersReducedMotion() || Math.abs(velocity) < 0.05) {
        velocity = 0;
        return;
      }
      let v = velocity; // px/ms
      let last = performance.now();
      let carry = 0;
      const decay = () => {
        const now = performance.now();
        const dt = now - last;
        last = now;
        v *= Math.pow(0.92, dt / 16); // ~400ms decay
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

    // Attach to the root `.xterm` element created by term.open. It's the
    // common parent of .xterm-viewport, .xterm-screen, and helpers, so
    // touches on any xterm surface bubble here first. Capture phase
    // guarantees we fire before xterm's document-level gesture handler.
    const viewport =
      container.querySelector<HTMLElement>(".xterm") ?? container;
    viewport.style.touchAction = "none";
    const touchOpts = { passive: false, capture: true } as const;
    viewport.addEventListener("touchstart", onTouchStart, touchOpts);
    viewport.addEventListener("touchmove", onTouchMove, touchOpts);
    viewport.addEventListener("touchend", onTouchEnd, touchOpts);
    viewport.addEventListener("touchcancel", onTouchEnd, touchOpts);

    // Trackpad pinch fires wheel events with ctrlKey=true (and Ctrl+wheel
    // mouse zoom matches the same convention). Debounce persistence so we
    // don't hammer localStorage on every frame of a pinch.
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
        persistFontSize(term.options.fontSize ?? DEFAULT_FONT_SIZE);
        wheelPersistTimer = null;
      }, WHEEL_PERSIST_DEBOUNCE_MS);
    };
    viewport.addEventListener("wheel", onWheel, { passive: false });

    return () => {
      cancelMomentum();
      viewport.removeEventListener("touchstart", onTouchStart, touchOpts);
      viewport.removeEventListener("touchmove", onTouchMove, touchOpts);
      viewport.removeEventListener("touchend", onTouchEnd, touchOpts);
      viewport.removeEventListener("touchcancel", onTouchEnd, touchOpts);
      viewport.removeEventListener("wheel", onWheel);
      if (wheelPersistTimer) clearTimeout(wheelPersistTimer);
      if (fontSizeRaf !== null) cancelAnimationFrame(fontSizeRaf);
      window.removeEventListener("resize", handleResize);
      dataDisposable?.dispose();
      resizeDisposable?.dispose();
      wsRef.current?.close();
      term.dispose();
      if (retryTimerRef.current) clearTimeout(retryTimerRef.current);
      if (countdownRef.current) clearInterval(countdownRef.current);
      termRef.current = null;
      wsRef.current = null;
      fitRef.current = null;
    };
    // We intentionally do NOT depend on settings.{mobile,desktop}FontSize —
    // that would tear down and reconnect the PTY every time the font
    // changed (via slider or pinch). The sync effect below mutates
    // term.options.fontSize in-place instead.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sessionId, wsPath]);

  // Apply font size changes (from settings UI or pinch persistence) to the
  // live terminal without recreating it.
  useEffect(() => {
    const term = termRef.current;
    const fit = fitRef.current;
    if (!term) return;
    const size =
      window.innerWidth < MOBILE_BREAKPOINT_PX
        ? settings.mobileFontSize
        : settings.desktopFontSize;
    if (term.options.fontSize !== size) {
      term.options.fontSize = size;
      fit?.fit();
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
    // Trigger effect by disconnecting current WS
    wsRef.current?.close();
  };

  const sendData = useCallback((data: string) => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      wsRef.current.send(new TextEncoder().encode(data));
    }
  }, []);

  return { containerRef, termRef, state, manualReconnect, sendData };
}
