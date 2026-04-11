import { useTerminal } from "../hooks/useTerminal";
import type { SessionResponse } from "../lib/types";
import "@xterm/xterm/css/xterm.css";

interface Props {
  session: SessionResponse;
}

export function TerminalView({ session }: Props) {
  const { containerRef, state, manualReconnect } = useTerminal(session.id);

  return (
    <div className="flex-1 flex flex-col overflow-hidden relative">
      {!state.connected && state.reconnecting && (
        <div className="bg-status-waiting/15 border-b border-status-waiting/30 px-4 py-1.5 flex items-center gap-2 shrink-0">
          <span className="font-body text-xs text-status-waiting">
            Reconnecting in {state.retryCountdown}s... ({state.retryCount}/3)
          </span>
        </div>
      )}
      {!state.connected && !state.reconnecting && state.retryCount >= 3 && (
        <div className="bg-status-error/10 border-b border-status-error/30 px-4 py-1.5 flex items-center gap-2 shrink-0">
          <span className="font-body text-xs text-status-error">
            Connection lost
          </span>
          <button
            onClick={manualReconnect}
            className="font-body text-xs text-brand-500 hover:text-brand-400 cursor-pointer underline"
          >
            Retry
          </button>
        </div>
      )}

      <div
        ref={containerRef}
        className="flex-1 overflow-hidden bg-surface-950"
      />
    </div>
  );
}
