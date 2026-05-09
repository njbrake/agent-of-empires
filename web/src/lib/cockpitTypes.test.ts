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
  // Mirrors the optimistic dispatch in useCockpit.sendPrompt — the
  // row id includes the wall-clock timestamp, distinct from the
  // `user-seq-N` form the reducer assigns when the server echoes.
  return {
    ...state,
    activity: state.activity.concat({
      id: `user-${Date.now()}-${state.activity.length}`,
      kind: "user_prompt",
      text,
      at: new Date().toISOString(),
    }),
    turnActive: true,
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
