export function EmptyState() {
  return (
    <div className="flex-1 flex flex-col items-center justify-center bg-surface-900 px-8">
      {/* Decorative terminal icon */}
      <div className="w-16 h-12 rounded-lg bg-surface-800 border border-surface-700/50 flex items-end justify-start p-2 mb-6">
        <span className="font-mono text-brand-500 text-sm animate-pulse">
          _
        </span>
      </div>

      <h2 className="font-display text-lg font-semibold text-text-primary mb-2">
        Select a session
      </h2>
      <p className="font-body text-sm text-text-muted text-center max-w-xs leading-relaxed">
        Choose a session from the sidebar to open a live terminal connection
      </p>

      <div className="mt-8 flex items-center gap-3">
        <kbd className="font-mono text-xs bg-surface-800 border border-surface-700/50 rounded px-2 py-1 text-text-dim">
          n
        </kbd>
        <span className="font-body text-xs text-text-dim">
          to create a new session
        </span>
      </div>
    </div>
  );
}
