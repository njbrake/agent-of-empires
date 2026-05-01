import { memo, useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import type { Workspace, RepoGroup, SessionStatus } from "../lib/types";
import {
  STATUS_DOT_CLASS,
  getStatusTextClass,
  isSessionActive,
} from "../lib/session";
import { renameSession, setSessionNotifications } from "../lib/api";
import { StatusGlyph } from "./StatusGlyph";
import { OwnerAvatar } from "./OwnerAvatar";

const SIDEBAR_WIDTH_KEY = "aoe-sidebar-width";
const DEFAULT_WIDTH = 280;
const MIN_WIDTH = 200;
const MAX_WIDTH = 480;

// Module-level bus for closing any open SessionRow context menu when a
// new one opens. Each SessionRow manages its own menu state; without
// this bus, long-pressing a second session on mobile leaves the first
// menu visible because document "click" listeners don't fire on
// touchstart. Publishing on open + subscribing here keeps "one menu at
// a time" without lifting state up to the parent.
const menuBus = new EventTarget();
function closeOtherContextMenus() {
  menuBus.dispatchEvent(new Event("close"));
}

interface Props {
  groups: RepoGroup[];
  activeId: string | null;
  open: boolean;
  onToggle: () => void;
  onSelect: (workspaceId: string) => void;
  onToggleRepo: (repoId: string) => void;
  onNew: () => void;
  onCreateSession: (repoPath: string) => void;
  onSettings: () => void;
  onDeleteSession?: (workspaceId: string) => void;
  readOnly?: boolean;
}

function bestSession(
  ws: Workspace,
): {
  status: SessionStatus;
  createdAt: string | null;
  idleEnteredAt: string | null;
} {
  const running = ws.sessions.find((s) => isSessionActive(s));
  if (running)
    return {
      status: running.status,
      createdAt: running.created_at,
      idleEnteredAt: running.idle_entered_at ?? null,
    };
  const error = ws.sessions.find((s) => s.status === "Error");
  if (error)
    return {
      status: "Error",
      createdAt: error.created_at,
      idleEnteredAt: null,
    };
  const first = ws.sessions[0];
  return {
    status: first?.status ?? "Unknown",
    createdAt: first?.created_at ?? null,
    idleEnteredAt: first?.idle_entered_at ?? null,
  };
}

/** Derive which of the three context-menu presets best describes a
 *  session's current per-event notification overrides. If the three
 *  overrides aren't all the same value, the session is in a "custom"
 *  mixed state, which the context menu renders as "Default" too
 *  (selecting "Default" then resets it cleanly). */
type NotifyPreset = "off" | "default" | "all";
function detectNotifyPreset(
  waiting: boolean | null | undefined,
  idle: boolean | null | undefined,
  error: boolean | null | undefined,
): NotifyPreset {
  if (waiting === false && idle === false && error === false) return "off";
  if (waiting === true && idle === true && error === true) return "all";
  return "default";
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
  onDelete,
  readOnly,
  indented,
}: {
  workspace: Workspace;
  isActive: boolean;
  onClick: () => void;
  onDelete?: (workspaceId: string) => void;
  readOnly?: boolean;
  indented?: boolean;
}) {
  const { status: sessionStatus, createdAt, idleEnteredAt } = bestSession(workspace);
  const textClass = getStatusTextClass({
    status: sessionStatus,
    idle_entered_at: idleEnteredAt,
  });
  const label =
    workspace.branch ?? workspace.sessions[0]?.title ?? "default";
  const firstSession = workspace.sessions[0];
  const sessionId = firstSession?.id;
  const isDeleting = sessionStatus === "Deleting";
  const notifyPreset = detectNotifyPreset(
    firstSession?.notify_on_waiting,
    firstSession?.notify_on_idle,
    firstSession?.notify_on_error,
  );

  const setNotifyPreset = async (preset: NotifyPreset) => {
    setContextMenu(null);
    if (!sessionId || preset === notifyPreset) return;
    await setSessionNotifications(sessionId, preset);
  };

  const [contextMenu, setContextMenu] = useState<{ x: number; y: number } | null>(null);
  const [renaming, setRenaming] = useState(false);
  const [renameValue, setRenameValue] = useState(label);
  const renameRef = useRef<HTMLInputElement>(null);
  const longPressTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const longPressFired = useRef(false);
  const touchOpenedAt = useRef(0);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    return () => {
      if (longPressTimer.current) clearTimeout(longPressTimer.current);
    };
  }, []);

  useEffect(() => {
    if (renaming) renameRef.current?.select();
  }, [renaming]);

  useEffect(() => {
    if (!contextMenu) return;
    const close = () => setContextMenu(null);
    const onDocClick = (e: MouseEvent) => {
      // Clicks inside the menu should be handled by item onClick
      // handlers, not by this dismiss listener.
      if (menuRef.current?.contains(e.target as Node)) return;
      // On mobile, lifting the finger after a long-press dispatches a
      // synthetic click even when touchend called preventDefault().
      // Ignore clicks that arrive shortly after a touch-triggered open.
      if (Date.now() - touchOpenedAt.current < 500) return;
      close();
    };
    // Defer so the event that opened the menu finishes bubbling first
    const id = requestAnimationFrame(() => {
      document.addEventListener("click", onDocClick);
      document.addEventListener("contextmenu", close);
    });
    // Listen for the "close" broadcast from any sibling SessionRow
    // that is opening its own menu.
    menuBus.addEventListener("close", close);
    return () => {
      cancelAnimationFrame(id);
      document.removeEventListener("click", onDocClick);
      document.removeEventListener("contextmenu", close);
      menuBus.removeEventListener("close", close);
    };
  }, [contextMenu]);

  const handleContextMenu = (e: React.MouseEvent) => {
    if (isDeleting) return;
    e.preventDefault();
    closeOtherContextMenus();
    setContextMenu({ x: e.clientX, y: e.clientY });
  };

  const clearLongPress = () => {
    if (longPressTimer.current) {
      clearTimeout(longPressTimer.current);
      longPressTimer.current = null;
    }
  };

  const handleTouchStart = (e: React.TouchEvent) => {
    clearLongPress();
    longPressFired.current = false;
    if (!sessionId || isDeleting) return;
    const touch = e.touches[0];
    if (!touch) return;
    const tx = touch.clientX;
    const ty = touch.clientY;
    longPressTimer.current = setTimeout(() => {
      longPressFired.current = true;
      touchOpenedAt.current = Date.now();
      closeOtherContextMenus();
      setContextMenu({ x: tx, y: ty });
    }, 500);
  };

  const handleTouchEnd = (e: React.TouchEvent) => {
    clearLongPress();
    if (longPressFired.current) {
      e.preventDefault();
    }
  };

  const startRename = () => {
    if (renaming) return;
    setContextMenu(null);
    setRenameValue(label);
    setRenaming(true);
  };

  const commitRename = async () => {
    setRenaming(false);
    const trimmed = renameValue.trim();
    if (!trimmed || trimmed === label || !sessionId) return;
    await renameSession(sessionId, trimmed);
  };

  const handleDelete = () => {
    setContextMenu(null);
    onDelete?.(workspace.id);
  };

  if (renaming) {
    return (
      <div className={`py-1 ${indented ? "pl-6 pr-3" : "px-3"}`}>
        <input
          ref={renameRef}
          type="text"
          value={renameValue}
          onChange={(e) => setRenameValue(e.target.value)}
          onBlur={commitRename}
          onKeyDown={(e) => {
            if (e.key === "Enter") commitRename();
            if (e.key === "Escape") setRenaming(false);
          }}
          className="w-full bg-surface-900 border border-brand-600 rounded px-2 py-1 text-[13px] md:text-[14px] font-mono text-text-primary focus:outline-none"
        />
      </div>
    );
  }

  return (
    <>
      <button
        onClick={() => { if (!longPressFired.current) onClick(); }}
        onContextMenu={handleContextMenu}
        onTouchStart={handleTouchStart}
        onTouchEnd={handleTouchEnd}
        onTouchMove={clearLongPress}
        onTouchCancel={clearLongPress}
        className={`w-full text-left py-2 cursor-pointer select-none transition-colors duration-75 ${
          indented ? "pl-6 pr-3" : "px-3"
        } ${
          isActive
            ? "bg-surface-850 border-l-2 border-brand-600"
            : "border-l-2 border-transparent hover:bg-surface-700/40"
        } ${isDeleting ? "opacity-50 pointer-events-none" : ""}`}
      >
        <div className="flex items-center gap-2">
          <span
            className={`text-sm shrink-0 leading-none font-mono ${textClass}`}
          >
            <StatusGlyph
              status={sessionStatus}
              createdAt={createdAt}
              idleEnteredAt={idleEnteredAt}
            />
          </span>
          <span className={`text-[13px] md:text-[14px] truncate flex-1 ${isSessionActive({ status: sessionStatus, idle_entered_at: idleEnteredAt }) ? textClass : isActive ? "text-text-primary" : "text-text-secondary"}`} title={label}>
            {label}
          </span>
        </div>
      </button>
      {contextMenu && createPortal(
        <div
          ref={menuRef}
          className="fixed z-50 bg-surface-800 border border-surface-700 rounded-lg shadow-lg py-1 min-w-[180px]"
          style={{ left: contextMenu.x, top: contextMenu.y }}
        >
          <button
            onClick={startRename}
            className="w-full text-left px-3 py-2 md:py-2 max-md:py-3 text-sm text-text-secondary hover:bg-surface-700/50 cursor-pointer transition-colors"
          >
            Rename
          </button>
          <div className="border-t border-surface-700/20 my-1" />
          <div className="px-3 py-1 text-[11px] font-mono uppercase tracking-widest text-text-muted">
            Notifications
          </div>
          {(["off", "default", "all"] as const).map((preset) => {
            const label =
              preset === "off"
                ? "Off"
                : preset === "default"
                  ? "Default"
                  : "All events";
            const selected = notifyPreset === preset;
            return (
              <button
                key={preset}
                onClick={() => void setNotifyPreset(preset)}
                className={`w-full text-left pl-6 pr-3 py-2 md:py-2 max-md:py-3 text-sm hover:bg-surface-700/50 cursor-pointer transition-colors flex items-center gap-2 ${
                  selected ? "text-text-primary" : "text-text-secondary"
                }`}
              >
                <span className="w-3 text-brand-500">
                  {selected ? "✓" : ""}
                </span>
                {label}
              </button>
            );
          })}
          {!readOnly && (
            <>
              <div className="border-t border-surface-700/20 my-1" />
              <button
                onClick={handleDelete}
                className="w-full text-left px-3 py-2 md:py-2 max-md:py-3 text-sm text-status-error hover:bg-status-error/10 cursor-pointer transition-colors"
              >
                Delete
              </button>
            </>
          )}
        </div>,
        document.body,
      )}
    </>
  );
});

const RepoGroupHeader = memo(function RepoGroupHeader({
  group,
  hasActiveChild,
  onClick,
  onNewSession,
}: {
  group: RepoGroup;
  hasActiveChild: boolean;
  onClick: () => void;
  onNewSession: () => void;
}) {
  const dotClass =
    STATUS_DOT_CLASS[
      group.status === "active" ? "Running" : "Idle"
    ] ?? "bg-status-idle";

  return (
    <div
      className={`flex items-center gap-2 px-3 py-2 transition-colors duration-75 text-text-secondary hover:bg-surface-800/50 ${
        hasActiveChild ? "border-l-2 border-brand-600" : ""
      }`}
    >
      <span className={`w-2 h-2 rounded-full shrink-0 ${dotClass}`} />
      <button
        onClick={onClick}
        aria-expanded={!group.collapsed}
        className="flex items-center gap-2 flex-1 min-w-0 text-left cursor-pointer"
      >
        <svg
          width="10"
          height="10"
          viewBox="0 0 10 10"
          fill="currentColor"
          className={`shrink-0 text-text-dim transition-transform duration-75 ${
            group.collapsed ? "-rotate-90" : ""
          }`}
        >
          <path d="M2 3 L5 6.5 L8 3" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round" />
        </svg>
        <OwnerAvatar owner={group.remoteOwner} size={16} />
        <span className="text-[13px] md:text-[14px] font-medium truncate flex-1" title={group.repoPath}>
          {group.displayName}
        </span>
      </button>
      <Tooltip text="New session">
        <button
          onClick={onNewSession}
          className="w-8 h-8 flex items-center justify-center shrink-0 rounded-md transition-colors text-text-muted hover:text-text-secondary hover:bg-surface-700/50 cursor-pointer"
          aria-label={`New session in ${group.displayName}`}
        >
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round">
            <line x1="12" y1="5" x2="12" y2="19" />
            <line x1="5" y1="12" x2="19" y2="12" />
          </svg>
        </button>
      </Tooltip>
    </div>
  );
});

function Tooltip({ text, children }: { text: string; children: React.ReactNode }) {
  return (
    <span className="relative group/tip inline-flex">
      {children}
      <span className="pointer-events-none absolute left-1/2 -translate-x-1/2 top-full mt-1.5 px-2 py-1 rounded bg-surface-950 border border-surface-700 text-[11px] text-text-secondary whitespace-nowrap opacity-0 scale-95 transition-all duration-100 group-hover/tip:opacity-100 group-hover/tip:scale-100 z-50">
        {text}
      </span>
    </span>
  );
}

function workspaceMatchesFilter(ws: Workspace, q: string): boolean {
  return (
    ws.displayName.toLowerCase().includes(q) ||
    ws.projectPath.toLowerCase().includes(q) ||
    ws.agents.some((a) => a.toLowerCase().includes(q)) ||
    ws.sessions.some((s) => s.title.toLowerCase().includes(q))
  );
}

export function WorkspaceSidebar({
  groups,
  activeId,
  open,
  onToggle,
  onSelect,
  onToggleRepo,
  onNew,
  onCreateSession,
  onSettings,
  onDeleteSession,
  readOnly,
}: Props) {
  const [width, setWidth] = useState(loadSavedWidth);
  const [filterOpen, setFilterOpen] = useState(false);
  const [filterQuery, setFilterQuery] = useState("");
  const filterRef = useRef<HTMLInputElement>(null);
  const dragging = useRef(false);

  const q = filterQuery.trim().toLowerCase();

  const filteredGroups = q
    ? groups
        .map((g) => ({
          ...g,
          workspaces: g.workspaces.filter((ws) =>
            workspaceMatchesFilter(ws, q) ||
            g.displayName.toLowerCase().includes(q),
          ),
        }))
        .filter((g) => g.workspaces.length > 0)
    : groups;

  const hasResults = filteredGroups.length > 0;

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
        className={`fixed top-12 inset-x-0 bottom-0 z-30 md:hidden transition-opacity duration-300 ${
          open ? "bg-black/50" : "opacity-0 pointer-events-none"
        }`}
        onClick={onToggle}
      />
      <div
        style={{ width }}
        className={`fixed top-12 bottom-0 left-0 z-40 md:static md:z-auto bg-surface-800 flex flex-col md:h-full shrink-0 transition-transform duration-300 ease-in-out md:transition-none ${
          open ? "translate-x-0" : "-translate-x-full md:hidden"
        }`}
      >
        <div className="px-3 pt-3 pb-1 flex items-center">
          <span className="text-sm text-text-muted flex-1">
            Projects
          </span>
          <Tooltip text="Filter">
            <button
              onClick={toggleFilter}
              className={`w-8 h-8 flex items-center justify-center cursor-pointer rounded-md transition-colors ${
                filterOpen
                  ? "text-text-secondary"
                  : "text-text-dim hover:text-text-secondary"
              }`}
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
          </Tooltip>
          <Tooltip text="Add project">
            <button
              onClick={onNew}
              className="w-8 h-8 flex items-center justify-center text-text-muted hover:text-text-secondary hover:bg-surface-800 cursor-pointer rounded-md transition-colors"
              aria-label="Add project"
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
                <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z" />
                <line x1="12" y1="11" x2="12" y2="17" />
                <line x1="9" y1="14" x2="15" y2="14" />
              </svg>
            </button>
          </Tooltip>
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
              className="w-full bg-surface-800 border border-surface-700 rounded-md px-2.5 py-1.5 text-[13px] text-text-primary placeholder:text-text-dim focus:border-brand-600 focus:outline-none"
            />
          </div>
        )}

        <div className="flex-1 overflow-y-auto overflow-x-hidden">
          {filteredGroups.map((group) => {
            const showExpanded = q ? true : !group.collapsed;
            const hasActiveChild = group.workspaces.some(
              (ws) => ws.id === activeId,
            );
            return (
              <div key={group.id}>
                <RepoGroupHeader
                  group={{ ...group, collapsed: !showExpanded }}
                  hasActiveChild={!showExpanded && hasActiveChild}
                  onClick={() => !q && onToggleRepo(group.id)}
                  onNewSession={() => onCreateSession(group.repoPath)}
                />
                {showExpanded &&
                  group.workspaces.map((ws) => (
                    <SessionRow
                      key={ws.id}
                      workspace={ws}
                      isActive={ws.id === activeId}
                      onClick={() => onSelect(ws.id)}
                      onDelete={onDeleteSession}
                      readOnly={readOnly}
                      indented
                    />
                  ))}
              </div>
            );
          })}

          {!hasResults && filterQuery && (
            <div className="px-4 py-8 text-center">
              <p className="text-sm text-text-muted">
                No matches for &ldquo;{filterQuery}&rdquo;
              </p>
            </div>
          )}
        </div>

        <div className="border-t border-surface-700/20 p-2">
          <button
            onClick={onSettings}
            className="w-8 h-8 flex items-center justify-center text-text-secondary hover:text-text-primary hover:bg-surface-800/50 cursor-pointer rounded-md transition-colors"
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
          </button>
        </div>
      </div>
      {/* Resize handle (desktop only) */}
      <div
        onMouseDown={handleMouseDown}
        className="hidden md:block w-1 cursor-col-resize shrink-0 bg-surface-800 hover:bg-brand-600/50 transition-colors duration-75"
      />
    </>
  );
}
