import { useCallback, useMemo, useState } from "react";
import type { Workspace, RepoGroup } from "../lib/types";
import { safeGetItem, safeRemoveItem, safeSetItem } from "../lib/safeStorage";
import {
  applyRepoAppearanceUpdate,
  loadRepoAppearances,
  persistRepoAppearances,
  type RepoAppearanceUpdate,
} from "../lib/repoAppearance";
import {
  compareWorkspacesByLastActivityDesc,
  repoGroupLastActivityMs,
  workspaceTriageTier,
  type SidebarSortMode,
} from "../lib/sidebarSort";

const COLLAPSED_KEY_PREFIX = "aoe-repo-collapsed-";
export const MULTI_REPO_GROUP_ID = "__multi_repo__";
export const SCRATCH_GROUP_ID = "__scratch__";

function loadCollapsed(id: string): boolean {
  return safeGetItem(`${COLLAPSED_KEY_PREFIX}${id}`) === "1";
}

function isMultiRepoWorkspace(ws: Workspace): boolean {
  return ws.sessions.some((s) => (s.workspace_repos?.length ?? 0) > 1);
}

// Scratch sessions live under `<app_dir>/scratch/<id>/`, so bucketing
// by projectPath gives each its own one-session group. Collapse them
// into a synthetic "Scratch" group instead, mirroring the multi-repo
// pattern. Detection keys off `SessionResponse.scratch` (which the
// server already exposes for the recents filter), not the path, so a
// `--keep-scratch` rename or relocation does not break grouping.
function isScratchWorkspace(ws: Workspace): boolean {
  return ws.sessions.some((s) => s.scratch);
}

// Workspaces and their groups both sort by their position in
// `workspaceOrdering` (the persisted user order, prepended by App.tsx
// whenever a new workspace appears). For groups, "position" is the best
// (lowest) rank held by any of the group's workspaces, newest workspace
// in the group pulls the group up. See #1169.
//
// When `sortMode === "lastActivity"` (opt-in, per-browser, #1418), the
// manual rank is bypassed in favour of a recency comparator that keys on
// max(last_accessed_at, idle_entered_at, created_at) across each
// workspace's sessions. The multi-repo group stays pinned to the bottom
// in both modes so its position is predictable across toggles.
export function useRepoGroups(
  workspaces: Workspace[],
  workspaceOrdering: readonly string[] = [],
  sortMode: SidebarSortMode = "manual",
): {
  groups: RepoGroup[];
  toggleRepoCollapsed: (repoId: string) => void;
  updateRepoAppearance: (repoId: string, update: RepoAppearanceUpdate) => void;
} {
  const [collapsedMap, setCollapsedMap] = useState<Record<string, boolean>>({});
  const [appearanceMap, setAppearanceMap] = useState(loadRepoAppearances);

  const groups = useMemo(() => {
    const rank = new Map(workspaceOrdering.map((id, i) => [id, i] as const));
    const rankOf = (id: string) => rank.get(id) ?? Infinity;
    // Triage tier (pinned at top, sunk at bottom) wins over every sort
    // mode, so both rank-based and activity-based comparators apply it
    // first and fall back to their respective within-tier comparison.
    // See #1581.
    const sortByRank = (list: Workspace[]) =>
      [...list].sort((a, b) => {
        const aTier = workspaceTriageTier(a);
        const bTier = workspaceTriageTier(b);
        if (aTier !== bTier) return aTier - bTier;
        // Two unranked workspaces both yield `Infinity`, and
        // `Infinity - Infinity` is `NaN`; `Array.sort` treats NaN
        // like equality and silently skips the tie-break, leaving
        // ordering at the mercy of input order. Compare with `<`/`>`
        // and fall through to a deterministic id tie-break so the
        // render order is stable across re-renders.
        const ar = rankOf(a.id);
        const br = rankOf(b.id);
        if (ar < br) return -1;
        if (ar > br) return 1;
        return a.id.localeCompare(b.id);
      });
    const sortByActivity = (list: Workspace[]) =>
      [...list].sort(compareWorkspacesByLastActivityDesc);
    const sortWorkspaces =
      sortMode === "lastActivity" ? sortByActivity : sortByRank;

    const byRepo = new Map<string, Workspace[]>();
    const multiRepo: Workspace[] = [];
    const scratch: Workspace[] = [];

    for (const ws of workspaces) {
      // Check scratch before multi-repo: a scratch session is
      // single-repo by construction (no worktrees, no extra repos), so
      // the order is defensive rather than load-bearing, but it makes
      // the precedence explicit if someone later widens scratch to
      // allow extras.
      if (isScratchWorkspace(ws)) {
        scratch.push(ws);
        continue;
      }
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
      const sorted = sortWorkspaces(repoWorkspaces);
      const hasActive = sorted.some((ws) => ws.status === "active");
      const collapsed = collapsedMap[repoPath] ?? loadCollapsed(repoPath);
      const remoteOwner = sorted[0]?.sessions[0]?.remote_owner ?? null;
      const appearance = appearanceMap[repoPath];
      const defaultDisplayName = repoPath.split("/").pop() ?? repoPath;

      repoGroups.push({
        id: repoPath,
        repoPath,
        displayName: appearance?.alias ?? defaultDisplayName,
        defaultDisplayName,
        alias: appearance?.alias ?? null,
        color: appearance?.color ?? null,
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
      const appearance = appearanceMap[MULTI_REPO_GROUP_ID];
      const defaultDisplayName = "Multi-repo";
      repoGroups.push({
        id: MULTI_REPO_GROUP_ID,
        repoPath: MULTI_REPO_GROUP_ID,
        displayName: appearance?.alias ?? defaultDisplayName,
        defaultDisplayName,
        alias: appearance?.alias ?? null,
        color: appearance?.color ?? null,
        remoteOwner: null,
        workspaces: sorted,
        status: hasActive ? "active" : "idle",
        collapsed,
      });
    }

    if (scratch.length > 0) {
      const sorted = sortWorkspaces(scratch);
      const hasActive = sorted.some((ws) => ws.status === "active");
      const collapsed =
        collapsedMap[SCRATCH_GROUP_ID] ?? loadCollapsed(SCRATCH_GROUP_ID);
      const appearance = appearanceMap[SCRATCH_GROUP_ID];
      const defaultDisplayName = "Scratch";
      repoGroups.push({
        id: SCRATCH_GROUP_ID,
        repoPath: SCRATCH_GROUP_ID,
        displayName: appearance?.alias ?? defaultDisplayName,
        defaultDisplayName,
        alias: appearance?.alias ?? null,
        color: appearance?.color ?? null,
        remoteOwner: null,
        workspaces: sorted,
        status: hasActive ? "active" : "idle",
        collapsed,
      });
    }

    repoGroups.sort((a, b) => {
      // Synthetic groups pin to the bottom in a stable order:
      // real repos → multi-repo → scratch. Scratch is "most ad hoc"
      // so it sits below multi-repo workspaces (which still
      // represent real work).
      if (a.id === SCRATCH_GROUP_ID) return 1;
      if (b.id === SCRATCH_GROUP_ID) return -1;
      if (a.id === MULTI_REPO_GROUP_ID) return 1;
      if (b.id === MULTI_REPO_GROUP_ID) return -1;
      if (sortMode === "lastActivity") {
        const ak = repoGroupLastActivityMs(a.workspaces);
        const bk = repoGroupLastActivityMs(b.workspaces);
        if (ak !== bk) return bk - ak;
        return a.repoPath.localeCompare(b.repoPath);
      }
      const am = Math.min(...a.workspaces.map((w) => rankOf(w.id)));
      const bm = Math.min(...b.workspaces.map((w) => rankOf(w.id)));
      if (am !== bm) return am - bm;
      return a.repoPath.localeCompare(b.repoPath);
    });

    return repoGroups;
  }, [workspaces, workspaceOrdering, sortMode, collapsedMap, appearanceMap]);

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

  const updateRepoAppearance = useCallback(
    (repoId: string, update: RepoAppearanceUpdate) => {
      setAppearanceMap((prev) => {
        const next = applyRepoAppearanceUpdate(prev, repoId, update);
        persistRepoAppearances(next);
        return next;
      });
    },
    [],
  );

  return { groups, toggleRepoCollapsed, updateRepoAppearance };
}
