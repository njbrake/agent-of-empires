import type { SessionStatus } from "../lib/types";

interface Props {
  sessionCount: number;
  activeSessionTitle: string | null;
  activeStatus: SessionStatus | null;
  onSessionsTab: () => void;
  onSettingsTab: () => void;
  onWorktreesTab: () => void;
  activeTab: "sessions" | "settings" | "worktrees";
}

const STATUS_COLORS: Record<SessionStatus, string> = {
  Running: "bg-status-running",
  Waiting: "bg-status-waiting",
  Idle: "bg-status-idle",
  Error: "bg-status-error",
  Starting: "bg-status-starting",
  Stopped: "bg-status-stopped",
  Unknown: "bg-status-idle",
  Deleting: "bg-status-error",
};

export function MobileNav({
  sessionCount,
  activeSessionTitle,
  activeStatus,
  onSessionsTab,
  onSettingsTab,
  onWorktreesTab,
  activeTab,
}: Props) {
  return (
    <nav className="md:hidden h-12 bg-surface-850 border-t border-surface-700 flex items-center justify-around shrink-0 safe-area-bottom">
      <button
        onClick={onSessionsTab}
        className={`flex flex-col items-center gap-0.5 px-4 py-1 cursor-pointer ${
          activeTab === "sessions" ? "text-brand-500" : "text-text-muted"
        }`}
      >
        <span className="text-lg">&#9632;</span>
        <span className="font-mono text-label-sm">
          {activeSessionTitle ? (
            <span className="flex items-center gap-1">
              {activeStatus && (
                <span
                  className={`w-1 h-1 rounded-full inline-block ${STATUS_COLORS[activeStatus]}`}
                />
              )}
              {activeSessionTitle.slice(0, 8)}
            </span>
          ) : (
            `${sessionCount} sessions`
          )}
        </span>
      </button>
      <button
        onClick={onWorktreesTab}
        className={`flex flex-col items-center gap-0.5 px-4 py-1 cursor-pointer ${
          activeTab === "worktrees" ? "text-brand-500" : "text-text-muted"
        }`}
      >
        <span className="font-mono text-sm">wt</span>
        <span className="font-mono text-label-sm">Worktrees</span>
      </button>
      <button
        onClick={onSettingsTab}
        className={`flex flex-col items-center gap-0.5 px-4 py-1 cursor-pointer ${
          activeTab === "settings" ? "text-brand-500" : "text-text-muted"
        }`}
      >
        <span className="font-mono text-sm">cfg</span>
        <span className="font-mono text-label-sm">Settings</span>
      </button>
    </nav>
  );
}
