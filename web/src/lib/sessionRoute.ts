// Tiny URL-based routing for the active session. We avoid pulling in a
// router library for one query param. The URL is `?session=<id>` when a
// session is active, or no query when on the dashboard. Notification
// clicks land on `?session=<id>` (see sw.js) and this module is what
// makes that URL actually select the session.

export const SESSION_PARAM = "session";

export function readSessionFromUrl(): string | null {
  if (typeof window === "undefined") return null;
  return new URLSearchParams(window.location.search).get(SESSION_PARAM);
}

export function writeSessionToUrl(sessionId: string | null): void {
  if (typeof window === "undefined") return;
  const url = new URL(window.location.href);
  if (sessionId) {
    url.searchParams.set(SESSION_PARAM, sessionId);
  } else {
    url.searchParams.delete(SESSION_PARAM);
  }
  const next = url.pathname + url.search + url.hash;
  const current =
    window.location.pathname + window.location.search + window.location.hash;
  if (next !== current) {
    window.history.pushState({}, "", next);
  }
}

// Custom event dispatched when an in-app toast (for a focused PWA
// client) is tapped. App listens and selects the session.
export const OPEN_SESSION_EVENT = "aoe-open-session";

export function requestOpenSession(sessionId: string): void {
  if (typeof window === "undefined") return;
  window.dispatchEvent(
    new CustomEvent(OPEN_SESSION_EVENT, { detail: { sessionId } }),
  );
}
