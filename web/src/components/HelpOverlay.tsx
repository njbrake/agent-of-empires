interface Props {
  onClose: () => void;
}

const SHORTCUTS = [
  { key: "n", desc: "New session" },
  { key: "D", desc: "Toggle diff panel" },
  { key: "s", desc: "Toggle settings" },
  { key: "Esc", desc: "Close dialog" },
  { key: "?", desc: "Toggle this help" },
];

const TERMINAL_SHORTCUTS = [
  { key: "All keys", desc: "Relayed directly to the agent via PTY" },
  { key: "Ctrl+C", desc: "Send interrupt to agent" },
  { key: "Ctrl+D", desc: "Send EOF to agent" },
  { key: "Up/Down", desc: "Scroll terminal history" },
];

export function HelpOverlay({ onClose }: Props) {
  return (
    <div
      className="fixed inset-0 bg-black/60 flex items-center justify-center z-50 animate-fade-in"
      onClick={onClose}
    >
      <div
        className="bg-surface-800 border border-surface-700/50 rounded-xl w-[480px] max-w-[90vw] shadow-2xl animate-slide-up"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between px-5 py-4 border-b border-surface-700">
          <h2 className="font-display text-sm font-semibold text-text-bright">
            Keyboard Shortcuts
          </h2>
          <button
            onClick={onClose}
            className="text-text-muted hover:text-text-secondary cursor-pointer"
          >
            &times;
          </button>
        </div>

        <div className="p-5">
          <div className="mb-5">
            <h3 className="font-mono text-sm uppercase tracking-widest text-text-muted mb-2">
              Dashboard
            </h3>
            <div className="space-y-1">
              {SHORTCUTS.map((s) => (
                <div key={s.key} className="flex items-center gap-3">
                  <kbd className="font-mono text-sm bg-surface-900 border border-surface-700 rounded px-1.5 py-0.5 text-brand-500 min-w-[32px] text-center">
                    {s.key}
                  </kbd>
                  <span className="font-body text-sm text-text-secondary">
                    {s.desc}
                  </span>
                </div>
              ))}
            </div>
          </div>

          <div>
            <h3 className="font-mono text-sm uppercase tracking-widest text-text-muted mb-2">
              Terminal
            </h3>
            <div className="space-y-1">
              {TERMINAL_SHORTCUTS.map((s) => (
                <div key={s.key} className="flex items-center gap-3">
                  <kbd className="font-mono text-sm bg-surface-900 border border-surface-700 rounded px-1.5 py-0.5 text-accent-600 min-w-[32px] text-center">
                    {s.key}
                  </kbd>
                  <span className="font-body text-sm text-text-secondary">
                    {s.desc}
                  </span>
                </div>
              ))}
            </div>
          </div>
        </div>

        <div className="px-5 py-3 border-t border-surface-700">
          <p className="font-body text-sm text-text-dim">
            Shortcuts are disabled when typing in input fields or the terminal.
          </p>
        </div>
      </div>
    </div>
  );
}
