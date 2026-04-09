import { useState } from "react";
import { useSessions } from "./hooks/useSessions";
import { updateSession } from "./lib/api";
import type { SessionResponse } from "./lib/types";
import { Sidebar } from "./components/Sidebar";
import { TerminalView } from "./components/TerminalView";
import { DiffView } from "./components/DiffView";
import { EmptyState } from "./components/EmptyState";
import { RenameDialog } from "./components/RenameDialog";
import { ProfileSelector } from "./components/ProfileSelector";

type ContentView = "terminal" | "diff";

export default function App() {
  const { sessions, error, refresh } = useSessions();
  const [activeId, setActiveId] = useState<string | null>(null);
  const [mobileShowTerminal, setMobileShowTerminal] = useState(false);
  const [contentView, setContentView] = useState<ContentView>("terminal");
  const [renameTarget, setRenameTarget] = useState<SessionResponse | null>(
    null,
  );
  const [activeProfile, setActiveProfile] = useState<string | null>(null);

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

  return (
    <div className="h-screen flex flex-col bg-surface-900 text-slate-200">
      {/* Header */}
      <header className="h-12 bg-surface-850 border-b border-surface-700 flex items-center px-4 shrink-0">
        <h1 className="font-display text-sm font-semibold tracking-wide text-slate-100">
          Agent of Empires
          <span className="font-body font-normal text-slate-500 ml-1.5">
            Dashboard
          </span>
        </h1>

        <div className="ml-auto flex items-center gap-3">
          <ProfileSelector
            activeProfile={activeProfile}
            onSelect={setActiveProfile}
          />
          <span className="font-mono text-[11px] text-slate-500">
            {error
              ? "connection error"
              : `${filteredSessions.length} session${filteredSessions.length !== 1 ? "s" : ""}`}
          </span>
        </div>
      </header>

      {/* Main */}
      <div className="flex flex-1 overflow-hidden">
        <div className={mobileShowTerminal ? "max-md:hidden" : ""}>
          <Sidebar
            sessions={filteredSessions}
            activeId={activeId}
            onSelect={handleSelect}
            onRefresh={refresh}
            onRename={setRenameTarget}
            onDiff={handleDiff}
          />
        </div>

        <div
          className={`flex-1 flex flex-col overflow-hidden ${!mobileShowTerminal ? "max-md:hidden" : ""}`}
        >
          {activeSession ? (
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

      {/* Rename dialog */}
      {renameTarget && (
        <RenameDialog
          currentTitle={renameTarget.title}
          currentGroup={renameTarget.group_path}
          onSave={handleRename}
          onCancel={() => setRenameTarget(null)}
        />
      )}
    </div>
  );
}
