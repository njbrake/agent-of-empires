interface Props {
  fullscreen: boolean;
  onToggle: () => void;
}

// Mobile-only toggle that releases the keyboard reservation so the
// terminal expands into the full viewport. Tapping again clamps back to
// the smaller "keyboard reserved" size. This is the only thing that
// resizes the PTY on mobile in steady state, so each tap is the user
// explicitly accepting one SIGWINCH and one claude redraw.
export function ViewportFullscreenFab({ fullscreen, onToggle }: Props) {
  return (
    <button
      type="button"
      aria-label={
        fullscreen ? "Exit fullscreen terminal" : "Expand terminal to fullscreen"
      }
      aria-pressed={fullscreen}
      onClick={onToggle}
      className="absolute right-3 bottom-16 z-10 w-10 h-10 rounded-full bg-surface-800/90 border border-surface-700/30 text-text-secondary flex items-center justify-center shadow-lg backdrop-blur-sm active:scale-95"
    >
      {fullscreen ? (
        <svg
          width="16"
          height="16"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <path d="M9 4 H4 V9" />
          <path d="M15 4 H20 V9" />
          <path d="M9 20 H4 V15" />
          <path d="M15 20 H20 V15" />
        </svg>
      ) : (
        <svg
          width="16"
          height="16"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1.5"
          strokeLinecap="round"
          strokeLinejoin="round"
          aria-hidden="true"
        >
          <path d="M4 9 V4 H9" />
          <path d="M20 9 V4 H15" />
          <path d="M4 15 V20 H9" />
          <path d="M20 15 V20 H15" />
        </svg>
      )}
    </button>
  );
}
