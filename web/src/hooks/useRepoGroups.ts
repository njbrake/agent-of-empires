import { useCallback, useMemo, useState } from "react";
import type { Workspace, RepoGroup } from "../lib/types";
import { safeGetItem, safeRemoveItem, safeSetItem } from "../lib/safeStorage";

const COLLAPSED_KEY_PREFIX = "aoe-repo-collapsed-";
export const MULTI_REPO_GROUP_ID = "__multi_repo__";

function loadCollapsed(id: string): boolean {
  return safeGetItem(`${COLLAPSED_KEY_PREFIX}${id}`) === "1";
}

function isMultiRepoWorkspace(ws: Workspace): boolean {
  return ws.sessions.some((s) => (s.workspace_repos?.length ?? 0) > 1);
}

// Workspaces and their groups both sort by their position in
// `workspaceOrdering` (the persisted user order, prepended by App.tsx
// whenever a new workspace appears). For groups, "position" is the best
// (lowest) rank held by any of the group's workspaces — newest workspace
// in the group pulls the group up. See #1169.
export function useRepoGroups(
  workspaces: Workspace[],
  workspaceOrdering: readonly string[] = [],
): {
  groups: RepoGroup[];
  toggleRepoCollapsed: (repoId: string) => void;
} {
  const [collapsedMap, setCollapsedMap] = useState<Record<string, boolean>>({});

  const groups = useMemo(() => {
    const rank = new Map(workspaceOrdering.map((id, i) => [id, i] as const));
    const rankOf = (id: string) => rank.get(id) ?? Infinity;
    const sortByRank = (list: Workspace[]) =>
      [...list].sort((a, b) => rankOf(a.id) - rankOf(b.id));

    const byRepo = new Map<string, Workspace[]>();
    const multiRepo: Workspace[] = [];

    for (const ws of workspaces) {
      if (isMultiRepoWorkspace(ws)) {
        multiRepo.push(ws);
        continue;
      }
      const existing = byRepo.get(ws.projectPath);
      if (existing) existing.push(ws);
      else byRepo.set(ws.projectPath, [ws]);
    }

    const repoGroups: RepoGroup[] = [];

    for (const [repoPath, repoWorkspaces] of byRepo) {
      const sorted = sortByRank(repoWorkspaces);
      const hasActive = sorted.some((ws) => ws.status === "active");
      const collapsed = collapsedMap[repoPath] ?? loadCollapsed(repoPath);
      const remoteOwner = sorted[0]?.sessions[0]?.remote_owner ?? null;

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
      const sorted = sortByRank(multiRepo);
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
      const am = Math.min(...a.workspaces.map((w) => rankOf(w.id)));
      const bm = Math.min(...b.workspaces.map((w) => rankOf(w.id)));
      if (am !== bm) return am - bm;
      return a.repoPath.localeCompare(b.repoPath);
    });

    return repoGroups;
  }, [workspaces, workspaceOrdering, collapsedMap]);

  const toggleRepoCollapsed = useCallback((repoId: string) => {
    setCollapsedMap((prev) => {
      const current = prev[repoId] ?? loadCollapsed(repoId);
      const next = !current;
      if (next) {
        safeSetItem(`${COLLAPSED_KEY_PREFIX}${repoId}`, "1");
      } else {
        safeRemoveItem(`${COLLAPSED_KEY_PREFIX}${repoId}`);
      }
      return { ...prev, [repoId]: next };
    });
  }, []);

  return { groups, toggleRepoCollapsed };
}
