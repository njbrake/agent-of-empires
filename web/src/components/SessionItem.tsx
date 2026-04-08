import type { SessionResponse, SessionStatus } from "../lib/types";

const STATUS_COLORS: Record<SessionStatus, string> = {
  Running: "bg-green-500",
  Waiting: "bg-yellow-500",
  Idle: "bg-gray-500",
  Error: "bg-red-500",
  Starting: "bg-orange-500",
  Stopped: "bg-gray-600 opacity-50",
  Unknown: "bg-gray-600 opacity-50",
  Deleting: "bg-red-500 opacity-50",
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
      className={`w-full text-left px-2.5 py-2 rounded-md cursor-pointer transition-colors mb-0.5 ${
        isActive
          ? "bg-[#1c2129] border-l-2 border-blue-400 pl-2"
          : "hover:bg-[#1c2129]"
      }`}
    >
      <div className="flex items-center gap-1.5 text-sm font-medium text-gray-200 truncate">
        <span
          className={`w-1.5 h-1.5 rounded-full shrink-0 ${STATUS_COLORS[session.status]}`}
        />
        {session.title}
      </div>
      <div className="flex items-center gap-1.5 text-xs text-gray-500 mt-0.5">
        <span className="capitalize">{session.tool}</span>
        {session.branch && (
          <>
            <span>&middot;</span>
            <span className="truncate">{session.branch}</span>
          </>
        )}
      </div>
    </button>
  );
}
