import { memo } from "react";
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

export const SessionItem = memo(function SessionItem({
  session,
  isActive,
  onClick,
}: Props) {
  return (
    <button
      onClick={onClick}
      className={`w-full text-left px-3 py-2.5 rounded-lg cursor-pointer transition-all duration-100 mb-1 ${
        isActive
          ? "bg-surface-900 shadow-sm shadow-black/20 border-l-2 border-brand-500 pl-2.5"
          : "hover:bg-surface-900/50"
      }`}
    >
      <div className="flex items-center gap-2 font-body text-sm font-medium text-text-primary truncate">
        <span
          className={`w-2 h-2 rounded-full shrink-0 ${STATUS_COLORS[session.status]}`}
        />
        {session.title}
      </div>
      <div className="flex items-center gap-1.5 font-body text-xs text-text-muted mt-1 pl-4">
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
});
