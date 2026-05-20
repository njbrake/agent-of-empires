// @vitest-environment jsdom
//
// Lifecycle tests for the useTerminal hook. Drives the connect path,
// ws.onopen/onmessage/onclose, retry backoff, and disposal against a
// FakeWebSocket + a hand-rolled Terminal mock that mimics enough of
// xterm.js's surface for the hook to mount. Companion to the pure-
// helper suites (useTerminal.theme.test.ts, useTerminal.backoff.test.ts);
// covers the WS-driven branches that those tests can't reach.

import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// ── xterm.js mocks ───────────────────────────────────────────────────
// The hook builds a Terminal, loads addons (FitAddon, WebLinks,
// optionally WebGL), and calls open / onResize / onData / dispose /
// focus / write / options / attachCustomWheelEventHandler. Each mock
// is a no-op or a captured callback so the test can poke onResize
// from outside and observe what the hook sent over the WS.
//
// vi.mock factories are hoisted above any module-scope code, so the
// captured store and the fake classes live inside vi.hoisted to keep
// the factory closures self-contained.

const { captured } = vi.hoisted(() => ({
  captured: {
    disposed: false as boolean,
    writes: [] as Array<string | Uint8Array>,
    options: { fontSize: 14, theme: undefined as unknown },
    onResize: undefined as
      | ((s: { cols: number; rows: number }) => void)
      | undefined,
    onData: undefined as ((data: string) => void) | undefined,
    customWheel: undefined as ((e: WheelEvent) => boolean) | undefined,
  },
}));

vi.mock("@xterm/xterm", () => {
  class FakeTerminal {
    cols = 80;
    rows = 24;
    options: typeof captured.options;
    element: HTMLDivElement | null = null;
    textarea: HTMLTextAreaElement | null = null;
    constructor(opts: { fontSize?: number; theme?: unknown }) {
      captured.options = { fontSize: opts.fontSize ?? 14, theme: opts.theme };
      this.options = captured.options;
    }
    loadAddon(): void {}
    open(parent: HTMLElement): void {
      this.element = document.createElement("div");
      this.element.classList.add("xterm");
      this.textarea = document.createElement("textarea");
      this.textarea.classList.add("xterm-helper-textarea");
      this.element.appendChild(this.textarea);
      parent.appendChild(this.element);
    }
    focus(): void {}
    dispose(): void {
      captured.disposed = true;
    }
    write(data: string | Uint8Array): void {
      captured.writes.push(data);
    }
    onResize(cb: (s: { cols: number; rows: number }) => void): {
      dispose: () => void;
    } {
      captured.onResize = cb;
      return { dispose: () => {} };
    }
    onData(cb: (data: string) => void): { dispose: () => void } {
      captured.onData = cb;
      return { dispose: () => {} };
    }
    attachCustomWheelEventHandler(fn: (e: WheelEvent) => boolean): void {
      captured.customWheel = fn;
    }
    resize(cols: number, rows: number): void {
      this.cols = cols;
      this.rows = rows;
      captured.onResize?.({ cols, rows });
    }
  }
  return { Terminal: FakeTerminal };
});

vi.mock("@xterm/addon-fit", () => {
  class FakeFitAddon {
    fit(): void {
      captured.onResize?.({ cols: 100, rows: 30 });
    }
  }
  return { FitAddon: FakeFitAddon };
});

vi.mock("@xterm/addon-webgl", () => {
  class FakeWebglAddon {
    onContextLoss(): void {}
    dispose(): void {}
  }
  return { WebglAddon: FakeWebglAddon };
});

vi.mock("@xterm/addon-web-links", () => {
  class FakeWebLinksAddon {}
  return { WebLinksAddon: FakeWebLinksAddon };
});

// ── ResizeObserver shim ──────────────────────────────────────────────
// jsdom doesn't ship ResizeObserver; the hook registers one on the
// terminal element. We capture the callback so the test can drive
// "container resized" events deterministically. Without this stub the
// hook constructor throws and the whole test file fails to load.

class FakeResizeObserver {
  constructor(_cb: ResizeObserverCallback) {
    void _cb;
  }
  observe(): void {}
  disconnect(): void {}
  unobserve(): void {}
}

// ── WebSocket fake ───────────────────────────────────────────────────

interface FakeSocket {
  url: string;
  protocols: string[] | string | undefined;
  readyState: number;
  onopen: ((ev: Event) => void) | null;
  onclose: ((ev: CloseEvent) => void) | null;
  onerror: ((ev: Event) => void) | null;
  onmessage: ((ev: MessageEvent) => void) | null;
  binaryType: string;
  sent: Array<string | Uint8Array>;
  close: () => void;
  send: (data: string | ArrayBufferLike | Blob | ArrayBufferView) => void;
}

const sockets: FakeSocket[] = [];
let originalWebSocket: typeof WebSocket;
let originalResizeObserver: typeof ResizeObserver | undefined;

class FakeWebSocket implements FakeSocket {
  url: string;
  protocols: string[] | string | undefined;
  readyState = 0;
  onopen: ((ev: Event) => void) | null = null;
  onclose: ((ev: CloseEvent) => void) | null = null;
  onerror: ((ev: Event) => void) | null = null;
  onmessage: ((ev: MessageEvent) => void) | null = null;
  binaryType = "blob";
  sent: Array<string | Uint8Array> = [];
  static CONNECTING = 0;
  static OPEN = 1;
  static CLOSING = 2;
  static CLOSED = 3;
  constructor(url: string, protocols?: string | string[]) {
    this.url = url;
    this.protocols = protocols;
    sockets.push(this);
  }
  close(): void {
    this.readyState = FakeWebSocket.CLOSED;
    this.onclose?.({
      code: 1006,
      reason: "test close",
      wasClean: false,
    } as CloseEvent);
  }
  send(data: string | ArrayBufferLike | Blob | ArrayBufferView): void {
    if (typeof data === "string") this.sent.push(data);
    else this.sent.push(new Uint8Array(data as ArrayBuffer));
  }
}

beforeEach(() => {
  vi.useFakeTimers();
  sockets.length = 0;
  captured.disposed = false;
  captured.writes = [];
  captured.onResize = undefined;
  captured.onData = undefined;
  captured.customWheel = undefined;
  originalWebSocket = global.WebSocket;
  global.WebSocket = FakeWebSocket as unknown as typeof WebSocket;
  originalResizeObserver = global.ResizeObserver;
  global.ResizeObserver = FakeResizeObserver as unknown as typeof ResizeObserver;
  // localStorage starts empty so the hook reads bundled-default font
  // sizes and theme colors.
  window.localStorage.clear();
});

afterEach(() => {
  global.WebSocket = originalWebSocket;
  if (originalResizeObserver) global.ResizeObserver = originalResizeObserver;
  else
    (global as unknown as { ResizeObserver: undefined }).ResizeObserver =
      undefined;
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

async function flushAsync(times = 8): Promise<void> {
  await act(async () => {
    for (let i = 0; i < times; i++) await Promise.resolve();
  });
}

// renderHook attaches its container to document.body for us; the
// tests below just need to wire each session's div into the hook's
// containerRef so the connect effect sees a mounted element.
import { useTerminal } from "./useTerminal";

describe("useTerminal lifecycle", () => {
  it("does not open a WebSocket while sessionId is null", async () => {
    renderHook(() => useTerminal(null, "ws", false, false));
    await flushAsync();
    expect(sockets).toHaveLength(0);
  });

  it("opens a WebSocket once a sessionId becomes available", async () => {
    // Mount with a div so containerRef has somewhere to attach.
    const div = document.createElement("div");
    document.body.appendChild(div);
    try {
      const { result, rerender } = renderHook(
        (props: { id: string | null }) => {
          const term = useTerminal(props.id, "ws", false, false);
          if (term.containerRef && !term.containerRef.current) {
            (
              term.containerRef as unknown as {
                current: HTMLDivElement | null;
              }
            ).current = div;
          }
          return term;
        },
        { initialProps: { id: null } },
      );
      expect(sockets).toHaveLength(0);
      rerender({ id: "s-1" });
      await flushAsync();
      expect(sockets).toHaveLength(1);
      expect(sockets[0]!.url).toContain("/sessions/s-1/ws");
      expect(result.current.state.connected).toBe(false);
    } finally {
      div.remove();
    }
  });

  it("ws.onopen flips state.connected and sends activate + resize", async () => {
    const div = document.createElement("div");
    document.body.appendChild(div);
    try {
      const { result } = renderHook(() => {
        const term = useTerminal("s-2", "ws", false, false);
        if (term.containerRef && !term.containerRef.current) {
          (
            term.containerRef as unknown as { current: HTMLDivElement | null }
          ).current = div;
        }
        return term;
      });
      await flushAsync();
      expect(sockets).toHaveLength(1);
      const ws = sockets[0]!;

      // Drive ws.onopen and let the rAF + debounce settle so the
      // initial resize lands on the server.
      act(() => {
        ws.readyState = FakeWebSocket.OPEN;
        ws.onopen?.(new Event("open"));
      });
      await flushAsync();
      await act(async () => {
        await vi.advanceTimersByTimeAsync(300);
      });
      await flushAsync();

      expect(result.current.state.connected).toBe(true);
      expect(result.current.state.isPrimary).toBe(true);

      // Activate JSON message must have been queued on connect.
      const activate = ws.sent.find(
        (m) => typeof m === "string" && m.includes('"activate"'),
      );
      expect(activate).toBeDefined();
      // FitAddon's stub emits cols=100/rows=30, so the resize message
      // should reflect that exact pair.
      const resize = ws.sent.find(
        (m) =>
          typeof m === "string" && m.includes('"resize"') && m.includes("100"),
      );
      expect(resize).toBeDefined();
    } finally {
      div.remove();
    }
  });

  it("suppresses tiny resize messages from hidden containers", async () => {
    // Hidden container = no offsetParent + a tiny proposed grid. The
    // hook should never let that bogus measurement reach the server.
    // Simulate by stubbing the FakeFitAddon's fit() to emit (10, 4)
    // before any RO fires, and force offsetParent to null so the
    // hidden-container guard engages even though jsdom's default
    // would normally let any size through.
    const FakeFitAddonClass = (await import("@xterm/addon-fit")).FitAddon as unknown as {
      prototype: { fit: () => void };
    };
    const origFit = FakeFitAddonClass.prototype.fit;
    FakeFitAddonClass.prototype.fit = function () {
      captured.onResize?.({ cols: 10, rows: 4 });
    };
    const div = document.createElement("div");
    document.body.appendChild(div);
    try {
      const { result } = renderHook(() => {
        const term = useTerminal("s-hidden", "ws", false, false);
        if (term.containerRef && !term.containerRef.current) {
          (
            term.containerRef as unknown as { current: HTMLDivElement | null }
          ).current = div;
        }
        return term;
      });
      await flushAsync();
      const ws = sockets[0]!;
      act(() => {
        ws.readyState = FakeWebSocket.OPEN;
        ws.onopen?.(new Event("open"));
      });
      await flushAsync();
      await act(async () => {
        await vi.advanceTimersByTimeAsync(400);
      });
      await flushAsync();

      // Activate is fine (it does not carry a measurement), but no
      // resize message should have shipped at the tiny grid.
      const tinyResize = ws.sent.find(
        (m) =>
          typeof m === "string" &&
          m.includes('"resize"') &&
          m.includes('"cols":10') &&
          m.includes('"rows":4'),
      );
      expect(tinyResize).toBeUndefined();
      expect(result.current.state.connected).toBe(true);
    } finally {
      FakeFitAddonClass.prototype.fit = origFit;
      div.remove();
    }
  });

  it("clears retryCount once the first ws.onmessage arrives", async () => {
    const div = document.createElement("div");
    document.body.appendChild(div);
    try {
      const { result } = renderHook(() => {
        const term = useTerminal("s-3", "ws", false, false);
        if (term.containerRef && !term.containerRef.current) {
          (
            term.containerRef as unknown as { current: HTMLDivElement | null }
          ).current = div;
        }
        return term;
      });
      await flushAsync();
      const ws = sockets[0]!;
      act(() => {
        ws.readyState = FakeWebSocket.OPEN;
        ws.onopen?.(new Event("open"));
      });
      await flushAsync();
      // Now drop and reconnect once so retryCount > 0 before the
      // first message lands. Need to force the retry path which only
      // fires when readyState is CLOSED at close-time.
      act(() => {
        ws.readyState = FakeWebSocket.CLOSED;
        ws.onclose?.({
          code: 1006,
          reason: "",
          wasClean: false,
        } as CloseEvent);
      });
      expect(result.current.state.retryCount).toBe(1);

      // Advance past the first backoff so a fresh socket opens.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(1500);
      });
      await flushAsync();
      expect(sockets.length).toBeGreaterThan(1);
      const ws2 = sockets[sockets.length - 1]!;
      act(() => {
        ws2.readyState = FakeWebSocket.OPEN;
        ws2.onopen?.(new Event("open"));
        ws2.onmessage?.({ data: new TextEncoder().encode("hi").buffer } as MessageEvent);
      });
      await flushAsync();
      // The first payload byte resets the counter.
      expect(result.current.state.retryCount).toBe(0);
      // And the terminal received the bytes via term.write.
      expect(captured.writes.length).toBeGreaterThan(0);
    } finally {
      div.remove();
    }
  });

  it("primary_status JSON control message updates state.isPrimary", async () => {
    const div = document.createElement("div");
    document.body.appendChild(div);
    try {
      const { result } = renderHook(() => {
        const term = useTerminal("s-4", "ws", false, false);
        if (term.containerRef && !term.containerRef.current) {
          (
            term.containerRef as unknown as { current: HTMLDivElement | null }
          ).current = div;
        }
        return term;
      });
      await flushAsync();
      const ws = sockets[0]!;
      act(() => {
        ws.readyState = FakeWebSocket.OPEN;
        ws.onopen?.(new Event("open"));
        ws.onmessage?.({
          data: JSON.stringify({
            type: "primary_status",
            is_primary: false,
          }),
        } as MessageEvent);
      });
      await flushAsync();
      expect(result.current.state.isPrimary).toBe(false);
    } finally {
      div.remove();
    }
  });

  it("server close code 4001 short-circuits to retries exhausted", async () => {
    const div = document.createElement("div");
    document.body.appendChild(div);
    try {
      const { result } = renderHook(() => {
        const term = useTerminal("s-5", "ws", false, false);
        if (term.containerRef && !term.containerRef.current) {
          (
            term.containerRef as unknown as { current: HTMLDivElement | null }
          ).current = div;
        }
        return term;
      });
      await flushAsync();
      const ws = sockets[0]!;
      act(() => {
        ws.readyState = FakeWebSocket.CLOSED;
        ws.onclose?.({
          code: 4001,
          reason: "pty dead",
          wasClean: false,
        } as CloseEvent);
      });
      await flushAsync();
      // No backoff was armed, the retry counter jumped past max.
      expect(result.current.state.reconnecting).toBe(false);
      expect(result.current.state.retryCount).toBe(result.current.maxRetries);
    } finally {
      div.remove();
    }
  });

  it("manualReconnect dials a fresh socket when the previous one is already CLOSED", async () => {
    const div = document.createElement("div");
    document.body.appendChild(div);
    try {
      const { result } = renderHook(() => {
        const term = useTerminal("s-6", "ws", false, false);
        if (term.containerRef && !term.containerRef.current) {
          (
            term.containerRef as unknown as { current: HTMLDivElement | null }
          ).current = div;
        }
        return term;
      });
      await flushAsync();
      const ws = sockets[0]!;
      act(() => {
        ws.readyState = FakeWebSocket.CLOSED;
      });
      // Without dialing a fresh ws, calling close() would be a no-op
      // and onclose wouldn't fire. manualReconnect must detect this
      // and open a new socket directly.
      act(() => {
        result.current.manualReconnect();
      });
      await flushAsync();
      expect(sockets.length).toBeGreaterThanOrEqual(2);
    } finally {
      div.remove();
    }
  });

  it("disposes the Terminal and closes the WS on sessionId change", async () => {
    const div = document.createElement("div");
    document.body.appendChild(div);
    try {
      const { rerender } = renderHook(
        (props: { id: string | null }) => {
          const term = useTerminal(props.id, "ws", false, false);
          if (term.containerRef && !term.containerRef.current) {
            (
              term.containerRef as unknown as {
                current: HTMLDivElement | null;
              }
            ).current = div;
          }
          return term;
        },
        { initialProps: { id: "s-7" } },
      );
      await flushAsync();
      expect(sockets).toHaveLength(1);
      expect(captured.disposed).toBe(false);

      rerender({ id: "s-8" });
      await flushAsync();
      // First session's terminal was disposed; a new socket opened
      // for the next session.
      expect(captured.disposed).toBe(true);
      expect(sockets.length).toBeGreaterThanOrEqual(2);
    } finally {
      div.remove();
    }
  });
});
