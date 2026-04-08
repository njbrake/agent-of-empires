import { useState } from "react";
import { useSessions } from "./hooks/useSessions";
import { Sidebar } from "./components/Sidebar";
import { TerminalView } from "./components/TerminalView";
import { EmptyState } from "./components/EmptyState";

export default function App() {
  const { sessions, error, refresh } = useSessions();
  const [activeId, setActiveId] = useState<string | null>(null);
  const [mobileShowTerminal, setMobileShowTerminal] = useState(false);

  const activeSession = sessions.find((s) => s.id === activeId);

  const handleSelect = (id: string) => {
    setActiveId(id);
    setMobileShowTerminal(true);
  };

  const handleBack = () => {
    setMobileShowTerminal(false);
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
        <div className="ml-auto font-mono text-[11px] text-slate-500">
          {error
            ? "connection error"
            : `${sessions.length} session${sessions.length !== 1 ? "s" : ""}`}
        </div>
      </header>

      {/* Main */}
      <div className="flex flex-1 overflow-hidden">
        <div className={mobileShowTerminal ? "max-md:hidden" : ""}>
          <Sidebar
            sessions={sessions}
            activeId={activeId}
            onSelect={handleSelect}
            onRefresh={refresh}
          />
        </div>

        <div
          className={`flex-1 flex flex-col overflow-hidden ${!mobileShowTerminal ? "max-md:hidden" : ""}`}
        >
          {activeSession ? (
            <TerminalView
              key={activeSession.id}
              session={activeSession}
              onBack={handleBack}
            />
          ) : (
            <EmptyState />
          )}
        </div>
      </div>
    </div>
  );
}
