import { useCallback, useState } from "react";
import { useSessions } from "./hooks/useSessions";
import { useKeyboardShortcuts } from "./hooks/useKeyboardShortcuts";
import { updateSession, createSession, deleteSession } from "./lib/api";
import type { SessionResponse } from "./lib/types";
import { Sidebar } from "./components/Sidebar";
import { TerminalView } from "./components/TerminalView";
import { DiffView } from "./components/DiffView";
import { EmptyState } from "./components/EmptyState";
import { RenameDialog } from "./components/RenameDialog";
import { ProfileSelector } from "./components/ProfileSelector";
import { HelpOverlay } from "./components/HelpOverlay";
import { SettingsView } from "./components/SettingsView";
import { WorktreeList } from "./components/WorktreeList";
import { ConfirmDialog } from "./components/ConfirmDialog";
import { MobileNav } from "./components/MobileNav";
import {
  CreateSessionPanel,
  type CreateSessionData,
} from "./components/CreateSessionPanel";

type ContentView = "terminal" | "diff" | "settings" | "worktrees";

export default function App() {
  const { sessions, error, refresh } = useSessions();
  const [activeId, setActiveId] = useState<string | null>(null);
  const [mobileShowTerminal, setMobileShowTerminal] = useState(false);
  const [contentView, setContentView] = useState<ContentView>("terminal");
  const [renameTarget, setRenameTarget] = useState<SessionResponse | null>(
    null,
  );
  const [deleteTarget, setDeleteTarget] = useState<SessionResponse | null>(
    null,
  );
  const [activeProfile, setActiveProfile] = useState<string | null>(null);
  const [showCreate, setShowCreate] = useState(false);
  const [showHelp, setShowHelp] = useState(false);
  const [sidebarSearchOpen, setSidebarSearchOpen] = useState(false);

  const filteredSessions = activeProfile
    ? sessions.filter(
        (s) =>
          s.group_path.startsWith(activeProfile) ||
          s.project_path.includes(activeProfile),
      )
    : sessions;

  const activeSession = sessions.find((s) => s.id === activeId);

  const handleSelect = (id: string) => {
    setActiveId(id);
    setContentView("terminal");
    setMobileShowTerminal(true);
  };

  const handleBack = () => {
    setMobileShowTerminal(false);
  };

  const handleRename = async (title: string, group: string) => {
    if (!renameTarget) return;
    await updateSession(renameTarget.id, {
      title: title !== renameTarget.title ? title : undefined,
      group_path: group !== renameTarget.group_path ? group : undefined,
    });
    setRenameTarget(null);
    refresh();
  };

  const handleDiff = (session: SessionResponse) => {
    setActiveId(session.id);
    setContentView("diff");
  };

  const handleCreate = async (data: CreateSessionData) => {
    const result = await createSession(data);
    if (result) {
      setShowCreate(false);
      setActiveId(result.id);
      setContentView("terminal");
      refresh();
    }
  };

  const handleDelete = async () => {
    if (!deleteTarget) return;
    await deleteSession(deleteTarget.id);
    setDeleteTarget(null);
    if (activeId === deleteTarget.id) setActiveId(null);
    refresh();
  };

  // Keyboard shortcuts
  useKeyboardShortcuts(
    useCallback(
      () => ({
        onSearch: () => setSidebarSearchOpen((v) => !v),
        onNew: () => setShowCreate(true),
        onDelete: () => {
          if (activeSession) setDeleteTarget(activeSession);
        },
        onRename: () => {
          if (activeSession) setRenameTarget(activeSession);
        },
        onDiff: () => {
          if (activeSession) handleDiff(activeSession);
        },
        onEscape: () => {
          setShowCreate(false);
          setShowHelp(false);
          setRenameTarget(null);
          setDeleteTarget(null);
        },
        onHelp: () => setShowHelp((h) => !h),
        onSettings: () =>
          setContentView((v) => (v === "settings" ? "terminal" : "settings")),
      }),
      [activeSession],
    ),
  );

  return (
    <div className="h-screen flex flex-col bg-surface-900 text-text-primary">
      {/* Header */}
      <header className="h-14 bg-surface-850 border-b border-surface-700/30 flex items-center px-5 shrink-0">
        <div className="flex items-center gap-2.5">
          <div className="w-6 h-6 rounded-md bg-brand-600/20 flex items-center justify-center">
            <span className="font-display text-xs font-bold text-brand-500">
              A
            </span>
          </div>
          <h1 className="font-display text-base font-semibold tracking-tight text-text-bright">
            Agent of Empires
          </h1>
        </div>

        <div className="ml-auto flex items-center gap-1">
          <button
            onClick={() => setContentView("worktrees")}
            className="hidden md:flex items-center gap-1.5 font-body text-xs text-text-dim hover:text-text-secondary hover:bg-surface-700/30 cursor-pointer px-2.5 py-1.5 rounded-md transition-colors"
            title="Worktrees"
          >
            Worktrees
          </button>
          <button
            onClick={() =>
              setContentView((v) =>
                v === "settings" ? "terminal" : "settings",
              )
            }
            className="hidden md:flex items-center gap-1.5 font-body text-xs text-text-dim hover:text-text-secondary hover:bg-surface-700/30 cursor-pointer px-2.5 py-1.5 rounded-md transition-colors"
            title="Settings (s)"
          >
            Settings
          </button>
          <button
            onClick={() => setShowHelp(true)}
            className="hidden md:flex items-center justify-center w-8 h-8 font-mono text-sm text-text-dim hover:text-text-secondary hover:bg-surface-700/30 cursor-pointer rounded-md transition-colors"
            title="Help (?)"
          >
            ?
          </button>
          <div className="hidden md:block w-px h-5 bg-surface-700/50 mx-1" />
          <ProfileSelector
            activeProfile={activeProfile}
            onSelect={setActiveProfile}
          />
          <div className="hidden md:block w-px h-5 bg-surface-700/50 mx-1" />
          <span className="font-mono text-xs text-text-dim tabular-nums">
            {error
              ? "offline"
              : `${filteredSessions.length} session${filteredSessions.length !== 1 ? "s" : ""}`}
          </span>
        </div>
      </header>

      {/* Main area -- sidebar and content side by side, full remaining height */}
      <div className="flex flex-1 min-h-0">
        {contentView !== "settings" && contentView !== "worktrees" && (
          <div
            className={`flex shrink-0 ${mobileShowTerminal ? "max-md:hidden" : ""}`}
          >
            <Sidebar
              sessions={filteredSessions}
              activeId={activeId}
              onSelect={handleSelect}
              onRefresh={refresh}
              onRename={setRenameTarget}
              onDiff={handleDiff}
              onNew={() => setShowCreate(true)}
              searchOpen={sidebarSearchOpen}
              onSearchToggle={setSidebarSearchOpen}
            />
          </div>
        )}

        <div
          className={`flex-1 flex flex-col min-h-0 ${!mobileShowTerminal && contentView !== "settings" && contentView !== "worktrees" ? "max-md:hidden" : ""}`}
        >
          {contentView === "settings" ? (
            <SettingsView onClose={() => setContentView("terminal")} />
          ) : contentView === "worktrees" ? (
            <WorktreeList
              onClose={() => setContentView("terminal")}
              onNavigateToSession={(id) => {
                setActiveId(id);
                setContentView("terminal");
              }}
            />
          ) : activeSession ? (
            contentView === "diff" ? (
              <DiffView
                sessionId={activeSession.id}
                onClose={() => setContentView("terminal")}
              />
            ) : (
              <TerminalView
                key={activeSession.id}
                session={activeSession}
                onBack={handleBack}
              />
            )
          ) : (
            <EmptyState />
          )}
        </div>
      </div>

      {/* Overlays */}
      {showCreate && (
        <CreateSessionPanel
          onSubmit={handleCreate}
          onCancel={() => setShowCreate(false)}
        />
      )}

      {renameTarget && (
        <RenameDialog
          currentTitle={renameTarget.title}
          currentGroup={renameTarget.group_path}
          onSave={handleRename}
          onCancel={() => setRenameTarget(null)}
        />
      )}

      {deleteTarget && (
        <ConfirmDialog
          title="Delete Session"
          message={`Delete "${deleteTarget.title}"? This will stop the session and remove it.`}
          confirmLabel="Delete"
          danger
          onConfirm={handleDelete}
          onCancel={() => setDeleteTarget(null)}
        />
      )}

      {showHelp && <HelpOverlay onClose={() => setShowHelp(false)} />}

      {/* Mobile bottom nav */}
      <MobileNav
        sessionCount={filteredSessions.length}
        activeSessionTitle={activeSession?.title ?? null}
        activeStatus={activeSession?.status ?? null}
        activeTab={
          contentView === "settings"
            ? "settings"
            : contentView === "worktrees"
              ? "worktrees"
              : "sessions"
        }
        onSessionsTab={() => {
          setContentView("terminal");
          setMobileShowTerminal(false);
        }}
        onSettingsTab={() => setContentView("settings")}
        onWorktreesTab={() => setContentView("worktrees")}
      />
    </div>
  );
}
