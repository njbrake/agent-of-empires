import type { SessionResponse } from "../lib/types";
import { stopSession, restartSession } from "../lib/api";
import { SessionItem } from "./SessionItem";

interface Props {
  sessions: SessionResponse[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onRefresh: () => void;
}

export function Sidebar({ sessions, activeId, onSelect, onRefresh }: Props) {
  const activeSession = sessions.find((s) => s.id === activeId);

  const handleStop = async (id: string) => {
    await stopSession(id);
    onRefresh();
  };

  const handleRestart = async (id: string) => {
    await restartSession(id);
    onRefresh();
  };

  return (
    <aside className="w-[280px] min-w-[280px] bg-surface-900 border-r border-surface-700 flex flex-col overflow-hidden max-md:w-full max-md:min-w-full max-md:max-h-[40vh] max-md:border-r-0 max-md:border-b max-md:border-surface-700">
      <div className="px-3.5 pt-3 pb-2 font-mono text-[11px] font-semibold uppercase tracking-widest text-slate-500">
        Sessions
      </div>

      <div className="flex-1 overflow-y-auto px-1.5 pb-1.5">
        {sessions.length === 0 ? (
          <div className="px-3.5 py-5 text-center text-slate-600 text-xs font-body">
            No sessions found.
            <br />
            <code className="font-mono text-brand-600 text-[11px]">
              aoe add /path/to/project
            </code>
          </div>
        ) : (
          sessions.map((s) => (
            <SessionItem
              key={s.id}
              session={s}
              isActive={s.id === activeId}
              onClick={() => onSelect(s.id)}
            />
          ))
        )}
      </div>

      {activeSession && (
        <div className="px-3.5 py-2.5 border-t border-surface-700 flex gap-1.5">
          {activeSession.status !== "Stopped" && (
            <button
              onClick={() => handleStop(activeSession.id)}
              className="px-3 py-1 font-body text-xs rounded-md border border-status-error/40 text-status-error hover:bg-status-error/10 transition-colors cursor-pointer"
            >
              Stop
            </button>
          )}
          {(activeSession.status === "Stopped" ||
            activeSession.status === "Error") && (
            <button
              onClick={() => handleRestart(activeSession.id)}
              className="px-3 py-1 font-body text-xs rounded-md border border-brand-600/40 text-brand-500 hover:bg-brand-600/10 transition-colors cursor-pointer"
            >
              Restart
            </button>
          )}
        </div>
      )}
    </aside>
  );
}
