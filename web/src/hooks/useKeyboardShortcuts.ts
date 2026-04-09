import { useEffect } from "react";

interface ShortcutActions {
  onSearch: () => void;
  onNew: () => void;
  onDelete: () => void;
  onRename: () => void;
  onDiff: () => void;
  onEscape: () => void;
}

/**
 * Global keyboard shortcuts for the dashboard.
 * Only fires when no input/textarea is focused (to avoid conflicts with typing).
 */
export function useKeyboardShortcuts(actions: ShortcutActions) {
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      const target = e.target as HTMLElement;
      const isInput =
        target.tagName === "INPUT" ||
        target.tagName === "TEXTAREA" ||
        target.isContentEditable;

      // Escape always works
      if (e.key === "Escape") {
        actions.onEscape();
        return;
      }

      // Other shortcuts only when not typing in an input
      if (isInput) return;

      switch (e.key) {
        case "/":
          e.preventDefault();
          actions.onSearch();
          break;
        case "n":
          actions.onNew();
          break;
        case "d":
          actions.onDelete();
          break;
        case "r":
          actions.onRename();
          break;
        case "D":
          actions.onDiff();
          break;
      }
    };

    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [actions]);
}
