import { useEffect, useState } from "react";
import {
  readCachedResolvedTheme,
  THEME_CHANGED_EVENT,
  type ResolvedTheme,
} from "../lib/theme";
import { DEFAULT_SHIKI_THEME } from "../lib/highlighter";

/** Current Shiki theme name from the resolved theme. Updates when the
 *  user picks a new theme (via the THEME_CHANGED_EVENT broadcast from
 *  useResolvedTheme). Components that highlight code should put the
 *  returned value in their effect dependency list so blocks re-render
 *  against the new theme. */
export function useShikiTheme(): string {
  const [theme, setTheme] = useState<string>(() => {
    const cached = readCachedResolvedTheme();
    return cached?.syntax.shikiTheme ?? DEFAULT_SHIKI_THEME;
  });
  useEffect(() => {
    const onChange = (event: Event) => {
      const next = (event as CustomEvent<ResolvedTheme>).detail;
      if (next?.syntax.shikiTheme) setTheme(next.syntax.shikiTheme);
    };
    window.addEventListener(THEME_CHANGED_EVENT, onChange);
    return () => {
      window.removeEventListener(THEME_CHANGED_EVENT, onChange);
    };
  }, []);
  return theme;
}
