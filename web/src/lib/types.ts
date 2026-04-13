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
  profile: string;
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

/** Agent info returned by /api/agents */
export interface AgentInfo {
  name: string;
  description: string;
  binary: string;
  host_only: boolean;
  installed: boolean;
}

/** Profile info returned by /api/profiles */
export interface ProfileInfo {
  name: string;
  is_default: boolean;
}

/** Directory entry returned by /api/filesystem/browse */
export interface DirEntry {
  name: string;
  path: string;
  is_dir: boolean;
  is_git_repo: boolean;
}

/** Branch info returned by /api/git/branches */
export interface BranchInfo {
  name: string;
  is_current: boolean;
}

/** Group info returned by /api/groups */
export interface GroupInfo {
  path: string;
  session_count: number;
}

/** Docker status returned by /api/docker/status */
export interface DockerStatusResponse {
  available: boolean;
  runtime: string | null;
}

/** Request body for POST /api/sessions */
export interface CreateSessionRequest {
  title?: string;
  path: string;
  tool: string;
  group?: string;
  yolo_mode?: boolean;
  worktree_branch?: string;
  create_new_branch?: boolean;
  sandbox?: boolean;
  extra_args?: string;
  sandbox_image?: string;
  extra_env?: string[];
  extra_repo_paths?: string[];
  command_override?: string;
  custom_instruction?: string;
  cpu_limit?: string;
  memory_limit?: string;
  port_mappings?: string[];
  mount_ssh?: boolean;
  volume_ignores?: string[];
  extra_volumes?: string[];
}
