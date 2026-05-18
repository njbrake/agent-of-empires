import type { Workspace } from "./types";

// Sort workspaces by the user-configured ordering (persisted server-side
// via `/api/workspace-ordering` so it syncs across devices), with the
// workspace's earliest session `created_at` descending as the fallback
// for ids the user hasn't placed (newest at the top). `id` is the final
// tiebreak so identical-`created_at` rows have a deterministic order.
// See #1169.
export function sortWorkspaces(
  workspaces: readonly Workspace[],
  ordering: readonly string[],
): Workspace[] {
  const rank = new Map(ordering.map((id, i) => [id, i]));
  return [...workspaces].sort((a, b) => {
    const ar = rank.get(a.id) ?? Infinity;
    const br = rank.get(b.id) ?? Infinity;
    if (ar !== br) return ar - br;
    const ak = workspaceCreatedAt(a);
    const bk = workspaceCreatedAt(b);
    if (ak !== bk) return bk.localeCompare(ak);
    return a.id.localeCompare(b.id);
  });
}

export function groupCreatedAt(workspaces: Workspace[]): string {
  let earliest = "";
  for (const ws of workspaces) {
    const k = workspaceCreatedAt(ws);
    if (k && (!earliest || k < earliest)) earliest = k;
  }
  return earliest;
}

function workspaceCreatedAt(ws: Workspace): string {
  let earliest = "";
  for (const s of ws.sessions) {
    const c = s.created_at ?? "";
    if (c && (!earliest || c < earliest)) earliest = c;
  }
  return earliest;
}
