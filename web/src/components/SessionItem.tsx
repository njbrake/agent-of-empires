import type { SessionResponse, SessionStatus } from "../lib/types";

const STATUS_COLORS: Record<SessionStatus, string> = {
  Running: "bg-status-running",
  Waiting: "bg-status-waiting",
  Idle: "bg-status-idle",
  Error: "bg-status-error",
  Starting: "bg-status-starting",
  Stopped: "bg-status-stopped opacity-50",
  Unknown: "bg-status-idle opacity-50",
  Deleting: "bg-status-error opacity-50",
};

interface Props {
  session: SessionResponse;
  isActive: boolean;
  onClick: () => void;
}

export function SessionItem({ session, isActive, onClick }: Props) {
  return (
    <button
      onClick={onClick}
      className={`w-full text-left px-3 py-2 rounded-md cursor-pointer transition-colors duration-100 mb-0.5 ${
        isActive
          ? "bg-surface-800 border-l-2 border-brand-600 pl-2.5"
          : "hover:bg-surface-800/60"
      }`}
    >
      <div className="flex items-center gap-1.5 font-body text-[13px] font-medium text-slate-200 truncate">
        <span
          className={`w-1.5 h-1.5 rounded-full shrink-0 ${STATUS_COLORS[session.status]}`}
        />
        {session.title}
      </div>
      <div className="flex items-center gap-1.5 font-body text-[11px] text-slate-500 mt-0.5 pl-3">
        <span className="capitalize">{session.tool}</span>
        {session.branch && (
          <>
            <span className="text-surface-700">&middot;</span>
            <span className="truncate text-accent-600">{session.branch}</span>
          </>
        )}
      </div>
    </button>
  );
}
