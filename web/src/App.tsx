import { useCallback, useMemo, useState } from "react";
import { useSessions } from "./hooks/useSessions";
import { useWorkspaces } from "./hooks/useWorkspaces";
import { useKeyboardShortcuts } from "./hooks/useKeyboardShortcuts";
import { isSessionActive } from "./lib/session";
import { WorkspaceSidebar } from "./components/WorkspaceSidebar";
import { WorkspaceHeader } from "./components/WorkspaceHeader";
import { ContentSplit } from "./components/ContentSplit";
import { TerminalView } from "./components/TerminalView";
import { RightPanel } from "./components/RightPanel";
import { SettingsView } from "./components/SettingsView";
import { HelpOverlay } from "./components/HelpOverlay";

export default function App() {
  const { sessions, error } = useSessions();
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
      <header className="h-10 bg-surface-800 border-b border-surface-700/20 flex items-center px-2 shrink-0 gap-1.5">
        <button
          onClick={() => setSidebarOpen((o) => !o)}
          className={`w-10 h-10 flex items-center justify-center cursor-pointer rounded-md transition-colors -ml-1 ${
            sidebarOpen
              ? "text-text-secondary"
              : "text-text-dim hover:text-text-secondary"
          }`}
          title="Toggle sidebar"
          aria-label="Toggle sidebar"
        >
          <svg
            width="18"
            height="18"
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

        <div className="flex-1" />

        <div className="ml-auto flex items-center gap-1">
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
          <button
            onClick={toggleDiff}
            className={`hidden sm:flex w-10 h-10 items-center justify-center cursor-pointer rounded-md transition-colors ${
              diffCollapsed
                ? "text-text-dim hover:text-text-secondary"
                : "text-text-secondary"
            }`}
            title="Toggle diff panel"
            aria-label="Toggle diff panel"
          >
            <svg
              width="18"
              height="18"
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

      {/* Not supported dialog */}
      {showCreate && (
        <div
          className="fixed inset-0 bg-black/60 flex items-center justify-center z-50 animate-fade-in"
          onClick={() => setShowCreate(false)}
        >
          <div
            className="bg-surface-800 border border-surface-700/30 rounded-xl px-6 py-5 max-w-sm text-center"
            onClick={(e) => e.stopPropagation()}
          >
            <p className="font-body text-sm text-text-primary mb-1">
              Not supported yet
            </p>
            <p className="font-body text-xs text-text-dim mb-4">
              Create sessions from the terminal with the aoe CLI.
            </p>
            <button
              onClick={() => setShowCreate(false)}
              className="font-body text-xs text-text-muted hover:text-text-secondary cursor-pointer"
            >
              Close
            </button>
          </div>
        </div>
      )}

      {showHelp && <HelpOverlay onClose={() => setShowHelp(false)} />}
    </div>
  );
}
