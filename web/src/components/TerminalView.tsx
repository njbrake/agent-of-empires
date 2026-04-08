import { useTerminal } from "../hooks/useTerminal";
import type { SessionResponse } from "../lib/types";
import "@xterm/xterm/css/xterm.css";

interface Props {
  session: SessionResponse;
  onBack?: () => void;
}

export function TerminalView({ session, onBack }: Props) {
  const containerRef = useTerminal(session.id);

  return (
    <div className="flex-1 flex flex-col overflow-hidden">
      <div className="h-10 bg-[#161b22] border-b border-[#30363d] flex items-center px-4 text-sm shrink-0">
        {onBack && (
          <button
            onClick={onBack}
            className="text-blue-400 mr-2.5 md:hidden cursor-pointer"
          >
            &larr;
          </button>
        )}
        <span className="font-semibold text-gray-200">{session.title}</span>
        <span className="text-gray-500 ml-3 text-xs">
          {[session.tool, session.branch, session.is_sandboxed && "sandboxed"]
            .filter(Boolean)
            .join(" \u00b7 ")}
        </span>
        <span className="ml-auto text-xs text-gray-500 flex items-center gap-1.5">
          <span
            className={`w-1.5 h-1.5 rounded-full ${
              session.status === "Running"
                ? "bg-green-500"
                : session.status === "Waiting"
                  ? "bg-yellow-500"
                  : session.status === "Error"
                    ? "bg-red-500"
                    : "bg-gray-500"
            }`}
          />
          {session.status}
        </span>
      </div>
      <div ref={containerRef} className="flex-1 overflow-hidden bg-[#0d1117]" />
    </div>
  );
}
