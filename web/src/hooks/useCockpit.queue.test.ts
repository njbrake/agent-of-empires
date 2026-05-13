// Reducer tests for the client-side prompt-queue feature (#1031).
//
// While a turn is running, sendPrompt dispatches `enqueue_prompt`
// instead of the immediate POST path. The reducer keeps the queue
// across re-renders so the drain effect can pop heads on Stopped, and
// the QueuedPromptsStrip can render / edit / drop entries before they
// fire.

import { describe, expect, it, vi } from "vitest";

import { emptyCockpitState, type QueuedPrompt } from "../lib/cockpitTypes";
import { cockpitHookReducer, combineQueuedPrompts } from "./useCockpit";

describe("cockpitHookReducer / queue actions", () => {
  it("emptyCockpitState starts with an empty queue", () => {
    expect(emptyCockpitState().queuedPrompts).toEqual([]);
  });

  it("enqueue_prompt appends to the end of queuedPrompts", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-01-01T00:00:00Z"));
    try {
      const s1 = cockpitHookReducer(emptyCockpitState(), {
        kind: "enqueue_prompt",
        text: "first",
      });
      const s2 = cockpitHookReducer(s1, {
        kind: "enqueue_prompt",
        text: "second",
      });
      expect(s2.queuedPrompts).toHaveLength(2);
      expect(s2.queuedPrompts[0]?.text).toBe("first");
      expect(s2.queuedPrompts[1]?.text).toBe("second");
      expect(s2.queuedPrompts[0]?.queuedAt).toBe(
        "2026-01-01T00:00:00.000Z",
      );
    } finally {
      vi.useRealTimers();
    }
  });

  it("dequeue_prompt removes the matching entry by id", () => {
    const s1 = cockpitHookReducer(emptyCockpitState(), {
      kind: "enqueue_prompt",
      text: "first",
    });
    const s2 = cockpitHookReducer(s1, {
      kind: "enqueue_prompt",
      text: "second",
    });
    const headId = s2.queuedPrompts[0]?.id;
    expect(headId).toBeDefined();
    const s3 = cockpitHookReducer(s2, {
      kind: "dequeue_prompt",
      id: headId!,
    });
    expect(s3.queuedPrompts).toHaveLength(1);
    expect(s3.queuedPrompts[0]?.text).toBe("second");
  });

  it("dequeue_prompt is a no-op for a missing id", () => {
    const s1 = cockpitHookReducer(emptyCockpitState(), {
      kind: "enqueue_prompt",
      text: "first",
    });
    const s2 = cockpitHookReducer(s1, {
      kind: "dequeue_prompt",
      id: "nope",
    });
    expect(s2.queuedPrompts).toHaveLength(1);
  });

  it("edit_queued_prompt updates only the targeted entry's text", () => {
    const s1 = cockpitHookReducer(emptyCockpitState(), {
      kind: "enqueue_prompt",
      text: "first",
    });
    const s2 = cockpitHookReducer(s1, {
      kind: "enqueue_prompt",
      text: "second",
    });
    const targetId = s2.queuedPrompts[1]?.id;
    expect(targetId).toBeDefined();
    const s3 = cockpitHookReducer(s2, {
      kind: "edit_queued_prompt",
      id: targetId!,
      text: "second (edited)",
    });
    expect(s3.queuedPrompts[0]?.text).toBe("first");
    expect(s3.queuedPrompts[1]?.text).toBe("second (edited)");
  });

  it("clear_queue drops every entry", () => {
    const s1 = cockpitHookReducer(emptyCockpitState(), {
      kind: "enqueue_prompt",
      text: "first",
    });
    const s2 = cockpitHookReducer(s1, {
      kind: "enqueue_prompt",
      text: "second",
    });
    const s3 = cockpitHookReducer(s2, { kind: "clear_queue" });
    expect(s3.queuedPrompts).toEqual([]);
  });

  it("queue is independent of activity / turnActive state", () => {
    // Enqueue while a turn is mid-flight (turnActive=true, activity has
    // a user_prompt row) and ensure the queue mutation does not clobber
    // the rest of state.
    const base = {
      ...emptyCockpitState(),
      activity: [
        {
          id: "user-1",
          kind: "user_prompt" as const,
          text: "original",
          at: "2026-01-01T00:00:00Z",
        },
      ],
      turnActive: true,
    };
    const next = cockpitHookReducer(base, {
      kind: "enqueue_prompt",
      text: "queued follow-up",
    });
    expect(next.activity).toEqual(base.activity);
    expect(next.turnActive).toBe(true);
    expect(next.queuedPrompts[0]?.text).toBe("queued follow-up");
  });
});

describe("combineQueuedPrompts (combined drain mode)", () => {
  const mk = (id: string, text: string): QueuedPrompt => ({
    id,
    text,
    queuedAt: "2026-01-01T00:00:00.000Z",
  });

  it("joins entries with a blank line", () => {
    const out = combineQueuedPrompts([
      mk("a", "first"),
      mk("b", "second"),
      mk("c", "third"),
    ]);
    expect(out).toBe("first\n\nsecond\n\nthird");
  });

  it("preserves intra-entry newlines unchanged", () => {
    const out = combineQueuedPrompts([
      mk("a", "line 1\nline 2"),
      mk("b", "after"),
    ]);
    expect(out).toBe("line 1\nline 2\n\nafter");
  });

  it("returns an empty string for an empty queue", () => {
    expect(combineQueuedPrompts([])).toBe("");
  });

  it("returns a single entry unchanged for a one-item queue", () => {
    expect(combineQueuedPrompts([mk("a", "only one")])).toBe("only one");
  });
});
