import { useEffect, useMemo, useRef } from "react";
import type { RepoGroup, SessionResponse, SessionStatus } from "../lib/types";
import { getStatusTextClass, isSessionActive } from "../lib/session";
import { useIdleDecayWindowMs } from "../lib/idleDecay";
import { StatusGlyph } from "./StatusGlyph";

interface Props {
  groups: RepoGroup[];
  activeWorkspaceId: string | null;
  onSelectWorkspace: (workspaceId: string) => void;
}

const STATUS_PRIORITY: SessionStatus[] = [
  "Running",
  "Waiting",
  "Starting",
  "Creating",
  "Error",
  "Idle",
  "Stopped",
  "Unknown",
  "Deleting",
];

function bestSession(
  sessions: SessionResponse[],
  idleDecayWindowMs: number,
): SessionResponse | null {
  const active = sessions.find((s) => isSessionActive(s, idleDecayWindowMs));
  if (active) return active;

  return (
    [...sessions].sort(
      (a, b) =>
        STATUS_PRIORITY.indexOf(a.status) - STATUS_PRIORITY.indexOf(b.status),
    )[0] ?? null
  );
}

function groupSessions(group: RepoGroup): SessionResponse[] {
  return group.workspaces.flatMap((workspace) => workspace.sessions);
}

export function ProjectStrip({
  groups,
  activeWorkspaceId,
  onSelectWorkspace,
}: Props) {
  const idleDecayWindowMs = useIdleDecayWindowMs();
  const activeButtonRef = useRef<HTMLButtonElement | null>(null);

  const items = useMemo(
    () =>
      groups
        .filter((group) => group.workspaces.length > 0)
        .map((group) => {
          const sessions = groupSessions(group);
          const session = bestSession(sessions, idleDecayWindowMs);
          const active = group.workspaces.some((w) => w.id === activeWorkspaceId);
          return {
            group,
            session,
            active,
            workspaceId: group.workspaces[0]!.id,
            count: sessions.length,
          };
        }),
    [groups, activeWorkspaceId, idleDecayWindowMs],
  );

  useEffect(() => {
    if (!activeButtonRef.current?.scrollIntoView) return;
    activeButtonRef.current.scrollIntoView({
      block: "nearest",
      inline: "center",
    });
  }, [activeWorkspaceId]);

  if (items.length === 0) return null;

  return (
    <nav
      aria-label="Project switcher"
      data-testid="project-strip"
      className="h-10 shrink-0 border-b border-surface-700/20 bg-surface-900/95"
    >
      <div
        role="tablist"
        aria-label="Projects"
        className="flex h-full items-center gap-1 overflow-x-auto px-2 [scrollbar-width:thin]"
      >
        {items.map(({ group, session, active, workspaceId, count }) => {
          const status = session?.status ?? "Unknown";
          const textClass = getStatusTextClass(
            {
              status,
              idle_entered_at: session?.idle_entered_at ?? null,
            },
            idleDecayWindowMs,
          );
          return (
            <button
              key={group.id}
              ref={active ? activeButtonRef : undefined}
              type="button"
              role="tab"
              aria-selected={active}
              data-testid="project-strip-tab"
              onClick={() => onSelectWorkspace(workspaceId)}
              className={`flex h-8 min-w-[9rem] max-w-[16rem] items-center gap-2 rounded-md border px-2 text-left transition-colors ${
                active
                  ? "border-brand-600 bg-surface-800 text-text-primary"
                  : "border-transparent text-text-muted hover:border-surface-700 hover:bg-surface-800/70 hover:text-text-secondary"
              }`}
              title={group.repoPath}
            >
              <span
                className={`w-4 shrink-0 text-center font-mono text-[12px] ${textClass}`}
                aria-hidden="true"
              >
                <StatusGlyph
                  status={status}
                  createdAt={session?.created_at ?? null}
                  idleEnteredAt={session?.idle_entered_at ?? null}
                />
              </span>
              <span className="min-w-0 flex-1">
                <span className="block truncate text-[12px] leading-4">
                  {group.displayName}
                </span>
                <span className="block truncate font-mono text-[10px] leading-3 text-text-dim">
                  {count} session{count === 1 ? "" : "s"}
                </span>
              </span>
            </button>
          );
        })}
      </div>
    </nav>
  );
}
