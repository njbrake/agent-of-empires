import { useTerminal } from "../hooks/useTerminal";
import type { SessionResponse, SessionStatus } from "../lib/types";
import "@xterm/xterm/css/xterm.css";

const STATUS_DOT: Record<SessionStatus, string> = {
  Running: "bg-status-running",
  Waiting: "bg-status-waiting",
  Idle: "bg-status-idle",
  Error: "bg-status-error",
  Starting: "bg-status-starting",
  Stopped: "bg-status-stopped",
  Unknown: "bg-status-idle",
  Deleting: "bg-status-error",
};

interface Props {
  session: SessionResponse;
  onBack?: () => void;
}

export function TerminalView({ session, onBack }: Props) {
  const containerRef = useTerminal(session.id);

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      <div className="h-10 bg-surface-850 border-b border-surface-700 flex items-center px-4 text-sm shrink-0">
        {onBack && (
          <button
            onClick={onBack}
            className="text-brand-500 mr-2.5 md:hidden cursor-pointer"
          >
            &larr;
          </button>
        )}
        <span className="font-body font-semibold text-text-primary">
          {session.title}
        </span>
        <span className="font-body text-text-muted ml-3 text-xs">
          {[session.tool, session.branch, session.is_sandboxed && "sandboxed"]
            .filter(Boolean)
            .join(" \u00b7 ")}
        </span>
        <span className="ml-auto font-mono text-sm text-text-muted flex items-center gap-1.5">
          <span
            className={`w-1.5 h-1.5 rounded-full ${STATUS_DOT[session.status]}`}
          />
          {session.status}
        </span>
      </div>
      <div ref={containerRef} className="flex-1 overflow-hidden bg-surface-950" />
    </div>
  );
}
