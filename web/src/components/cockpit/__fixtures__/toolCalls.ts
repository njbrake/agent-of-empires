// Reusable ToolCall / ActivityRow fixtures for cockpit per-kind
// render tests. Shapes mirror the wire types emitted by
// `src/cockpit/state.rs` and consumed by `ToolCards.tsx`.

import type { ActivityRow, ToolCall } from "../../../lib/cockpitTypes";

export function makeToolCall(over: Partial<ToolCall> = {}): ToolCall {
  return {
    id: "tc-1",
    name: "Tool",
    kind: "other",
    args_preview: "{}",
    started_at: "2026-05-21T00:00:00Z",
    ...over,
  };
}

export function makeCompletion(
  over: Partial<ActivityRow> = {},
): ActivityRow {
  return {
    id: "row-1",
    kind: "tool_complete",
    text: "",
    toolCallId: "tc-1",
    at: "2026-05-21T00:00:01Z",
    ...over,
  };
}

export function makeError(over: Partial<ActivityRow> = {}): ActivityRow {
  return makeCompletion({ kind: "tool_error", text: "failed", ...over });
}

export const fixtures = {
  bash: makeToolCall({
    id: "bash-1",
    name: "Bash",
    kind: "execute",
    args_preview: JSON.stringify({ command: "ls -la", description: "list" }),
  }),
  read: makeToolCall({
    id: "read-1",
    name: "Read",
    kind: "read",
    args_preview: JSON.stringify({ file_path: "/tmp/main.rs" }),
  }),
  edit: makeToolCall({
    id: "edit-1",
    name: "Edit",
    kind: "edit",
    args_preview: JSON.stringify({
      file_path: "/tmp/main.rs",
      old_string: "fn foo() {}",
      new_string: "fn foo() { bar(); }",
    }),
  }),
  write: makeToolCall({
    id: "write-1",
    name: "Write",
    kind: "edit",
    args_preview: JSON.stringify({
      file_path: "/tmp/new.rs",
      content: "fn main() {}",
    }),
  }),
  del: makeToolCall({
    id: "del-1",
    name: "Delete",
    kind: "delete",
    args_preview: JSON.stringify({ file_path: "/tmp/gone.rs" }),
  }),
  search: makeToolCall({
    id: "search-1",
    name: "Grep",
    kind: "search",
    args_preview: JSON.stringify({ pattern: "TODO", path: "/tmp" }),
  }),
  fetch: makeToolCall({
    id: "fetch-1",
    name: "WebFetch",
    kind: "fetch",
    args_preview: JSON.stringify({ url: "https://example.com" }),
  }),
  think: makeToolCall({
    id: "think-1",
    name: "Think",
    kind: "think",
    args_preview: JSON.stringify({ thought: "consider the problem" }),
  }),
  todoWrite: makeToolCall({
    id: "todo-1",
    name: "TodoWrite",
    kind: "other",
    args_preview: JSON.stringify({
      todos: [
        { content: "Step one", status: "completed", activeForm: "doing one" },
        { content: "Step two", status: "in_progress", activeForm: "doing two" },
        { content: "Step three", status: "pending", activeForm: "doing three" },
      ],
    }),
  }),
  skill: makeToolCall({
    id: "skill-1",
    name: "Skill",
    kind: "other",
    args_preview: JSON.stringify({ skill: "investigate" }),
  }),
  scheduleWakeup: makeToolCall({
    id: "sched-1",
    name: "ScheduleWakeup",
    kind: "other",
    args_preview: JSON.stringify({
      delaySeconds: 300,
      reason: "checking deploy",
    }),
  }),
  mcp: makeToolCall({
    id: "mcp-1",
    name: "mcp__slack__send_message",
    kind: "other",
    args_preview: JSON.stringify({ channel: "#general", text: "hi" }),
  }),
  generic: makeToolCall({
    id: "gen-1",
    name: "WeirdTool",
    kind: "other",
    args_preview: JSON.stringify({ x: 1 }),
  }),
};
