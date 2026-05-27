// @vitest-environment jsdom
//
// Hook tests for useRepoGroups (#1418). The hook owns three orthogonal
// concerns:
//   1. Grouping workspaces by repo (and pulling multi-repo workspaces
//      into a synthetic group).
//   2. Sorting workspaces inside each group, plus sorting groups across
//      the sidebar. Branches on sortMode.
//   3. Collapsed + appearance state plumbed through localStorage.
//
// The sort comparator's edge cases (null timestamps, tie-breakers) are
// covered by sidebarSort.test.ts. These tests assert the integration:
// manual mode preserves the #1171 rank-driven order, lastActivity mode
// flips it, the multi-repo group stays pinned in both modes, and the
// stateful API (toggleRepoCollapsed, updateRepoAppearance) round-trips
// through localStorage.

import { renderHook, act } from "@testing-library/react";
import { beforeEach, describe, expect, it } from "vitest";

import { useRepoGroups, MULTI_REPO_GROUP_ID } from "./useRepoGroups";
import type {
  SessionResponse,
  Workspace,
  WorkspaceRepoSummary,
} from "../lib/types";

function session(over: Partial<SessionResponse> = {}): SessionResponse {
  return {
    id: "s1",
    title: "t",
    project_path: "/repo-a",
    group_path: "/repo-a",
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

function workspace(
  id: string,
  projectPath: string,
  sessions: SessionResponse[],
  over: Partial<Workspace> = {},
): Workspace {
  return {
    id,
    branch: null,
    projectPath,
    displayName: id,
    agents: ["claude"],
    primaryAgent: "claude",
    status: "idle",
    sessions,
    ...over,
  };
}

const multiRepos: WorkspaceRepoSummary[] = [
  { name: "repo-a", source_path: "/repo-a", branch: "main" },
  { name: "repo-b", source_path: "/repo-b", branch: "main" },
];

beforeEach(() => {
  window.localStorage.clear();
});

describe("useRepoGroups grouping", () => {
  it("groups single-repo workspaces by projectPath and pins multi-repo to the bottom", () => {
    const wA1 = workspace("a1", "/repo-a", [
      session({ id: "s-a1", created_at: "2025-01-01T00:00:00Z" }),
    ]);
    const wA2 = workspace("a2", "/repo-a", [
      session({ id: "s-a2", created_at: "2025-02-01T00:00:00Z" }),
    ]);
    const wB1 = workspace("b1", "/repo-b", [
      session({ id: "s-b1", created_at: "2025-03-01T00:00:00Z" }),
    ]);
    const wMulti = workspace("multi", "/repo-a", [
      session({
        id: "s-multi",
        created_at: "2025-04-01T00:00:00Z",
        workspace_repos: multiRepos,
      }),
    ]);

    const { result } = renderHook(() =>
      useRepoGroups([wA1, wA2, wB1, wMulti], ["a1", "a2", "b1", "multi"]),
    );

    const groups = result.current.groups;
    expect(groups.map((g) => g.id)).toEqual([
      "/repo-a",
      "/repo-b",
      MULTI_REPO_GROUP_ID,
    ]);
    expect(groups[0].workspaces.map((w) => w.id)).toEqual(["a1", "a2"]);
    expect(groups[1].workspaces.map((w) => w.id)).toEqual(["b1"]);
    expect(groups[2].workspaces.map((w) => w.id)).toEqual(["multi"]);
  });

  it("derives group displayName from the last path segment", () => {
    const w = workspace("a1", "/home/user/code/repo-x", [session()]);
    const { result } = renderHook(() => useRepoGroups([w], ["a1"]));
    expect(result.current.groups[0].displayName).toBe("repo-x");
  });

  it("marks a group active when any workspace is active", () => {
    const wIdle = workspace(
      "a1",
      "/repo-a",
      [session({ id: "s1" })],
      { status: "idle" },
    );
    const wActive = workspace(
      "a2",
      "/repo-a",
      [session({ id: "s2" })],
      { status: "active" },
    );
    const { result } = renderHook(() =>
      useRepoGroups([wIdle, wActive], ["a1", "a2"]),
    );
    expect(result.current.groups[0].status).toBe("active");
  });
});

describe("useRepoGroups sortMode = manual (#1171 behaviour)", () => {
  it("orders workspaces inside a group by their position in workspaceOrdering", () => {
    const wNew = workspace("new", "/repo-a", [
      session({ id: "s-new", created_at: "2025-09-01T00:00:00Z" }),
    ]);
    const wOld = workspace("old", "/repo-a", [
      session({ id: "s-old", created_at: "2025-01-01T00:00:00Z" }),
    ]);
    // Server pins old first even though new is newer.
    const { result } = renderHook(() =>
      useRepoGroups([wNew, wOld], ["old", "new"], "manual"),
    );
    expect(result.current.groups[0].workspaces.map((w) => w.id)).toEqual([
      "old",
      "new",
    ]);
  });

  it("orders groups by the minimum rank of their workspaces", () => {
    const wA = workspace("a1", "/repo-a", [session({ id: "s-a" })]);
    const wB = workspace("b1", "/repo-b", [session({ id: "s-b" })]);
    // b1 has the lower rank, so /repo-b group renders first.
    const { result } = renderHook(() =>
      useRepoGroups([wA, wB], ["b1", "a1"], "manual"),
    );
    expect(result.current.groups.map((g) => g.id)).toEqual([
      "/repo-b",
      "/repo-a",
    ]);
  });
});

describe("useRepoGroups sortMode = lastActivity (#1418)", () => {
  it("orders workspaces inside a group by recency descending, ignoring workspaceOrdering", () => {
    const wOld = workspace("old", "/repo-a", [
      session({ id: "s-old", created_at: "2025-01-01T00:00:00Z" }),
    ]);
    const wNew = workspace("new", "/repo-a", [
      session({
        id: "s-new",
        created_at: "2025-03-01T00:00:00Z",
        last_accessed_at: "2025-09-01T00:00:00Z",
      }),
    ]);
    // Server ordering pins old first; lastActivity overrides.
    const { result } = renderHook(() =>
      useRepoGroups([wOld, wNew], ["old", "new"], "lastActivity"),
    );
    expect(result.current.groups[0].workspaces.map((w) => w.id)).toEqual([
      "new",
      "old",
    ]);
  });

  it("orders groups by the most-recent activity timestamp across each group's workspaces", () => {
    const wA = workspace("a1", "/repo-a", [
      session({ id: "s-a", created_at: "2025-01-01T00:00:00Z" }),
    ]);
    const wB = workspace("b1", "/repo-b", [
      session({
        id: "s-b",
        created_at: "2025-01-01T00:00:00Z",
        last_accessed_at: "2025-09-01T00:00:00Z",
      }),
    ]);
    const { result } = renderHook(() =>
      useRepoGroups([wA, wB], ["a1", "b1"], "lastActivity"),
    );
    // /repo-b's max activity is newer, so it floats above /repo-a.
    expect(result.current.groups.map((g) => g.id)).toEqual([
      "/repo-b",
      "/repo-a",
    ]);
  });

  it("falls back to repoPath alphabetical when two groups have identical activity", () => {
    const ts = "2025-05-01T00:00:00Z";
    const wA = workspace("a1", "/repo-a", [
      session({ id: "s-a", created_at: ts }),
    ]);
    const wB = workspace("b1", "/repo-b", [
      session({ id: "s-b", created_at: ts }),
    ]);
    const { result } = renderHook(() =>
      useRepoGroups([wB, wA], ["b1", "a1"], "lastActivity"),
    );
    expect(result.current.groups.map((g) => g.id)).toEqual([
      "/repo-a",
      "/repo-b",
    ]);
  });

  it("keeps the multi-repo group pinned at the bottom even when its activity is the freshest", () => {
    const wSingle = workspace("single", "/repo-a", [
      session({ id: "s-single", created_at: "2025-01-01T00:00:00Z" }),
    ]);
    const wMulti = workspace("multi", "/repo-a", [
      session({
        id: "s-multi",
        created_at: "2025-01-01T00:00:00Z",
        last_accessed_at: "2025-12-31T00:00:00Z",
        workspace_repos: multiRepos,
      }),
    ]);
    const { result } = renderHook(() =>
      useRepoGroups([wMulti, wSingle], ["multi", "single"], "lastActivity"),
    );
    expect(result.current.groups.map((g) => g.id)).toEqual([
      "/repo-a",
      MULTI_REPO_GROUP_ID,
    ]);
  });
});

describe("useRepoGroups stateful API", () => {
  it("toggleRepoCollapsed flips collapsed and round-trips to localStorage", () => {
    const w = workspace("a1", "/repo-a", [session()]);
    const { result, rerender } = renderHook(() =>
      useRepoGroups([w], ["a1"]),
    );
    expect(result.current.groups[0].collapsed).toBe(false);

    act(() => {
      result.current.toggleRepoCollapsed("/repo-a");
    });
    rerender();
    expect(result.current.groups[0].collapsed).toBe(true);
    expect(window.localStorage.getItem("aoe-repo-collapsed-/repo-a")).toBe(
      "1",
    );

    act(() => {
      result.current.toggleRepoCollapsed("/repo-a");
    });
    rerender();
    expect(result.current.groups[0].collapsed).toBe(false);
    expect(
      window.localStorage.getItem("aoe-repo-collapsed-/repo-a"),
    ).toBeNull();
  });

  it("updateRepoAppearance applies an alias and surfaces it in displayName", () => {
    const w = workspace("a1", "/repo-a", [session()]);
    const { result, rerender } = renderHook(() =>
      useRepoGroups([w], ["a1"]),
    );
    expect(result.current.groups[0].displayName).toBe("repo-a");

    act(() => {
      result.current.updateRepoAppearance("/repo-a", { alias: "pretty-name" });
    });
    rerender();

    expect(result.current.groups[0].displayName).toBe("pretty-name");
    expect(result.current.groups[0].alias).toBe("pretty-name");
  });
});
