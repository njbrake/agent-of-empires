import { describe, expect, it } from "vitest";
import {
  SUBAGENT_TASK_NAME,
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

describe("activityToThreadMessages; tool-call grouping (#1057)", () => {
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

  it("smuggles parent_tool_call_id through args_preview as _aoe_parent_tool_call_id (#1041)", () => {
    const childTool: ToolCall = {
      id: "ch-1",
      name: "Read",
      kind: "read",
      args_preview: JSON.stringify({ path: "/tmp/x" }),
      started_at: "2026-05-12T00:00:00Z",
      parent_tool_call_id: "task-parent-1",
    };
    const row: ActivityRow = {
      id: "start-ch-1",
      kind: "tool_start",
      text: "Read",
      toolCallId: "ch-1",
      tool: childTool,
      at: "2026-05-12T00:00:00Z",
    };
    const messages = activityToThreadMessages([userRow("go"), row], false);
    const assistant = messages.find((m) => m.role === "assistant")!;
    const parts = assistant.content as Array<{
      type: string;
      argsText?: string;
    }>;
    const child = parts.find((p) => p.type === "tool-call")!;
    const parsed = JSON.parse(child.argsText!);
    expect(parsed._aoe_parent_tool_call_id).toBe("task-parent-1");
  });

  it("collapses a parent Task + its children into a _aoe_subagent_task part (#1041)", () => {
    const parent: ToolCall = {
      id: "task-1",
      name: "Investigate auth bug",
      kind: "think",
      args_preview: JSON.stringify({
        description: "Investigate auth bug",
        _aoe_title: "Investigate auth bug",
      }),
      started_at: "2026-05-12T00:00:00Z",
    };
    const parentRow: ActivityRow = {
      id: "start-task-1",
      kind: "tool_start",
      text: "Task",
      toolCallId: "task-1",
      tool: parent,
      at: "2026-05-12T00:00:00Z",
    };
    const child: ToolCall = {
      id: "ch-1",
      name: "Read",
      kind: "read",
      args_preview: JSON.stringify({ path: "/x" }),
      started_at: "2026-05-12T00:00:01Z",
      parent_tool_call_id: "task-1",
    };
    const childRow: ActivityRow = {
      id: "start-ch-1",
      kind: "tool_start",
      text: "Read",
      toolCallId: "ch-1",
      tool: child,
      at: "2026-05-12T00:00:01Z",
    };
    const messages = activityToThreadMessages(
      [userRow("go"), parentRow, childRow],
      false,
    );
    const assistant = messages.find((m) => m.role === "assistant")!;
    const parts = assistant.content as Array<{
      type: string;
      toolName?: string;
      argsText?: string;
    }>;
    const subagentParts = parts.filter(
      (p) => p.type === "tool-call" && p.toolName === SUBAGENT_TASK_NAME,
    );
    expect(subagentParts).toHaveLength(1);
    const payload = JSON.parse(subagentParts[0]!.argsText!);
    expect(payload.parent.toolCallId).toBe("task-1");
    expect(payload.children).toHaveLength(1);
    expect(payload.children[0].toolCallId).toBe("ch-1");
    // The original child part should not appear as a top-level tool-call.
    const directChild = parts.find(
      (p) => p.type === "tool-call" && p.toolName !== SUBAGENT_TASK_NAME,
    );
    expect(directChild).toBeUndefined();
  });

  it("leaves orphan children in place when their parent is absent", () => {
    const orphanChild: ToolCall = {
      id: "ch-1",
      name: "Read",
      kind: "read",
      args_preview: JSON.stringify({ path: "/x" }),
      started_at: "2026-05-12T00:00:00Z",
      parent_tool_call_id: "task-elsewhere",
    };
    const childRow: ActivityRow = {
      id: "start-ch-1",
      kind: "tool_start",
      text: "Read",
      toolCallId: "ch-1",
      tool: orphanChild,
      at: "2026-05-12T00:00:00Z",
    };
    const messages = activityToThreadMessages([userRow("go"), childRow], false);
    const assistant = messages.find((m) => m.role === "assistant")!;
    const parts = assistant.content as Array<{
      type: string;
      toolName?: string;
    }>;
    const subagentParts = parts.filter(
      (p) => p.type === "tool-call" && p.toolName === SUBAGENT_TASK_NAME,
    );
    expect(subagentParts).toHaveLength(0);
    const toolParts = parts.filter((p) => p.type === "tool-call");
    expect(toolParts).toHaveLength(1);
  });

  it("does not fold subagent parent + children into the generic tool group", () => {
    // Three children would otherwise hit TOOL_GROUP_MIN_RUN=3.
    const parent: ToolCall = {
      id: "task-1",
      name: "Task",
      kind: "think",
      args_preview: JSON.stringify({ description: "go" }),
      started_at: "2026-05-12T00:00:00Z",
    };
    const parentRow: ActivityRow = {
      id: "start-task-1",
      kind: "tool_start",
      text: "Task",
      toolCallId: "task-1",
      tool: parent,
      at: "2026-05-12T00:00:00Z",
    };
    const mkChild = (id: string): ActivityRow => ({
      id: `start-${id}`,
      kind: "tool_start",
      text: "Read",
      toolCallId: id,
      tool: {
        id,
        name: "Read",
        kind: "read",
        args_preview: JSON.stringify({ path: `/${id}` }),
        started_at: "2026-05-12T00:00:00Z",
        parent_tool_call_id: "task-1",
      },
      at: "2026-05-12T00:00:00Z",
    });
    const messages = activityToThreadMessages(
      [userRow("go"), parentRow, mkChild("a"), mkChild("b"), mkChild("c")],
      false,
    );
    const assistant = messages.find((m) => m.role === "assistant")!;
    const parts = assistant.content as Array<{
      type: string;
      toolName?: string;
    }>;
    const groups = parts.filter(
      (p) => p.type === "tool-call" && p.toolName === TOOL_GROUP_NAME,
    );
    expect(groups).toHaveLength(0);
    const subagents = parts.filter(
      (p) => p.type === "tool-call" && p.toolName === SUBAGENT_TASK_NAME,
    );
    expect(subagents).toHaveLength(1);
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
