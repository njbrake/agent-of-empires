export function EmptyState() {
  return (
    <div className="flex-1 flex flex-col items-center justify-center text-slate-600 bg-surface-900">
      <div className="font-mono text-4xl text-surface-700 mb-4">&gt;_</div>
      <h2 className="font-body text-base font-medium text-slate-400 mb-1">
        Select a session
      </h2>
      <p className="font-body text-sm text-slate-600">
        Click a session in the sidebar to connect
      </p>
    </div>
  );
}
