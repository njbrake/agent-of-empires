import { useEffect, useState } from "react";
import { fetchCurrentTheme, fetchResolvedTheme } from "../lib/api";
import {
  applyResolvedTheme,
  dispatchThemeChanged,
  readCachedResolvedTheme,
  type ResolvedTheme,
} from "../lib/theme";

/** Event name fired by the settings UI after the user picks a new
 *  theme. The hook listens for it and refetches /api/theme/current
 *  (or, if the event carries a `detail.name`, /api/themes/:name) so
 *  the dashboard repaints without a settings round-trip. */
export const THEME_PICKER_CHANGED_EVENT = "aoe:theme-picker-changed";

export interface ThemePickerChangedDetail {
  /** New theme name selected by the user. Optional: when omitted the
   *  hook refetches /api/theme/current. */
  name?: string;
}

/** Apply the user's selected theme on mount and on settings updates.
 *  Reads the cached payload from localStorage first to prevent FOUC,
 *  then fetches the authoritative payload and applies it. */
export function useResolvedTheme(): ResolvedTheme | null {
  const [theme, setTheme] = useState<ResolvedTheme | null>(() =>
    readCachedResolvedTheme(),
  );

  useEffect(() => {
    let cancelled = false;
    fetchCurrentTheme().then((next) => {
      if (cancelled || !next) return;
      applyResolvedTheme(next);
      dispatchThemeChanged(next);
      setTheme(next);
    });

    const onChange = (event: Event) => {
      const detail = (event as CustomEvent<ThemePickerChangedDetail>).detail;
      const promise = detail?.name
        ? fetchResolvedTheme(detail.name)
        : fetchCurrentTheme();
      promise.then((next) => {
        if (!next) return;
        applyResolvedTheme(next);
        dispatchThemeChanged(next);
        setTheme(next);
      });
    };
    window.addEventListener(THEME_PICKER_CHANGED_EVENT, onChange);
    return () => {
      cancelled = true;
      window.removeEventListener(THEME_PICKER_CHANGED_EVENT, onChange);
    };
  }, []);

  return theme;
}

/** Fire from the settings UI after the user picks a new theme. */
export function dispatchThemePickerChanged(name?: string): void {
  window.dispatchEvent(
    new CustomEvent<ThemePickerChangedDetail>(THEME_PICKER_CHANGED_EVENT, {
      detail: { name },
    }),
  );
}
