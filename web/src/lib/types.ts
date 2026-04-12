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
  main_repo_path: string | null;
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

/** Diff response */
export interface DiffResponse {
  files: DiffFileInfo[];
  raw: string;
}

export interface DiffFileInfo {
  path: string;
  status: string;
}

/** Workspace status derived from session states */
export type WorkspaceStatus = "active" | "idle";

/** Repository group: workspaces sharing the same parent repo */
export interface RepoGroup {
  id: string;
  repoPath: string;
  displayName: string;
  workspaces: Workspace[];
  status: WorkspaceStatus;
  collapsed: boolean;
}

/** Workspace: a group of sessions sharing the same project + branch */
export interface Workspace {
  id: string;
  branch: string | null;
  projectPath: string;
  displayName: string;
  agents: string[];
  primaryAgent: string;
  status: WorkspaceStatus;
  sessions: SessionResponse[];
  diff?: DiffResponse;
}
