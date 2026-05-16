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

export function groupCreatedAt(workspaces: Workspace[]): string {
  let earliest = "";
  for (const ws of workspaces) {
    const k = workspaceCreatedAt(ws);
    if (!k) continue;
    if (!earliest || k < earliest) earliest = k;
  }
  return earliest;
}

// Stable comparator. Sorts by workspace birth descending (newest at top),
// with `id` as the tiebreak. Never references `status` or any
// last-accessed timestamp, so a session flipping active/idle does not
// reshuffle the list. See #1169.
//
// This is a derived order, not a user-configurable one. The follow-up
// work for drag-to-reorder (#TODO) will introduce a server-side
// `display_order` field; this comparator will then become the fallback
// for workspaces without an explicit order rather than the primary key.
export function compareWorkspaces(a: Workspace, b: Workspace): number {
  const ak = workspaceCreatedAt(a);
  const bk = workspaceCreatedAt(b);
  if (ak !== bk) return bk.localeCompare(ak);
  return a.id.localeCompare(b.id);
}
