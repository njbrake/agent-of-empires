import type { Workspace, SessionResponse, WorkspaceStatus } from "../lib/types";

interface Props {
  workspace: Workspace;
  activeSession: SessionResponse | null;
  diffCollapsed: boolean;
  diffFileCount: number;
  actionPending: boolean;
  onStop: () => void;
  onRestart: () => void;
  onLifecycleChange: (status: WorkspaceStatus) => void;
  onToggleDiff: () => void;
}

const LIFECYCLE_BADGE: Record<
  WorkspaceStatus,
  { label: string; classes: string }
> = {
  active: {
    label: "Active",
    classes: "bg-status-running/15 text-status-running",
  },
  idle: { label: "Idle", classes: "bg-surface-700/30 text-text-muted" },
  reviewing: {
    label: "Reviewing",
    classes: "bg-status-waiting/15 text-status-waiting",
  },
  archived: { label: "Archived", classes: "bg-surface-700/30 text-text-dim" },
};

export function WorkspaceHeader({
  workspace,
  activeSession,
  diffCollapsed,
  diffFileCount,
  actionPending,
  onStop,
  onRestart,
  onLifecycleChange,
  onToggleDiff,
}: Props) {
  const badge = LIFECYCLE_BADGE[workspace.status];
  const agentLabel = activeSession?.tool ?? workspace.primaryAgent;

  const btnBase =
    "font-body text-[12px] px-2 py-1 rounded-md border border-surface-700 text-text-muted hover:bg-surface-800 cursor-pointer transition-colors disabled:opacity-40 disabled:cursor-not-allowed";

  return (
    <div className="h-10 bg-surface-900 border-b border-surface-700 flex items-center px-3 gap-2 shrink-0">
      <span className="font-mono text-sm font-semibold text-accent-600 truncate">
        {workspace.displayName}
      </span>
      <span className="hidden sm:inline font-body text-xs text-text-dim truncate">
        {agentLabel}
      </span>
      <span
        className={`hidden sm:inline font-mono text-xs px-1.5 py-px rounded-full uppercase tracking-wider ${badge.classes}`}
      >
        {badge.label}
      </span>

      {diffCollapsed && diffFileCount > 0 && (
        <button
          onClick={onToggleDiff}
          className="font-mono text-xs px-2 py-0.5 rounded-full bg-accent-600/15 text-accent-600 cursor-pointer hover:bg-accent-600/25 transition-colors"
        >
          {diffFileCount} change{diffFileCount !== 1 ? "s" : ""}
        </button>
      )}

      <div className="flex-1" />

      {workspace.status !== "archived" && activeSession && (
        <div className="hidden sm:flex items-center gap-1">
          <button
            onClick={onStop}
            disabled={actionPending}
            className={btnBase}
          >
            {actionPending ? "..." : "Stop"}
          </button>
          <button
            onClick={onRestart}
            disabled={actionPending}
            className={btnBase}
          >
            {actionPending ? "..." : "Restart"}
          </button>
        </div>
      )}

      {workspace.status === "active" || workspace.status === "idle" ? (
        <button
          onClick={() => onLifecycleChange("reviewing")}
          className={`hidden sm:inline ${btnBase}`}
        >
          Review
        </button>
      ) : workspace.status === "reviewing" ? (
        <button
          onClick={() => onLifecycleChange("archived")}
          className="hidden sm:inline font-body text-[12px] px-2 py-1 rounded-md bg-brand-600 text-surface-950 font-semibold hover:bg-brand-700 cursor-pointer transition-colors"
        >
          Archive
        </button>
      ) : (
        <button
          onClick={() => onLifecycleChange("active")}
          className={`hidden sm:inline ${btnBase}`}
        >
          Unarchive
        </button>
      )}
    </div>
  );
}
