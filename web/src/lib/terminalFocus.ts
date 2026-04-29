export const FOCUS_TERMINAL_EVENT = "aoe:focus-terminal";

export type TerminalFocusTarget = "agent" | "paired";

export interface FocusTerminalDetail {
  target: TerminalFocusTarget;
}

export function dispatchFocusTerminal(target: TerminalFocusTarget) {
  window.dispatchEvent(
    new CustomEvent<FocusTerminalDetail>(FOCUS_TERMINAL_EVENT, {
      detail: { target },
    }),
  );
}

// When the right panel is collapsed, the paired terminal is unmounted, so
// dispatching a focus event before the panel re-renders has no listener to
// receive it. The shortcut handler stashes the intent here, and PairedTerminal
// consumes it once it mounts and its PTY becomes ready.
let pendingFocus: TerminalFocusTarget | null = null;

export function setPendingTerminalFocus(target: TerminalFocusTarget) {
  pendingFocus = target;
}

export function consumePendingTerminalFocus(
  target: TerminalFocusTarget,
): boolean {
  if (pendingFocus === target) {
    pendingFocus = null;
    return true;
  }
  return false;
}
