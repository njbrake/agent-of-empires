import type { SessionStatus } from "./types";

/** Tailwind class for status dot background color by session status */
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

/** Tailwind class for status text color by session status */
export const STATUS_TEXT_CLASS: Record<SessionStatus, string> = {
  Running: "text-status-running",
  Waiting: "text-status-waiting",
  Idle: "text-status-idle",
  Error: "text-status-error",
  Starting: "text-status-starting",
  Stopped: "text-status-stopped",
  Unknown: "text-status-idle",
  Deleting: "text-status-error",
};

/** Whether a session status means the agent is actively doing something */
export function isSessionActive(status: SessionStatus): boolean {
  return status === "Running" || status === "Waiting" || status === "Starting";
}
