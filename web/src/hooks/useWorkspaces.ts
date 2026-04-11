import { useMemo } from "react";
import type {
  SessionResponse,
  Workspace,
  WorkspaceStatus,
} from "../lib/types";
import { isSessionActive } from "../lib/session";

const LIFECYCLE_STORAGE_KEY = "aoe-workspace-lifecycle";

/** Strip trailing slashes for consistent grouping */
function normalizePath(p: string): string {
  return p.replace(/\/+$/, "");
}

function getLifecycleOverrides(): Record<string, WorkspaceStatus> {
  try {
    const raw = localStorage.getItem(LIFECYCLE_STORAGE_KEY);
    return raw ? JSON.parse(raw) : {};
  } catch {
    return {};
  }
}

export function setLifecycleOverride(
  workspaceId: string,
  status: WorkspaceStatus | null,
) {
  const overrides = getLifecycleOverrides();
  if (status === null) {
    delete overrides[workspaceId];
  } else {
    overrides[workspaceId] = status;
  }
  localStorage.setItem(LIFECYCLE_STORAGE_KEY, JSON.stringify(overrides));
}

function deriveStatus(sessions: SessionResponse[]): "active" | "idle" {
  return sessions.some((s) => isSessionActive(s.status)) ? "active" : "idle";
}

export function useWorkspaces(sessions: SessionResponse[]): Workspace[] {
  return useMemo(() => {
    const overrides = getLifecycleOverrides();
    const groups = new Map<string, SessionResponse[]>();

    for (const session of sessions) {
      const path = normalizePath(session.project_path);
      const key = `${path}::${session.branch ?? "__default__"}`;
      const existing = groups.get(key);
      if (existing) {
        existing.push(session);
      } else {
        groups.set(key, [session]);
      }
    }

    const workspaces: Workspace[] = [];

    for (const [id, groupSessions] of groups) {
      const first = groupSessions[0];
      const agents = [...new Set(groupSessions.map((s) => s.tool))];
      const computedStatus = deriveStatus(groupSessions);
      const override = overrides[id];

      // Active session status overrides manual reviewing/archived
      let status: WorkspaceStatus;
      if (computedStatus === "active") {
        status = "active";
      } else if (override === "reviewing" || override === "archived") {
        status = override;
      } else {
        status = computedStatus;
      }

      const branch = first.branch;
      const projectPath = normalizePath(first.project_path);
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

    // Sort: active first, then idle, then reviewing, then archived
    const order: Record<WorkspaceStatus, number> = {
      active: 0,
      idle: 1,
      reviewing: 2,
      archived: 3,
    };
    workspaces.sort((a, b) => order[a.status] - order[b.status]);

    return workspaces;
  }, [sessions]);
}
