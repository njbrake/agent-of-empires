import { useCallback, useSyncExternalStore } from "react";

import { safeGetItem, safeSetItem } from "../lib/safeStorage";

const STORAGE_KEY = "aoe-web-settings";

export interface WebSettings {
  mobileFontSize: number;
  desktopFontSize: number;
  autoOpenKeyboard: boolean;
  diffViewMode: "flat" | "tree";
  collapsedDiffDirs: string[];
}

function getDefaults(): WebSettings {
  return {
    mobileFontSize: 8,
    desktopFontSize: 14,
    autoOpenKeyboard: true,
    diffViewMode: window.innerWidth < 768 ? "flat" : "tree",
    collapsedDiffDirs: [],
  };
}

function getSnapshot(): WebSettings {
  const raw = safeGetItem(STORAGE_KEY);
  if (raw) {
    try {
      return { ...getDefaults(), ...JSON.parse(raw) };
    } catch {
      // malformed JSON; fall through to defaults
    }
  }
  return getDefaults();
}

// Subscribers for useSyncExternalStore
let listeners: Array<() => void> = [];

function subscribe(listener: () => void) {
  listeners = [...listeners, listener];
  return () => {
    listeners = listeners.filter((l) => l !== listener);
  };
}

function emitChange() {
  for (const l of listeners) l();
}

// Cache snapshot to return stable reference when nothing changed
let cachedRaw: string | null = null;
let cachedSettings: WebSettings = getDefaults();

function getStableSnapshot(): WebSettings {
  const raw = safeGetItem(STORAGE_KEY);
  if (raw !== cachedRaw) {
    cachedRaw = raw;
    cachedSettings = getSnapshot();
  }
  return cachedSettings;
}

export function useWebSettings() {
  const settings = useSyncExternalStore(subscribe, getStableSnapshot);

  const update = useCallback((patch: Partial<WebSettings>) => {
    const current = getSnapshot();
    const next = { ...current, ...patch };
    if (!safeSetItem(STORAGE_KEY, JSON.stringify(next))) {
      console.warn("aoe-web-settings: failed to persist (storage full or disabled)");
    }
    cachedRaw = null;
    emitChange();
  }, []);

  return { settings, update };
}
