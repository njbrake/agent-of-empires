import { useMemo } from "react";
import type { SessionResponse, SessionStatus } from "../lib/types";
import { STATUS_TEXT_CLASS, isSessionActive } from "../lib/session";
import { StatusGlyph } from "./StatusGlyph";

interface Props {
  sessions: SessionResponse[];
  onSelectSession: (sessionId: string) => void;
}

interface ProjectGroup {
  repoPath: string;
  displayName: string;
  sessions: SessionResponse[];
  hasActive: boolean;
  activeCount: number;
  errorCount: number;
  lastAccessedAt: string | null;
}

function statusPriority(status: SessionStatus): number {
  switch (status) {
    case "Error": return 0;
    case "Waiting": return 1;
    case "Running": return 2;
    case "Starting": return 3;
    default: return 4;
  }
}

function timeAgo(iso: string | null): string {
  if (!iso) return "";
  const diff = Date.now() - new Date(iso).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins}m`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h`;
  const days = Math.floor(hours / 24);
  return `${days}d`;
}

export function Dashboard({ sessions, onSelectSession }: Props) {
  const groups = useMemo<ProjectGroup[]>(() => {
    const map = new Map<string, ProjectGroup>();
    for (const s of sessions) {
      const key = s.main_repo_path || s.project_path;
      const existing = map.get(key);
      const active = isSessionActive(s.status);
      const errored = s.status === "Error";
      if (existing) {
        existing.sessions.push(s);
        if (active) existing.hasActive = true;
        if (active) existing.activeCount += 1;
        if (errored) existing.errorCount += 1;
        if ((s.last_accessed_at ?? "") > (existing.lastAccessedAt ?? "")) {
          existing.lastAccessedAt = s.last_accessed_at;
        }
      } else {
        map.set(key, {
          repoPath: key,
          displayName: key.split("/").filter(Boolean).pop() || key,
          sessions: [s],
          hasActive: active,
          activeCount: active ? 1 : 0,
          errorCount: errored ? 1 : 0,
          lastAccessedAt: s.last_accessed_at,
        });
      }
    }

    for (const group of map.values()) {
      group.sessions.sort((a, b) => {
        const pa = statusPriority(a.status);
        const pb = statusPriority(b.status);
        if (pa !== pb) return pa - pb;
        return (b.last_accessed_at ?? "").localeCompare(a.last_accessed_at ?? "");
      });
    }

    return Array.from(map.values()).sort((a, b) => {
      if (a.hasActive !== b.hasActive) return a.hasActive ? -1 : 1;
      if (a.errorCount !== b.errorCount) return b.errorCount - a.errorCount;
      return a.displayName.localeCompare(b.displayName);
    });
  }, [sessions]);

  if (sessions.length === 0) {
    return (
      <div className="flex-1 flex flex-col items-center justify-center bg-surface-950 px-4">
        <svg
          width="48"
          height="48"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="1"
          strokeLinecap="round"
          strokeLinejoin="round"
          className="text-text-dim/40 mb-4"
          aria-hidden="true"
        >
          <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
        </svg>
        <p className="text-sm text-text-muted mb-1">No sessions yet</p>
        <p className="text-xs text-text-dim">
          Create a session from the sidebar to get started.
        </p>
      </div>
    );
  }

  const totalActive = sessions.filter((s) => isSessionActive(s.status)).length;
  const totalError = sessions.filter((s) => s.status === "Error").length;
  const totalWaiting = sessions.filter((s) => s.status === "Waiting").length;

  return (
    <div className="flex-1 overflow-y-auto bg-surface-950">
      <div className="max-w-6xl mx-auto px-4 py-6">
        {/* Summary */}
        <div className="flex items-baseline flex-wrap gap-3 mb-6">
          <h2 className="text-base font-semibold text-text-primary">
            {groups.length} project{groups.length !== 1 ? "s" : ""}
          </h2>
          <div className="flex items-center gap-3 text-xs">
            {totalActive > 0 && (
              <span className="text-status-running">
                {totalActive} active
              </span>
            )}
            {totalWaiting > 0 && (
              <span className="text-status-waiting">
                {totalWaiting} waiting
              </span>
            )}
            {totalError > 0 && (
              <span className="text-status-error">
                {totalError} error{totalError !== 1 ? "s" : ""}
              </span>
            )}
          </div>
        </div>

        <p className="text-xs text-text-dim mb-4 md:hidden">
          Tap the sidebar icon in the top left for projects and settings.
        </p>

        {/* Grid of project cards */}
        <div className="grid grid-cols-1 md:grid-cols-2 xl:grid-cols-3 gap-3">
          {groups.map((group) => (
            <ProjectCard
              key={group.repoPath}
              group={group}
              onSelectSession={onSelectSession}
            />
          ))}
        </div>
      </div>
    </div>
  );
}

function ProjectCard({
  group,
  onSelectSession,
}: {
  group: ProjectGroup;
  onSelectSession: (id: string) => void;
}) {
  const ago = timeAgo(group.lastAccessedAt);

  return (
    <div
      className={`rounded-lg border bg-surface-900 overflow-hidden transition-colors ${
        group.errorCount > 0
          ? "border-status-error/20"
          : group.hasActive
            ? "border-surface-700"
            : "border-surface-800"
      }`}
    >
      {/* Card header */}
      <div className="px-3 py-2 border-b border-surface-800 flex items-center gap-2">
        <span className="text-sm font-medium text-text-primary truncate flex-1" title={group.repoPath}>
          {group.displayName}
        </span>
        <span className="font-mono text-[11px] text-text-dim shrink-0">
          {group.sessions.length}
        </span>
      </div>

      {/* Session rows */}
      <div className="py-1">
        {group.sessions.map((s) => (
          <SessionRow
            key={s.id}
            session={s}
            onClick={() => onSelectSession(s.id)}
          />
        ))}
      </div>

      {/* Footer: last active */}
      {ago && (
        <div className="px-3 py-1.5 border-t border-surface-800 flex items-center gap-2">
          <span className="text-[11px] text-text-dim">
            {group.hasActive ? "active" : "last touched"} · {ago} ago
          </span>
        </div>
      )}
    </div>
  );
}

function SessionRow({
  session,
  onClick,
}: {
  session: SessionResponse;
  onClick: () => void;
}) {
  const textClass = STATUS_TEXT_CLASS[session.status] ?? "text-status-idle";
  const active = isSessionActive(session.status);
  const label = session.branch ?? session.title ?? "default";

  return (
    <button
      onClick={onClick}
      className={`w-full text-left px-3 py-1.5 cursor-pointer transition-colors focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-brand-600 ${
        session.status === "Error"
          ? "hover:bg-status-error/5"
          : "hover:bg-surface-800/60"
      }`}
    >
      <div className="flex items-center gap-2.5">
        <span
          className={`shrink-0 font-mono text-sm leading-none w-3 text-center ${textClass}`}
          aria-hidden="true"
        >
          <StatusGlyph status={session.status} createdAt={session.created_at} />
        </span>
        <span
          className={`text-[13px] truncate flex-1 font-mono ${
            active ? textClass : "text-text-secondary"
          }`}
          title={label}
        >
          {label}
        </span>
        <span className="text-[10px] text-text-dim font-mono shrink-0 uppercase tracking-wider">
          {session.tool}
        </span>
      </div>
      {session.status === "Error" && session.last_error && (
        <p className="text-[11px] text-status-error mt-0.5 pl-[22px] truncate">
          {session.last_error}
        </p>
      )}
    </button>
  );
}
