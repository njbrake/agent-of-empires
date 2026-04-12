import { useState, memo } from "react";
import type { Workspace, SessionStatus } from "../lib/types";
import { STATUS_DOT_CLASS, isSessionActive } from "../lib/session";

interface Props {
  workspaces: Workspace[];
  activeId: string | null;
  onToggle: () => void;
  onSelect: (workspaceId: string) => void;
  onNew: () => void;
}

function bestSessionStatus(ws: Workspace): SessionStatus {
  const running = ws.sessions.find((s) => isSessionActive(s.status));
  if (running) return running.status;
  const error = ws.sessions.find((s) => s.status === "Error");
  if (error) return "Error";
  return ws.sessions[0]?.status ?? "Unknown";
}

const SessionRow = memo(function SessionRow({
  workspace,
  isActive,
  onClick,
}: {
  workspace: Workspace;
  isActive: boolean;
  onClick: () => void;
}) {
  const sessionStatus = bestSessionStatus(workspace);
  const dotClass = STATUS_DOT_CLASS[sessionStatus] ?? "bg-status-idle";
  const label =
    workspace.branch ?? workspace.sessions[0]?.title ?? "default";

  return (
    <button
      onClick={onClick}
      className={`w-full text-left flex items-center gap-2.5 px-3 py-2.5 cursor-pointer transition-colors duration-75 ${
        isActive
          ? "bg-surface-850 text-text-primary"
          : "text-text-secondary hover:bg-surface-800/50"
      }`}
    >
      <span
        className={`w-2 h-2 rounded-full shrink-0 ${dotClass} ${
          sessionStatus === "Waiting" ? "animate-pulse" : ""
        }`}
      />
      <span className="font-body text-[13px] truncate flex-1" title={label}>
        {label}
      </span>
      <span className="font-mono text-xs text-accent-600 shrink-0">
        {workspace.primaryAgent}
      </span>
    </button>
  );
});

export function WorkspaceSidebar({
  workspaces,
  activeId,
  onToggle,
  onSelect,
  onNew,
}: Props) {
  const [searchQuery, setSearchQuery] = useState("");

  const filtered = searchQuery.trim()
    ? workspaces.filter((ws) => {
        const q = searchQuery.toLowerCase();
        return (
          ws.displayName.toLowerCase().includes(q) ||
          ws.projectPath.toLowerCase().includes(q) ||
          ws.agents.some((a) => a.toLowerCase().includes(q))
        );
      })
    : workspaces;

  return (
    <>
      <div
        className="fixed inset-0 bg-black/50 z-30 md:hidden"
        onClick={onToggle}
      />
      <div className="fixed inset-y-0 left-0 z-40 w-[280px] md:static md:z-auto bg-surface-900 border-r border-surface-700 flex flex-col h-full">
        <div className="p-3 pb-2 flex items-center gap-2">
          <button
            onClick={onNew}
            className="flex-1 bg-brand-600 hover:bg-brand-700 text-surface-950 font-body text-sm font-semibold py-2.5 px-3 rounded-md cursor-pointer transition-colors text-left"
          >
            + New Session
          </button>
          <button
            onClick={onToggle}
            className="md:hidden w-10 h-10 flex items-center justify-center text-text-dim hover:text-text-secondary cursor-pointer rounded-md hover:bg-surface-800"
          >
            &times;
          </button>
        </div>

        <div className="px-3 pb-2">
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Search... (/)"
            className="w-full bg-surface-800 border border-surface-700 rounded-md px-2.5 py-1.5 font-body text-[13px] text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none"
          />
        </div>

        <div className="flex-1 overflow-y-auto">
          {filtered.map((ws) => (
            <SessionRow
              key={ws.id}
              workspace={ws}
              isActive={ws.id === activeId}
              onClick={() => onSelect(ws.id)}
            />
          ))}

          {filtered.length === 0 && searchQuery && (
            <div className="px-4 py-8 text-center">
              <p className="font-body text-sm text-text-muted">
                No matches for "{searchQuery}"
              </p>
            </div>
          )}
        </div>
      </div>
    </>
  );
}
