import { useEffect } from "react";

interface ShortcutActions {
  onNew: () => void;
  onDiff: () => void;
  onEscape: () => void;
  onHelp: () => void;
  onSettings: () => void;
}

/**
 * Global keyboard shortcuts for the dashboard.
 * Only fires when no input/textarea/terminal is focused.
 */
export function useKeyboardShortcuts(getActions: () => ShortcutActions) {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      const isInput =
        target.tagName === "INPUT" ||
        target.tagName === "TEXTAREA" ||
        target.isContentEditable;

      const actions = getActions();

      if (e.key === "Escape") {
        actions.onEscape();
        return;
      }

      if (isInput) return;

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
