import { describe, expect, it } from "vitest";
import type { SessionResponse, Workspace } from "./types";
import {
  compareByBirth,
  groupCreatedAt,
  makeCompareWorkspaces,
  workspaceCreatedAt,
} from "./workspaceSort";

// With no user-defined ordering, `makeCompareWorkspaces([])` reduces to
// the birth-key comparator. We test the default contract here; the
// ordering-priority contract gets its own block below.
const compareWorkspaces = makeCompareWorkspaces([]);

function mkSession(overrides: Partial<SessionResponse> = {}): SessionResponse {
  return {
    id: "s",
    title: "t",
    project_path: "/repo",
    group_path: "",
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
    ...overrides,
  };
}

function mkWorkspace(
  id: string,
  status: "active" | "idle",
  sessions: SessionResponse[],
): Workspace {
  return {
    id,
    branch: null,
    projectPath: "/repo",
    displayName: id,
    agents: ["claude"],
    primaryAgent: "claude",
    status,
    sessions,
  };
}

describe("compareWorkspaces (sidebar stable order, #1169)", () => {
  it("does not reorder when a session's status flips active <-> idle", () => {
    const wsA = mkWorkspace("a", "active", [
      mkSession({ id: "sa", created_at: "2025-01-01T00:00:00Z" }),
    ]);
    const wsB = mkWorkspace("b", "idle", [
      mkSession({ id: "sb", created_at: "2025-02-01T00:00:00Z" }),
    ]);

    const initial = [wsA, wsB].sort(compareWorkspaces).map((w) => w.id);

    const wsAFlipped = { ...wsA, status: "idle" as const };
    const wsBFlipped = { ...wsB, status: "active" as const };
    const flipped = [wsAFlipped, wsBFlipped]
      .sort(compareWorkspaces)
      .map((w) => w.id);

    expect(flipped).toEqual(initial);
  });

  it("places the newest workspace at index 0 (created_at descending)", () => {
    const wsOld = mkWorkspace("old", "idle", [
      mkSession({ id: "s-old", created_at: "2025-01-01T00:00:00Z" }),
    ]);
    const wsMid = mkWorkspace("mid", "idle", [
      mkSession({ id: "s-mid", created_at: "2025-03-01T00:00:00Z" }),
    ]);
    const wsNew = mkWorkspace("new", "idle", [
      mkSession({ id: "s-new", created_at: "2025-05-01T00:00:00Z" }),
    ]);

    const sorted = [wsOld, wsMid, wsNew].sort(compareWorkspaces).map((w) => w.id);

    expect(sorted).toEqual(["new", "mid", "old"]);
  });

  it("breaks ties on `id` deterministically when created_at matches", () => {
    const ts = "2025-04-01T12:00:00Z";
    const wsZ = mkWorkspace("z-ws", "idle", [
      mkSession({ id: "sz", created_at: ts }),
    ]);
    const wsA = mkWorkspace("a-ws", "idle", [
      mkSession({ id: "sa", created_at: ts }),
    ]);
    const wsM = mkWorkspace("m-ws", "idle", [
      mkSession({ id: "sm", created_at: ts }),
    ]);

    const order = [wsZ, wsA, wsM].sort(compareWorkspaces).map((w) => w.id);
    const reverseOrder = [wsM, wsZ, wsA]
      .sort(compareWorkspaces)
      .map((w) => w.id);

    expect(order).toEqual(["a-ws", "m-ws", "z-ws"]);
    expect(reverseOrder).toEqual(order);
  });

  it("ignores `last_accessed_at` and `idle_entered_at` when keying", () => {
    const baseTs = "2025-01-01T00:00:00Z";
    const wsA = mkWorkspace("a", "idle", [
      mkSession({
        id: "sa",
        created_at: baseTs,
        last_accessed_at: "2099-01-01T00:00:00Z",
      }),
    ]);
    const wsB = mkWorkspace("b", "idle", [
      mkSession({
        id: "sb",
        created_at: baseTs,
        idle_entered_at: "2099-06-01T00:00:00Z",
      }),
    ]);

    const order = [wsB, wsA].sort(compareWorkspaces).map((w) => w.id);
    expect(order).toEqual(["a", "b"]);
  });

  it("sorts workspaces with empty created_at to the bottom", () => {
    const wsReal = mkWorkspace("real", "idle", [
      mkSession({ id: "s1", created_at: "2025-01-01T00:00:00Z" }),
    ]);
    const wsEmpty = mkWorkspace("empty", "idle", [
      mkSession({ id: "s2", created_at: "" }),
    ]);

    const order = [wsEmpty, wsReal].sort(compareWorkspaces).map((w) => w.id);
    expect(order).toEqual(["real", "empty"]);
  });
});

describe("makeCompareWorkspaces (user-defined ordering)", () => {
  function ws(id: string, createdAt: string): Workspace {
    return mkWorkspace(id, "idle", [mkSession({ id: `s-${id}`, created_at: createdAt })]);
  }

  it("ranks workspaces by their position in the ordering list (lower index sorts first)", () => {
    const w1 = ws("a", "2025-01-01T00:00:00Z");
    const w2 = ws("b", "2025-04-01T00:00:00Z");
    const w3 = ws("c", "2025-02-01T00:00:00Z");

    const cmp = makeCompareWorkspaces(["c", "a", "b"]);
    const order = [w1, w2, w3].sort(cmp).map((w) => w.id);
    expect(order).toEqual(["c", "a", "b"]);
  });

  it("sorts unranked workspaces after ranked ones, falling back to birth-key order", () => {
    const ranked1 = ws("ranked-1", "2025-01-01T00:00:00Z");
    const ranked2 = ws("ranked-2", "2025-02-01T00:00:00Z");
    const newest = ws("new", "2025-12-01T00:00:00Z");
    const middle = ws("mid", "2025-06-01T00:00:00Z");

    const cmp = makeCompareWorkspaces(["ranked-1", "ranked-2"]);
    const order = [newest, middle, ranked2, ranked1].sort(cmp).map((w) => w.id);
    // Ranked first (in ordering-list order), then unranked newest-first.
    expect(order).toEqual(["ranked-1", "ranked-2", "new", "mid"]);
  });

  it("matches the birth-key comparator when the ordering is empty", () => {
    const wsOld = ws("old", "2025-01-01T00:00:00Z");
    const wsNew = ws("new", "2025-05-01T00:00:00Z");
    const cmp = makeCompareWorkspaces([]);
    const order = [wsOld, wsNew].sort(cmp).map((w) => w.id);
    const expected = [wsOld, wsNew].sort(compareByBirth).map((w) => w.id);
    expect(order).toEqual(expected);
    expect(order).toEqual(["new", "old"]);
  });

  it("ignores stale entries (ids in the ordering that aren't current workspaces)", () => {
    const wsA = ws("a", "2025-01-01T00:00:00Z");
    const wsB = ws("b", "2025-02-01T00:00:00Z");

    // "ghost" is in the ordering but not in the sort input. The
    // comparator must not crash and must still order live workspaces.
    const cmp = makeCompareWorkspaces(["ghost", "b", "a"]);
    const order = [wsA, wsB].sort(cmp).map((w) => w.id);
    expect(order).toEqual(["b", "a"]);
  });
});

describe("workspaceCreatedAt", () => {
  it("returns the earliest session created_at (workspace birth)", () => {
    const ws = mkWorkspace("w", "idle", [
      mkSession({ id: "s2", created_at: "2025-03-01T00:00:00Z" }),
      mkSession({ id: "s1", created_at: "2025-01-01T00:00:00Z" }),
      mkSession({ id: "s3", created_at: "2025-02-01T00:00:00Z" }),
    ]);
    expect(workspaceCreatedAt(ws)).toBe("2025-01-01T00:00:00Z");
  });

  it("skips empty/missing created_at strings without crashing", () => {
    const ws = mkWorkspace("w", "idle", [
      mkSession({ id: "s1", created_at: "" }),
      mkSession({ id: "s2", created_at: "2025-02-01T00:00:00Z" }),
    ]);
    expect(workspaceCreatedAt(ws)).toBe("2025-02-01T00:00:00Z");
  });
});

describe("groupCreatedAt", () => {
  it("returns the earliest workspace birth across the group", () => {
    const wsA = mkWorkspace("a", "idle", [
      mkSession({ id: "sa", created_at: "2025-04-01T00:00:00Z" }),
    ]);
    const wsB = mkWorkspace("b", "idle", [
      mkSession({ id: "sb", created_at: "2025-02-01T00:00:00Z" }),
    ]);
    expect(groupCreatedAt([wsA, wsB])).toBe("2025-02-01T00:00:00Z");
  });
});
