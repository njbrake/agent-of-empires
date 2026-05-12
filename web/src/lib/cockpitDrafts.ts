// Cockpit composer drafts live in localStorage under one key per session
// (`cockpit:draft:<session_id>`). This module centralises the storage
// shape and exposes a tiny pub/sub so non-composer UI (e.g. the sidebar
// "unsent draft" dot) can react to writes from any tab.

import { useMemo, useSyncExternalStore } from "react";

const DRAFT_KEY_PREFIX = "cockpit:draft:";

function draftKey(sessionId: string): string {
  return `${DRAFT_KEY_PREFIX}${sessionId}`;
}

function sessionIdFromKey(key: string): string | null {
  if (!key.startsWith(DRAFT_KEY_PREFIX)) return null;
  return key.slice(DRAFT_KEY_PREFIX.length);
}

type Listener = () => void;

// Each listener may register an optional id filter. When present, the
// listener only fires for changes to a draft whose session id is in the
// set; null means "fire for any draft change" (and for cross-tab
// `localStorage.clear()`, where we don't know which keys went away).
const localListeners = new Map<Listener, ReadonlySet<string> | null>();

function notify(sessionId: string | null) {
  for (const [cb, filter] of localListeners) {
    if (filter === null || sessionId === null || filter.has(sessionId)) cb();
  }
}

export function getDraft(sessionId: string): string {
  try {
    return localStorage.getItem(draftKey(sessionId)) ?? "";
  } catch {
    return "";
  }
}

export function setDraft(sessionId: string, text: string): void {
  try {
    if (text.length === 0) {
      localStorage.removeItem(draftKey(sessionId));
    } else {
      localStorage.setItem(draftKey(sessionId), text);
    }
  } catch {
    /* localStorage blocked / quota; persistence is best-effort */
  }
  notify(sessionId);
}

export function hasDraft(sessionId: string): boolean {
  try {
    const v = localStorage.getItem(draftKey(sessionId));
    return v !== null && v.length > 0;
  } catch {
    return false;
  }
}

// Subscribe to draft changes. `filter` scopes the listener to a specific
// set of session ids; pass null/undefined to receive every draft change.
// Fires for writes in the current tab (manually emitted) and for writes
// in other tabs (storage event). Returns an unsubscribe function.
export function subscribeDrafts(
  cb: Listener,
  filter: ReadonlySet<string> | null = null,
): () => void {
  localListeners.set(cb, filter);
  const onStorage = (e: StorageEvent) => {
    // e.key is null when localStorage.clear() is called from another
    // tab; treat that as "everything changed" and unconditionally fire.
    if (e.key === null) {
      cb();
      return;
    }
    const sid = sessionIdFromKey(e.key);
    if (sid === null) return;
    if (filter === null || filter.has(sid)) cb();
  };
  window.addEventListener("storage", onStorage);
  return () => {
    localListeners.delete(cb);
    window.removeEventListener("storage", onStorage);
  };
}

// Returns true when ANY of the given session ids has a non-empty draft.
// Re-renders the calling component only when one of THESE ids changes,
// not on every cockpit draft write anywhere in the app.
export function useHasDraftForSessions(sessionIds: readonly string[]): boolean {
  // Stable join key so getSnapshot returns the same primitive across
  // renders unless the relevant drafts actually change — otherwise
  // useSyncExternalStore would tear under React 18's strict checks.
  const ids = sessionIds.join("|");
  const subscribe = useMemo(() => {
    const filter = new Set(ids ? ids.split("|").filter(Boolean) : []);
    return (cb: Listener) => subscribeDrafts(cb, filter);
  }, [ids]);
  return useSyncExternalStore(
    subscribe,
    () => {
      for (const id of ids ? ids.split("|") : []) {
        if (id && hasDraft(id)) return true;
      }
      return false;
    },
    () => false,
  );
}
