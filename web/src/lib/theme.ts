// Resolved theme types + runtime applicator. The server (Rust) owns
// the projection from canonical TUI Theme -> CSS variables; this file
// just consumes the typed payload from /api/themes/:name and applies
// it on the root element via document.documentElement.style.setProperty,
// then mirrors the payload into localStorage so the next page load
// can paint the right palette before the React app hydrates.

export type ThemeAppearance = "dark" | "light";

export type ResolvedThemeSource = "builtin" | "custom" | "fallback";

export interface CssVarProjection {
  cssVars: Record<string, string>;
}

export interface ResolvedTheme {
  name: string;
  source: ResolvedThemeSource;
  appearance: ThemeAppearance;
  web: CssVarProjection;
  terminal: CssVarProjection;
  syntax: { shikiTheme: string };
}

const STORAGE_KEY = "aoe-resolved-theme";

export function readCachedResolvedTheme(): ResolvedTheme | null {
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    if (!raw) return null;
    return JSON.parse(raw) as ResolvedTheme;
  } catch {
    return null;
  }
}

function writeCachedResolvedTheme(theme: ResolvedTheme): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(theme));
  } catch {
    // Quota / private mode; safe to ignore.
  }
}

// Apply the resolved theme to the document root. Uses setProperty
// (not a dynamic <style> tag) so no CSP allowance is needed and
// Tailwind v4 utilities that reference the same variable names
// repaint immediately.
export function applyResolvedTheme(theme: ResolvedTheme): void {
  const root = document.documentElement;
  for (const [name, value] of Object.entries(theme.web.cssVars)) {
    root.style.setProperty(name, value);
  }
  for (const [name, value] of Object.entries(theme.terminal.cssVars)) {
    root.style.setProperty(name, value);
  }
  root.dataset.theme = theme.name;
  root.dataset.themeAppearance = theme.appearance;
  root.style.colorScheme = theme.appearance;
  writeCachedResolvedTheme(theme);
}

// Notification key used by the theme hook to broadcast theme changes
// across components (e.g. shiki call sites re-render against the new
// syntax theme without needing to subscribe to a context).
export const THEME_CHANGED_EVENT = "aoe:theme-changed";

export function dispatchThemeChanged(theme: ResolvedTheme): void {
  window.dispatchEvent(
    new CustomEvent<ResolvedTheme>(THEME_CHANGED_EVENT, { detail: theme }),
  );
}
