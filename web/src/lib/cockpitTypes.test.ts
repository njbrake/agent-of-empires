// Reducer tests for the cockpit memory/recall feature.
//
// These cover the wire-protocol contract: the server publishes a
// UserPromptSent event before forwarding the prompt to the agent, the
// frontend's optimistic dispatch produces a placeholder activity row,
// and the reducer dedupes the two by promoting the placeholder's id
// to the seq-based form when the server echo arrives.
//
// If this dedupe regresses, the user will see every prompt twice in
// the conversation log on every reload.

import { describe, expect, it } from "vitest";

import {
  applyEvent,
  emptyCockpitState,
  isTurnActive,
  normaliseTurnCounters,
  type CockpitFrame,
  type CockpitState,
} from "./cockpitTypes";

function frame(seq: number, text: string): CockpitFrame {
  return {
    session_id: "s-1",
    seq,
    event: { UserPromptSent: { text } },
  };
}

function withOptimisticPrompt(state: CockpitState, text: string): CockpitState {
  // Mirrors the optimistic dispatch in useCockpit.sendPrompt: row id
  // includes the wall-clock timestamp (distinct from the `user-seq-N`
  // form the reducer assigns when the server echoes), and
  // `pendingUserPromptSeq` bumps so a subsequent server echo on the
  // matching row doesn't double-count. See #1170.
  const pendingUserPromptSeq = state.pendingUserPromptSeq + 1;
  return {
    ...state,
    activity: state.activity.concat({
      id: `user-${Date.now()}-${state.activity.length}`,
      kind: "user_prompt",
      text,
      at: new Date().toISOString(),
    }),
    pendingUserPromptSeq,
    turnActive: pendingUserPromptSeq > state.lastStoppedSeq,
  };
}

describe("applyEvent / UserPromptSent", () => {
  it("appends a user_prompt row when no optimistic placeholder exists", () => {
    const next = applyEvent(emptyCockpitState(), frame(1, "hi"));
    expect(next.activity).toHaveLength(1);
    expect(next.activity[0]).toMatchObject({
      id: "user-seq-1",
      kind: "user_prompt",
      text: "hi",
    });
    expect(next.lastSeq).toBe(1);
    expect(next.turnActive).toBe(true);
  });

  it("dedupes against the optimistic row by promoting its id", () => {
    // Simulate: useCockpit.sendPrompt fires an optimistic dispatch,
    // then the server's UserPromptSent echo arrives over the WS.
    const optimistic = withOptimisticPrompt(emptyCockpitState(), "test prompt");
    expect(optimistic.activity).toHaveLength(1);
    expect(optimistic.activity[0].id.startsWith("user-seq-")).toBe(false);

    const next = applyEvent(optimistic, frame(7, "test prompt"));
    // Single row preserved, id rewritten to the authoritative form so
    // future replays dedupe against it via seq.
    expect(next.activity).toHaveLength(1);
    expect(next.activity[0].id).toBe("user-seq-7");
    expect(next.activity[0].text).toBe("test prompt");
    expect(next.lastSeq).toBe(7);
  });

  it("does not dedupe when the optimistic text differs from the echo", () => {
    // Edge case: user typed two prompts back-to-back. The optimistic
    // row for the FIRST prompt should not be overwritten by the
    // server echo of the SECOND prompt.
    const optimistic = withOptimisticPrompt(emptyCockpitState(), "first");
    const next = applyEvent(optimistic, frame(2, "second"));
    expect(next.activity).toHaveLength(2);
    expect(next.activity[0].text).toBe("first");
    expect(next.activity[1].id).toBe("user-seq-2");
    expect(next.activity[1].text).toBe("second");
  });

  it("dedupes the OLDEST matching optimistic row when same text is sent twice", () => {
    // Regression: user clicks Send with the same text twice in quick
    // succession. Two optimistic rows are queued. The first server
    // echo (seq=N) corresponds to the first submission and must
    // promote row 0, not row 1. If we promoted the most-recent row,
    // row 0 would be orphaned forever and the second echo (seq=N+1)
    // would append a third row, leaving the user with three rows on
    // screen for two prompts.
    let state = withOptimisticPrompt(emptyCockpitState(), "ping");
    state = withOptimisticPrompt(state, "ping");
    expect(state.activity).toHaveLength(2);

    state = applyEvent(state, frame(10, "ping"));
    state = applyEvent(state, frame(11, "ping"));

    expect(state.activity).toHaveLength(2);
    expect(state.activity[0].id).toBe("user-seq-10");
    expect(state.activity[1].id).toBe("user-seq-11");
    expect(state.activity[0].text).toBe("ping");
    expect(state.activity[1].text).toBe("ping");
  });

  it("does not double-dedupe a prompt that already has a seq-based id", () => {
    // Replay scenario: reducer applied frame(seq=3) once, then a
    // later reconnect re-delivers the same frame. Without seq dedupe
    // the reducer would walk the optimistic-promotion branch a second
    // time and clobber the row's metadata.
    let state = applyEvent(emptyCockpitState(), frame(3, "echoed"));
    expect(state.activity[0].id).toBe("user-seq-3");

    // Re-deliver the same frame — frame.seq <= state.lastSeq must be
    // a no-op so the same row isn't promoted again.
    state = applyEvent(state, frame(3, "echoed"));
    expect(state.activity).toHaveLength(1);
    expect(state.activity[0].id).toBe("user-seq-3");
    expect(state.lastSeq).toBe(3);
  });

  it("clears assistantMessage and turnActive flags so the new turn starts clean", () => {
    const stale: CockpitState = {
      ...emptyCockpitState(),
      assistantMessage: "stale partial reply",
      startupError: "old error",
      lastError: "old action error",
      turnActive: false,
    };
    const next = applyEvent(stale, frame(1, "new prompt"));
    expect(next.assistantMessage).toBe("");
    expect(next.startupError).toBeNull();
    expect(next.lastError).toBeNull();
    expect(next.turnActive).toBe(true);
  });

  it("renders tool output from ToolCallCompleted.content", () => {
    // Most agents (Claude's claude-agent-acp included) ship the tool's
    // textual output on the *completion* update via fields.content. If
    // we lose this, the bash card body literally reads "completed".
    let state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: {
        ToolCallStarted: {
          tool_call: {
            id: "tc-bash",
            name: "Terminal",
            kind: "execute",
            args_preview: "{}",
            started_at: new Date().toISOString(),
          },
        },
      },
    });
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 2,
      event: {
        ToolCallCompleted: {
          tool_call_id: "tc-bash",
          is_error: false,
          content: "abc1234 first commit\ndef5678 second commit\n",
        },
      },
    });
    const done = state.activity.find((a) => a.id === "done-tc-bash");
    expect(done).toBeDefined();
    expect(done!.kind).toBe("tool_complete");
    expect(done!.text).toBe(
      "abc1234 first commit\ndef5678 second commit\n",
    );
    expect(state.inFlightTool).toBeNull();
  });

  it("falls back to streamed ToolCallContent when completion has empty content", () => {
    // Some agents stream stdout via interim ToolCallUpdate notifications
    // (status=in_progress with content) and emit a final completion
    // with empty content. The reducer buffers interim chunks keyed by
    // tool_call_id and drains the buffer on completion.
    let state = emptyCockpitState();
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 1,
      event: {
        ToolCallContent: {
          tool_call_id: "tc-bash",
          content: "line1\n",
        },
      },
    });
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 2,
      event: {
        ToolCallContent: {
          tool_call_id: "tc-bash",
          content: "line1\nline2\n",
        },
      },
    });
    expect(state.toolOutputs["tc-bash"]).toBe("line1\nline2\n");
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 3,
      event: {
        ToolCallCompleted: {
          tool_call_id: "tc-bash",
          is_error: false,
          content: "",
        },
      },
    });
    const done = state.activity.find((a) => a.id === "done-tc-bash");
    expect(done!.text).toBe("line1\nline2\n");
    // Buffer drained so a re-completion (replay) doesn't double-render.
    expect(state.toolOutputs["tc-bash"]).toBeUndefined();
  });

  it("falls back to status word when no content arrived at all", () => {
    const state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: {
        ToolCallCompleted: {
          tool_call_id: "tc-x",
          is_error: false,
          content: "",
        },
      },
    });
    const done = state.activity.find((a) => a.id === "done-tc-x");
    expect(done!.text).toBe("completed");
  });

  it("patches tool_start args/title when ToolCallUpdated arrives later", () => {
    // Claude's claude-agent-acp emits the initial tool_call with an
    // empty raw_input and a generic title ("Terminal"); the actual
    // command lands in a follow-up ToolCallUpdate. The reducer must
    // overwrite the row's tool payload so the card header shows
    // `$ git log -n 10` rather than `$ Terminal`.
    let state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: {
        ToolCallStarted: {
          tool_call: {
            id: "tc-bash",
            name: "Terminal",
            kind: "execute",
            args_preview: "{}",
            started_at: new Date().toISOString(),
          },
        },
      },
    });
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 2,
      event: {
        ToolCallUpdated: {
          tool_call_id: "tc-bash",
          title: null,
          args_preview: '{"command":"git log -n 10"}',
        },
      },
    });
    const startRow = state.activity.find(
      (a) => a.kind === "tool_start" && a.toolCallId === "tc-bash",
    );
    expect(startRow?.tool?.args_preview).toBe(
      '{"command":"git log -n 10"}',
    );
    expect(startRow?.tool?.name).toBe("Terminal");
    expect(state.inFlightTool?.args_preview).toBe(
      '{"command":"git log -n 10"}',
    );
  });

  it("uses 'tool failed' when error event has no content", () => {
    const state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: {
        ToolCallCompleted: {
          tool_call_id: "tc-y",
          is_error: true,
          content: "",
        },
      },
    });
    const done = state.activity.find((a) => a.id === "done-tc-y");
    expect(done!.kind).toBe("tool_error");
    expect(done!.text).toBe("tool failed");
  });

  it("reconstructs the user side of the conversation from a replay", () => {
    // Server restart scenario: client connects, WS drain delivers all
    // events from the on-disk store including UserPromptSent rows.
    // Without these, the assistant chunks would collapse into a
    // single blob; with them, each turn gets its own user message.
    const replay: CockpitFrame[] = [
      { session_id: "s-1", seq: 1, event: { UserPromptSent: { text: "hi" } } },
      {
        session_id: "s-1",
        seq: 2,
        event: { AgentMessageChunk: { text: "Hello!" } },
      },
      {
        session_id: "s-1",
        seq: 3,
        event: { UserPromptSent: { text: "thanks" } },
      },
      {
        session_id: "s-1",
        seq: 4,
        event: { AgentMessageChunk: { text: "Anytime." } },
      },
    ];
    const final = replay.reduce(
      (state, f) => applyEvent(state, f),
      emptyCockpitState(),
    );
    const userPrompts = final.activity.filter((a) => a.kind === "user_prompt");
    const messages = final.activity.filter((a) => a.kind === "message");
    expect(userPrompts.map((u) => u.text)).toEqual(["hi", "thanks"]);
    expect(messages.map((m) => m.text)).toEqual(["Hello!", "Anytime."]);
    expect(final.lastSeq).toBe(4);
  });
});

describe("applyEvent / AvailableCommandsUpdated", () => {
  it("populates availableCommands and replaces the prior list", () => {
    const f1: CockpitFrame = {
      session_id: "s-1",
      seq: 1,
      event: {
        AvailableCommandsUpdated: {
          commands: [
            { name: "help", description: "Show help", accepts_input: false },
          ],
        },
      },
    };
    const s1 = applyEvent(emptyCockpitState(), f1);
    expect(s1.availableCommands).toHaveLength(1);
    expect(s1.availableCommands[0].name).toBe("help");

    const f2: CockpitFrame = {
      session_id: "s-1",
      seq: 2,
      event: {
        AvailableCommandsUpdated: {
          commands: [
            { name: "review", description: "Review PR", accepts_input: true },
            { name: "clear", description: "Clear context", accepts_input: false },
          ],
        },
      },
    };
    const s2 = applyEvent(s1, f2);
    expect(s2.availableCommands.map((c) => c.name)).toEqual(["review", "clear"]);
    expect(s2.availableCommands[0].accepts_input).toBe(true);
  });
});

describe("applyEvent / ACP session id lifecycle", () => {
  it("AcpSessionAssigned is a no-op for the conversation surface", () => {
    const before = emptyCockpitState();
    const after = applyEvent(before, {
      session_id: "s-1",
      seq: 1,
      event: { AcpSessionAssigned: { acp_session_id: "uuid-1234" } },
    });
    // Seq advanced; no activity row appended; usage untouched.
    expect(after.lastSeq).toBe(1);
    expect(after.activity).toEqual([]);
    expect(after.sessionUsage).toBeNull();
  });

  it("SessionContextReset clears stale usage and appends a context_reset row", () => {
    let state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: { UsageUpdated: { usage: { used: 75000, size: 200000 } } },
    });
    expect(state.sessionUsage?.used).toBe(75000);

    state = applyEvent(state, {
      session_id: "s-1",
      seq: 2,
      event: { UserPromptSent: { text: "hi" } },
    });

    state = applyEvent(state, {
      session_id: "s-1",
      seq: 3,
      event: {
        SessionContextReset: { reason: "session/load failed: bad id" },
      },
    });
    expect(state.sessionUsage).toBeNull();
    const last = state.activity[state.activity.length - 1];
    expect(last?.kind).toBe("context_reset");
    expect(last?.text).toContain("session/load failed");
  });

  it("SessionContextReset uses a fallback message when reason is empty", () => {
    let state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: { UserPromptSent: { text: "hi" } },
    });
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 2,
      event: { SessionContextReset: { reason: "" } },
    });
    const last = state.activity[state.activity.length - 1];
    expect(last?.kind).toBe("context_reset");
    expect(last?.text.length).toBeGreaterThan(0);
  });

  it("SessionContextReset is silent on a session with no prior user prompt", () => {
    // 0-message session: agent never persisted a transcript, so
    // session/load failing on the next spawn is expected. Don't
    // surface a meaningless "context reset" warning.
    let state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: { UsageUpdated: { usage: { used: 100, size: 200000 } } },
    });
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 2,
      event: {
        SessionContextReset: { reason: "session/load failed: bad id" },
      },
    });
    // Usage still cleared (defensive — should already be safe to drop).
    expect(state.sessionUsage).toBeNull();
    // No visible row appended.
    expect(state.activity.some((r) => r.kind === "context_reset")).toBe(false);
    expect(state.lastSeq).toBe(2);
  });

  it("SessionContextReset that arrives BEFORE the first prompt stays hidden after later prompts", () => {
    // Replay order: reset@2, then prompt@3. The reset must NOT appear
    // above the prompt later — applyEvent processes events in seq order
    // and decides based on what's been seen so far.
    let state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: { UsageUpdated: { usage: { used: 100, size: 200000 } } },
    });
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 2,
      event: { SessionContextReset: { reason: "session/load failed" } },
    });
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 3,
      event: { UserPromptSent: { text: "hi" } },
    });
    expect(state.activity.some((r) => r.kind === "context_reset")).toBe(false);
  });

  it("SessionContextReset with prior prompt sets contextPrimerAvailable (#1004)", () => {
    let state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: { UserPromptSent: { text: "do a thing" } },
    });
    expect(state.contextPrimerAvailable).toBeNull();
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 2,
      event: { SessionContextReset: { reason: "load failed: bad id" } },
    });
    expect(state.contextPrimerAvailable).toEqual({
      resetSeq: 2,
      reason: "load failed: bad id",
    });
  });

  it("SessionContextReset without prior prompt does not set contextPrimerAvailable", () => {
    const state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: { SessionContextReset: { reason: "load failed" } },
    });
    expect(state.contextPrimerAvailable).toBeNull();
  });

  it("UserPromptSent clears contextPrimerAvailable (one-shot affordance)", () => {
    let state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: { UserPromptSent: { text: "first" } },
    });
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 2,
      event: { SessionContextReset: { reason: "load failed" } },
    });
    expect(state.contextPrimerAvailable).not.toBeNull();
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 3,
      event: { UserPromptSent: { text: "second" } },
    });
    expect(state.contextPrimerAvailable).toBeNull();
  });
});

describe("applyEvent / Stopped empty-output fallback", () => {
  it("appends an empty_output row when the turn ended with no agent output", () => {
    let state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: { UserPromptSent: { text: "/usage" } },
    });
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 2,
      event: { Stopped: {} },
    });
    const last = state.activity[state.activity.length - 1];
    expect(last?.kind).toBe("empty_output");
    expect(last?.text).toContain("no output");
    expect(state.turnActive).toBe(false);
  });

  it("does not append the notice when the agent emitted a message", () => {
    let state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: { UserPromptSent: { text: "/context" } },
    });
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 2,
      event: { AgentMessageChunk: { text: "Context Usage" } },
    });
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 3,
      event: { Stopped: {} },
    });
    expect(state.activity.find((r) => r.kind === "empty_output")).toBeUndefined();
  });

  it("does not append the notice when a tool call ran during the turn", () => {
    let state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: { UserPromptSent: { text: "do a thing" } },
    });
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 2,
      event: {
        ToolCallStarted: {
          tool_call: {
            id: "t1",
            name: "Bash",
            kind: "execute",
            args_preview: "{}",
            started_at: new Date().toISOString(),
          },
        },
      },
    });
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 3,
      event: { Stopped: {} },
    });
    expect(state.activity.find((r) => r.kind === "empty_output")).toBeUndefined();
  });
});

describe("applyEvent / Stopped user_stopped", () => {
  it("sets workerStopped on reason=user_stopped and clears turnActive", () => {
    let state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: { UserPromptSent: { text: "long task" } },
    });
    expect(state.turnActive).toBe(true);
    expect(state.workerStopped).toBe(false);
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 2,
      event: { Stopped: { reason: "user_stopped" } },
    });
    expect(state.workerStopped).toBe(true);
    expect(state.turnActive).toBe(false);
  });

  it("does NOT set workerStopped on reason=prompt_complete", () => {
    let state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: { UserPromptSent: { text: "hi" } },
    });
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 2,
      event: { Stopped: { reason: "prompt_complete" } },
    });
    expect(state.workerStopped).toBe(false);
  });

  it("clears workerStopped on the next UserPromptSent", () => {
    let state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: { Stopped: { reason: "user_stopped" } },
    });
    expect(state.workerStopped).toBe(true);
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 2,
      event: { UserPromptSent: { text: "back online" } },
    });
    expect(state.workerStopped).toBe(false);
  });

  it("clears workerStopped on AcpSessionAssigned (manual reconnect succeeded)", () => {
    let state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: { Stopped: { reason: "user_stopped" } },
    });
    expect(state.workerStopped).toBe(true);
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 2,
      event: { AcpSessionAssigned: { acp_session_id: "abc-123" } },
    });
    expect(state.workerStopped).toBe(false);
  });
});

describe("applyEvent / Stopped restart_pending", () => {
  it("sets workerRestarting (not workerStopped) on reason=restart_pending", () => {
    const state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: { Stopped: { reason: "restart_pending" } },
    });
    expect(state.workerRestarting).toBe(true);
    expect(state.workerStopped).toBe(false);
    expect(state.turnActive).toBe(false);
  });

  it("clears workerRestarting on AcpSessionAssigned (reconciler auto-respawn finished)", () => {
    let state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: { Stopped: { reason: "restart_pending" } },
    });
    expect(state.workerRestarting).toBe(true);
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 2,
      event: { AcpSessionAssigned: { acp_session_id: "fresh-id" } },
    });
    expect(state.workerRestarting).toBe(false);
  });

  it("user_stopped → restart_pending transitions cleanly", () => {
    // Edge case: user runs `aoe cockpit stop`, then realises they meant
    // `restart`. The two reasons must not pile up — restart_pending
    // wins because it's the most recent signal from the daemon.
    let state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: { Stopped: { reason: "user_stopped" } },
    });
    expect(state.workerStopped).toBe(true);
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 2,
      event: { Stopped: { reason: "restart_pending" } },
    });
    expect(state.workerStopped).toBe(false);
    expect(state.workerRestarting).toBe(true);
  });
});

describe("applyEvent / WakeupScheduled lifecycle", () => {
  it("user-typed prompt mid-wait keeps the pending wakeup", () => {
    // Regression for #1091: a user-typed follow-up during the wait
    // is NOT the wake firing. Reducer must keep `nextWakeupAt` when
    // the scheduled time is still in the future.
    const future = new Date(Date.now() + 95_000).toISOString();
    let state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: { WakeupScheduled: { at: future, reason: "test wake" } },
    });
    expect(state.nextWakeupAt).toBe(future);
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 2,
      event: { UserPromptSent: { text: "btw, ping me when you wake" } },
    });
    expect(state.nextWakeupAt).toBe(future);
    expect(state.nextWakeupReason).toBe("test wake");
  });

  it("prompt after wakeup `at` clears the pending wakeup", () => {
    // The self-fired prompt from /loop arrives once the scheduled
    // moment has passed; that's the genuine wake-fired signal.
    const past = new Date(Date.now() - 5_000).toISOString();
    let state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: { WakeupScheduled: { at: past, reason: "test wake" } },
    });
    expect(state.nextWakeupAt).toBe(past);
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 2,
      event: { UserPromptSent: { text: "Wake-up fired. Confirm." } },
    });
    expect(state.nextWakeupAt).toBeNull();
    expect(state.nextWakeupReason).toBeNull();
  });
});

describe("applyEvent / SessionCleared", () => {
  // /clear wipes the model's memory. The reducer appends a divider row
  // so the renderer can fold pre-clear turns behind a disclosure
  // (#1101), and resets only the per-turn / in-flight fields the
  // cleared context invalidates. Capability caches (slash commands,
  // modes) are preserved because claude-agent-sdk caches them at
  // Query init and does not rotate them on /clear (#1128).
  it("appends a session_cleared divider row", () => {
    const next = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 5,
      event: "SessionCleared",
    });
    expect(next.activity).toHaveLength(1);
    expect(next.activity[0]).toMatchObject({
      id: "cleared-5",
      kind: "session_cleared",
    });
    expect(next.lastSeq).toBe(5);
  });

  it("resets per-turn state but preserves capability caches (#1128)", () => {
    const seeded: CockpitState = {
      ...emptyCockpitState(),
      availableCommands: [
        { name: "foo", description: "", accepts_input: false },
      ],
      availableModes: [{ id: "m1", name: "Mode One" }],
      currentModeId: "m1",
      plan: {
        plan_id: "p-1",
        version: 1,
        steps: [{ id: "s-1", title: "step", status: "Pending" }],
      },
      mode: "Plan",
      pendingApprovals: [
        {
          nonce: "n-1",
          tool_call: {
            id: "tc-1",
            name: "Bash",
            kind: "execute",
            args_preview: "ls",
            started_at: new Date().toISOString(),
          },
          destructive: false,
          requested_at: new Date().toISOString(),
        },
      ],
      sessionUsage: { used: 10, size: 200_000 },
    };
    const next = applyEvent(seeded, {
      session_id: "s-1",
      seq: 7,
      event: "SessionCleared",
    });
    // Per-turn / in-flight state cleared:
    expect(next.plan).toBeNull();
    expect(next.mode).toBe("Default");
    expect(next.pendingApprovals).toEqual([]);
    expect(next.sessionUsage).toBeNull();
    // Capability caches preserved (slash palette + mode picker keep
    // working after /clear):
    expect(next.availableCommands).toEqual(seeded.availableCommands);
    expect(next.availableModes).toEqual(seeded.availableModes);
    expect(next.currentModeId).toBe("m1");
  });
});

describe("applyEvent / ConversationCompacted", () => {
  // /compact is NOT memory loss: the model retains continuity through
  // the summary. The primer banner (which nudges the user to pre-fill
  // a recap) is therefore inappropriate here, so this event variant
  // exists as a separate signal from SessionContextReset and leaves
  // contextPrimerAvailable alone. See #1109.
  it("appends a compacted divider row and drops the stale usage snapshot", () => {
    const seeded: CockpitState = {
      ...emptyCockpitState(),
      sessionUsage: { used: 100, size: 200_000 },
    };
    const next = applyEvent(seeded, {
      session_id: "s-1",
      seq: 9,
      event: "ConversationCompacted",
    });
    expect(next.activity).toHaveLength(1);
    expect(next.activity[0]).toMatchObject({
      id: "compacted-9",
      kind: "compacted",
    });
    expect(next.sessionUsage).toBeNull();
  });

  it("does not arm the primer banner", () => {
    // Regression: /compact previously routed through SessionContextReset
    // and the primer banner offered to pre-fill duplicate content the
    // model already had summarised. Verify the new variant doesn't
    // re-introduce that behaviour.
    const next = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 3,
      event: "ConversationCompacted",
    });
    expect(next.contextPrimerAvailable).toBeNull();
  });
});

describe("turnActive derivation from prompt/stop counters (#1170)", () => {
  // `turnActive` derives from `pendingUserPromptSeq > lastStoppedSeq`.
  // The boolean field is kept on `CockpitState` as a memoised alias so
  // existing `state.turnActive` reads stay correct, but the counters
  // are the source of truth a late `Stopped` cannot clobber.

  it("isTurnActive flips on / off when counters cross", () => {
    expect(
      isTurnActive({ pendingUserPromptSeq: 2, lastStoppedSeq: 1 }),
    ).toBe(true);
    expect(
      isTurnActive({ pendingUserPromptSeq: 1, lastStoppedSeq: 1 }),
    ).toBe(false);
    expect(
      isTurnActive({ pendingUserPromptSeq: 0, lastStoppedSeq: 0 }),
    ).toBe(false);
  });

  it("Stopped advances lastStoppedSeq by one and recomputes turnActive", () => {
    // Single-prompt happy path: send → Stopped flips turnActive off.
    let state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: { UserPromptSent: { text: "hi" } },
    });
    expect(state.pendingUserPromptSeq).toBe(1);
    expect(state.lastStoppedSeq).toBe(0);
    expect(state.turnActive).toBe(true);

    state = applyEvent(state, {
      session_id: "s-1",
      seq: 2,
      event: { Stopped: { reason: "prompt_complete" } },
    });
    expect(state.pendingUserPromptSeq).toBe(1);
    expect(state.lastStoppedSeq).toBe(1);
    expect(state.turnActive).toBe(false);
  });

  it("late Stopped from prior turn does NOT clobber turnActive after a fresh follow-up", async () => {
    // The bug. Prior turn: pendingUserPromptSeq=1, lastStoppedSeq=0
    // (turnActive=true). User submits a follow-up before the prior
    // turn's Stopped frame has been applied client-side; the
    // optimistic `user_prompt` action bumps pending to 2. A beat
    // later the Stopped frame for turn 1 lands. Under the old
    // unconditional `turnActive=false`, the spinner died and the
    // late agent chunks reordered visually below the new prompt.
    // Under the counter model, lastStoppedSeq advances to 1
    // (capped at pending) and `2 > 1` keeps turnActive true.
    const { cockpitHookReducer } = await import("../hooks/useCockpit");

    let state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: { UserPromptSent: { text: "first turn" } },
    });
    expect(state.turnActive).toBe(true);
    // User taps Send the instant the turn ends; the optimistic
    // dispatch lands BEFORE the Stopped frame for the prior turn.
    state = cockpitHookReducer(state, {
      kind: "user_prompt",
      text: "follow-up",
    });
    expect(state.pendingUserPromptSeq).toBe(2);
    expect(state.turnActive).toBe(true);
    // Late Stopped (was for turn 1) now arrives. Must NOT kill the
    // spinner because turn 2 is the active turn.
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 2,
      event: { Stopped: { reason: "prompt_complete" } },
    });
    expect(state.pendingUserPromptSeq).toBe(2);
    expect(state.lastStoppedSeq).toBe(1);
    expect(state.turnActive).toBe(true);

    // Eventually turn 2's own Stopped lands and flips it off.
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 3,
      event: { Stopped: { reason: "prompt_complete" } },
    });
    expect(state.lastStoppedSeq).toBe(2);
    expect(state.turnActive).toBe(false);
  });

  it("spurious Stopped on an idle session does not flip a future prompt off", () => {
    // Defence-in-depth: a Stopped frame arriving with no outstanding
    // turn must not advance `lastStoppedSeq` past `pendingUserPromptSeq`,
    // otherwise the next prompt's increment wouldn't catch up and
    // `turnActive` would stay false even with a real turn in flight.
    let state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: { UserPromptSent: { text: "hi" } },
    });
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 2,
      event: { Stopped: { reason: "prompt_complete" } },
    });
    expect(state.turnActive).toBe(false);
    // Spurious extra Stopped (e.g. duplicate replay of the close).
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 3,
      event: { Stopped: { reason: "prompt_complete" } },
    });
    expect(state.lastStoppedSeq).toBe(1);
    expect(state.pendingUserPromptSeq).toBe(1);
    // Next real prompt: turn must reactivate.
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 4,
      event: { UserPromptSent: { text: "second" } },
    });
    expect(state.pendingUserPromptSeq).toBe(2);
    expect(state.lastStoppedSeq).toBe(1);
    expect(state.turnActive).toBe(true);
  });

  it("optimistic user_prompt + matching server echo only bump pending once", async () => {
    // Avoids double-counting: the server's UserPromptSent that matches
    // and promotes an existing optimistic row must not bump
    // `pendingUserPromptSeq` again.
    const { cockpitHookReducer } = await import("../hooks/useCockpit");
    let state = cockpitHookReducer(emptyCockpitState(), {
      kind: "user_prompt",
      text: "echo me",
    });
    expect(state.pendingUserPromptSeq).toBe(1);
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 5,
      event: { UserPromptSent: { text: "echo me" } },
    });
    expect(state.pendingUserPromptSeq).toBe(1);
    expect(state.turnActive).toBe(true);
  });

  it("AgentStartupError advances lastStoppedSeq, preserving the race-safe semantics", () => {
    let state = applyEvent(emptyCockpitState(), {
      session_id: "s-1",
      seq: 1,
      event: { UserPromptSent: { text: "first" } },
    });
    state = applyEvent(state, {
      session_id: "s-1",
      seq: 2,
      event: { AgentStartupError: { message: "boom" } },
    });
    expect(state.lastStoppedSeq).toBe(1);
    expect(state.turnActive).toBe(false);
    expect(state.startupError).toBe("boom");
  });

  it("optimistic-match UserPromptSent resets per-turn flags (turnHasOutput, worker banners, wakeup)", () => {
    // The optimistic-match branch used to early-return after just
    // promoting the row id, leaving `turnHasOutput`, `workerStopped`,
    // `workerRestarting`, and the wakeup countdown stale from the
    // prior turn. With #1170's race-safe semantics that desync can
    // suppress the empty-output notice on a follow-up that produces
    // nothing, so the resets now run on BOTH UserPromptSent branches.
    const stale: CockpitState = {
      ...withOptimisticPrompt(emptyCockpitState(), "follow-up"),
      turnHasOutput: true,
      workerStopped: true,
      workerRestarting: true,
      nextWakeupAt: new Date(Date.now() - 1_000).toISOString(),
      nextWakeupReason: "tick",
    };
    const next = applyEvent(stale, {
      session_id: "s-1",
      seq: 9,
      event: { UserPromptSent: { text: "follow-up" } },
    });
    expect(next.activity).toHaveLength(1);
    expect(next.activity[0].id).toBe("user-seq-9");
    expect(next.turnHasOutput).toBe(false);
    expect(next.workerStopped).toBe(false);
    expect(next.workerRestarting).toBe(false);
    expect(next.nextWakeupAt).toBeNull();
    expect(next.nextWakeupReason).toBeNull();
    // pendingUserPromptSeq must NOT double-count: withOptimisticPrompt
    // bumped it to 1, the server echo matched the optimistic row, so
    // it stays at 1.
    expect(next.pendingUserPromptSeq).toBe(1);
    expect(next.turnActive).toBe(true);
  });
});

describe("normaliseTurnCounters (#1170 persisted-state backfill)", () => {
  it("backfills counters from cached turnActive=true", () => {
    const cached = {
      ...emptyCockpitState(),
      turnActive: true,
    } as CockpitState & { pendingUserPromptSeq?: number; lastStoppedSeq?: number };
    delete cached.pendingUserPromptSeq;
    delete cached.lastStoppedSeq;
    const normalised = normaliseTurnCounters(cached);
    expect(normalised.pendingUserPromptSeq).toBe(1);
    expect(normalised.lastStoppedSeq).toBe(0);
    expect(normalised.turnActive).toBe(true);
  });

  it("backfills counters from cached turnActive=false", () => {
    const cached = {
      ...emptyCockpitState(),
      turnActive: false,
    } as CockpitState & { pendingUserPromptSeq?: number; lastStoppedSeq?: number };
    delete cached.pendingUserPromptSeq;
    delete cached.lastStoppedSeq;
    const normalised = normaliseTurnCounters(cached);
    expect(normalised.pendingUserPromptSeq).toBe(0);
    expect(normalised.lastStoppedSeq).toBe(0);
    expect(normalised.turnActive).toBe(false);
  });

  it("passes through entries that already carry counters", () => {
    const fresh: CockpitState = {
      ...emptyCockpitState(),
      pendingUserPromptSeq: 5,
      lastStoppedSeq: 3,
      turnActive: false,
    };
    const normalised = normaliseTurnCounters(fresh);
    expect(normalised.pendingUserPromptSeq).toBe(5);
    expect(normalised.lastStoppedSeq).toBe(3);
    // Even if the cached `turnActive` boolean was stale, the derived
    // value wins so the spinner gate matches the counters.
    expect(normalised.turnActive).toBe(true);
  });
});

describe("cockpitHookReducer / dismiss_primer", () => {
  // Banner dismiss used to live in component-local useState and
  // re-armed itself on every session switch. Moved into the reducer so
  // the dismissal survives mount/unmount; the next SessionContextReset
  // re-seeds contextPrimerAvailable with a new resetSeq so a later
  // incident still surfaces the banner. See #1110.
  it("clears contextPrimerAvailable", async () => {
    const { cockpitHookReducer } = await import("../hooks/useCockpit");
    const seeded: CockpitState = {
      ...emptyCockpitState(),
      contextPrimerAvailable: {
        resetSeq: 12,
        reason: "Conversation context reset; agent transcript was unavailable.",
      },
    };
    const next = cockpitHookReducer(seeded, { kind: "dismiss_primer" });
    expect(next.contextPrimerAvailable).toBeNull();
  });
});
