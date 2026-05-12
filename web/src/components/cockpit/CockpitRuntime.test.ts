import { describe, expect, it } from "vitest";
import {
  TOOL_GROUP_NAME,
  activityToThreadMessages,
} from "./CockpitRuntime";
import type { ActivityRow, ToolCall } from "../../lib/cockpitTypes";

function userRow(text: string, id = "u1"): ActivityRow {
  return {
    id,
    kind: "user_prompt",
    text,
    at: "2026-05-12T00:00:00Z",
  };
}

function toolStart(id: string, kind = "read"): ActivityRow {
  const tool: ToolCall = {
    id,
    name: "Read",
    kind,
    args_preview: JSON.stringify({ path: `/tmp/${id}.txt` }),
    started_at: "2026-05-12T00:00:00Z",
  };
  return {
    id: `start-${id}`,
    kind: "tool_start",
    text: "Read",
    toolCallId: id,
    tool,
    at: "2026-05-12T00:00:00Z",
  };
}

function messageRow(text: string, id = "m1"): ActivityRow {
  return {
    id,
    kind: "message",
    text,
    at: "2026-05-12T00:00:00Z",
  };
}

describe("activityToThreadMessages — tool-call grouping (#1057)", () => {
  it("folds a run of ≥3 consecutive tool calls into one group", () => {
    const messages = activityToThreadMessages(
      [
        userRow("go"),
        toolStart("t1"),
        toolStart("t2"),
        toolStart("t3"),
        toolStart("t4"),
      ],
      false,
    );
    const assistant = messages.find((m) => m.role === "assistant");
    expect(assistant).toBeDefined();
    const parts = (assistant!.content as Array<{ type: string; toolName?: string }>);
    const toolParts = parts.filter((p) => p.type === "tool-call");
    expect(toolParts).toHaveLength(1);
    expect(toolParts[0]!.toolName).toBe(TOOL_GROUP_NAME);
  });

  it("does not group runs of 1 or 2 tool calls", () => {
    const messages = activityToThreadMessages(
      [userRow("go"), toolStart("t1"), toolStart("t2")],
      false,
    );
    const assistant = messages.find((m) => m.role === "assistant")!;
    const parts = (assistant.content as Array<{ type: string; toolName?: string }>);
    const toolParts = parts.filter((p) => p.type === "tool-call");
    expect(toolParts).toHaveLength(2);
    for (const p of toolParts) expect(p.toolName).not.toBe(TOOL_GROUP_NAME);
  });

  it("text between tool calls splits two runs", () => {
    const messages = activityToThreadMessages(
      [
        userRow("go"),
        toolStart("a1"),
        toolStart("a2"),
        toolStart("a3"),
        messageRow("Found it."),
        toolStart("b1"),
        toolStart("b2"),
        toolStart("b3"),
      ],
      false,
    );
    const assistant = messages.find((m) => m.role === "assistant")!;
    const parts = (assistant.content as Array<{ type: string; toolName?: string }>);
    const groups = parts.filter(
      (p) => p.type === "tool-call" && p.toolName === TOOL_GROUP_NAME,
    );
    expect(groups).toHaveLength(2);
  });

  it("exempts TodoWrite calls from folding (#1064)", () => {
    const todoTool: ToolCall = {
      id: "td-1",
      name: "Update TODOs: a, b",
      kind: "think",
      args_preview: JSON.stringify({
        todos: [
          { content: "a", status: "pending" },
          { content: "b", status: "in_progress" },
        ],
      }),
      started_at: "2026-05-12T00:00:00Z",
    };
    const todoRow: ActivityRow = {
      id: "start-td-1",
      kind: "tool_start",
      text: "Update TODOs",
      toolCallId: "td-1",
      tool: todoTool,
      at: "2026-05-12T00:00:00Z",
    };
    const messages = activityToThreadMessages(
      [userRow("go"), toolStart("a"), toolStart("b"), todoRow, toolStart("c")],
      false,
    );
    const assistant = messages.find((m) => m.role === "assistant")!;
    const parts = (assistant.content as Array<{ type: string; toolName?: string }>);
    const groups = parts.filter(
      (p) => p.type === "tool-call" && p.toolName === TOOL_GROUP_NAME,
    );
    expect(groups).toHaveLength(0);
    const toolParts = parts.filter((p) => p.type === "tool-call");
    expect(toolParts).toHaveLength(4);
  });

  it("does not group across user-prompt boundaries (separate messages)", () => {
    const messages = activityToThreadMessages(
      [
        userRow("first", "u1"),
        toolStart("t1"),
        toolStart("t2"),
        userRow("second", "u2"),
        toolStart("t3"),
      ],
      false,
    );
    // Each user_prompt starts a fresh assistant message; neither run is
    // long enough to fold on its own.
    const assistants = messages.filter((m) => m.role === "assistant");
    expect(assistants).toHaveLength(2);
    for (const m of assistants) {
      const parts = (m.content as Array<{ type: string; toolName?: string }>);
      for (const p of parts.filter((p) => p.type === "tool-call")) {
        expect(p.toolName).not.toBe(TOOL_GROUP_NAME);
      }
    }
  });
});
