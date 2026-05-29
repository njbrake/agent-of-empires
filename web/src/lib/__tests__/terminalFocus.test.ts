// @vitest-environment jsdom
//
// Unit tests for requestSessionInputFocus (#1454): the pure dispatch +
// pending-latch helper the sidebar select handlers delegate to. Covers the
// coarse-pointer suppression and the cockpit / non-cockpit target choice,
// asserting both the dispatched event and the stashed latch.

import { afterEach, describe, expect, it, vi } from "vitest";
import {
  FOCUS_TERMINAL_EVENT,
  consumePendingTerminalFocus,
  requestSessionInputFocus,
  type FocusTerminalDetail,
} from "../terminalFocus";

function captureDispatch(): () => FocusTerminalDetail[] {
  const seen: FocusTerminalDetail[] = [];
  const handler = (e: Event) => {
    seen.push((e as CustomEvent<FocusTerminalDetail>).detail);
  };
  window.addEventListener(FOCUS_TERMINAL_EVENT, handler);
  return () => {
    window.removeEventListener(FOCUS_TERMINAL_EVENT, handler);
    return seen;
  };
}

afterEach(() => {
  // Drain any latch left over so tests stay independent.
  consumePendingTerminalFocus("agent");
  consumePendingTerminalFocus("composer");
  consumePendingTerminalFocus("paired");
  vi.restoreAllMocks();
});

describe("requestSessionInputFocus", () => {
  it("does nothing on a coarse pointer", () => {
    const done = captureDispatch();
    requestSessionInputFocus(true, true);
    requestSessionInputFocus(false, true);
    const events = done();
    expect(events).toHaveLength(0);
    expect(consumePendingTerminalFocus("composer")).toBe(false);
    expect(consumePendingTerminalFocus("agent")).toBe(false);
  });

  it("targets the composer for cockpit sessions on a fine pointer", () => {
    const done = captureDispatch();
    requestSessionInputFocus(true, false);
    const events = done();
    expect(events).toEqual([{ target: "composer" }]);
    // Latch was set for the not-yet-mounted case.
    expect(consumePendingTerminalFocus("composer")).toBe(true);
    expect(consumePendingTerminalFocus("composer")).toBe(false);
  });

  it("targets the agent terminal for non-cockpit sessions on a fine pointer", () => {
    const done = captureDispatch();
    requestSessionInputFocus(false, false);
    const events = done();
    expect(events).toEqual([{ target: "agent" }]);
    expect(consumePendingTerminalFocus("agent")).toBe(true);
  });
});
