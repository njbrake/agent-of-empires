import type { Workspace, SessionResponse } from "../lib/types";

interface Props {
  workspace: Workspace;
  activeSession: SessionResponse | null;
  diffCollapsed: boolean;
  diffFileCount: number;
  onToggleDiff: () => void;
}

export function WorkspaceHeader({
  workspace,
  activeSession,
  diffCollapsed,
  diffFileCount,
  onToggleDiff,
}: Props) {
  const agentLabel = activeSession?.tool ?? workspace.primaryAgent;

  return (
    <div className="h-10 bg-surface-900 border-b border-surface-700/20 flex items-center px-3 gap-2 shrink-0">
      <span className="font-mono text-sm font-semibold text-accent-600 truncate">
        {workspace.displayName}
      </span>
      <span className="hidden sm:inline font-body text-xs text-text-dim truncate">
        {agentLabel}
      </span>

      {diffCollapsed && diffFileCount > 0 && (
        <button
          onClick={onToggleDiff}
          className="font-mono text-xs px-2 py-0.5 rounded-full bg-accent-600/15 text-accent-600 cursor-pointer hover:bg-accent-600/25 transition-colors"
        >
          {diffFileCount} change{diffFileCount !== 1 ? "s" : ""}
        </button>
      )}
    </div>
  );
}
