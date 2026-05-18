import { lazy, Suspense, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useMatch, useNavigate } from "react-router-dom";
import { IDLE_DECAY_WINDOW_MS, isSessionActive } from "./lib/session";
import { useSessions } from "./hooks/useSessions";
import { clearCockpitCache } from "./hooks/useCockpit";
import { CockpitPrefsProvider } from "./lib/cockpitPrefs";
import { useWorkspaces } from "./hooks/useWorkspaces";
import { useRepoGroups } from "./hooks/useRepoGroups";
import { useKeyboardShortcuts } from "./hooks/useKeyboardShortcuts";
import { useResolvedTheme } from "./hooks/useResolvedTheme";
import { useDiffFiles } from "./hooks/useDiffFiles";
import { useDiffComments } from "./hooks/useDiffComments";
import { SendCommentsDialog } from "./components/diff/comments/SendCommentsDialog";
import { useCommandActions } from "./hooks/useCommandActions";
import { useEdgeSwipe } from "./hooks/useEdgeSwipe";
import { useMobileKeyboard } from "./hooks/useMobileKeyboard";
import {
  loginStatus,
  logout,
  deleteSession,
  fetchAbout,
  fetchSettings,
  updateWorkspaceOrdering,
} from "./lib/api";
import type { DeleteSessionOptions, ServerAbout } from "./lib/api";
import {
  IdleDecayWindowContext,
  parseIdleDecayWindowMs,
  useIdleDecayWindowMs,
} from "./lib/idleDecay";
import { toastBus } from "./lib/toastBus";
import { OPEN_SESSION_EVENT } from "./lib/sessionRoute";
import {
  dispatchFocusTerminal,
  setPendingTerminalFocus,
} from "./lib/terminalFocus";
import { WorkspaceSidebar } from "./components/WorkspaceSidebar";
import { DeleteSessionDialog } from "./components/DeleteSessionDialog";
import { TopBar } from "./components/TopBar";
import { ContentSplit } from "./components/ContentSplit";
import { TerminalView } from "./components/TerminalView";
// Lazy-load the cockpit surface so non-cockpit users never download
// the @assistant-ui/react, shiki, and in-house StringDiff/DiffLine
// dependency tree. Cuts ~hundreds of KB off the cold-start bundle
// for the (currently default) tmux-only flow. The Suspense fallback
// below covers the brief load while the chunk arrives.
const CockpitView = lazy(() =>
  import("./components/cockpit/CockpitView").then((m) => ({
    default: m.CockpitView,
  })),
);
import { RightPanel } from "./components/RightPanel";
import { DiffFileViewer } from "./components/diff/DiffFileViewer";
import { SettingsView } from "./components/SettingsView";
import { ProjectsView } from "./components/ProjectsView";
import { HelpOverlay } from "./components/HelpOverlay";
import { SessionWizard } from "./components/session-wizard/SessionWizard";
import type { WizardPrefill } from "./components/session-wizard/SessionWizard";
import type { SessionResponse } from "./lib/types";
import { Dashboard } from "./components/Dashboard";
import { LoginPage } from "./components/LoginPage";
import { TokenEntryPage } from "./components/TokenEntryPage";
import {
  LOGIN_REQUIRED_EVENT,
  TOKEN_EXPIRED_EVENT,
  resetTokenExpired,
} from "./lib/fetchInterceptor";
import { AboutModal } from "./components/AboutModal";
import { CommandPalette } from "./components/command-palette/CommandPalette";
import { DisconnectBanner } from "./components/DisconnectBanner";
import { ElevationPrompt } from "./components/ElevationPrompt";
import { UpdateBanner } from "./components/UpdateBanner";

const RIGHT_PANEL_COLLAPSED_KEY = "aoe-right-collapsed";

export default function App() {
  // Apply the user-selected theme as CSS custom properties on the root
  // element. Runs once on mount + on settings-driven theme changes.
  // The pre-React /theme-bootstrap.js (referenced from index.html)
  // paints the cached theme before hydration; this hook keeps it in
  // sync with the server's view.
  useResolvedTheme();
  const [loginRequired, setLoginRequired] = useState<boolean | null>(null);
  const [loginAuthenticated, setLoginAuthenticated] = useState(true);
  const [tokenExpired, setTokenExpired] = useState(false);
  const [idleDecayWindowMs, setIdleDecayWindowMs] = useState(IDLE_DECAY_WINDOW_MS);

  useEffect(() => {
    const onTokenExpired = () => setTokenExpired(true);
    window.addEventListener(TOKEN_EXPIRED_EVENT, onTokenExpired);
    return () => window.removeEventListener(TOKEN_EXPIRED_EVENT, onTokenExpired);
  }, []);

  // Clearing tokenExpired here matters: the render order below shows
  // TokenEntryPage above LoginPage, so without the reset a token that's
  // actually fine would keep getting shown the wrong screen.
  useEffect(() => {
    const onLoginRequired = () => {
      setTokenExpired(false);
      setLoginRequired(true);
      setLoginAuthenticated(false);
    };
    window.addEventListener(LOGIN_REQUIRED_EVENT, onLoginRequired);
    return () =>
      window.removeEventListener(LOGIN_REQUIRED_EVENT, onLoginRequired);
  }, []);

  useEffect(() => {
    loginStatus().then(({ required, authenticated }) => {
      setLoginRequired(required);
      setLoginAuthenticated(authenticated);
    });
  }, []);

  useEffect(() => {
    fetchSettings().then((settings) => {
      setIdleDecayWindowMs(parseIdleDecayWindowMs(settings));
    });
  }, []);

  const handleTokenSuccess = () => {
    setTokenExpired(false);
    // Re-check login status now that token auth works
    loginStatus().then(({ required, authenticated }) => {
      setLoginRequired(required);
      setLoginAuthenticated(authenticated);
    });
  };

  const handleLoginSuccess = () => {
    setLoginAuthenticated(true);
    // Reset dedup flags so a future session expiry can re-fire the event.
    resetTokenExpired();
  };

  const handleLogout = async () => {
    await logout();
    setLoginAuthenticated(false);
  };

  // Token auth is the first factor; show token entry before anything else
  if (tokenExpired) {
    return <TokenEntryPage onSuccess={handleTokenSuccess} />;
  }

  if (loginRequired && !loginAuthenticated) {
    return <LoginPage onSuccess={handleLoginSuccess} />;
  }

  if (loginRequired === null) {
    return <div className="h-dvh bg-surface-900 safe-area-inset" />;
  }

  return (
    <IdleDecayWindowContext.Provider value={idleDecayWindowMs}>
      <AppContent loginRequired={loginRequired} onLogout={handleLogout} />
      <ElevationPrompt />
    </IdleDecayWindowContext.Provider>
  );
}

/** Walk from the event target up to the document root looking for any
 *  text-input surface, so global hotkeys don't fire when the user is
 *  typing in an `<input>`, `<textarea>`, or contenteditable element
 *  (or any contenteditable ancestor of a deeper rich-text widget). */
function isInsideEditable(target: EventTarget | null): boolean {
  let el: HTMLElement | null =
    target instanceof HTMLElement ? target : null;
  while (el) {
    const tag = el.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA" || el.isContentEditable) {
      return true;
    }
    el = el.parentElement;
  }
  return false;
}

function AppContent({ loginRequired, onLogout }: { loginRequired: boolean; onLogout: () => void }) {
  const navigate = useNavigate();
  const idleDecayWindowMs = useIdleDecayWindowMs();
  const sessionMatch = useMatch("/session/:sessionId");
  const settingsRootMatch = useMatch("/settings");
  const settingsTabMatch = useMatch("/settings/:tab");
  const projectsMatch = useMatch("/projects");
  const activeSessionId = sessionMatch?.params.sessionId ?? null;
  const showSettings = settingsRootMatch !== null || settingsTabMatch !== null;
  const showProjects = projectsMatch !== null;
  const settingsTab = settingsTabMatch?.params.tab ?? null;

  const {
    sessions,
    workspaceOrdering,
    setWorkspaceOrdering,
    markLocalOrderingUpdate,
    error,
    injectSession,
    setSessionStatus,
  } = useSessions();
  const workspaces = useWorkspaces(sessions);

  const { groups, toggleRepoCollapsed } = useRepoGroups(
    workspaces,
    workspaceOrdering,
  );

  // Drag-end handler for the sidebar. Optimistically applies the new
  // order locally so the row snaps into place, then persists to the
  // server. `markLocalOrderingUpdate` opens a short window during
  // which polled responses do not clobber our just-applied state, so a
  // poll firing mid-PUT can't revert the drag.
  const handleReorderWorkspaces = useCallback(
    (newOrder: string[]) => {
      setWorkspaceOrdering(newOrder);
      markLocalOrderingUpdate();
      void updateWorkspaceOrdering(newOrder);
    },
    [setWorkspaceOrdering, markLocalOrderingUpdate],
  );

  // Selected diff-file identity. `repoName` is undefined for single-repo
  // sessions and the workspace member name for multi-repo workspaces.
  // Kept as one state so the path + repo always update together; with
  // two parallel states we'd briefly fetch the wrong repo when only
  // one side changed (workspace path collisions across repos make this
  // a real bug, not theoretical). See #1047.
  const [selectedFile, setSelectedFile] = useState<{
    path: string;
    repoName?: string;
  } | null>(null);
  const selectedFilePath = selectedFile?.path ?? null;
  const selectedRepoName = selectedFile?.repoName;
  const [diffCollapsed, setDiffCollapsed] = useState(() => {
    const stored = localStorage.getItem(RIGHT_PANEL_COLLAPSED_KEY);
    if (stored === "1") return true;
    if (stored === "0") return false;
    return window.innerWidth < 768;
  });
  useEffect(() => {
    localStorage.setItem(RIGHT_PANEL_COLLAPSED_KEY, diffCollapsed ? "1" : "0");
  }, [diffCollapsed]);
  const [showSessionWizard, setShowSessionWizard] = useState(false);
  const [showHelp, setShowHelp] = useState(false);
  const [showPalette, setShowPalette] = useState(false);
  const [showAbout, setShowAbout] = useState(false);
  const [sidebarOpen, setSidebarOpen] = useState(
    () => window.innerWidth >= 768,
  );
  const keyboardProxyRef = useRef<HTMLTextAreaElement>(null);

  const activeWorkspace = useMemo(() => {
    if (!activeSessionId) return undefined;
    return workspaces.find((w) =>
      w.sessions.some((s) => s.id === activeSessionId),
    );
  }, [workspaces, activeSessionId]);
  const activeSession = activeWorkspace?.sessions.find(
    (s) => s.id === activeSessionId,
  );

  const {
    files: diffFiles,
    perRepoBases,
    warning,
    loading: diffFilesLoading,
    revision,
    refresh: refreshDiffFiles,
  } = useDiffFiles(activeSessionId, !diffCollapsed);

  // Diff-viewer comments (#928). Cockpit-only and session-scoped. The
  // banner lives in RightPanel while the inline UI lives inside
  // DiffFileViewer, so the store is lifted here and threaded to both.
  const diffComments = useDiffComments(activeSessionId);
  const commentsEnabled = !!activeSession?.cockpit_mode;
  const commentSendEnabled =
    commentsEnabled && activeSession?.cockpit_worker_state === "running";
  const commentSendDisabledReason = !commentsEnabled
    ? "Diff comments require a cockpit session"
    : "Cockpit worker is not running";
  const commentsIsMultiRepo = (activeSession?.workspace_repos.length ?? 0) > 0;
  const [sendDialogOpen, setSendDialogOpen] = useState(false);

  useEffect(() => {
    if (!commentSendEnabled) return;
    const onKey = (e: KeyboardEvent) => {
      if (!((e.metaKey || e.ctrlKey) && e.shiftKey && e.key.toLowerCase() === "s")) {
        return;
      }
      if (isInsideEditable(e.target)) return;
      if (diffComments.count === 0) return;
      e.preventDefault();
      setSendDialogOpen(true);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [commentSendEnabled, diffComments.count]);

  useEffect(() => {
    if (!activeSessionId) {
      setSelectedFile(null);
      return;
    }
    if (
      selectedFilePath &&
      !diffFilesLoading &&
      !diffFiles.some((f) => f.path === selectedFilePath)
    ) {
      setSelectedFile(null);
    }
  }, [activeSessionId, diffFiles, diffFilesLoading, selectedFilePath]);

  useEffect(() => {
    setSelectedFile(null);
  }, [activeSessionId]);

  const focusKeyboardProxy = () => {
    if (window.innerWidth < 768 && navigator.maxTouchPoints > 0) {
      keyboardProxyRef.current?.focus();
    }
  };

  const handleSelectSession = useCallback((sessionId: string) => {
    const ws = workspaces.find((w) => w.sessions.some((s) => s.id === sessionId));
    if (ws) {
      navigate(`/session/${encodeURIComponent(sessionId)}`);
      focusKeyboardProxy();
      if (window.innerWidth < 768) setSidebarOpen(false);
    }
  }, [navigate, workspaces]);

  const handleSelectWorkspace = (workspaceId: string) => {
    const ws = workspaces.find((w) => w.id === workspaceId);
    if (ws) {
      const running = ws.sessions.find((s) =>
        isSessionActive(s, idleDecayWindowMs),
      );
      const picked = running?.id ?? ws.sessions[0]?.id ?? null;
      if (picked) {
        navigate(`/session/${encodeURIComponent(picked)}`);
      } else {
        navigate("/");
      }
    }
    focusKeyboardProxy();
    if (window.innerWidth < 768) {
      setSidebarOpen(false);
    }
  };

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

  const refreshServerAbout = useCallback(async () => {
    const about = await fetchAbout();
    if (about) setServerAbout(about);
  }, []);

  useEffect(() => {
    refreshServerAbout();
  }, [refreshServerAbout]);

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
      navigate("/");
    }

    const result = await deleteSession(sessionId, options);
    if (!result.ok) {
      // Revert status on failure
      setSessionStatus(sessionId, "Error");
      toastBus.handler?.error(result.error || "Failed to delete session");
      return;
    }

    // Drop the per-session cockpit cache so a recreated session with
    // the same id doesn't briefly show the prior transcript on
    // remount before fetchReplay clears it.
    clearCockpitCache(sessionId);

    toastBus.handler?.info("Session deleted");
  }, [deletingSession, activeSessionId, setSessionStatus, navigate]);

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
    setShowSessionWizard(true);
  }, [sessions]);

  const toggleDiff = useCallback(() => setDiffCollapsed((c) => !c), []);

  const handleSelectFile = useCallback(
    (path: string, repoName?: string) => {
      setSelectedFile({ path, repoName });
    },
    [],
  );

  const handleCloseFile = useCallback(() => {
    setSelectedFile(null);
  }, []);

  const handleGoDashboard = useCallback(() => {
    navigate("/");
    setSelectedFile(null);
  }, [navigate]);

  const handleOpenSettings = useCallback(() => {
    navigate("/settings");
    if (window.innerWidth < 768) setSidebarOpen(false);
  }, [navigate]);

  const handleOpenProjects = useCallback(() => {
    navigate("/projects");
    if (window.innerWidth < 768) setSidebarOpen(false);
  }, [navigate]);

  const handleCloseProjects = useCallback(() => {
    if (activeSessionId) {
      navigate(`/session/${encodeURIComponent(activeSessionId)}`);
    } else {
      navigate("/");
    }
  }, [navigate, activeSessionId]);

  const handleCloseSettings = useCallback(() => {
    if (activeSessionId) {
      navigate(`/session/${encodeURIComponent(activeSessionId)}`);
    } else {
      navigate("/");
    }
  }, [navigate, activeSessionId]);

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
    setShowSessionWizard(true);
  }, []);

  const handleCloneFromUrl = useCallback(() => {
    setWizardPrefill({ initialTab: "clone" });
    setShowSessionWizard(true);
  }, []);

  const handleToggleTerminalFocus = useCallback(() => {
    if (!activeSessionId) return;
    // ContentSplit renders the right pane twice (desktop inline + mobile
    // overlay); each instance mounts its own PairedTerminal. Probing by
    // data-term attribute is robust against that duplication and against
    // future panel reorderings.
    //
    // Semantic: VSCode-like "Cmd+` opens/focuses the terminal." So if the
    // user is NOT in the paired terminal, send them there; only flip back
    // to agent when they're already in paired.
    const active = document.activeElement;
    const pairedPanels = document.querySelectorAll<HTMLElement>(
      '[data-term="paired"]',
    );
    let inPaired = false;
    if (active) {
      for (const p of pairedPanels) {
        if (p.contains(active)) {
          inPaired = true;
          break;
        }
      }
    }
    const target = inPaired ? "agent" : "paired";

    if (target === "paired" && diffCollapsed) {
      // Right panel is collapsed; paired terminal is unmounted. Set the
      // pending intent so PairedTerminal grabs focus once it mounts and
      // its PTY is ready, then expand the panel.
      setPendingTerminalFocus("paired");
      setDiffCollapsed(false);
      return;
    }
    if (target === "agent" && selectedFilePath) {
      // Agent terminal is hidden under the diff viewer; close the diff first
      // so the wrapper un-hides, then dispatch on the next frame because
      // focus() on a display:none element is a no-op.
      setSelectedFile(null);
      requestAnimationFrame(() => dispatchFocusTerminal("agent"));
      return;
    }
    dispatchFocusTerminal(target);
  }, [activeSessionId, diffCollapsed, selectedFilePath]);

  useKeyboardShortcuts(
    useCallback(
      () => ({
        onNew: () => setShowSessionWizard(true),
        onDiff: () => toggleDiff(),
        // Escape closes local UI surfaces only (dialogs, palette,
        // wizard, settings, help, file viewer). Never wire this to
        // cockpit.cancelPrompt; Claude Code CLI does that and stray
        // Escape presses kill in-flight turns the user didn't mean to
        // abort. Cancel/stop must stay behind an explicit gesture
        // (the assistant-ui Stop button in the composer).
        onEscape: () => {
          if (deletingWorkspaceId) {
            setDeletingWorkspaceId(null);
            return;
          }
          if (showPalette) {
            setShowPalette(false);
            return;
          }
          setShowSessionWizard(false);
          setShowHelp(false);
          if (showSettings) handleCloseSettings();
          setShowAbout(false);
          setSelectedFile(null);
        },
        onHelp: () => setShowHelp((h) => !h),
        onSettings: () => (showSettings ? handleCloseSettings() : navigate("/settings")),
        onPalette: () => setShowPalette((p) => !p),
        onToggleSidebar: () => setSidebarOpen((o) => !o),
        onToggleRightPanel: () => setDiffCollapsed((c) => !c),
        onToggleTerminalFocus: handleToggleTerminalFocus,
      }),
      [toggleDiff, showPalette, deletingWorkspaceId, showSettings, handleCloseSettings, navigate, handleToggleTerminalFocus],
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
      return (
        <SettingsView
          tab={settingsTab}
          onClose={handleCloseSettings}
          onSelectTab={(t) => navigate(`/settings/${t}`)}
          serverAbout={serverAbout}
          onServerAboutRefresh={refreshServerAbout}
        />
      );
    }

    if (showProjects) {
      return (
        <ProjectsView
          onClose={handleCloseProjects}
          readOnly={serverAbout?.read_only}
        />
      );
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
                {activeSession?.cockpit_mode ? (
                  <Suspense fallback={<CockpitLoadingFallback />}>
                    <CockpitView
                      key={activeSessionId}
                      sessionId={activeSessionId!}
                      cockpitWorkerState={activeSession.cockpit_worker_state ?? "absent"}
                    />
                  </Suspense>
                ) : (
                  <TerminalView
                    key={activeSessionId}
                    session={activeSession}
                    cockpitMasterEnabled={
                      !!serverAbout?.cockpit_master_enabled
                    }
                  />
                )}
              </div>

              {selectedFilePath && activeSessionId && (
                <DiffFileViewer
                  sessionId={activeSessionId}
                  filePath={selectedFilePath}
                  repoName={selectedRepoName}
                  revision={revision}
                  onClose={handleCloseFile}
                  commentsEnabled={commentsEnabled}
                  commentsStore={diffComments}
                />
              )}
            </div>
          }
          right={
            <RightPanel
              session={activeSession ?? null}
              sessionId={activeSessionId}
              files={diffFiles}
              perRepoBases={perRepoBases}
              warning={warning}
              filesLoading={diffFilesLoading}
              selectedFilePath={selectedFilePath}
              selectedRepoName={selectedRepoName}
              onSelectFile={handleSelectFile}
              onDiffRefresh={refreshDiffFiles}
              commentsEnabled={commentsEnabled}
              commentsCount={diffComments.count}
              commentsSendEnabled={commentSendEnabled}
              commentsSendDisabledReason={commentSendDisabledReason}
              onOpenSendDialog={() => setSendDialogOpen(true)}
              onDiscardAllComments={diffComments.clearComments}
            />
          }
        />
        {sendDialogOpen && commentsEnabled && activeSessionId && (
          <SendCommentsDialog
            sessionId={activeSessionId}
            comments={diffComments.comments}
            isMultiRepo={commentsIsMultiRepo}
            sendEnabled={commentSendEnabled}
            sendDisabledReason={commentSendDisabledReason}
            introDraft={diffComments.introDraft}
            outroDraft={diffComments.outroDraft}
            clearAfterSend={diffComments.clearAfterSend}
            onChangeIntro={diffComments.setIntroDraft}
            onChangeOutro={diffComments.setOutroDraft}
            onChangeClearAfterSend={diffComments.setClearAfterSend}
            onClose={() => setSendDialogOpen(false)}
            onSent={() => {
              if (diffComments.clearAfterSend) {
                diffComments.clearComments();
                diffComments.setIntroDraft("");
                diffComments.setOutroDraft("");
              }
              setSendDialogOpen(false);
              // Close the diff viewer so the cockpit transcript is in
              // view: the user just dispatched feedback and wants to
              // see the agent's response. They can re-open any file
              // from the right-panel list afterwards.
              setSelectedFile(null);
              toastBus.handler?.info("Comments sent to agent");
            }}
          />
        )}
      </div>
    );
  };

  // Lock the root height to the latched max innerHeight on mobile. Without
  // this, iOS PWA / iOS 26 Safari / Android Chrome shrink innerHeight
  // (and therefore 100dvh) when the soft keyboard opens, which propagates
  // to the terminal pane and SIGWINCHes claude on every show/hide.
  // Pinning to the no-keyboard height combined with the keyboard
  // reservation in TerminalView keeps the layout stable across the
  // keyboard cycle.
  //
  // Cockpit substrate doesn't host xterm.js, so the SIGWINCH concern
  // doesn't apply; leaving the pin on for cockpit traps the composer
  // below the keyboard on Android Chrome PWA (#1177). Drop the pin when
  // the active session is cockpit so `h-dvh` plus the viewport meta's
  // `interactive-widget=resizes-content` shrink the container with the
  // keyboard and lift the composer back into view.
  const { isMobile, stableViewportHeight } = useMobileKeyboard();
  const pinRootHeight =
    isMobile && stableViewportHeight > 0 && !activeSession?.cockpit_mode;
  const rootStyle = pinRootHeight
    ? { height: `${stableViewportHeight}px` }
    : undefined;

  const cockpitPrefs = useMemo(
    () => ({
      showToolDurations: serverAbout?.cockpit_show_tool_durations ?? true,
      queueDrainMode: serverAbout?.cockpit_queue_drain_mode ?? "combined",
      forceEndTurnThresholdSecs:
        serverAbout?.cockpit_force_end_turn_threshold_secs ?? 30,
      replayEvents: serverAbout?.cockpit_replay_events ?? 0,
    }),
    [
      serverAbout?.cockpit_show_tool_durations,
      serverAbout?.cockpit_queue_drain_mode,
      serverAbout?.cockpit_force_end_turn_threshold_secs,
      serverAbout?.cockpit_replay_events,
    ],
  );

  return (
    <CockpitPrefsProvider value={cockpitPrefs}>
    <div
      className="h-dvh flex flex-col bg-surface-900 text-text-primary overflow-hidden safe-area-inset"
      style={rootStyle}
    >
      <TopBar
        activeWorkspace={activeWorkspace}
        activeSession={activeSession ?? null}
        onToggleSidebar={handleToggleSidebar}
        onOpenPalette={() => setShowPalette(true)}
        onToggleDiff={toggleDiff}
        diffCollapsed={diffCollapsed}
        onOpenHelp={handleOpenHelp}
        onOpenAbout={handleOpenAbout}
        onLogout={onLogout}
        loginRequired={loginRequired}
        isOffline={!!error}
        onGoDashboard={handleGoDashboard}
      />

      <DisconnectBanner />
      <UpdateBanner />

      <div className="flex flex-1 min-h-0">
        {!showSettings && !showProjects && (
          <WorkspaceSidebar
            groups={groups}
            onReorderWorkspaces={handleReorderWorkspaces}
            activeId={activeWorkspace?.id ?? null}
            open={sidebarOpen}
            onToggle={() => setSidebarOpen(false)}
            onSelect={handleSelectWorkspace}
            onToggleRepo={toggleRepoCollapsed}
            onNew={() => { setWizardPrefill(undefined); setShowSessionWizard(true); }}
            onCreateSession={handleCreateSession}
            onSettings={handleOpenSettings}
            onProjects={handleOpenProjects}
            onDeleteSession={handleDeleteSession}
            readOnly={serverAbout?.read_only}
          />
        )}

        <div className="flex-1 flex flex-col min-h-0 min-w-0">
          {renderContent()}
        </div>
      </div>

      {showSessionWizard && (
        <SessionWizard
          onClose={() => { setShowSessionWizard(false); setWizardPrefill(undefined); }}
          onCreated={(session?: SessionResponse) => {
            if (session) {
              injectSession(session);
              navigate(`/session/${encodeURIComponent(session.id)}`);
              if (window.innerWidth < 768) setSidebarOpen(false);
            }
            setShowSessionWizard(false);
            setWizardPrefill(undefined);
          }}
          prefill={wizardPrefill}
          cockpitMasterEnabled={
            !!serverAbout?.cockpit_master_enabled
          }
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
    </CockpitPrefsProvider>
  );
}

function CockpitLoadingFallback() {
  return (
    <div className="flex h-full items-center justify-center bg-surface-900 text-text-dim">
      <div className="text-xs font-mono uppercase tracking-wide">
        Loading cockpit…
      </div>
    </div>
  );
}
