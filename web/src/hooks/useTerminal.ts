import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import type { ResizeMessage } from "../lib/types";

/**
 * Manages an xterm.js terminal connected to a PTY-relayed WebSocket.
 * Returns a ref to attach to a container div.
 */
export function useTerminal(sessionId: string | null) {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const wsRef = useRef<WebSocket | null>(null);
  const fitRef = useRef<FitAddon | null>(null);

  useEffect(() => {
    if (!sessionId || !containerRef.current) return;

    // Clean up previous instance
    wsRef.current?.close();
    termRef.current?.dispose();

    const container = containerRef.current;
    container.innerHTML = "";

    const term = new Terminal({
      cursorBlink: true,
      fontSize: 14,
      fontFamily: "'JetBrains Mono', ui-monospace, monospace",
      theme: {
        background: "#020617",
        foreground: "#e2e8f0",
        cursor: "#d97706",
        cursorAccent: "#020617",
        selectionBackground: "rgba(217, 119, 6, 0.2)",
        black: "#0f172a",
        red: "#ef4444",
        green: "#22c55e",
        yellow: "#fbbf24",
        blue: "#0d9488",
        magenta: "#a78bfa",
        cyan: "#14b8a6",
        white: "#e2e8f0",
        brightBlack: "#475569",
        brightRed: "#f87171",
        brightGreen: "#4ade80",
        brightYellow: "#fde68a",
        brightBlue: "#2dd4bf",
        brightMagenta: "#c4b5fd",
        brightCyan: "#5eead4",
        brightWhite: "#f8fafc",
      },
    });

    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(container);

    termRef.current = term;
    fitRef.current = fitAddon;

    // Fit after DOM settles
    requestAnimationFrame(() => fitAddon.fit());

    // WebSocket for PTY relay
    const proto = location.protocol === "https:" ? "wss:" : "ws:";
    const ws = new WebSocket(
      `${proto}//${location.host}/sessions/${sessionId}/ws`,
    );
    ws.binaryType = "arraybuffer";
    wsRef.current = ws;

    ws.onopen = () => {
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
      term.write("\r\n\x1b[33m[Connection closed]\x1b[0m\r\n");
    };

    ws.onerror = () => {
      term.write("\r\n\x1b[31m[WebSocket error]\x1b[0m\r\n");
    };

    // Relay keystrokes as binary
    const dataDisposable = term.onData((data: string) => {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(new TextEncoder().encode(data));
      }
    });

    // Relay resize
    const resizeDisposable = term.onResize(({ cols, rows }) => {
      if (ws.readyState === WebSocket.OPEN) {
        const msg: ResizeMessage = { type: "resize", cols, rows };
        ws.send(JSON.stringify(msg));
      }
    });

    // Window resize -> fit terminal
    const handleResize = () => fitAddon.fit();
    window.addEventListener("resize", handleResize);

    return () => {
      window.removeEventListener("resize", handleResize);
      dataDisposable.dispose();
      resizeDisposable.dispose();
      ws.close();
      term.dispose();
      termRef.current = null;
      wsRef.current = null;
      fitRef.current = null;
    };
  }, [sessionId]);

  return containerRef;
}
