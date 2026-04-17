import { useCallback, useMemo, useState } from "react";
import type { Workspace, RepoGroup } from "../lib/types";

const COLLAPSED_KEY_PREFIX = "aoe-repo-collapsed-";

function loadCollapsed(id: string): boolean {
  try {
    return localStorage.getItem(`${COLLAPSED_KEY_PREFIX}${id}`) === "1";
  } catch {
    return false;
  }
}

export function useRepoGroups(workspaces: Workspace[]): {
  groups: RepoGroup[];
  toggleRepoCollapsed: (repoId: string) => void;
} {
  const [collapsedMap, setCollapsedMap] = useState<Record<string, boolean>>({});

  const groups = useMemo(() => {
    const byRepo = new Map<string, Workspace[]>();

    for (const ws of workspaces) {
      const existing = byRepo.get(ws.projectPath);
      if (existing) {
        existing.push(ws);
      } else {
        byRepo.set(ws.projectPath, [ws]);
      }
    }

    const repoGroups: RepoGroup[] = [];

    for (const [repoPath, repoWorkspaces] of byRepo) {
      const sorted = [...repoWorkspaces].sort((a, b) => {
        if (a.status === "active" && b.status !== "active") return -1;
        if (a.status !== "active" && b.status === "active") return 1;
        const aName = a.branch ?? "";
        const bName = b.branch ?? "";
        return aName.localeCompare(bName) || a.id.localeCompare(b.id);
      });

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

    repoGroups.sort((a, b) => {
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
