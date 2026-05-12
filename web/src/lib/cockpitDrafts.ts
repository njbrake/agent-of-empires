// Cockpit composer drafts live in localStorage under one key per session
// (`cockpit:draft:<session_id>`). This module centralises the storage
// shape and exposes a tiny pub/sub so non-composer UI (e.g. the sidebar
// "unsent draft" dot) can react to writes from any tab.

import { useSyncExternalStore } from "react";

const DRAFT_KEY_PREFIX = "cockpit:draft:";

function draftKey(sessionId: string): string {
  return `${DRAFT_KEY_PREFIX}${sessionId}`;
}

type Listener = () => void;

const localListeners = new Set<Listener>();

function emitLocal() {
  for (const cb of localListeners) cb();
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
  emitLocal();
}

export function hasDraft(sessionId: string): boolean {
  try {
    const v = localStorage.getItem(draftKey(sessionId));
    return v !== null && v.length > 0;
  } catch {
    return false;
  }
}

// Subscribe to draft changes. Fires for writes in the current tab
// (manually emitted) and for writes in other tabs (storage event).
// Returns an unsubscribe function.
export function subscribeDrafts(cb: Listener): () => void {
  localListeners.add(cb);
  const onStorage = (e: StorageEvent) => {
    // e.key is null when localStorage.clear() is called from another
    // tab; treat that as "everything changed" and re-read.
    if (e.key === null || e.key.startsWith(DRAFT_KEY_PREFIX)) cb();
  };
  window.addEventListener("storage", onStorage);
  return () => {
    localListeners.delete(cb);
    window.removeEventListener("storage", onStorage);
  };
}

// Returns true when ANY of the given session ids has a non-empty draft.
// Re-renders the calling component whenever drafts change.
export function useHasDraftForSessions(sessionIds: readonly string[]): boolean {
  // Pre-join the ids into a stable key so getSnapshot returns the same
  // primitive across renders unless drafts actually change. Otherwise
  // useSyncExternalStore would tear under React 18's strict checks.
  const ids = sessionIds.join("|");
  return useSyncExternalStore(
    subscribeDrafts,
    () => {
      for (const id of ids ? ids.split("|") : []) {
        if (id && hasDraft(id)) return true;
      }
      return false;
    },
    () => false,
  );
}
