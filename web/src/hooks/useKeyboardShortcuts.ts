import { useEffect } from "react";

const IS_MAC =
  typeof navigator !== "undefined" &&
  /Mac|iPhone|iPad|iPod/.test(navigator.platform);

interface ShortcutActions {
  onNew: () => void;
  /** Fast-path: opens the wizard pre-configured for a scratch session
   *  and jumped to the Review step so Cmd+Enter immediately creates it. */
  onNewScratch: () => void;
  onDiff: () => void;
  onEscape: () => void;
  onHelp: () => void;
  onSettings: () => void;
  onPalette: () => void;
  onToggleSidebar: () => void;
  onToggleRightPanel: () => void;
  onToggleTerminalFocus: () => void;
}

/**
 * Global keyboard shortcuts for the dashboard.
 * Single-key shortcuts fire only when no input/textarea/terminal is focused.
 * Cmd+K (Mac) / Ctrl+K (other) and Escape fire regardless of focus.
 */
export function useKeyboardShortcuts(getActions: () => ShortcutActions) {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement | null;
      const isInput =
        !!target &&
        (target.tagName === "INPUT" ||
          target.tagName === "TEXTAREA" ||
          target.isContentEditable);

      const actions = getActions();
      const mod = IS_MAC ? e.metaKey : e.metaKey || e.ctrlKey;

      // Palette: Cmd+K (Mac) / Ctrl+K (other), works everywhere.
      if (mod && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "k") {
        e.preventDefault();
        e.stopPropagation();
        actions.onPalette();
        return;
      }

      // Toggle terminal focus: Cmd+` (Mac) / Ctrl+` (other), works everywhere.
      // Use e.code so layouts where backtick lives behind a modifier still match.
      // No stopPropagation: preventDefault is enough to suppress the browser's
      // own Cmd+` window cycling, and we don't want to silently shadow other
      // doc-level listeners that might want this combo.
      if (mod && !e.shiftKey && !e.altKey && e.code === "Backquote") {
        e.preventDefault();
        actions.onToggleTerminalFocus();
        return;
      }

      // Use e.code for B shortcuts because Option+B on Mac produces "∫"
      // instead of "b", causing e.key matching to fail.
      // Toggle right panel: Cmd+Opt+B (Mac) / Ctrl+Alt+B (other)
      // Check alt combo first so Cmd+B doesn't swallow Cmd+Opt+B.
      if (mod && !e.shiftKey && e.altKey && e.code === "KeyB") {
        e.preventDefault();
        e.stopPropagation();
        actions.onToggleRightPanel();
        return;
      }

      // Toggle left sidebar: Cmd+B (Mac) / Ctrl+B (other)
      if (mod && !e.shiftKey && !e.altKey && e.code === "KeyB") {
        e.preventDefault();
        e.stopPropagation();
        actions.onToggleSidebar();
        return;
      }

      // New scratch session: Cmd+Shift+N (Mac) / Ctrl+Shift+N (other).
      // Use e.code so Shift+layout punctuation doesn't break match (Shift+N
      // is still "N" by e.key but `code === "KeyN"` is layout-stable).
      // Works regardless of focus so the user can fire it from anywhere
      // including the terminal pane.
      if (mod && e.shiftKey && !e.altKey && e.code === "KeyN") {
        e.preventDefault();
        e.stopPropagation();
        actions.onNewScratch();
        return;
      }

      if (e.key === "Escape") {
        actions.onEscape();
        return;
      }

      if (isInput) return;
      if (e.metaKey || e.ctrlKey || e.altKey) return;

      switch (e.key) {
        case "n":
          e.preventDefault();
          actions.onNew();
          break;
        case "D":
          e.preventDefault();
          actions.onDiff();
          break;
        case "?":
          e.preventDefault();
          actions.onHelp();
          break;
        case "s":
          e.preventDefault();
          actions.onSettings();
          break;
      }
    };

    // Capture phase so we observe the keydown before xterm.js's helper
    // textarea sees it. xterm.js calls stopPropagation on a handful of
    // modifier combos (Cmd/Ctrl + letter), which otherwise blocks global
    // shortcuts like Cmd+K, Cmd+`, and Ctrl+Alt+B whenever the terminal
    // is focused.
    document.addEventListener("keydown", handler, true);
    return () => document.removeEventListener("keydown", handler, true);
  }, [getActions]);
}
