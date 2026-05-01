import { useMemo } from "react";
import type { SessionResponse, Workspace } from "../lib/types";
import { isSessionActive } from "../lib/session";

/** Strip trailing slashes for consistent grouping */
function normalizePath(p: string): string {
  return p.replace(/\/+$/, "");
}

export function useWorkspaces(sessions: SessionResponse[]): Workspace[] {
  return useMemo(() => {
    const groups = new Map<string, SessionResponse[]>();

    for (const session of sessions) {
      const repoPath = normalizePath(
        session.main_repo_path ?? session.project_path,
      );
      const key = `${repoPath}::${session.branch ?? "__default__"}`;
      const existing = groups.get(key);
      if (existing) {
        existing.push(session);
      } else {
        groups.set(key, [session]);
      }
    }

    const workspaces: Workspace[] = [];

    for (const [id, groupSessions] of groups) {
      const first = groupSessions[0]!;
      const agents = [...new Set(groupSessions.map((s) => s.tool))];
      const status = groupSessions.some((s) => isSessionActive(s))
        ? "active"
        : "idle";

      const branch = first.branch;
      const projectPath = normalizePath(
        first.main_repo_path ?? first.project_path,
      );
      const displayName =
        branch ?? projectPath.split("/").pop() ?? projectPath;

      workspaces.push({
        id,
        branch,
        projectPath,
        displayName,
        agents,
        primaryAgent: agents[0] ?? "",
        status,
        sessions: groupSessions,
      });
    }

    workspaces.sort((a, b) => {
      if (a.status === "active" && b.status !== "active") return -1;
      if (a.status !== "active" && b.status === "active") return 1;
      return 0;
    });

    return workspaces;
  }, [sessions]);
}
