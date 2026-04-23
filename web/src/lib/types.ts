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
  has_managed_worktree: boolean;
  has_terminal: boolean;
  profile: string;
  cleanup_defaults: CleanupDefaults;
  remote_owner: string | null;
  /** Per-session push-notification overrides. null means "inherit the
   *  server default" for that event type; boolean is an explicit toggle. */
  notify_on_waiting: boolean | null;
  notify_on_idle: boolean | null;
  notify_on_error: boolean | null;
}

export interface CleanupDefaults {
  delete_worktree: boolean;
  delete_branch: boolean;
  delete_sandbox: boolean;
}

export type SessionStatus =
  | "Running"
  | "Waiting"
  | "Idle"
  | "Error"
  | "Starting"
  | "Stopped"
  | "Unknown"
  | "Deleting"
  | "Creating";

/** WebSocket control messages sent from browser to server */
export interface ResizeMessage {
  type: "resize";
  cols: number;
  rows: number;
}

export interface ActivateMessage {
  type: "activate";
}

/** Pause the pane's foreground process (SIGSTOP). Sent by mobile web
 *  clients when entering tmux scrollback so claude's continued output
 *  doesn't shift what the user is reading. Paired with `resume_output`. */
export interface PauseOutputMessage {
  type: "pause_output";
}

export interface ResumeOutputMessage {
  type: "resume_output";
}

/** Server → client control message indicating primary status */
export interface PrimaryStatusMessage {
  type: "primary_status";
  is_primary: boolean;
}

/** Rich diff file info with addition/deletion stats */
export interface RichDiffFile {
  path: string;
  old_path: string | null;
  status:
    | "added"
    | "modified"
    | "deleted"
    | "renamed"
    | "copied"
    | "untracked"
    | "conflicted";
  additions: number;
  deletions: number;
}

/** Response from /api/sessions/{id}/diff/files */
export interface RichDiffFilesResponse {
  files: RichDiffFile[];
  base_branch: string;
  warning: string | null;
}

/** A single line in a structured diff */
export interface RichDiffLine {
  type: "add" | "delete" | "equal";
  old_line_num: number | null;
  new_line_num: number | null;
  content: string;
}

/** A hunk in a structured diff */
export interface RichDiffHunk {
  old_start: number;
  old_lines: number;
  new_start: number;
  new_lines: number;
  lines: RichDiffLine[];
}

/** Response from /api/sessions/{id}/diff/file?path=... */
export interface RichFileDiffResponse {
  file: RichDiffFile;
  hunks: RichDiffHunk[];
  is_binary: boolean;
  /** True if the file was too large to diff inline. */
  truncated: boolean;
}

/** Workspace status derived from session states */
export type WorkspaceStatus = "active" | "idle";

/** Repository group: workspaces sharing the same parent repo */
export interface RepoGroup {
  id: string;
  repoPath: string;
  displayName: string;
  remoteOwner: string | null;
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
}

/** Agent info returned by /api/agents */
export interface AgentInfo {
  name: string;
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

/** Browse response returned by /api/filesystem/browse */
export interface BrowseResponse {
  entries: DirEntry[];
  has_more: boolean;
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
  profile?: string;
}
