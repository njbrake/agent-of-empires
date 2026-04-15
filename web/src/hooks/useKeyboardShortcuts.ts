import { useEffect } from "react";

interface ShortcutActions {
  onNew: () => void;
  onDiff: () => void;
  onEscape: () => void;
  onHelp: () => void;
  onSettings: () => void;
  onPalette: () => void;
}

/**
 * Global keyboard shortcuts for the dashboard.
 * Single-key shortcuts fire only when no input/textarea/terminal is focused.
 * Cmd/Ctrl+K (palette) and Escape fire regardless of focus.
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

      // Palette: Cmd+K / Ctrl+K, works everywhere.
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "k") {
        e.preventDefault();
        e.stopPropagation();
        actions.onPalette();
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
          actions.onNew();
          break;
        case "D":
          actions.onDiff();
          break;
        case "?":
          actions.onHelp();
          break;
        case "s":
          actions.onSettings();
          break;
      }
    };

    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [getActions]);
}
