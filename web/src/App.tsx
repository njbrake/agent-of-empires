import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useSessions } from "./hooks/useSessions";
import { useWorkspaces, setLifecycleOverride } from "./hooks/useWorkspaces";
import { useKeyboardShortcuts } from "./hooks/useKeyboardShortcuts";
import { createSession, stopSession, restartSession } from "./lib/api";
import { isSessionActive } from "./lib/session";
import type { WorkspaceStatus } from "./lib/types";
import { WorkspaceSidebar } from "./components/WorkspaceSidebar";
import { WorkspaceHeader } from "./components/WorkspaceHeader";
import { ContentSplit } from "./components/ContentSplit";
import { TerminalView } from "./components/TerminalView";
import { DiffPanel } from "./components/DiffPanel";
import { SettingsView } from "./components/SettingsView";
import { HelpOverlay } from "./components/HelpOverlay";
import {
  CreateWorkspaceModal,
  type CreateWorkspaceData,
} from "./components/CreateWorkspaceModal";

export default function App() {
  const { sessions, error, refresh } = useSessions();
  const workspaces = useWorkspaces(sessions);

  const [activeWorkspaceId, setActiveWorkspaceId] = useState<string | null>(
    null,
  );
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [diffCollapsed, setDiffCollapsed] = useState(
    () => window.innerWidth < 768,
  );
  const [diffFileCount, setDiffFileCount] = useState(0);
  const [showCreate, setShowCreate] = useState(false);
  const [showHelp, setShowHelp] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(
    () => window.innerWidth >= 768,
  );
  const [actionPending, setActionPending] = useState(false);

  // For post-create selection: store what to select, pick it up when sessions update
  const pendingSelectRef = useRef<{
    wsId: string;
    sessionId: string;
  } | null>(null);

  const activeWorkspace = workspaces.find((w) => w.id === activeWorkspaceId);
  const activeSession = activeWorkspace?.sessions.find(
    (s) => s.id === activeSessionId,
  );

  const knownPaths = useMemo(() => {
    const paths = new Set(sessions.map((s) => s.project_path));
    return [...paths].sort();
  }, [sessions]);

  // Alert counts for header
  const alertCounts = useMemo(() => {
    let errors = 0;
    let waiting = 0;
    for (const s of sessions) {
      if (s.status === "Error") errors++;
      if (s.status === "Waiting") waiting++;
    }
    return { errors, waiting };
  }, [sessions]);

  // Pick up pending selection when sessions update after create
  useEffect(() => {
    if (!pendingSelectRef.current) return;
    const { wsId, sessionId } = pendingSelectRef.current;
    const found = sessions.find((s) => s.id === sessionId);
    if (found) {
      setActiveWorkspaceId(wsId);
      setActiveSessionId(sessionId);
      pendingSelectRef.current = null;
    }
  }, [sessions]);

  const handleSelectWorkspace = (workspaceId: string) => {
    setActiveWorkspaceId(workspaceId);
    const ws = workspaces.find((w) => w.id === workspaceId);
    if (ws) {
      const running = ws.sessions.find((s) => isSessionActive(s.status));
      setActiveSessionId(running?.id ?? ws.sessions[0]?.id ?? null);
    }
    if (window.innerWidth < 768) {
      setSidebarOpen(false);
    }
  };

  const handleCreate = async (data: CreateWorkspaceData) => {
    const result = await createSession(data);
    if (result) {
      setShowCreate(false);
      const path = data.path.replace(/\/+$/, "");
      const branch = data.worktree_branch ?? null;
      pendingSelectRef.current = {
        wsId: `${path}::${branch ?? "__default__"}`,
        sessionId: result.id,
      };
      refresh();
    }
  };

  const handleStop = async () => {
    if (!activeSessionId) return;
    setActionPending(true);
    await stopSession(activeSessionId);
    refresh();
    setActionPending(false);
  };

  const handleRestart = async () => {
    if (!activeSessionId) return;
    setActionPending(true);
    await restartSession(activeSessionId);
    refresh();
    setActionPending(false);
  };

  const handleLifecycleChange = (status: WorkspaceStatus) => {
    if (!activeWorkspace) return;
    if (status === "active") {
      setLifecycleOverride(activeWorkspace.id, null);
    } else {
      setLifecycleOverride(activeWorkspace.id, status);
    }
    refresh();
  };

  const toggleDiff = () => setDiffCollapsed((c) => !c);

  useKeyboardShortcuts(
    useCallback(
      () => ({
        onNew: () => setShowCreate(true),
        onDiff: () => toggleDiff(),
        onEscape: () => {
          setShowCreate(false);
          setShowHelp(false);
          setShowSettings(false);
        },
        onHelp: () => setShowHelp((h) => !h),
        onSettings: () => setShowSettings((s) => !s),
      }),
      [],
    ),
  );

  const renderContent = () => {
    if (showSettings) {
      return <SettingsView onClose={() => setShowSettings(false)} />;
    }

    if (!activeWorkspace || !activeSession) {
      return (
        <div className="flex-1 flex flex-col items-center justify-center bg-surface-950 px-4">
          <p className="font-body text-sm text-text-dim text-center">
            {workspaces.length === 0
              ? "No sessions yet"
              : "Select a session"}
          </p>
          {workspaces.length === 0 && (
            <button
              onClick={() => setShowCreate(true)}
              className="mt-3 px-4 py-1.5 font-body text-xs rounded-md bg-brand-600 text-surface-950 font-semibold hover:bg-brand-700 cursor-pointer transition-colors"
            >
              Create session
            </button>
          )}
        </div>
      );
    }

    return (
      <div className="flex-1 flex flex-col min-h-0">
        <WorkspaceHeader
          workspace={activeWorkspace}
          activeSession={activeSession}
          diffCollapsed={diffCollapsed}
          diffFileCount={diffFileCount}
          actionPending={actionPending}
          onStop={handleStop}
          onRestart={handleRestart}
          onLifecycleChange={handleLifecycleChange}
          onToggleDiff={toggleDiff}
        />

        <ContentSplit
          collapsed={diffCollapsed}
          onToggleCollapse={toggleDiff}
          left={
            <TerminalView key={activeSessionId} session={activeSession} />
          }
          right={
            <DiffPanel
              sessionId={activeSessionId}
              expanded={!diffCollapsed}
              onFileCountChange={setDiffFileCount}
            />
          }
        />
      </div>
    );
  };

  return (
    <div className="h-dvh flex flex-col bg-surface-900 text-text-primary overflow-hidden">
      {/* Header */}
      <header className="h-10 bg-surface-850 border-b border-surface-700/30 flex items-center px-2 shrink-0 gap-1.5">
        <button
          onClick={() => setSidebarOpen((o) => !o)}
          className="w-10 h-10 flex items-center justify-center text-text-dim hover:text-text-secondary cursor-pointer rounded-md hover:bg-surface-700/30 transition-colors -ml-1"
          title="Toggle sidebar"
          aria-label="Toggle sidebar"
        >
          <svg
            width="18"
            height="18"
            viewBox="0 0 16 16"
            fill="none"
            stroke="currentColor"
            strokeWidth="1.5"
            strokeLinecap="round"
          >
            <line x1="2" y1="4" x2="14" y2="4" />
            <line x1="2" y1="8" x2="14" y2="8" />
            <line x1="2" y1="12" x2="14" y2="12" />
          </svg>
        </button>

        <div className="flex-1" />

        <div className="ml-auto flex items-center gap-1">
          {/* Alert badges */}
          {alertCounts.errors > 0 && (
            <span className="font-mono text-[11px] px-1.5 py-0.5 rounded-full bg-status-error/15 text-status-error">
              {alertCounts.errors} error{alertCounts.errors !== 1 ? "s" : ""}
            </span>
          )}
          {alertCounts.waiting > 0 && (
            <span className="font-mono text-[11px] px-1.5 py-0.5 rounded-full bg-status-waiting/15 text-status-waiting">
              {alertCounts.waiting} waiting
            </span>
          )}
          {error && (
            <span className="font-mono text-xs text-status-error">
              offline
            </span>
          )}
        </div>
      </header>

      {/* Main: sidebar + content */}
      <div className="flex flex-1 min-h-0">
        {sidebarOpen && (
          <WorkspaceSidebar
            workspaces={workspaces}
            activeId={activeWorkspaceId}
            onToggle={() => setSidebarOpen(false)}
            onSelect={handleSelectWorkspace}
            onNew={() => setShowCreate(true)}
            onSettings={() => setShowSettings((s) => !s)}
          />
        )}

        <div className="flex-1 flex flex-col min-h-0 min-w-0">
          {renderContent()}
        </div>
      </div>

      {/* Overlays */}
      {showCreate && (
        <CreateWorkspaceModal
          knownPaths={knownPaths}
          onSubmit={handleCreate}
          onCancel={() => setShowCreate(false)}
        />
      )}

      {showHelp && <HelpOverlay onClose={() => setShowHelp(false)} />}
    </div>
  );
}
