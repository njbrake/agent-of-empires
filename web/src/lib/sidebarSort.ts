import type { Workspace } from "./types";
import { safeGetItem, safeSetItem } from "./safeStorage";

export type SidebarSortMode = "manual" | "lastActivity";

export const SIDEBAR_SORT_MODE_KEY = "aoe-sidebar-sort-mode";

const VALID_MODES: readonly SidebarSortMode[] = ["manual", "lastActivity"];

export function loadSidebarSortMode(): SidebarSortMode {
  const raw = safeGetItem(SIDEBAR_SORT_MODE_KEY);
  if (raw && (VALID_MODES as readonly string[]).includes(raw)) {
    return raw as SidebarSortMode;
  }
  return "manual";
}

export function saveSidebarSortMode(mode: SidebarSortMode): void {
  safeSetItem(SIDEBAR_SORT_MODE_KEY, mode);
}

function epochOr(ts: string | null | undefined): number {
  if (!ts) return Number.NEGATIVE_INFINITY;
  const n = Date.parse(ts);
  return Number.isFinite(n) ? n : Number.NEGATIVE_INFINITY;
}

/** Most-recent activity timestamp across a workspace's sessions, in epoch ms.
 *  Considers `last_accessed_at`, `idle_entered_at`, and `created_at`; nulls
 *  and unparseable strings are skipped. Returns `Number.NEGATIVE_INFINITY`
 *  when no usable timestamp exists. */
export function workspaceLastActivityMs(ws: Workspace): number {
  let best = Number.NEGATIVE_INFINITY;
  for (const s of ws.sessions) {
    const m = Math.max(
      epochOr(s.last_accessed_at),
      epochOr(s.idle_entered_at),
      epochOr(s.created_at),
    );
    if (m > best) best = m;
  }
  return best;
}

/** Group-level activity key: max across the group's workspaces. */
export function repoGroupLastActivityMs(
  workspaces: readonly Workspace[],
): number {
  let best = Number.NEGATIVE_INFINITY;
  for (const ws of workspaces) {
    const m = workspaceLastActivityMs(ws);
    if (m > best) best = m;
  }
  return best;
}

/** Stable, deterministic comparator. Activity descending, tie-break by id
 *  ascending so equal timestamps never flake the render order. The two
 *  activity keys are compared with `<` / `>` rather than subtraction
 *  because workspaces with no usable timestamp return
 *  `Number.NEGATIVE_INFINITY`; `-Infinity - -Infinity` is `NaN`, which
 *  `Array.prototype.sort` treats like `0` (equal) and would silently skip
 *  the id tie-break, leaving ordering at the mercy of input order. */
export function compareWorkspacesByLastActivityDesc(
  a: Workspace,
  b: Workspace,
): number {
  const aMs = workspaceLastActivityMs(a);
  const bMs = workspaceLastActivityMs(b);
  if (aMs < bMs) return 1;
  if (aMs > bMs) return -1;
  return a.id.localeCompare(b.id);
}
