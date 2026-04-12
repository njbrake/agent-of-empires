import { useEffect, useRef, useState } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import type { ResizeMessage } from "../lib/types";

const MAX_RETRIES = 3;
const RETRY_DELAY = 5000;

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

    const fontSize = window.innerWidth < 768 ? 12 : 14;

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
        if (dims) {
          const msg: ResizeMessage = {
            type: "resize",
            cols: dims.cols,
            rows: dims.rows,
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

    return () => {
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
  }, [sessionId, wsPath]);

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

  return { containerRef, state, manualReconnect };
}
