interface Props {
  onClick: () => void;
  /** Distance from the top of the positioned parent, in Tailwind units. */
  topOffset?: "top-2" | "top-3";
}

/**
 * Floating pill button shown when the user has scrolled the terminal up
 * into tmux copy-mode. Tapping it calls `exitScrollback()` (which sends
 * Escape to the PTY), returning the pane to its live view.
 *
 * Rendered in an absolutely-positioned overlay; the parent must be
 * `position: relative`.
 */
export function BackToLiveButton({ onClick, topOffset = "top-3" }: Props) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label="Back to live"
      className={`absolute left-1/2 ${topOffset} -translate-x-1/2 z-10 flex items-center gap-1.5 font-mono text-[12px] text-text-primary bg-surface-800/95 border border-surface-700 rounded-full px-3 py-1.5 shadow-lg backdrop-blur-sm active:scale-95 motion-safe:animate-[fadeIn_200ms_ease-out]`}
    >
      <svg
        width="12"
        height="12"
        viewBox="0 0 24 24"
        fill="none"
        stroke="currentColor"
        strokeWidth="2"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden="true"
      >
        <polyline points="6 9 12 15 18 9" />
      </svg>
      Back to live
    </button>
  );
}
