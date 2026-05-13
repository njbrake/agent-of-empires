// Cockpit display preferences sourced from the daemon's resolved
// `[cockpit]` config (config.toml). Single source of truth: the
// `/api/about` endpoint exposes the resolved active-profile values as
// `ServerAbout.cockpit_*`. App.tsx fetches that on mount and
// republishes the relevant slice through this context so any cockpit
// renderer (deeply-nested tool cards in particular) can subscribe
// without prop-drilling.
//
// Cross-device by construction: every browser pointed at the same
// daemon reads the same value. Toggling from the web Settings panel
// rewrites config.toml via `PATCH /api/profiles/:name/settings`, then
// `App.refreshServerAbout()` re-fetches `/api/about` and the context
// repopulates.

import { createContext, useContext, type ReactNode } from "react";

export interface CockpitPrefs {
  /** Resolved `cockpit.show_tool_durations` from the active profile.
   *  When true, tool-card headers display a per-call elapsed-time
   *  label. Imprecise on claude-agent-acp today; see
   *  `CardChromeProps.startedAt` in ToolCards.tsx for the upstream
   *  limitation. */
  showToolDurations: boolean;
  /** Resolved `cockpit.queue_drain_mode` from the active profile. Selects
   *  how the composer drains client-side queued follow-up prompts on
   *  Stopped: `combined` (default) joins them with blank lines into a
   *  single prompt; `serial` fires one entry at a time. See #1031. */
  queueDrainMode: "combined" | "serial";
}

const DEFAULT_PREFS: CockpitPrefs = {
  showToolDurations: true,
  queueDrainMode: "combined",
};

const CockpitPrefsContext = createContext<CockpitPrefs>(DEFAULT_PREFS);

export function CockpitPrefsProvider({
  value,
  children,
}: {
  value: CockpitPrefs;
  children: ReactNode;
}) {
  return (
    <CockpitPrefsContext.Provider value={value}>
      {children}
    </CockpitPrefsContext.Provider>
  );
}

export function useCockpitPrefs(): CockpitPrefs {
  return useContext(CockpitPrefsContext);
}
