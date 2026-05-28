// @vitest-environment jsdom

import { beforeEach, describe, expect, it } from "vitest";
import type { SessionResponse, Workspace } from "../types";
import {
  SIDEBAR_SORT_MODE_KEY,
  compareWorkspacesByLastActivityDesc,
  loadSidebarSortMode,
  repoGroupLastActivityMs,
  saveSidebarSortMode,
  workspaceIsPinned,
  workspaceIsSunk,
  workspaceLastActivityMs,
  workspaceTriageTier,
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

describe("workspaceIsPinned", () => {
  it("returns true when any session has pinned_at set", () => {
    const ws = workspace("w", [
      session({ id: "s1" }),
      session({ id: "s2", pinned_at: "2025-01-01T00:00:00Z" }),
    ]);
    expect(workspaceIsPinned(ws)).toBe(true);
  });

  it("returns false when no session is pinned", () => {
    const ws = workspace("w", [session({ id: "s1" }), session({ id: "s2" })]);
    expect(workspaceIsPinned(ws)).toBe(false);
  });
});

describe("workspaceIsSunk", () => {
  it("returns true when every session is archived or snoozed", () => {
    const ws = workspace("w", [
      session({ id: "s1", archived_at: "2025-01-01T00:00:00Z" }),
      session({ id: "s2", snoozed_until: "2026-01-01T00:00:00Z" }),
    ]);
    expect(workspaceIsSunk(ws)).toBe(true);
  });

  it("returns false when even one session is live", () => {
    // A multi-session workspace with one live session must stay in the
    // active tier rather than sinking the whole group out of sight.
    const ws = workspace("w", [
      session({ id: "s1", archived_at: "2025-01-01T00:00:00Z" }),
      session({ id: "s2" }),
    ]);
    expect(workspaceIsSunk(ws)).toBe(false);
  });

  it("returns false on an empty workspace", () => {
    const ws = workspace("w", []);
    expect(workspaceIsSunk(ws)).toBe(false);
  });
});

describe("workspaceTriageTier", () => {
  it("returns 0 (pinned) for any pinned session, overriding sink fields", () => {
    // Pin clears archive/snooze server-side, but a sibling session in
    // the same workspace could still be archived. Any-pinned wins.
    const ws = workspace("w", [
      session({ id: "s1", pinned_at: "2025-01-01T00:00:00Z" }),
      session({ id: "s2", archived_at: "2025-01-01T00:00:00Z" }),
    ]);
    expect(workspaceTriageTier(ws)).toBe(0);
  });

  it("returns 1 (live) when neither pinned nor fully sunk", () => {
    const ws = workspace("w", [session({ id: "s1" })]);
    expect(workspaceTriageTier(ws)).toBe(1);
  });

  it("returns 2 (sunk) when every session is archived or snoozed", () => {
    const ws = workspace("w", [
      session({ id: "s1", archived_at: "2025-01-01T00:00:00Z" }),
    ]);
    expect(workspaceTriageTier(ws)).toBe(2);
  });
});

describe("compareWorkspacesByLastActivityDesc triage tier", () => {
  it("sinks fully-archived workspaces below live ones regardless of activity", () => {
    // The archived workspace has the most recent activity timestamp; if
    // the comparator naively used activity only it would sort first.
    // Triage tier forces it to the bottom.
    const archived = workspace("archived-newer", [
      session({
        id: "sa",
        created_at: "2025-09-01T00:00:00Z",
        archived_at: "2025-09-02T00:00:00Z",
      }),
    ]);
    const live = workspace("live-older", [
      session({ id: "sl", created_at: "2025-01-01T00:00:00Z" }),
    ]);
    const list = [archived, live].sort(compareWorkspacesByLastActivityDesc);
    expect(list.map((w) => w.id)).toEqual(["live-older", "archived-newer"]);
  });

  it("lifts pinned workspaces above live ones regardless of activity", () => {
    // The pinned workspace has the older activity timestamp; without
    // the tier prefix the live one would sort first.
    const pinned = workspace("pinned-older", [
      session({
        id: "sp",
        created_at: "2025-01-01T00:00:00Z",
        pinned_at: "2025-01-02T00:00:00Z",
      }),
    ]);
    const live = workspace("live-newer", [
      session({ id: "sl", created_at: "2025-09-01T00:00:00Z" }),
    ]);
    const list = [live, pinned].sort(compareWorkspacesByLastActivityDesc);
    expect(list.map((w) => w.id)).toEqual(["pinned-older", "live-newer"]);
  });

  it("preserves activity order within the same tier", () => {
    const livesA = workspace("live-a", [
      session({ id: "sa", created_at: "2025-09-01T00:00:00Z" }),
    ]);
    const livesB = workspace("live-b", [
      session({ id: "sb", created_at: "2025-01-01T00:00:00Z" }),
    ]);
    const list = [livesB, livesA].sort(compareWorkspacesByLastActivityDesc);
    expect(list.map((w) => w.id)).toEqual(["live-a", "live-b"]);
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
