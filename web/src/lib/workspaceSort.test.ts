import { describe, expect, it } from "vitest";
import type { SessionResponse, Workspace } from "./types";
import {
  compareWorkspaces,
  groupCreatedAt,
  workspaceCreatedAt,
} from "./workspaceSort";

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
