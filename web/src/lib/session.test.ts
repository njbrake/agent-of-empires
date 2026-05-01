import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  IDLE_DECAY_WINDOW_MS,
  getStatusDotClass,
  getStatusTextClass,
  idleAgeMs,
  isFreshIdle,
  isSessionActive,
} from "./session";
import type { SessionResponse, SessionStatus } from "./types";

const NOW = Date.parse("2026-05-01T12:00:00Z");

beforeEach(() => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date(NOW));
});

afterEach(() => {
  vi.useRealTimers();
});

function session(
  status: SessionStatus,
  idleEnteredAt: string | null,
): Pick<SessionResponse, "status" | "idle_entered_at"> {
  return { status, idle_entered_at: idleEnteredAt };
}

describe("idleAgeMs", () => {
  it("returns null for non-Idle sessions", () => {
    expect(idleAgeMs(session("Running", new Date(NOW - 1000).toISOString()))).toBeNull();
  });

  it("returns null when idle_entered_at is missing", () => {
    expect(idleAgeMs(session("Idle", null))).toBeNull();
  });

  it("returns null when idle_entered_at is unparseable", () => {
    expect(idleAgeMs(session("Idle", "not-a-date"))).toBeNull();
  });

  it("returns null for future timestamps (clock skew)", () => {
    // Clock skew between server and browser must not look like a fresh idle.
    expect(
      idleAgeMs(session("Idle", new Date(NOW + 60_000).toISOString())),
    ).toBeNull();
  });

  it("returns elapsed milliseconds for past Idle transition", () => {
    expect(
      idleAgeMs(session("Idle", new Date(NOW - 5_000).toISOString())),
    ).toBe(5_000);
  });
});

describe("isFreshIdle", () => {
  it("is true within the decay window", () => {
    expect(
      isFreshIdle(session("Idle", new Date(NOW - 60_000).toISOString())),
    ).toBe(true);
  });

  it("is false past the decay window", () => {
    expect(
      isFreshIdle(
        session("Idle", new Date(NOW - IDLE_DECAY_WINDOW_MS - 1).toISOString()),
      ),
    ).toBe(false);
  });

  it("is false for non-Idle sessions even with a recent timestamp", () => {
    expect(
      isFreshIdle(session("Running", new Date(NOW - 1_000).toISOString())),
    ).toBe(false);
  });
});

describe("getStatusDotClass", () => {
  it("uses fresh-idle class when within window", () => {
    expect(
      getStatusDotClass(session("Idle", new Date(NOW - 1_000).toISOString())),
    ).toBe("bg-status-fresh-idle");
  });

  it("falls back to idle class past the window", () => {
    expect(
      getStatusDotClass(
        session("Idle", new Date(NOW - IDLE_DECAY_WINDOW_MS - 1_000).toISOString()),
      ),
    ).toBe("bg-status-idle");
  });

  it("preserves non-Idle classes regardless of idle_entered_at", () => {
    expect(
      getStatusDotClass(session("Waiting", new Date(NOW - 1_000).toISOString())),
    ).toBe("bg-status-waiting");
  });
});

describe("getStatusTextClass", () => {
  it("uses fresh-idle class when within window", () => {
    expect(
      getStatusTextClass(session("Idle", new Date(NOW - 1_000).toISOString())),
    ).toBe("text-status-fresh-idle");
  });

  it("falls back to idle class past the window", () => {
    expect(getStatusTextClass(session("Idle", null))).toBe("text-status-idle");
  });
});

describe("isSessionActive", () => {
  it("treats fresh-idle as active", () => {
    expect(
      isSessionActive(session("Idle", new Date(NOW - 1_000).toISOString())),
    ).toBe(true);
  });

  it("treats decayed Idle as inactive", () => {
    expect(
      isSessionActive(
        session("Idle", new Date(NOW - IDLE_DECAY_WINDOW_MS - 1_000).toISOString()),
      ),
    ).toBe(false);
  });

  it("retains the legacy string-only API for callers without idle_entered_at", () => {
    // Some callers (legacy paths, unit tests) still pass a bare status. The
    // overload must keep classifying Running/Waiting/Starting as active.
    expect(isSessionActive("Running")).toBe(true);
    expect(isSessionActive("Idle")).toBe(false);
  });
});
