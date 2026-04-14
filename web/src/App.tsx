import { useCallback, useEffect, useRef, useState } from "react";
import { isSessionActive } from "./lib/session";
import { useSessions } from "./hooks/useSessions";
import { useWorkspaces } from "./hooks/useWorkspaces";
import { useRepoGroups } from "./hooks/useRepoGroups";
import { useKeyboardShortcuts } from "./hooks/useKeyboardShortcuts";
import { useDiffFiles } from "./hooks/useDiffFiles";
import { createSession, loginStatus, logout } from "./lib/api";
import { WorkspaceSidebar } from "./components/WorkspaceSidebar";
import { WorkspaceHeader } from "./components/WorkspaceHeader";
import { ContentSplit } from "./components/ContentSplit";
import { TerminalView } from "./components/TerminalView";
import { RightPanel } from "./components/RightPanel";
import { DiffFileViewer } from "./components/diff/DiffFileViewer";
import { SettingsView } from "./components/SettingsView";
import { HelpOverlay } from "./components/HelpOverlay";
import { SessionWizard } from "./components/session-wizard/SessionWizard";
import { Dashboard } from "./components/Dashboard";
import { LoginPage } from "./components/LoginPage";

export default function App() {
  const [loginRequired, setLoginRequired] = useState<boolean | null>(null);
  const [loginAuthenticated, setLoginAuthenticated] = useState(true);

  useEffect(() => {
    loginStatus().then(({ required, authenticated }) => {
      setLoginRequired(required);
      setLoginAuthenticated(authenticated);
    });
  }, []);

  const handleLoginSuccess = () => {
    setLoginAuthenticated(true);
  };

  const handleLogout = async () => {
    await logout();
    setLoginAuthenticated(false);
  };

  if (loginRequired && !loginAuthenticated) {
    return <LoginPage onSuccess={handleLoginSuccess} />;
  }

  if (loginRequired === null) {
    return <div className="h-dvh bg-surface-900" />;
  }

  return <AppContent loginRequired={loginRequired} onLogout={handleLogout} />;
}

function AppContent({ loginRequired, onLogout }: { loginRequired: boolean; onLogout: () => void }) {
  const { sessions, error } = useSessions();
  const workspaces = useWorkspaces(sessions);
  const { groups, toggleRepoCollapsed } = useRepoGroups(workspaces);

  const [activeWorkspaceId, setActiveWorkspaceId] = useState<string | null>(
    null,
  );
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [selectedFilePath, setSelectedFilePath] = useState<string | null>(null);
  const [diffCollapsed, setDiffCollapsed] = useState(
    () => window.innerWidth < 768,
  );
  const [showAddProject, setShowAddProject] = useState(false);
  const [creatingForProject, setCreatingForProject] = useState<string | null>(null);
  const [showHelp, setShowHelp] = useState(false);
  const [showSettings, setShowSettings] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(
    () => window.innerWidth >= 768,
  );
  const keyboardProxyRef = useRef<HTMLTextAreaElement>(null);

  const activeWorkspace = workspaces.find((w) => w.id === activeWorkspaceId);
  const activeSession = activeWorkspace?.sessions.find(
    (s) => s.id === activeSessionId,
  );

  // Fetch diff files at App level so we can share with RightPanel and viewer.
  // Only poll when a session is selected and the right panel is visible.
  const { files: diffFiles, baseBranch, warning, loading: diffFilesLoading, revision } =
    useDiffFiles(activeSessionId, !diffCollapsed);

  // Reset file selection when session changes, when the selected file
  // disappears from the list, or when the list becomes empty (all changes
  // reverted/committed). Guard on diffFilesLoading so we don't clear the
  // selection during the brief gap before the first fetch completes.
  useEffect(() => {
    if (!activeSessionId) {
      setSelectedFilePath(null);
      return;
    }
    if (
      selectedFilePath &&
      !diffFilesLoading &&
      !diffFiles.some((f) => f.path === selectedFilePath)
    ) {
      setSelectedFilePath(null);
    }
  }, [activeSessionId, diffFiles, diffFilesLoading, selectedFilePath]);

  // Reset file selection when switching sessions.
  useEffect(() => {
    setSelectedFilePath(null);
  }, [activeSessionId]);

  const focusKeyboardProxy = () => {
    if (window.innerWidth < 768 && navigator.maxTouchPoints > 0) {
      keyboardProxyRef.current?.focus();
    }
  };

  const handleSelectSession = (sessionId: string) => {
    const ws = workspaces.find((w) => w.sessions.some((s) => s.id === sessionId));
    if (ws) {
      setActiveWorkspaceId(ws.id);
      setActiveSessionId(sessionId);
      focusKeyboardProxy();
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
    focusKeyboardProxy();
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

  const handleSelectFile = useCallback((path: string) => {
    setSelectedFilePath(path);
  }, []);

  const handleCloseFile = useCallback(() => {
    setSelectedFilePath(null);
  }, []);

  useKeyboardShortcuts(
    useCallback(
      () => ({
        onNew: () => setShowAddProject(true),
        onDiff: () => toggleDiff(),
        onEscape: () => {
          setShowAddProject(false);
          setShowHelp(false);
          setShowSettings(false);
          // Also close file diff view if open
          setSelectedFilePath(null);
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
        />
      );
    }

    return (
      <div className="flex-1 flex flex-col min-h-0">
        <WorkspaceHeader
          workspace={activeWorkspace}
          activeSession={activeSession}
          diffCollapsed={diffCollapsed}
          diffFileCount={diffFiles.length}
          onToggleDiff={toggleDiff}
        />

        <ContentSplit
          collapsed={diffCollapsed}
          onToggleCollapse={toggleDiff}
          left={
            <div className="flex-1 flex flex-col min-h-0 overflow-hidden relative">
              {/* Terminal kept mounted (hidden when a file diff is shown) to preserve xterm state */}
              <div
                className={
                  selectedFilePath
                    ? "hidden"
                    : "flex-1 flex flex-col min-h-0 overflow-hidden"
                }
              >
                <TerminalView key={activeSessionId} session={activeSession} />
              </div>

              {selectedFilePath && activeSessionId && (
                <DiffFileViewer
                  sessionId={activeSessionId}
                  filePath={selectedFilePath}
                  revision={revision}
                  onClose={handleCloseFile}
                />
              )}
            </div>
          }
          right={
            <RightPanel
              session={activeSession ?? null}
              sessionId={activeSessionId}
              files={diffFiles}
              baseBranch={baseBranch}
              warning={warning}
              filesLoading={diffFilesLoading}
              selectedFilePath={selectedFilePath}
              onSelectFile={handleSelectFile}
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
          onClick={() => { setActiveWorkspaceId(null); setActiveSessionId(null); setShowSettings(false); setSelectedFilePath(null); }}
          className="flex items-center gap-1.5 text-text-muted hover:text-text-secondary transition-colors cursor-pointer"
          aria-label="Go to dashboard"
        >
          <img src="/icon-192.png" alt="" width="18" height="18" className="rounded-sm" />
          <span className="font-mono text-xs leading-none">aoe</span>
        </button>

        <div className="flex-1" />

        <div className="flex items-center gap-1.5">
          {error && (
            <span
              className="font-mono text-[11px] px-1.5 py-0.5 rounded-full bg-status-error/10 text-status-error flex items-center gap-1.5"
              title="Disconnected from backend"
            >
              <span className="w-1.5 h-1.5 rounded-full bg-status-error animate-pulse" />
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
          {loginRequired && (
            <button
              onClick={onLogout}
              className="px-2 h-8 flex items-center justify-center cursor-pointer rounded-md transition-colors text-text-dim hover:text-text-secondary hover:bg-surface-700/50 font-mono text-xs"
              title="Sign out"
              aria-label="Sign out"
            >
              log out
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

      {showAddProject && (
        <SessionWizard
          onClose={() => setShowAddProject(false)}
          onCreated={() => setShowAddProject(false)}
        />
      )}

      {showHelp && <HelpOverlay onClose={() => setShowHelp(false)} />}

      <textarea
        ref={keyboardProxyRef}
        aria-hidden="true"
        tabIndex={-1}
        className="fixed opacity-0 w-0 h-0 pointer-events-none"
        style={{ top: -9999, left: -9999 }}
      />
    </div>
  );
}
