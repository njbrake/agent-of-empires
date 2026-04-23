import { useCallback, useSyncExternalStore } from "react";

const STORAGE_KEY = "aoe-web-settings";

export interface WebSettings {
  mobileFontSize: number;
  desktopFontSize: number;
  autoOpenKeyboard: boolean;
  diffViewMode: "flat" | "tree";
  collapsedDiffDirs: string[];
}

const DEFAULTS: WebSettings = {
  mobileFontSize: 8,
  desktopFontSize: 14,
  autoOpenKeyboard: true,
  diffViewMode: "flat",
  collapsedDiffDirs: [],
};

function getSnapshot(): WebSettings {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (raw) return { ...DEFAULTS, ...JSON.parse(raw) };
  } catch {
    // ignore
  }
  return DEFAULTS;
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
let cachedSettings: WebSettings = DEFAULTS;

function getStableSnapshot(): WebSettings {
  const raw = localStorage.getItem(STORAGE_KEY);
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
    try {
      localStorage.setItem(STORAGE_KEY, JSON.stringify(next));
    } catch (err) {
      console.warn("aoe-web-settings: failed to persist", err);
    }
    cachedRaw = null;
    emitChange();
  }, []);

  return { settings, update };
}
