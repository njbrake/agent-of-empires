import { describe, expect, it } from "vitest";
import type { SessionResponse, Workspace } from "./types";
import { groupCreatedAt, sortWorkspaces } from "./workspaceSort";

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

function mkWorkspace(id: string, sessions: SessionResponse[]): Workspace {
  return {
    id,
    branch: null,
    projectPath: "/repo",
    displayName: id,
    agents: ["claude"],
    primaryAgent: "claude",
    status: "idle",
    sessions,
  };
}

const ids = (ws: Workspace[]) => ws.map((w) => w.id);

describe("sortWorkspaces", () => {
  it("does not reorder when a session's status flips active <-> idle (#1169)", () => {
    const wsA = { ...mkWorkspace("a", [mkSession({ id: "sa", created_at: "2025-01-01T00:00:00Z" })]), status: "active" as const };
    const wsB = { ...mkWorkspace("b", [mkSession({ id: "sb", created_at: "2025-02-01T00:00:00Z" })]), status: "idle" as const };

    expect(ids(sortWorkspaces([wsA, wsB], []))).toEqual(
      ids(sortWorkspaces(
        [{ ...wsA, status: "idle" }, { ...wsB, status: "active" }],
        [],
      )),
    );
  });

  it("places the newest workspace at index 0 (default order)", () => {
    const old = mkWorkspace("old", [mkSession({ id: "s-old", created_at: "2025-01-01T00:00:00Z" })]);
    const mid = mkWorkspace("mid", [mkSession({ id: "s-mid", created_at: "2025-03-01T00:00:00Z" })]);
    const fresh = mkWorkspace("new", [mkSession({ id: "s-new", created_at: "2025-05-01T00:00:00Z" })]);

    expect(ids(sortWorkspaces([old, mid, fresh], []))).toEqual(["new", "mid", "old"]);
  });

  it("ranks workspaces by their position in the ordering list", () => {
    const a = mkWorkspace("a", [mkSession({ id: "sa", created_at: "2025-01-01T00:00:00Z" })]);
    const b = mkWorkspace("b", [mkSession({ id: "sb", created_at: "2025-04-01T00:00:00Z" })]);
    const c = mkWorkspace("c", [mkSession({ id: "sc", created_at: "2025-02-01T00:00:00Z" })]);

    expect(ids(sortWorkspaces([a, b, c], ["c", "a", "b"]))).toEqual(["c", "a", "b"]);
  });

  it("sorts unranked workspaces after ranked ones, newest first", () => {
    const ranked1 = mkWorkspace("ranked-1", [mkSession({ id: "s1", created_at: "2025-01-01T00:00:00Z" })]);
    const ranked2 = mkWorkspace("ranked-2", [mkSession({ id: "s2", created_at: "2025-02-01T00:00:00Z" })]);
    const newest = mkWorkspace("new", [mkSession({ id: "s-new", created_at: "2025-12-01T00:00:00Z" })]);
    const middle = mkWorkspace("mid", [mkSession({ id: "s-mid", created_at: "2025-06-01T00:00:00Z" })]);

    expect(
      ids(sortWorkspaces([newest, middle, ranked2, ranked1], ["ranked-1", "ranked-2"])),
    ).toEqual(["ranked-1", "ranked-2", "new", "mid"]);
  });

  it("ignores stale entries in the ordering (ids that aren't current workspaces)", () => {
    const a = mkWorkspace("a", [mkSession({ id: "sa", created_at: "2025-01-01T00:00:00Z" })]);
    const b = mkWorkspace("b", [mkSession({ id: "sb", created_at: "2025-02-01T00:00:00Z" })]);
    expect(ids(sortWorkspaces([a, b], ["ghost", "b", "a"]))).toEqual(["b", "a"]);
  });
});

describe("groupCreatedAt", () => {
  it("returns the earliest workspace birth across the group", () => {
    const a = mkWorkspace("a", [mkSession({ id: "sa", created_at: "2025-04-01T00:00:00Z" })]);
    const b = mkWorkspace("b", [mkSession({ id: "sb", created_at: "2025-02-01T00:00:00Z" })]);
    expect(groupCreatedAt([a, b])).toBe("2025-02-01T00:00:00Z");
  });
});
