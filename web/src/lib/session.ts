import type { SessionStatus } from "./types";

/** Tailwind class for status dot color by session status */
export const STATUS_DOT_CLASS: Record<SessionStatus, string> = {
  Running: "bg-status-running",
  Waiting: "bg-status-waiting",
  Idle: "bg-status-idle",
  Error: "bg-status-error",
  Starting: "bg-status-starting",
  Stopped: "bg-status-stopped",
  Unknown: "bg-status-idle",
  Deleting: "bg-status-error",
};

/** Whether a session status means the agent is actively doing something */
export function isSessionActive(status: SessionStatus): boolean {
  return status === "Running" || status === "Waiting" || status === "Starting";
}
