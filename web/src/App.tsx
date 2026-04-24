import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { isSessionActive } from "./lib/session";
import { useSessions } from "./hooks/useSessions";
import { useWorkspaces } from "./hooks/useWorkspaces";
import { useRepoGroups } from "./hooks/useRepoGroups";
import { useKeyboardShortcuts } from "./hooks/useKeyboardShortcuts";
import { useDiffFiles } from "./hooks/useDiffFiles";
import { useCommandActions } from "./hooks/useCommandActions";
import { useEdgeSwipe } from "./hooks/useEdgeSwipe";
import { loginStatus, logout, deleteSession, fetchAbout } from "./lib/api";
import type { DeleteSessionOptions, ServerAbout } from "./lib/api";
import { toastBus } from "./lib/toastBus";
import {
  OPEN_SESSION_EVENT,
  readSessionFromUrl,
  writeSessionToUrl,
} from "./lib/sessionRoute";
import { WorkspaceSidebar } from "./components/WorkspaceSidebar";
import { DeleteSessionDialog } from "./components/DeleteSessionDialog";
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
import { DisconnectBanner } from "./components/DisconnectBanner";

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
    return <div className="h-dvh bg-surface-900 safe-area-inset" />;
  }

  return <AppContent loginRequired={loginRequired} onLogout={handleLogout} />;
}

function AppContent({ loginRequired, onLogout }: { loginRequired: boolean; onLogout: () => void }) {
  const { sessions, error, injectSession, setSessionStatus } = useSessions();
  const workspaces = useWorkspaces(sessions);
  const { groups, toggleRepoCollapsed } = useRepoGroups(workspaces);

  const [activeWorkspaceId, setActiveWorkspaceId] = useState<string | null>(
    null,
  );
  // Seed from `?session=<id>` so deep links and notification taps land on
  // the right session before the sessions list has finished loading.
  const [activeSessionId, setActiveSessionId] = useState<string | null>(() =>
    readSessionFromUrl(),
  );
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

  // Prefer workspace lookup by ID; fall back to finding the workspace
  // that contains activeSessionId so a URL-seeded session (notification
  // tap) renders even before handleSelectSession has run to set the
  // workspace ID explicitly.
  const activeWorkspace = useMemo(() => {
    const byId = activeWorkspaceId
      ? workspaces.find((w) => w.id === activeWorkspaceId)
      : undefined;
    if (byId) return byId;
    if (!activeSessionId) return undefined;
    return workspaces.find((w) =>
      w.sessions.some((s) => s.id === activeSessionId),
    );
  }, [workspaces, activeWorkspaceId, activeSessionId]);
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
      writeSessionToUrl(sessionId);
      focusKeyboardProxy();
      setShowSettings(false);
      if (window.innerWidth < 768) setSidebarOpen(false);
    }
  }, [workspaces]);

  const handleSelectWorkspace = (workspaceId: string) => {
    setActiveWorkspaceId(workspaceId);
    const ws = workspaces.find((w) => w.id === workspaceId);
    if (ws) {
      const running = ws.sessions.find((s) => isSessionActive(s.status));
      const picked = running?.id ?? ws.sessions[0]?.id ?? null;
      setActiveSessionId(picked);
      writeSessionToUrl(picked);
    }
    focusKeyboardProxy();
    setShowSettings(false);
    if (window.innerWidth < 768) {
      setSidebarOpen(false);
    }
  };

  // Sync browser back/forward to selection state. We always clear
  // activeWorkspaceId so the activeWorkspace memo re-derives it from
  // the URL-provided session. Otherwise back/forward across workspaces
  // leaves a stale workspace ID, the memo returns the wrong workspace,
  // the session lookup fails, and the app renders the dashboard.
  useEffect(() => {
    const onPop = () => {
      setActiveSessionId(readSessionFromUrl());
      setActiveWorkspaceId(null);
    };
    window.addEventListener("popstate", onPop);
    return () => window.removeEventListener("popstate", onPop);
  }, []);

  // In-app toast forwarded from the service worker sets this event when
  // the user taps it; navigate to the session that triggered the push.
  useEffect(() => {
    const onOpen = (e: Event) => {
      const detail = (e as CustomEvent).detail as
        | { sessionId?: string }
        | undefined;
      if (detail?.sessionId) {
        handleSelectSession(detail.sessionId);
      }
    };
    window.addEventListener(OPEN_SESSION_EVENT, onOpen);
    return () => window.removeEventListener(OPEN_SESSION_EVENT, onOpen);
  }, [handleSelectSession]);

  const [wizardPrefill, setWizardPrefill] = useState<WizardPrefill | undefined>(undefined);
  const [deletingWorkspaceId, setDeletingWorkspaceId] = useState<string | null>(null);
  const [serverAbout, setServerAbout] = useState<ServerAbout | null>(null);

  useEffect(() => {
    fetchAbout().then((about) => {
      if (about) setServerAbout(about);
    });
  }, []);

  const deletingWorkspace = deletingWorkspaceId
    ? workspaces.find((w) => w.id === deletingWorkspaceId)
    : null;
  const deletingSession = deletingWorkspace?.sessions[0] ?? null;

  const handleDeleteSession = useCallback((workspaceId: string) => {
    setDeletingWorkspaceId(workspaceId);
  }, []);

  const handleConfirmDelete = useCallback(async (options: DeleteSessionOptions) => {
    if (!deletingSession) return;
    const sessionId = deletingSession.id;
    const wasActive = sessionId === activeSessionId;

    // Close dialog and show "Deleting" status immediately
    setDeletingWorkspaceId(null);
    setSessionStatus(sessionId, "Deleting");

    if (wasActive) {
      setActiveWorkspaceId(null);
      setActiveSessionId(null);
      writeSessionToUrl(null);
    }

    const result = await deleteSession(sessionId, options);
    if (!result.ok) {
      // Revert status on failure
      setSessionStatus(sessionId, "Error");
      toastBus.handler?.error(result.error || "Failed to delete session");
      return;
    }

    toastBus.handler?.info("Session deleted");
  }, [deletingSession, activeSessionId, setSessionStatus]);

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
    writeSessionToUrl(null);
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

  const openSidebar = useCallback(() => setSidebarOpen(true), []);
  const openDiff = useCallback(() => setDiffCollapsed(false), []);
  useEdgeSwipe({
    edge: "left",
    enabled: !sidebarOpen,
    onSwipe: openSidebar,
    blurOnSwipe: true,
  });
  useEdgeSwipe({
    edge: "right",
    enabled: diffCollapsed && !!activeSessionId,
    onSwipe: openDiff,
  });

  const handleNewSession = useCallback(() => {
    setWizardPrefill(undefined);
    setShowAddProject(true);
  }, []);

  const handleCloneFromUrl = useCallback(() => {
    setWizardPrefill({ initialTab: "clone" });
    setShowAddProject(true);
  }, []);

  useKeyboardShortcuts(
    useCallback(
      () => ({
        onNew: () => setShowAddProject(true),
        onDiff: () => toggleDiff(),
        onEscape: () => {
          if (deletingWorkspaceId) {
            setDeletingWorkspaceId(null);
            return;
          }
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
      [toggleDiff, showPalette, deletingWorkspaceId],
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
          onCloneFromUrl={handleCloneFromUrl}
          onToggleSidebar={handleToggleSidebar}
          readOnly={serverAbout?.read_only}
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
    <div className="h-dvh flex flex-col bg-surface-900 text-text-primary overflow-hidden safe-area-inset">
      <TopBar
        activeWorkspace={activeWorkspace}
        activeSession={activeSession ?? null}
        onToggleSidebar={handleToggleSidebar}
        onOpenPalette={() => setShowPalette(true)}
        onToggleDiff={toggleDiff}
        diffCollapsed={diffCollapsed}
        onOpenSettings={handleOpenSettings}
        onOpenHelp={handleOpenHelp}
        onOpenAbout={handleOpenAbout}
        onLogout={onLogout}
        loginRequired={loginRequired}
        isOffline={!!error}
        onGoDashboard={handleGoDashboard}
      />

      <DisconnectBanner />

      <div className="flex flex-1 min-h-0">
        <WorkspaceSidebar
          groups={groups}
          activeId={activeWorkspace?.id ?? null}
          open={sidebarOpen}
          onToggle={() => setSidebarOpen(false)}
          onSelect={handleSelectWorkspace}
          onToggleRepo={toggleRepoCollapsed}
          onNew={() => { setWizardPrefill(undefined); setShowAddProject(true); }}
          onCreateSession={handleCreateSession}
          onSettings={() => { setShowSettings((s) => !s); if (window.innerWidth < 768) setSidebarOpen(false); }}
          onDeleteSession={handleDeleteSession}
          readOnly={serverAbout?.read_only}
        />

        <div className="flex-1 flex flex-col min-h-0 min-w-0">
          {renderContent()}
        </div>
      </div>

      {showAddProject && (
        <SessionWizard
          onClose={() => { setShowAddProject(false); setWizardPrefill(undefined); }}
          onCreated={(session?: SessionResponse) => {
            if (session) {
              injectSession(session);
              setActiveSessionId(session.id);
              writeSessionToUrl(session.id);
              // Key format must match useWorkspaces grouping key
              const repoPath = (session.main_repo_path ?? session.project_path).replace(/\/+$/, "");
              const wsId = `${repoPath}::${session.branch ?? "__default__"}`;
              setActiveWorkspaceId(wsId);
              if (window.innerWidth < 768) setSidebarOpen(false);
            }
            setShowAddProject(false);
            setWizardPrefill(undefined);
          }}
          prefill={wizardPrefill}
        />
      )}

      {showHelp && <HelpOverlay onClose={() => setShowHelp(false)} />}

      {showAbout && <AboutModal onClose={() => setShowAbout(false)} />}

      {deletingSession && (
        <DeleteSessionDialog
          sessionTitle={deletingSession.title}
          branchName={deletingSession.branch}
          hasManagedWorktree={deletingSession.has_managed_worktree}
          isSandboxed={deletingSession.is_sandboxed}
          cleanupDefaults={deletingSession.cleanup_defaults}
          onConfirm={handleConfirmDelete}
          onCancel={() => setDeletingWorkspaceId(null)}
        />
      )}

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
