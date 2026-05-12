// Tiny React context exposing the cockpit's `state.toolOutputs`
// (per-tool-call streaming buffer) to descendants that don't have
// direct access to the cockpit state. Driving use-case: ExecuteToolCard
// reads `useToolPartialOutput(toolId)` so it can render Bash output
// live while the command is still running. See #1075.

import { createContext, useContext, type ReactNode } from "react";

const EMPTY: Record<string, string> = {};

const ToolPartialsContext = createContext<Record<string, string>>(EMPTY);

export function ToolPartialsProvider({
  partials,
  children,
}: {
  partials: Record<string, string>;
  children: ReactNode;
}) {
  return (
    <ToolPartialsContext.Provider value={partials}>
      {children}
    </ToolPartialsContext.Provider>
  );
}

/** Read the live partial buffer for one tool call. Returns the empty
 *  string when no chunks have streamed yet (or the call has already
 *  completed and the buffer was drained). */
export function useToolPartialOutput(toolCallId: string): string {
  return useContext(ToolPartialsContext)[toolCallId] ?? "";
}
