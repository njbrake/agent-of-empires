// @vitest-environment jsdom

import { beforeEach, describe, expect, it } from "vitest";
import type { SessionResponse, Workspace } from "../types";
import {
  SIDEBAR_SORT_MODE_KEY,
  compareWorkspacesByLastActivityDesc,
  loadSidebarSortMode,
  repoGroupLastActivityMs,
  saveSidebarSortMode,
  workspaceLastActivityMs,
} from "../sidebarSort";

function session(over: Partial<SessionResponse> = {}): SessionResponse {
  return {
    id: "s1",
    title: "t",
    project_path: "/p",
    group_path: "/p",
    tool: "claude",
    status: "Idle",
    yolo_mode: false,
    created_at: "2025-01-01T00:00:00Z",
    last_accessed_at: null,
    idle_entered_at: null,
    last_error: null,
    branch: null,
    main_repo_path: null,
    is_sandboxed: false,
    favorited: false,
    has_managed_worktree: false,
    has_terminal: true,
    profile: "default",
    cleanup_defaults: {
      delete_worktree: false,
      delete_branch: false,
      delete_sandbox: false,
    },
    remote_owner: null,
    notify_on_waiting: null,
    notify_on_idle: null,
    notify_on_error: null,
    claude_fullscreen: false,
    workspace_repos: [],
    ...over,
  };
}

function workspace(id: string, sessions: SessionResponse[]): Workspace {
  return {
    id,
    branch: null,
    projectPath: "/p",
    displayName: id,
    agents: ["claude"],
    primaryAgent: "claude",
    status: "idle",
    sessions,
  };
}

describe("workspaceLastActivityMs", () => {
  it("returns max across last_accessed_at, idle_entered_at, created_at", () => {
    const ws = workspace("w", [
      session({
        created_at: "2025-01-01T00:00:00Z",
        idle_entered_at: "2025-03-01T00:00:00Z",
        last_accessed_at: "2025-02-01T00:00:00Z",
      }),
    ]);
    expect(workspaceLastActivityMs(ws)).toBe(
      Date.parse("2025-03-01T00:00:00Z"),
    );
  });

  it("ignores null timestamps; created_at fallback works", () => {
    const ws = workspace("w", [
      session({
        created_at: "2025-05-01T00:00:00Z",
        idle_entered_at: null,
        last_accessed_at: null,
      }),
    ]);
    expect(workspaceLastActivityMs(ws)).toBe(
      Date.parse("2025-05-01T00:00:00Z"),
    );
  });

  it("ignores unparseable strings (no NaN poison)", () => {
    const ws = workspace("w", [
      session({
        created_at: "2025-04-01T00:00:00Z",
        idle_entered_at: "not-a-date",
        last_accessed_at: "also-bad",
      }),
    ]);
    expect(workspaceLastActivityMs(ws)).toBe(
      Date.parse("2025-04-01T00:00:00Z"),
    );
  });

  it("returns max across every session in a multi-session workspace", () => {
    const ws = workspace("w", [
      session({ id: "s1", created_at: "2025-01-01T00:00:00Z" }),
      session({ id: "s2", created_at: "2025-06-01T00:00:00Z" }),
      session({ id: "s3", created_at: "2025-03-01T00:00:00Z" }),
    ]);
    expect(workspaceLastActivityMs(ws)).toBe(
      Date.parse("2025-06-01T00:00:00Z"),
    );
  });

  it("returns NEGATIVE_INFINITY when no usable timestamp exists", () => {
    const ws = workspace("w", [
      session({
        created_at: "bad",
        idle_entered_at: null,
        last_accessed_at: null,
      }),
    ]);
    expect(workspaceLastActivityMs(ws)).toBe(Number.NEGATIVE_INFINITY);
  });
});

describe("compareWorkspacesByLastActivityDesc", () => {
  it("orders newer activity first", () => {
    const older = workspace("older", [
      session({ id: "s1", created_at: "2025-01-01T00:00:00Z" }),
    ]);
    const newer = workspace("newer", [
      session({ id: "s2", created_at: "2025-09-01T00:00:00Z" }),
    ]);
    const list = [older, newer].sort(compareWorkspacesByLastActivityDesc);
    expect(list.map((w) => w.id)).toEqual(["newer", "older"]);
  });

  it("breaks ties by workspace id ascending (deterministic)", () => {
    const ts = "2025-01-01T00:00:00Z";
    const wsB = workspace("b", [session({ id: "sb", created_at: ts })]);
    const wsA = workspace("a", [session({ id: "sa", created_at: ts })]);
    const wsC = workspace("c", [session({ id: "sc", created_at: ts })]);
    const list = [wsB, wsA, wsC].sort(compareWorkspacesByLastActivityDesc);
    expect(list.map((w) => w.id)).toEqual(["a", "b", "c"]);
  });

  it("breaks ties by id when both sides have no usable timestamp", () => {
    // Both workspaces return NEGATIVE_INFINITY from
    // workspaceLastActivityMs. Subtracting two -Infinity values yields
    // NaN, which Array.sort treats like equality and skips the
    // tie-break, so this case would silently flake without the
    // explicit `<` / `>` comparison in the comparator.
    const bad = {
      created_at: "bad",
      idle_entered_at: null,
      last_accessed_at: null,
    };
    const wsB = workspace("b", [session({ id: "sb", ...bad })]);
    const wsA = workspace("a", [session({ id: "sa", ...bad })]);
    const list = [wsB, wsA].sort(compareWorkspacesByLastActivityDesc);
    expect(list.map((w) => w.id)).toEqual(["a", "b"]);
  });
});

describe("repoGroupLastActivityMs", () => {
  it("returns the max across workspaces in the group", () => {
    const a = workspace("a", [
      session({ id: "sa", created_at: "2025-01-01T00:00:00Z" }),
    ]);
    const b = workspace("b", [
      session({ id: "sb", created_at: "2025-07-01T00:00:00Z" }),
    ]);
    expect(repoGroupLastActivityMs([a, b])).toBe(
      Date.parse("2025-07-01T00:00:00Z"),
    );
  });

  it("returns NEGATIVE_INFINITY on an empty group", () => {
    expect(repoGroupLastActivityMs([])).toBe(Number.NEGATIVE_INFINITY);
  });
});

describe("loadSidebarSortMode / saveSidebarSortMode", () => {
  beforeEach(() => {
    window.localStorage.clear();
  });

  it("defaults to 'manual' when localStorage is empty", () => {
    expect(loadSidebarSortMode()).toBe("manual");
  });

  it("returns 'manual' for an unrecognised stored value", () => {
    window.localStorage.setItem(SIDEBAR_SORT_MODE_KEY, "nonsense");
    expect(loadSidebarSortMode()).toBe("manual");
  });

  it("round-trips 'lastActivity'", () => {
    saveSidebarSortMode("lastActivity");
    expect(window.localStorage.getItem(SIDEBAR_SORT_MODE_KEY)).toBe(
      "lastActivity",
    );
    expect(loadSidebarSortMode()).toBe("lastActivity");
  });

  it("round-trips 'manual'", () => {
    saveSidebarSortMode("lastActivity");
    saveSidebarSortMode("manual");
    expect(loadSidebarSortMode()).toBe("manual");
  });
});
