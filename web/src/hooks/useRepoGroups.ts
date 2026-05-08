import { useCallback, useMemo, useState } from "react";
import type { Workspace, RepoGroup } from "../lib/types";

const COLLAPSED_KEY_PREFIX = "aoe-repo-collapsed-";
export const MULTI_REPO_GROUP_ID = "__multi_repo__";

function loadCollapsed(id: string): boolean {
  try {
    return localStorage.getItem(`${COLLAPSED_KEY_PREFIX}${id}`) === "1";
  } catch {
    return false;
  }
}

function isMultiRepoWorkspace(ws: Workspace): boolean {
  return ws.sessions.some((s) => (s.workspace_repos?.length ?? 0) > 1);
}

export function useRepoGroups(workspaces: Workspace[]): {
  groups: RepoGroup[];
  toggleRepoCollapsed: (repoId: string) => void;
} {
  const [collapsedMap, setCollapsedMap] = useState<Record<string, boolean>>({});

  const groups = useMemo(() => {
    const byRepo = new Map<string, Workspace[]>();
    const multiRepo: Workspace[] = [];

    for (const ws of workspaces) {
      if (isMultiRepoWorkspace(ws)) {
        multiRepo.push(ws);
        continue;
      }
      const existing = byRepo.get(ws.projectPath);
      if (existing) {
        existing.push(ws);
      } else {
        byRepo.set(ws.projectPath, [ws]);
      }
    }

    const sortWorkspaces = (list: Workspace[]) =>
      [...list].sort((a, b) => {
        if (a.status === "active" && b.status !== "active") return -1;
        if (a.status !== "active" && b.status === "active") return 1;
        const aName = a.branch ?? "";
        const bName = b.branch ?? "";
        return aName.localeCompare(bName) || a.id.localeCompare(b.id);
      });

    const repoGroups: RepoGroup[] = [];

    for (const [repoPath, repoWorkspaces] of byRepo) {
      const sorted = sortWorkspaces(repoWorkspaces);
      const hasActive = sorted.some((ws) => ws.status === "active");
      const collapsed =
        collapsedMap[repoPath] ?? loadCollapsed(repoPath);

      const remoteOwner =
        sorted[0]?.sessions[0]?.remote_owner ?? null;

      repoGroups.push({
        id: repoPath,
        repoPath,
        displayName: repoPath.split("/").pop() ?? repoPath,
        remoteOwner,
        workspaces: sorted,
        status: hasActive ? "active" : "idle",
        collapsed,
      });
    }

    if (multiRepo.length > 0) {
      const sorted = sortWorkspaces(multiRepo);
      const hasActive = sorted.some((ws) => ws.status === "active");
      const collapsed =
        collapsedMap[MULTI_REPO_GROUP_ID] ?? loadCollapsed(MULTI_REPO_GROUP_ID);
      repoGroups.push({
        id: MULTI_REPO_GROUP_ID,
        repoPath: MULTI_REPO_GROUP_ID,
        displayName: "Multi-repo",
        remoteOwner: null,
        workspaces: sorted,
        status: hasActive ? "active" : "idle",
        collapsed,
      });
    }

    repoGroups.sort((a, b) => {
      if (a.id === MULTI_REPO_GROUP_ID) return 1;
      if (b.id === MULTI_REPO_GROUP_ID) return -1;
      if (a.status === "active" && b.status !== "active") return -1;
      if (a.status !== "active" && b.status === "active") return 1;
      return a.displayName.localeCompare(b.displayName) || a.repoPath.localeCompare(b.repoPath);
    });

    return repoGroups;
  }, [workspaces, collapsedMap]);

  const toggleRepoCollapsed = useCallback((repoId: string) => {
    setCollapsedMap((prev) => {
      const current = prev[repoId] ?? loadCollapsed(repoId);
      const next = !current;
      try {
        if (next) {
          localStorage.setItem(`${COLLAPSED_KEY_PREFIX}${repoId}`, "1");
        } else {
          localStorage.removeItem(`${COLLAPSED_KEY_PREFIX}${repoId}`);
        }
      } catch {
        // ignore
      }
      return { ...prev, [repoId]: next };
    });
  }, []);

  return { groups, toggleRepoCollapsed };
}
