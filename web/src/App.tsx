import { useCallback, useMemo, useState } from "react";
import { useSessions } from "./hooks/useSessions";
import { useWorkspaces } from "./hooks/useWorkspaces";
import { useRepoGroups } from "./hooks/useRepoGroups";
import { useKeyboardShortcuts } from "./hooks/useKeyboardShortcuts";
import { isSessionActive } from "./lib/session";
import { createSession } from "./lib/api";
import { WorkspaceSidebar } from "./components/WorkspaceSidebar";
import { WorkspaceHeader } from "./components/WorkspaceHeader";
import { ContentSplit } from "./components/ContentSplit";
import { TerminalView } from "./components/TerminalView";
import { RightPanel } from "./components/RightPanel";
import { SettingsView } from "./components/SettingsView";
import { HelpOverlay } from "./components/HelpOverlay";
import { SessionWizard } from "./components/session-wizard/SessionWizard";
import { Dashboard } from "./components/Dashboard";

export default function App() {
  const { sessions, error } = useSessions();
  const workspaces = useWorkspaces(sessions);
  const { groups, toggleRepoCollapsed } = useRepoGroups(workspaces);

  const [activeWorkspaceId, setActiveWorkspaceId] = useState<string | null>(
    null,
  );
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [diffCollapsed, setDiffCollapsed] = useState(
    () => window.innerWidth < 768,
  );
  const [diffFileCount, setDiffFileCount] = useState(0);
  const [showAddProject, setShowAddProject] = useState(false);
  const [creatingForProject, setCreatingForProject] = useState<string | null>(null);
  const [showHelp, setShowHelp] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(
    () => window.innerWidth >= 768,
  );

  const activeWorkspace = workspaces.find((w) => w.id === activeWorkspaceId);
  const activeSession = activeWorkspace?.sessions.find(
    (s) => s.id === activeSessionId,
  );

  const alertCounts = useMemo(() => {
    let errors = 0;
    let waiting = 0;
    for (const s of sessions) {
      if (s.status === "Error") errors++;
      if (s.status === "Waiting") waiting++;
    }
    return { errors, waiting };
  }, [sessions]);

  const handleSelectSession = (sessionId: string) => {
    const ws = workspaces.find((w) => w.sessions.some((s) => s.id === sessionId));
    if (ws) {
      setActiveWorkspaceId(ws.id);
      setActiveSessionId(sessionId);
      if (window.innerWidth < 768) setSidebarOpen(false);
    }
  };

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

  const handleCreateSession = useCallback(async (repoPath: string) => {
    if (creatingForProject) return;
    setCreatingForProject(repoPath);

    const projectSessions = sessions
      .filter((s) => (s.main_repo_path || s.project_path) === repoPath)
      .sort((a, b) => (b.last_accessed_at ?? "").localeCompare(a.last_accessed_at ?? ""));
    const latest = projectSessions[0];

    await createSession({
      path: repoPath,
      tool: latest?.tool ?? "claude",
      group: latest?.group_path || undefined,
      yolo_mode: latest?.yolo_mode ?? false,
      worktree_branch: "",
      create_new_branch: true,
      sandbox: latest?.is_sandboxed ?? false,
    });

    setCreatingForProject(null);
  }, [sessions, creatingForProject]);

  const toggleDiff = () => setDiffCollapsed((c) => !c);

  useKeyboardShortcuts(
    useCallback(
      () => ({
        onNew: () => setShowAddProject(true),
        onDiff: () => toggleDiff(),
        onEscape: () => {
          setShowAddProject(false);
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
        <Dashboard
          sessions={sessions}
          onSelectSession={handleSelectSession}
          onAddProject={() => setShowAddProject(true)}
        />
      );
    }

    return (
      <div className="flex-1 flex flex-col min-h-0">
        <WorkspaceHeader
          workspace={activeWorkspace}
          activeSession={activeSession}
          diffCollapsed={diffCollapsed}
          diffFileCount={diffFileCount}
          onToggleDiff={toggleDiff}
        />

        <ContentSplit
          collapsed={diffCollapsed}
          onToggleCollapse={toggleDiff}
          left={
            <TerminalView key={activeSessionId} session={activeSession} />
          }
          right={
            <RightPanel
              session={activeSession ?? null}
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
      <header className="h-12 bg-surface-800 border-b border-surface-700/20 flex items-center px-3 shrink-0 gap-2">
        <button
          onClick={() => setSidebarOpen((o) => !o)}
          className={`w-8 h-8 flex items-center justify-center cursor-pointer rounded-md transition-colors hover:bg-surface-700/50 ${
            sidebarOpen
              ? "text-text-secondary hover:text-text-primary"
              : "text-text-dim hover:text-text-secondary"
          }`}
          title="Toggle sidebar"
          aria-label="Toggle sidebar"
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
            <rect x="3" y="3" width="18" height="18" rx="2" />
            <line x1="9" y1="3" x2="9" y2="21" />
          </svg>
        </button>

        <button
          onClick={() => { setActiveWorkspaceId(null); setActiveSessionId(null); setShowSettings(false); }}
          className="flex items-center gap-1.5 text-text-muted hover:text-text-secondary transition-colors cursor-pointer"
          aria-label="Go to dashboard"
        >
          <img src="/icon-192.png" alt="" width="18" height="18" className="rounded-sm" />
          <span className="font-mono text-xs leading-none">aoe</span>
        </button>

        <div className="flex-1" />

        <div className="flex items-center gap-1.5">
          {alertCounts.errors > 0 && (
            <span className="font-mono text-[11px] px-1.5 py-0.5 rounded-full bg-status-error/10 text-status-error">
              {alertCounts.errors} error{alertCounts.errors !== 1 ? "s" : ""}
            </span>
          )}
          {alertCounts.waiting > 0 && (
            <span className="font-mono text-[11px] px-1.5 py-0.5 rounded-full bg-status-waiting/10 text-status-waiting">
              {alertCounts.waiting} waiting
            </span>
          )}
          {error && (
            <span className="font-mono text-xs text-status-error">
              offline
            </span>
          )}
          {activeWorkspace && activeSession && (
            <button
              onClick={toggleDiff}
              className={`w-8 h-8 flex items-center justify-center cursor-pointer rounded-md transition-colors hover:bg-surface-700/50 ${
                diffCollapsed
                  ? "text-text-dim hover:text-text-secondary"
                  : "text-text-secondary hover:text-text-primary"
              }`}
              title="Toggle diff panel"
              aria-label="Toggle diff panel"
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
                <rect x="3" y="3" width="18" height="18" rx="2" />
                <line x1="15" y1="3" x2="15" y2="21" />
              </svg>
            </button>
          )}
        </div>
      </header>

      {/* Main: sidebar + content */}
      <div className="flex flex-1 min-h-0">
        {sidebarOpen && (
          <WorkspaceSidebar
            groups={groups}
            activeId={activeWorkspaceId}
            creatingForProject={creatingForProject}
            onToggle={() => setSidebarOpen(false)}
            onSelect={handleSelectWorkspace}
            onToggleRepo={toggleRepoCollapsed}
            onNew={() => setShowAddProject(true)}
            onCreateSession={handleCreateSession}
            onSettings={() => { setShowSettings((s) => !s); if (window.innerWidth < 768) setSidebarOpen(false); }}
          />
        )}

        <div className="flex-1 flex flex-col min-h-0 min-w-0">
          {renderContent()}
        </div>
      </div>

      {/* Add project wizard */}
      {showAddProject && (
        <SessionWizard
          onClose={() => setShowAddProject(false)}
          onCreated={() => setShowAddProject(false)}
        />
      )}

      {showHelp && <HelpOverlay onClose={() => setShowHelp(false)} />}
    </div>
  );
}
