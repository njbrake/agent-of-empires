/** Session data returned by the API */
export interface SessionResponse {
  id: string;
  title: string;
  project_path: string;
  group_path: string;
  tool: string;
  status: SessionStatus;
  yolo_mode: boolean;
  created_at: string;
  last_accessed_at: string | null;
  last_error: string | null;
  branch: string | null;
  is_sandboxed: boolean;
  has_terminal: boolean;
}

export type SessionStatus =
  | "Running"
  | "Waiting"
  | "Idle"
  | "Error"
  | "Starting"
  | "Stopped"
  | "Unknown"
  | "Deleting";

/** WebSocket control messages sent from browser to server */
export interface ResizeMessage {
  type: "resize";
  cols: number;
  rows: number;
}
