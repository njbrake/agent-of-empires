import { useCallback, useEffect, useRef, useState } from "react";
import { isSessionActive } from "./lib/session";
import { useSessions } from "./hooks/useSessions";
import { useWorkspaces } from "./hooks/useWorkspaces";
import { useRepoGroups } from "./hooks/useRepoGroups";
import { useKeyboardShortcuts } from "./hooks/useKeyboardShortcuts";
import { useDiffFiles } from "./hooks/useDiffFiles";
import { useCommandActions } from "./hooks/useCommandActions";
import { createSession, loginStatus, logout } from "./lib/api";
import { WorkspaceSidebar } from "./components/WorkspaceSidebar";
import { TopBar } from "./components/TopBar";
import { ContentSplit } from "./components/ContentSplit";
import { TerminalView } from "./components/TerminalView";
import { RightPanel } from "./components/RightPanel";
import { DiffFileViewer } from "./components/diff/DiffFileViewer";
import { SettingsView } from "./components/SettingsView";
import { HelpOverlay } from "./components/HelpOverlay";
import { SessionWizard } from "./components/session-wizard/SessionWizard";
import { Dashboard } from "./components/Dashboard";
import { LoginPage } from "./components/LoginPage";
import { AboutModal } from "./components/AboutModal";
import { CommandPalette } from "./components/command-palette/CommandPalette";

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
  const [showPalette, setShowPalette] = useState(false);
  const [showAbout, setShowAbout] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(
    () => window.innerWidth >= 768,
  );
  const keyboardProxyRef = useRef<HTMLTextAreaElement>(null);

  const activeWorkspace = workspaces.find((w) => w.id === activeWorkspaceId);
  const activeSession = activeWorkspace?.sessions.find(
    (s) => s.id === activeSessionId,
  );

  const { files: diffFiles, baseBranch, warning, loading: diffFilesLoading, revision } =
    useDiffFiles(activeSessionId, !diffCollapsed);

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

  useEffect(() => {
    setSelectedFilePath(null);
  }, [activeSessionId]);

  const focusKeyboardProxy = () => {
    if (window.innerWidth < 768 && navigator.maxTouchPoints > 0) {
      keyboardProxyRef.current?.focus();
    }
  };

  const handleSelectSession = useCallback((sessionId: string) => {
    const ws = workspaces.find((w) => w.sessions.some((s) => s.id === sessionId));
    if (ws) {
      setActiveWorkspaceId(ws.id);
      setActiveSessionId(sessionId);
      focusKeyboardProxy();
      if (window.innerWidth < 768) setSidebarOpen(false);
    }
  }, [workspaces]);

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

  const toggleDiff = useCallback(() => setDiffCollapsed((c) => !c), []);

  const handleSelectFile = useCallback((path: string) => {
    setSelectedFilePath(path);
  }, []);

  const handleCloseFile = useCallback(() => {
    setSelectedFilePath(null);
  }, []);

  const handleGoDashboard = useCallback(() => {
    setActiveWorkspaceId(null);
    setActiveSessionId(null);
    setShowSettings(false);
    setSelectedFilePath(null);
  }, []);

  const handleOpenSettings = useCallback(() => {
    setShowSettings(true);
    if (window.innerWidth < 768) setSidebarOpen(false);
  }, []);

  const handleOpenHelp = useCallback(() => {
    setShowHelp(true);
  }, []);

  const handleOpenAbout = useCallback(() => {
    setShowAbout(true);
  }, []);

  const handleToggleSidebar = useCallback(() => {
    setSidebarOpen((o) => !o);
  }, []);

  const handleNewSession = useCallback(() => {
    setShowAddProject(true);
  }, []);

  useKeyboardShortcuts(
    useCallback(
      () => ({
        onNew: () => setShowAddProject(true),
        onDiff: () => toggleDiff(),
        onEscape: () => {
          if (showPalette) {
            setShowPalette(false);
            return;
          }
          setShowAddProject(false);
          setShowHelp(false);
          setShowSettings(false);
          setShowAbout(false);
          setSelectedFilePath(null);
        },
        onHelp: () => setShowHelp((h) => !h),
        onSettings: () => setShowSettings((s) => !s),
        onPalette: () => setShowPalette((p) => !p),
      }),
      [toggleDiff, showPalette],
    ),
  );

  const commandActions = useCommandActions({
    sessions,
    activeSessionId,
    loginRequired,
    hasActiveSession: !!activeSession,
    onNewSession: handleNewSession,
    onSelectSession: handleSelectSession,
    onToggleDiff: toggleDiff,
    onOpenSettings: handleOpenSettings,
    onOpenHelp: handleOpenHelp,
    onOpenAbout: handleOpenAbout,
    onGoDashboard: handleGoDashboard,
    onToggleSidebar: handleToggleSidebar,
    onLogout,
  });

  const renderContent = () => {
    if (showSettings) {
      return <SettingsView onClose={() => setShowSettings(false)} />;
    }

    if (!activeWorkspace || !activeSession) {
      return (
        <Dashboard
          sessions={sessions}
          onSelectSession={handleSelectSession}
          onNewSession={handleNewSession}
        />
      );
    }

    return (
      <div className="flex-1 flex flex-col min-h-0">
        <ContentSplit
          collapsed={diffCollapsed}
          onToggleCollapse={toggleDiff}
          left={
            <div className="flex-1 flex flex-col min-h-0 overflow-hidden relative">
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
      <TopBar
        activeWorkspace={activeWorkspace}
        activeSession={activeSession ?? null}
        onToggleSidebar={handleToggleSidebar}
        onOpenPalette={() => setShowPalette(true)}
        onToggleDiff={toggleDiff}
        diffCollapsed={diffCollapsed}
        diffFileCount={diffFiles.length}
        onOpenSettings={handleOpenSettings}
        onOpenHelp={handleOpenHelp}
        onOpenAbout={handleOpenAbout}
        onLogout={onLogout}
        loginRequired={loginRequired}
        isOffline={!!error}
        onGoDashboard={handleGoDashboard}
      />

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

      {showAbout && <AboutModal onClose={() => setShowAbout(false)} />}

      <CommandPalette
        open={showPalette}
        onClose={() => setShowPalette(false)}
        actions={commandActions}
      />

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
