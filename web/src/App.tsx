import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { isSessionActive } from "./lib/session";
import { useSessions } from "./hooks/useSessions";
import { useWorkspaces } from "./hooks/useWorkspaces";
import { useRepoGroups } from "./hooks/useRepoGroups";
import { useKeyboardShortcuts } from "./hooks/useKeyboardShortcuts";
import { useDiffFiles } from "./hooks/useDiffFiles";
import { useCommandActions } from "./hooks/useCommandActions";
import { loginStatus, logout } from "./lib/api";
import { WorkspaceSidebar } from "./components/WorkspaceSidebar";
import { TopBar } from "./components/TopBar";
import { ContentSplit } from "./components/ContentSplit";
import { TerminalView } from "./components/TerminalView";
import { RightPanel } from "./components/RightPanel";
import { DiffFileViewer } from "./components/diff/DiffFileViewer";
import { SettingsView } from "./components/SettingsView";
import { HelpOverlay } from "./components/HelpOverlay";
import { SessionWizard } from "./components/session-wizard/SessionWizard";
import type { WizardPrefill } from "./components/session-wizard/SessionWizard";
import type { SessionResponse } from "./lib/types";
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
  const { sessions, error, injectSession } = useSessions();
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

  const [wizardPrefill, setWizardPrefill] = useState<WizardPrefill | undefined>(undefined);

  const handleCreateSession = useCallback((repoPath: string) => {
    const projectSessions = sessions
      .filter((s) => (s.main_repo_path || s.project_path) === repoPath)
      .sort((a, b) => (b.last_accessed_at ?? "").localeCompare(a.last_accessed_at ?? ""));
    const latest = projectSessions[0];

    setWizardPrefill({
      path: repoPath,
      tool: latest?.tool ?? "claude",
      yoloMode: latest?.yolo_mode ?? false,
      sandboxEnabled: latest?.is_sandboxed ?? false,
      profile: latest?.profile || undefined,
      group: latest?.group_path || undefined,
      skipToReview: true,
    });
    setShowAddProject(true);
  }, [sessions]);

  const lastSession = useMemo(() =>
    sessions.length > 0
      ? [...sessions].sort((a, b) => (b.last_accessed_at ?? b.created_at ?? "").localeCompare(a.last_accessed_at ?? a.created_at ?? ""))[0]
      : null,
    [sessions],
  );

  const handleRepeatLast = useCallback(() => {
    if (!lastSession) return;
    setWizardPrefill({
      path: lastSession.main_repo_path || lastSession.project_path,
      tool: lastSession.tool,
      yoloMode: lastSession.yolo_mode,
      sandboxEnabled: lastSession.is_sandboxed ?? false,
      profile: lastSession.profile || undefined,
      group: lastSession.group_path || undefined,
      skipToReview: true,
    });
    setShowAddProject(true);
  }, [lastSession]);

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

  useEffect(() => {
    if (sidebarOpen) return;
    const EDGE_PX = 24;
    const THRESHOLD_PX = 60;
    let startX = 0;
    let startY = 0;
    let tracking = false;

    const onTouchStart = (e: TouchEvent) => {
      if (window.innerWidth >= 768 || e.touches.length !== 1) return;
      const t = e.touches[0];
      if (!t || t.clientX > EDGE_PX) return;
      tracking = true;
      startX = t.clientX;
      startY = t.clientY;
    };
    const onTouchMove = (e: TouchEvent) => {
      if (!tracking) return;
      const t = e.touches[0];
      if (!t) return;
      const dx = t.clientX - startX;
      const dy = t.clientY - startY;
      if (dx > THRESHOLD_PX && Math.abs(dx) > Math.abs(dy)) {
        tracking = false;
        setSidebarOpen(true);
      } else if (Math.abs(dy) > Math.abs(dx) && Math.abs(dy) > 16) {
        tracking = false;
      }
    };
    const onTouchEnd = () => {
      tracking = false;
    };

    window.addEventListener("touchstart", onTouchStart, { passive: true });
    window.addEventListener("touchmove", onTouchMove, { passive: true });
    window.addEventListener("touchend", onTouchEnd, { passive: true });
    window.addEventListener("touchcancel", onTouchEnd, { passive: true });
    return () => {
      window.removeEventListener("touchstart", onTouchStart);
      window.removeEventListener("touchmove", onTouchMove);
      window.removeEventListener("touchend", onTouchEnd);
      window.removeEventListener("touchcancel", onTouchEnd);
    };
  }, [sidebarOpen]);

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
        onToggleSidebar: () => setSidebarOpen((o) => !o),
        onToggleRightPanel: () => setDiffCollapsed((c) => !c),
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
        <WorkspaceSidebar
          groups={groups}
          activeId={activeWorkspaceId}
          creatingForProject={null}
          open={sidebarOpen}
          onToggle={() => setSidebarOpen(false)}
          onSelect={handleSelectWorkspace}
          onToggleRepo={toggleRepoCollapsed}
          onNew={() => { setWizardPrefill(undefined); setShowAddProject(true); }}
          onCreateSession={handleCreateSession}
          onSettings={() => { setShowSettings((s) => !s); if (window.innerWidth < 768) setSidebarOpen(false); }}
          onRepeatLast={handleRepeatLast}
          hasLastSession={!!lastSession}
        />

        <div className="flex-1 flex flex-col min-h-0 min-w-0">
          {renderContent()}
        </div>
      </div>

      {showAddProject && (
        <SessionWizard
          onClose={() => { setShowAddProject(false); setWizardPrefill(undefined); }}
          onCreated={(session?: SessionResponse) => { if (session) injectSession(session); setShowAddProject(false); setWizardPrefill(undefined); }}
          prefill={wizardPrefill}
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
