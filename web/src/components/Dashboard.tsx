import { useMemo } from "react";
import type { SessionResponse, SessionStatus } from "../lib/types";
import { STATUS_DOT_CLASS, STATUS_TEXT_CLASS, isSessionActive } from "../lib/session";

interface Props {
  sessions: SessionResponse[];
  onSelectSession: (sessionId: string) => void;
}

interface ProjectGroup {
  repoPath: string;
  displayName: string;
  sessions: SessionResponse[];
  hasActive: boolean;
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
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

export function Dashboard({ sessions, onSelectSession }: Props) {
  const groups = useMemo(() => {
    const map = new Map<string, ProjectGroup>();
    for (const s of sessions) {
      const key = s.main_repo_path || s.project_path;
      const existing = map.get(key);
      if (existing) {
        existing.sessions.push(s);
        if (isSessionActive(s.status)) existing.hasActive = true;
      } else {
        map.set(key, {
          repoPath: key,
          displayName: key.split("/").filter(Boolean).pop() || key,
          sessions: [s],
          hasActive: isSessionActive(s.status),
        });
      }
    }

    // Sort sessions within each group: active/error first, then by last accessed
    for (const group of map.values()) {
      group.sessions.sort((a, b) => {
        const pa = statusPriority(a.status);
        const pb = statusPriority(b.status);
        if (pa !== pb) return pa - pb;
        return (b.last_accessed_at ?? "").localeCompare(a.last_accessed_at ?? "");
      });
    }

    // Sort groups: active projects first, then alphabetical
    return Array.from(map.values()).sort((a, b) => {
      if (a.hasActive && !b.hasActive) return -1;
      if (!a.hasActive && b.hasActive) return 1;
      return a.displayName.localeCompare(b.displayName);
    });
  }, [sessions]);

  // Empty state
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

  // Count sessions needing attention
  const activeCount = sessions.filter((s) => isSessionActive(s.status)).length;
  const errorCount = sessions.filter((s) => s.status === "Error").length;

  return (
    <div className="flex-1 overflow-y-auto bg-surface-950">
      <div className="max-w-2xl mx-auto px-4 py-6">
        {/* Summary */}
        <div className="flex items-baseline justify-between mb-5">
          <div className="flex items-center gap-3">
            <h2 className="text-base font-semibold text-text-primary">
              {groups.length} project{groups.length !== 1 ? "s" : ""}
            </h2>
            <div className="flex items-center gap-2 text-xs">
              {activeCount > 0 && (
                <span className="text-status-running">
                  {activeCount} active
                </span>
              )}
              {errorCount > 0 && (
                <span className="text-status-error">
                  {errorCount} error{errorCount !== 1 ? "s" : ""}
                </span>
              )}
            </div>
          </div>
        </div>

        <p className="text-xs text-text-dim mb-4 md:hidden">
          Tap the sidebar icon in the top left for projects and settings.
        </p>

        {/* Project groups */}
        <div className="space-y-4">
          {groups.map((group) => (
            <div key={group.repoPath}>
              <div className="flex items-center gap-2 mb-1.5 px-1">
                <span className={`w-1.5 h-1.5 rounded-full shrink-0 ${group.hasActive ? "bg-status-running" : "bg-status-idle"}`} />
                <span className="text-xs font-medium text-text-muted truncate">
                  {group.displayName}
                </span>
                <span className="text-[11px] text-text-dim">
                  {group.sessions.length}
                </span>
              </div>
              <div className="space-y-1">
                {group.sessions.map((s) => (
                  <SessionCard
                    key={s.id}
                    session={s}
                    onClick={() => onSelectSession(s.id)}
                  />
                ))}
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function SessionCard({ session, onClick }: { session: SessionResponse; onClick: () => void }) {
  const dotClass = STATUS_DOT_CLASS[session.status] ?? "bg-status-idle";
  const textClass = STATUS_TEXT_CLASS[session.status] ?? "text-status-idle";
  const active = isSessionActive(session.status);
  const label = session.branch ?? session.title ?? "default";
  const ago = timeAgo(session.last_accessed_at);

  return (
    <button
      onClick={onClick}
      className={`w-full text-left px-3 py-3 rounded-lg border transition-colors cursor-pointer focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-brand-600 ${
        active
          ? "bg-surface-900 border-surface-700"
          : session.status === "Error"
            ? "bg-surface-900/50 border-status-error/20"
            : "bg-surface-900/50 border-transparent hover:bg-surface-900 hover:border-surface-700"
      }`}
    >
      <div className="flex items-center gap-2.5">
        <span className={`w-2 h-2 rounded-full shrink-0 ${dotClass}`} />
        <span className={`text-sm truncate flex-1 ${active ? textClass : "text-text-primary"}`}>
          {label}
        </span>
        {ago && (
          <span className="text-[11px] text-text-dim shrink-0">{ago}</span>
        )}
      </div>
      {session.status === "Error" && session.last_error && (
        <p className="text-[11px] text-status-error mt-1 ml-[18px] truncate">
          {session.last_error}
        </p>
      )}
    </button>
  );
}
