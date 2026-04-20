/**
 * Tracks whether the backend server is reachable. When the connection is known
 * to be down, the fetch interceptor suppresses per-request "network error"
 * toasts so the user sees one clear disconnect banner instead of a toast flood.
 *
 * The session poller (`useSessions`) is the source of truth: it hits
 * `/api/sessions` every 3s and calls `setServerDown(true/false)` based on
 * whether that request succeeds.
 */

let serverDown = false;
const listeners = new Set<(down: boolean) => void>();

export function setServerDown(down: boolean): void {
  if (serverDown === down) return;
  serverDown = down;
  for (const fn of listeners) fn(down);
}

export function isServerDown(): boolean {
  return serverDown;
}

export function onServerDownChange(fn: (down: boolean) => void): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}
