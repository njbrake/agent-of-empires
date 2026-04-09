import { useState } from "react";
import type { SessionResponse } from "../lib/types";
import { stopSession, restartSession, deleteSession } from "../lib/api";
import { SessionItem } from "./SessionItem";
import { SearchBar } from "./SearchBar";
import { ConfirmDialog } from "./ConfirmDialog";
import { SortSelect, type SortOrder } from "./SortSelect";

interface Props {
  sessions: SessionResponse[];
  activeId: string | null;
  onSelect: (id: string) => void;
  onRefresh: () => void;
  onRename: (session: SessionResponse) => void;
  onDiff: (session: SessionResponse) => void;
  onNew?: () => void;
}

export function Sidebar({
  sessions,
  activeId,
  onSelect,
  onRefresh,
  onRename,
  onDiff,
  onNew,
}: Props) {
  const [searchQuery, setSearchQuery] = useState("");
  const [showSearch, setShowSearch] = useState(false);
  const [sortOrder, setSortOrder] = useState<SortOrder>("created-desc");
  const [deleteTarget, setDeleteTarget] = useState<SessionResponse | null>(
    null,
  );

  const activeSession = sessions.find((s) => s.id === activeId);

  const searched = searchQuery
    ? sessions.filter(
        (s) =>
          s.title.toLowerCase().includes(searchQuery.toLowerCase()) ||
          s.project_path.toLowerCase().includes(searchQuery.toLowerCase()) ||
          s.tool.toLowerCase().includes(searchQuery.toLowerCase()) ||
          (s.branch || "").toLowerCase().includes(searchQuery.toLowerCase()),
      )
    : sessions;

  const filtered = [...searched].sort((a, b) => {
    switch (sortOrder) {
      case "created-desc":
        return b.created_at.localeCompare(a.created_at);
      case "created-asc":
        return a.created_at.localeCompare(b.created_at);
      case "accessed-desc":
        return (b.last_accessed_at || "").localeCompare(
          a.last_accessed_at || "",
        );
      case "accessed-asc":
        return (a.last_accessed_at || "").localeCompare(
          b.last_accessed_at || "",
        );
      case "title-asc":
        return a.title.localeCompare(b.title);
      case "title-desc":
        return b.title.localeCompare(a.title);
      default:
        return 0;
    }
  });

  // Group sessions by group_path
  const grouped = new Map<string, SessionResponse[]>();
  for (const s of filtered) {
    const group = s.group_path || "";
    if (!grouped.has(group)) grouped.set(group, []);
    grouped.get(group)!.push(s);
  }

  const handleStop = async (id: string) => {
    await stopSession(id);
    onRefresh();
  };

  const handleRestart = async (id: string) => {
    await restartSession(id);
    onRefresh();
  };

  const handleDelete = async () => {
    if (!deleteTarget) return;
    await deleteSession(deleteTarget.id);
    setDeleteTarget(null);
    onRefresh();
  };

  return (
    <aside className="w-[280px] min-w-[280px] bg-surface-900 border-r border-surface-700 flex flex-col overflow-hidden max-md:w-full max-md:min-w-full max-md:max-h-[40vh] max-md:border-r-0 max-md:border-b max-md:border-surface-700">
      {/* Header with new + search */}
      <div className="flex items-center justify-between px-3.5 pt-3 pb-2">
        <span className="font-mono text-[11px] font-semibold uppercase tracking-widest text-slate-500">
          Sessions
        </span>
        <div className="flex items-center gap-0.5">
          <SortSelect value={sortOrder} onChange={setSortOrder} />
          {onNew && (
            <button
              onClick={onNew}
              className="font-mono text-[11px] text-brand-600 hover:text-brand-500 cursor-pointer px-1"
              title="New session (n)"
            >
              +
            </button>
          )}
          <button
            onClick={() => setShowSearch(!showSearch)}
            className="font-mono text-[11px] text-slate-600 hover:text-slate-400 cursor-pointer px-1"
            title="Search (/)"
          >
            /
          </button>
        </div>
      </div>

      {showSearch && (
        <SearchBar
          value={searchQuery}
          onChange={setSearchQuery}
          onClose={() => {
            setShowSearch(false);
            setSearchQuery("");
          }}
        />
      )}

      {/* Session list with groups */}
      <div className="flex-1 overflow-y-auto px-1.5 pb-1.5">
        {filtered.length === 0 ? (
          <div className="px-3.5 py-5 text-center text-slate-600 text-xs font-body">
            {searchQuery ? (
              <>No sessions match &ldquo;{searchQuery}&rdquo;</>
            ) : (
              <>
                No sessions found.
                <br />
                <code className="font-mono text-brand-600 text-[11px]">
                  aoe add /path/to/project
                </code>
              </>
            )}
          </div>
        ) : (
          Array.from(grouped.entries()).map(([group, groupSessions]) => (
            <div key={group || "__ungrouped__"}>
              {group && (
                <div className="font-mono text-[10px] uppercase tracking-wider text-slate-600 px-3 pt-3 pb-1">
                  {group}
                </div>
              )}
              {groupSessions.map((s) => (
                <SessionItem
                  key={s.id}
                  session={s}
                  isActive={s.id === activeId}
                  onClick={() => onSelect(s.id)}
                />
              ))}
            </div>
          ))
        )}
      </div>

      {/* Actions for selected session */}
      {activeSession && (
        <div className="px-3.5 py-2.5 border-t border-surface-700 flex gap-1.5 flex-wrap">
          {activeSession.status !== "Stopped" && (
            <button
              onClick={() => handleStop(activeSession.id)}
              className="px-3 py-1 font-body text-xs rounded-md border border-status-error/40 text-status-error hover:bg-status-error/10 transition-colors cursor-pointer"
            >
              Stop
            </button>
          )}
          {(activeSession.status === "Stopped" ||
            activeSession.status === "Error") && (
            <button
              onClick={() => handleRestart(activeSession.id)}
              className="px-3 py-1 font-body text-xs rounded-md border border-brand-600/40 text-brand-500 hover:bg-brand-600/10 transition-colors cursor-pointer"
            >
              Restart
            </button>
          )}
          <button
            onClick={() => onRename(activeSession)}
            className="px-3 py-1 font-body text-xs rounded-md border border-surface-700 text-slate-400 hover:bg-surface-800 transition-colors cursor-pointer"
          >
            Rename
          </button>
          <button
            onClick={() => onDiff(activeSession)}
            className="px-3 py-1 font-body text-xs rounded-md border border-accent-600/40 text-accent-600 hover:bg-accent-600/10 transition-colors cursor-pointer"
          >
            Diff
          </button>
          <button
            onClick={() => setDeleteTarget(activeSession)}
            className="px-3 py-1 font-body text-xs rounded-md border border-status-error/20 text-slate-500 hover:text-status-error hover:bg-status-error/10 transition-colors cursor-pointer"
          >
            Delete
          </button>
        </div>
      )}

      {/* Delete confirmation */}
      {deleteTarget && (
        <ConfirmDialog
          title="Delete Session"
          message={`Delete "${deleteTarget.title}"? This will stop the session and remove it from the list.`}
          confirmLabel="Delete"
          danger
          onConfirm={handleDelete}
          onCancel={() => setDeleteTarget(null)}
        />
      )}
    </aside>
  );
}
