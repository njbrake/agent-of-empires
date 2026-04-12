import { memo } from "react";
import type { Workspace, SessionStatus } from "../lib/types";
import { STATUS_DOT_CLASS, isSessionActive } from "../lib/session";

interface Props {
  workspaces: Workspace[];
  activeId: string | null;
  onToggle: () => void;
  onSelect: (workspaceId: string) => void;
  onNew: () => void;
  onSettings: () => void;
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
  onSettings,
}: Props) {
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

        <div className="flex-1 overflow-y-auto">
          {workspaces.map((ws) => (
            <SessionRow
              key={ws.id}
              workspace={ws}
              isActive={ws.id === activeId}
              onClick={() => onSelect(ws.id)}
            />
          ))}
        </div>

        {/* Footer */}
        <div className="border-t border-surface-700 p-2">
          <button
            onClick={onSettings}
            className="w-full flex items-center gap-2.5 px-3 py-2 text-text-dim hover:text-text-secondary hover:bg-surface-800/50 cursor-pointer rounded-md transition-colors"
            title="Settings"
            aria-label="Settings"
          >
            <svg
              width="16"
              height="16"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <path d="M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2z" />
              <circle cx="12" cy="12" r="3" />
            </svg>
            <span className="font-body text-[13px]">Settings</span>
          </button>
        </div>
      </div>
    </>
  );
}
