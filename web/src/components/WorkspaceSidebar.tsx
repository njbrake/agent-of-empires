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

/** Group workspaces by project directory */
function groupByProject(workspaces: Workspace[]) {
  const groups = new Map<string, Workspace[]>();
  for (const ws of workspaces) {
    const existing = groups.get(ws.projectPath);
    if (existing) {
      existing.push(ws);
    } else {
      groups.set(ws.projectPath, [ws]);
    }
  }
  return groups;
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

  return (
    <button
      onClick={onClick}
      className={`w-full text-left flex items-center gap-2 pl-7 pr-3 py-2.5 cursor-pointer transition-colors duration-75 ${
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
      <span
        className="font-body text-[13px] truncate flex-1"
        title={workspace.branch ?? workspace.sessions[0]?.title ?? "default"}
      >
        {workspace.branch ?? workspace.sessions[0]?.title ?? "default"}
      </span>
      <span className="font-mono text-xs text-accent-600 shrink-0">
        {workspace.primaryAgent}
      </span>
      {workspace.agents.length > 1 && (
        <span className="font-mono text-xs text-text-dim shrink-0">
          +{workspace.agents.length - 1}
        </span>
      )}
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
  const [expandedProjects, setExpandedProjects] = useState<Set<string>>(
    new Set(),
  );

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

  const projectGroups = groupByProject(filtered);

  // Auto-expand projects that contain the active workspace
  const activeProjectPath = workspaces.find(
    (w) => w.id === activeId,
  )?.projectPath;

  const isExpanded = (path: string) =>
    expandedProjects.has(path) || path === activeProjectPath;

  const toggleProject = (path: string) => {
    setExpandedProjects((prev) => {
      const next = new Set(prev);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  };

  // Count active sessions per project for the summary dot
  function projectStatusDot(wsList: Workspace[]): string {
    const statuses = wsList.map((ws) => bestSessionStatus(ws));
    if (statuses.some((s) => s === "Running")) return "bg-status-running";
    if (statuses.some((s) => s === "Waiting")) return "bg-status-waiting";
    if (statuses.some((s) => s === "Error")) return "bg-status-error";
    return "bg-status-idle";
  }

  return (
    <>
      {/* Mobile backdrop */}
      <div
        className="fixed inset-0 bg-black/50 z-30 md:hidden"
        onClick={onToggle}
      />
      <div className="fixed inset-y-0 left-0 z-40 w-[280px] md:static md:z-auto bg-surface-900 border-r border-surface-700 flex flex-col h-full">
        {/* Header with close button on mobile */}
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

        {/* Search */}
        <div className="px-3 pb-2">
          <input
            type="text"
            value={searchQuery}
            onChange={(e) => setSearchQuery(e.target.value)}
            placeholder="Search... (/)"
            className="w-full bg-surface-800 border border-surface-700 rounded-md px-2.5 py-1.5 font-body text-[13px] text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none"
          />
        </div>

        {/* Project tree */}
        <div className="flex-1 overflow-y-auto">
          {[...projectGroups.entries()].map(([projectPath, wsList]) => {
            const dirName = projectPath.split("/").pop() ?? projectPath;
            const expanded = isExpanded(projectPath);
            const dot = projectStatusDot(wsList);

            return (
              <div key={projectPath}>
                {/* Project directory header */}
                <button
                  onClick={() => toggleProject(projectPath)}
                  className="w-full text-left flex items-center gap-2 px-3 py-2.5 cursor-pointer hover:bg-surface-800/50 transition-colors"
                >
                  <span className="font-mono text-xs text-text-dim w-3">
                    {expanded ? "▾" : "▸"}
                  </span>
                  <span className={`w-2 h-2 rounded-full shrink-0 ${dot}`} />
                  <span
                    className="font-mono text-sm text-text-primary truncate flex-1"
                    title={projectPath}
                  >
                    {dirName}
                  </span>
                  <span className="font-mono text-xs text-text-dim">
                    {wsList.length}
                  </span>
                </button>

                {/* Workspace sessions nested under project */}
                {expanded &&
                  wsList.map((ws) => (
                    <SessionRow
                      key={ws.id}
                      workspace={ws}
                      isActive={ws.id === activeId}
                      onClick={() => onSelect(ws.id)}
                    />
                  ))}
              </div>
            );
          })}

          {/* Empty state */}
          {filtered.length === 0 && (
            <div className="flex flex-col items-center justify-center px-4 py-12 text-center">
              <p className="font-body text-sm text-text-muted">
                {searchQuery
                  ? `No matches for "${searchQuery}"`
                  : "No sessions yet"}
              </p>
              {!searchQuery && (
                <button
                  onClick={onNew}
                  className="mt-3 px-3 py-1.5 font-body text-xs rounded-md bg-brand-600 text-surface-950 hover:bg-brand-700 cursor-pointer transition-colors"
                >
                  Create session
                </button>
              )}
            </div>
          )}
        </div>
      </div>
    </>
  );
}
