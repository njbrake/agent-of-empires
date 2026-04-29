import { useEffect } from "react";

const IS_MAC =
  typeof navigator !== "undefined" &&
  /Mac|iPhone|iPad|iPod/.test(navigator.platform);

interface ShortcutActions {
  onNew: () => void;
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
      if (mod && !e.shiftKey && !e.altKey && e.code === "Backquote") {
        e.preventDefault();
        e.stopPropagation();
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

    document.addEventListener("keydown", handler);
    return () => document.removeEventListener("keydown", handler);
  }, [getActions]);
}
