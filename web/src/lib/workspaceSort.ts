import type { Workspace } from "./types";

// Workspace birth = earliest `created_at` among the workspace's sessions.
// ISO-8601 strings sort chronologically by lexicographic compare. Returns
// "" when no session carries a `created_at` (a server bug; defensive only),
// which sorts to the bottom of a "newest first" list.
export function workspaceCreatedAt(ws: Workspace): string {
  let earliest = "";
  for (const s of ws.sessions) {
    const c = s.created_at ?? "";
    if (!c) continue;
    if (!earliest || c < earliest) earliest = c;
  }
  return earliest;
}

// Compares two workspaces by the derived "birth" key. Newest first, with
// `id` as the tiebreak. Used as the fallback when neither workspace has an
// explicit position in the user-defined ordering.
export function compareByBirth(a: Workspace, b: Workspace): number {
  const ak = workspaceCreatedAt(a);
  const bk = workspaceCreatedAt(b);
  if (ak !== bk) return bk.localeCompare(ak);
  return a.id.localeCompare(b.id);
}

// `workspaceOrdering` is the user-configured display order, persisted
// server-side via `/api/workspace-ordering` so it syncs across devices.
// IDs present in the ordering pin to their rank (lower index sorts
// first); IDs absent fall back to `compareByBirth` and sort after every
// pinned row. See #1169.
export function makeCompareWorkspaces(
  workspaceOrdering: readonly string[],
): (a: Workspace, b: Workspace) => number {
  const rank = new Map<string, number>();
  workspaceOrdering.forEach((id, i) => rank.set(id, i));

  return (a, b) => {
    const ar = rank.get(a.id);
    const br = rank.get(b.id);
    if (ar !== undefined && br !== undefined) return ar - br;
    if (ar !== undefined) return -1;
    if (br !== undefined) return 1;
    return compareByBirth(a, b);
  };
}

// Earliest workspace birth across the group, used for repo-group sorting.
// Group ordering is not yet user-configurable; that's a separate change
// once per-workspace ordering has proven out.
export function groupCreatedAt(workspaces: Workspace[]): string {
  let earliest = "";
  for (const ws of workspaces) {
    const k = workspaceCreatedAt(ws);
    if (!k) continue;
    if (!earliest || k < earliest) earliest = k;
  }
  return earliest;
}
