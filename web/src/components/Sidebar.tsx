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
    <aside className="w-[280px] min-w-[280px] bg-[#161b22] border-r border-[#30363d] flex flex-col overflow-hidden max-md:w-full max-md:min-w-full max-md:max-h-[40vh] max-md:border-r-0 max-md:border-b max-md:border-[#30363d]">
      <div className="px-3.5 pt-3 pb-2 text-[11px] font-semibold uppercase tracking-wider text-gray-500">
        Sessions
      </div>

      <div className="flex-1 overflow-y-auto px-1.5 pb-1.5 scrollbar-thin">
        {sessions.length === 0 ? (
          <div className="px-3.5 py-5 text-center text-gray-600 text-xs">
            No sessions found.
            <br />
            Create sessions via CLI.
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
        <div className="px-3.5 py-2.5 border-t border-[#30363d] flex gap-1.5">
          {activeSession.status !== "Stopped" && (
            <button
              onClick={() => handleStop(activeSession.id)}
              className="px-3 py-1 text-xs rounded-md border border-red-800 text-red-400 hover:bg-red-500/10 transition-colors cursor-pointer"
            >
              Stop
            </button>
          )}
          {(activeSession.status === "Stopped" ||
            activeSession.status === "Error") && (
            <button
              onClick={() => handleRestart(activeSession.id)}
              className="px-3 py-1 text-xs rounded-md border border-blue-700 text-blue-400 hover:bg-blue-500/10 transition-colors cursor-pointer"
            >
              Restart
            </button>
          )}
        </div>
      )}
    </aside>
  );
}
