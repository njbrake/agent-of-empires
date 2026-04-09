import { useEffect, useState } from "react";
import { fetchWorktrees, type WorktreeInfo } from "../lib/api";

interface Props {
  onClose: () => void;
  onNavigateToSession: (sessionId: string) => void;
}

export function WorktreeList({ onClose, onNavigateToSession }: Props) {
  const [worktrees, setWorktrees] = useState<WorktreeInfo[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    fetchWorktrees().then((wt) => {
      setWorktrees(wt);
      setLoading(false);
    });
  }, []);

  if (loading) {
    return (
      <div className="flex-1 flex items-center justify-center bg-surface-900 text-slate-500 font-mono text-sm">
        Loading worktrees...
      </div>
    );
  }

  return (
    <div className="flex-1 flex flex-col overflow-hidden bg-surface-900">
      <div className="h-10 bg-surface-850 border-b border-surface-700 flex items-center px-4 shrink-0">
        <button
          onClick={onClose}
          className="text-brand-500 mr-3 cursor-pointer font-body text-sm"
        >
          &larr; Back
        </button>
        <span className="font-mono text-[11px] uppercase tracking-wider text-slate-500">
          Worktrees
        </span>
        <span className="font-mono text-[11px] text-slate-600 ml-2">
          {worktrees.length} active
        </span>
      </div>

      <div className="flex-1 overflow-y-auto p-4">
        {worktrees.length === 0 ? (
          <div className="text-center py-12 text-slate-600 font-body text-sm">
            No active worktrees. Create a session with a worktree branch to see
            them here.
          </div>
        ) : (
          <div className="space-y-2 max-w-[700px]">
            {worktrees.map((wt) => (
              <div
                key={`${wt.session_id}-${wt.branch}`}
                className="bg-surface-800 border border-surface-700 rounded-md p-3"
              >
                <div className="flex items-center justify-between">
                  <div>
                    <span className="font-body text-sm font-medium text-slate-200">
                      {wt.branch}
                    </span>
                    {wt.managed_by_aoe && (
                      <span className="font-mono text-[10px] text-accent-600 ml-2">
                        managed
                      </span>
                    )}
                  </div>
                  <button
                    onClick={() => onNavigateToSession(wt.session_id)}
                    className="font-body text-xs text-brand-500 hover:text-brand-400 cursor-pointer"
                  >
                    Go to session &rarr;
                  </button>
                </div>
                <div className="font-mono text-[11px] text-slate-500 mt-1">
                  {wt.session_title}
                </div>
                <div className="font-mono text-[11px] text-slate-600 mt-0.5 truncate">
                  {wt.main_repo_path}
                </div>
              </div>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}
