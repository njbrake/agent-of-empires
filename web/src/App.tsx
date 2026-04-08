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
    <div className="h-screen flex flex-col bg-[#0d1117] text-gray-300">
      <header className="h-12 bg-[#161b22] border-b border-[#30363d] flex items-center px-4 shrink-0">
        <h1 className="text-sm font-semibold tracking-wide">
          Agent of Empires
          <span className="text-gray-500 font-normal ml-1.5">Dashboard</span>
        </h1>
        <div className="ml-auto text-xs text-gray-500">
          {error
            ? "Connection error"
            : `${sessions.length} session${sessions.length !== 1 ? "s" : ""}`}
        </div>
      </header>

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
