import { useState } from "react";
import { DiffPanel } from "./DiffPanel";
import { TerminalView } from "./TerminalView";
import type { SessionResponse } from "../lib/types";

interface Props {
  session: SessionResponse | null;
  sessionId: string | null;
  expanded: boolean;
  onFileCountChange: (count: number) => void;
}

type ShellMode = "host" | "container";

export function RightPanel({
  session,
  sessionId,
  expanded,
  onFileCountChange,
}: Props) {
  const [shellMode, setShellMode] = useState<ShellMode>("host");
  const isSandboxed = session?.is_sandboxed ?? false;

  return (
    <div className="flex-1 flex flex-col min-h-0 overflow-hidden">
      {/* Upper: diff */}
      <div className="flex-1 flex flex-col min-h-0 border-b border-surface-700">
        <DiffPanel
          sessionId={sessionId}
          expanded={expanded}
          onFileCountChange={onFileCountChange}
        />
      </div>

      {/* Lower: paired terminal */}
      <div className="flex-1 flex flex-col min-h-0">
        {/* Shell mode toggle */}
        <div className="flex items-center gap-1 px-2 py-1 bg-surface-900 border-b border-surface-700 shrink-0">
          <span className="font-mono text-[11px] uppercase tracking-wider text-text-dim mr-1">
            Shell
          </span>
          <button
            onClick={() => setShellMode("host")}
            className={`font-body text-[12px] px-2 py-0.5 rounded cursor-pointer transition-colors ${
              shellMode === "host"
                ? "text-brand-500 bg-brand-600/10"
                : "text-text-dim hover:text-text-muted"
            }`}
          >
            Host
          </button>
          {isSandboxed && (
            <button
              onClick={() => setShellMode("container")}
              className={`font-body text-[12px] px-2 py-0.5 rounded cursor-pointer transition-colors ${
                shellMode === "container"
                  ? "text-brand-500 bg-brand-600/10"
                  : "text-text-dim hover:text-text-muted"
              }`}
            >
              Container
            </button>
          )}
        </div>

        {/* Terminal placeholder */}
        {session ? (
          <div className="flex-1 flex items-center justify-center bg-surface-950 text-text-dim">
            <p className="font-mono text-xs">
              {shellMode === "container"
                ? "Container shell (coming soon)"
                : "Host shell (coming soon)"}
            </p>
          </div>
        ) : (
          <div className="flex-1 flex items-center justify-center bg-surface-950 text-text-dim">
            <p className="font-body text-sm">Select a session</p>
          </div>
        )}
      </div>
    </div>
  );
}
