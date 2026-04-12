import { memo, useCallback, useEffect, useRef, useState } from "react";
import type { Workspace, SessionStatus } from "../lib/types";
import { STATUS_DOT_CLASS, isSessionActive } from "../lib/session";

const SIDEBAR_WIDTH_KEY = "aoe-sidebar-width";
const DEFAULT_WIDTH = 280;
const MIN_WIDTH = 200;
const MAX_WIDTH = 480;

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

function loadSavedWidth(): number {
  try {
    const saved = localStorage.getItem(SIDEBAR_WIDTH_KEY);
    if (saved) {
      const w = parseInt(saved, 10);
      if (w >= MIN_WIDTH && w <= MAX_WIDTH) return w;
    }
  } catch {
    // ignore
  }
  return DEFAULT_WIDTH;
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
  const [width, setWidth] = useState(loadSavedWidth);
  const [filterOpen, setFilterOpen] = useState(false);
  const [filterQuery, setFilterQuery] = useState("");
  const filterRef = useRef<HTMLInputElement>(null);
  const dragging = useRef(false);

  const filtered = filterQuery.trim()
    ? workspaces.filter((ws) => {
        const q = filterQuery.toLowerCase();
        return (
          ws.displayName.toLowerCase().includes(q) ||
          ws.projectPath.toLowerCase().includes(q) ||
          ws.agents.some((a) => a.toLowerCase().includes(q)) ||
          ws.sessions.some((s) => s.title.toLowerCase().includes(q))
        );
      })
    : workspaces;

  const toggleFilter = () => {
    setFilterOpen((o) => {
      if (o) setFilterQuery("");
      return !o;
    });
  };

  useEffect(() => {
    if (filterOpen) filterRef.current?.focus();
  }, [filterOpen]);

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    dragging.current = true;
    document.body.style.cursor = "col-resize";
    document.body.style.userSelect = "none";
  }, []);

  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      if (!dragging.current) return;
      const newWidth = Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, e.clientX));
      setWidth(newWidth);
    };

    const handleMouseUp = () => {
      if (!dragging.current) return;
      dragging.current = false;
      document.body.style.cursor = "";
      document.body.style.userSelect = "";
      setWidth((w) => {
        localStorage.setItem(SIDEBAR_WIDTH_KEY, String(w));
        return w;
      });
    };

    document.addEventListener("mousemove", handleMouseMove);
    document.addEventListener("mouseup", handleMouseUp);
    return () => {
      document.removeEventListener("mousemove", handleMouseMove);
      document.removeEventListener("mouseup", handleMouseUp);
    };
  }, []);

  return (
    <>
      <div
        className="fixed inset-0 bg-black/50 z-30 md:hidden"
        onClick={onToggle}
      />
      <div
        style={{ width }}
        className="fixed inset-y-0 left-0 z-40 md:static md:z-auto bg-surface-900 flex flex-col h-full shrink-0"
      >
        <div className="px-3 pt-3 pb-1 flex items-center">
          <span className="font-mono text-xs uppercase tracking-wider text-text-dim flex-1">
            Sessions
          </span>
          <button
            onClick={toggleFilter}
            className={`w-8 h-8 flex items-center justify-center cursor-pointer rounded-md transition-colors ${
              filterOpen
                ? "text-brand-500 bg-brand-600/10"
                : "text-text-dim hover:text-text-secondary hover:bg-surface-800"
            }`}
            title="Filter sessions"
            aria-label="Filter sessions"
          >
            <svg
              width="14"
              height="14"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              <polygon points="22 3 2 3 10 12.46 10 19 14 21 14 12.46 22 3" />
            </svg>
          </button>
          <button
            onClick={onNew}
            className="w-8 h-8 flex items-center justify-center text-text-muted hover:text-text-secondary hover:bg-surface-800 cursor-pointer rounded-md transition-colors"
            title="New session"
            aria-label="New session"
          >
            <svg
              width="16"
              height="16"
              viewBox="0 0 24 24"
              fill="none"
              stroke="currentColor"
              strokeWidth="2"
              strokeLinecap="round"
            >
              <line x1="12" y1="5" x2="12" y2="19" />
              <line x1="5" y1="12" x2="19" y2="12" />
            </svg>
          </button>
          <button
            onClick={onToggle}
            className="md:hidden w-8 h-8 flex items-center justify-center text-text-dim hover:text-text-secondary cursor-pointer rounded-md hover:bg-surface-800 ml-1"
          >
            &times;
          </button>
        </div>

        {filterOpen && (
          <div className="px-3 pb-2">
            <input
              ref={filterRef}
              type="text"
              value={filterQuery}
              onChange={(e) => setFilterQuery(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Escape") toggleFilter();
              }}
              placeholder="Filter by name, branch, agent..."
              className="w-full bg-surface-800 border border-surface-700 rounded-md px-2.5 py-1.5 font-body text-[13px] text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none"
            />
          </div>
        )}

        <div className="flex-1 overflow-y-auto">
          {filtered.map((ws) => (
            <SessionRow
              key={ws.id}
              workspace={ws}
              isActive={ws.id === activeId}
              onClick={() => onSelect(ws.id)}
            />
          ))}

          {filtered.length === 0 && filterQuery && (
            <div className="px-4 py-8 text-center">
              <p className="font-body text-sm text-text-muted">
                No matches for "{filterQuery}"
              </p>
            </div>
          )}
        </div>

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
      {/* Resize handle (desktop only) */}
      <div
        onMouseDown={handleMouseDown}
        className="hidden md:block w-1 bg-surface-700 hover:bg-brand-600/50 cursor-col-resize shrink-0 transition-colors duration-75"
      />
    </>
  );
}
