import type {
  SessionResponse,
  RichDiffFilesResponse,
  RichFileDiffResponse,
  AgentInfo,
  AvkAgentInfo,
  AvkAgentRole,
  AvkBroadcastRequest,
  AvkBroadcastResponse,
  AvkHealthResponse,
  AvkMemoryEntry,
  AvkPanePeekResponse,
  FurkanChatRequest,
  FurkanChatResponse,
  FurkanInboxResponse,
  GitFlowError,
  GitFlowResponse,
  ErrorBoardError,
  ErrorBoardResponse,
  LinearQueueError,
  LinearQueueResponse,
  RoadmapError,
  RoadmapResponse,
  ProfileInfo,
  BrowseResponse,
  GroupInfo,
  ProjectInfo,
  DockerStatusResponse,
  CreateSessionRequest,
} from "./types";

// GET a JSON endpoint; returns null on non-2xx or network/parse errors.
async function fetchJson<T>(url: string, init?: RequestInit): Promise<T | null> {
  try {
    const res = await fetch(url, init);
    if (!res.ok) return null;
    return (await res.json()) as T;
  } catch {
    return null;
  }
}

// --- Sessions ---

export function fetchSessions(): Promise<SessionResponse[] | null> {
  return fetchJson<SessionResponse[]>("/api/sessions");
}

export interface EnsureSessionResult {
  ok: boolean;
  status?: "alive" | "restarted";
  error?: string;
  message?: string;
}

export async function ensureSession(
  id: string,
  signal?: AbortSignal,
): Promise<EnsureSessionResult> {
  try {
    const res = await fetch(`/api/sessions/${id}/ensure`, {
      method: "POST",
      signal,
    });
    const body = await res.json().catch(() => ({}));
    if (!res.ok) {
      return {
        ok: false,
        error: typeof body.error === "string" ? body.error : undefined,
        message:
          typeof body.message === "string"
            ? body.message
            : `Server error (${res.status})`,
      };
    }
    return {
      ok: true,
      status: body.status as "alive" | "restarted" | undefined,
    };
  } catch (e) {
    if ((e as { name?: string }).name === "AbortError") {
      return { ok: false, error: "aborted" };
    }
    return {
      ok: false,
      message: e instanceof Error ? e.message : "Network error",
    };
  }
}

export async function ensureTerminal(
  id: string,
  container = false,
): Promise<boolean> {
  const path = container ? "container-terminal" : "terminal";
  try {
    const res = await fetch(`/api/sessions/${id}/${path}`, {
      method: "POST",
    });
    return res.ok;
  } catch {
    return false;
  }
}

export function getSessionDiffFiles(
  id: string,
): Promise<RichDiffFilesResponse | null> {
  return fetchJson<RichDiffFilesResponse>(`/api/sessions/${id}/diff/files`);
}

export function getSessionFileDiff(
  id: string,
  filePath: string,
  repoName?: string,
): Promise<RichFileDiffResponse | null> {
  const params = new URLSearchParams({ path: filePath });
  if (repoName) params.set("repo", repoName);
  return fetchJson<RichFileDiffResponse>(
    `/api/sessions/${id}/diff/file?${params.toString()}`,
  );
}

// --- Settings ---

export interface SettingsResponse {
  theme?: {
    idle_decay_minutes?: number;
  };
  [key: string]: unknown;
}

export function fetchSettings(profile?: string): Promise<SettingsResponse | null> {
  const params = profile ? `?profile=${encodeURIComponent(profile)}` : "";
  return fetchJson<SettingsResponse>(`/api/settings${params}`);
}

export async function updateSettings(
  updates: Record<string, unknown>,
): Promise<boolean> {
  try {
    const res = await fetch("/api/settings", {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(updates),
    });
    return res.ok;
  } catch {
    return false;
  }
}

// --- Profile management ---

export async function createProfile(name: string): Promise<boolean> {
  try {
    const res = await fetch("/api/profiles", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name }),
    });
    return res.ok;
  } catch {
    return false;
  }
}

export async function deleteProfile(name: string): Promise<boolean> {
  try {
    const res = await fetch(`/api/profiles/${encodeURIComponent(name)}`, {
      method: "DELETE",
    });
    return res.ok;
  } catch {
    return false;
  }
}

export async function renameProfile(
  name: string,
  newName: string,
): Promise<boolean> {
  try {
    const res = await fetch(
      `/api/profiles/${encodeURIComponent(name)}/rename`,
      {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ new_name: newName }),
      },
    );
    return res.ok;
  } catch {
    return false;
  }
}

export async function setDefaultProfile(name: string): Promise<boolean> {
  try {
    const res = await fetch("/api/default-profile", {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ name }),
    });
    return res.ok;
  } catch {
    return false;
  }
}

export function getProfileSettings(
  name: string,
): Promise<Record<string, unknown> | null> {
  return fetchJson<Record<string, unknown>>(
    `/api/profiles/${encodeURIComponent(name)}/settings`,
  );
}

export async function updateProfileSettings(
  name: string,
  updates: Record<string, unknown>,
): Promise<boolean> {
  try {
    const res = await fetch(
      `/api/profiles/${encodeURIComponent(name)}/settings`,
      {
        method: "PATCH",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify(updates),
      },
    );
    return res.ok;
  } catch {
    return false;
  }
}

// --- Themes & Sounds ---

export async function fetchThemes(): Promise<string[]> {
  return (await fetchJson<string[]>("/api/themes")) ?? [];
}

export async function fetchSounds(): Promise<string[]> {
  return (await fetchJson<string[]>("/api/sounds")) ?? [];
}

// --- About / server info ---

export interface ServerAbout {
  version: string;
  auth_required: boolean;
  passphrase_enabled: boolean;
  read_only: boolean;
  behind_tunnel: boolean;
  profile: string;
  /** Live value of the cockpit master switch (`config.cockpit.enabled`).
   *  Toggleable from the web settings via PATCH /api/cockpit/master.
   *  When true, new sessions for ACP-capable tools default to cockpit
   *  mode; when false, every new session is tmux. */
  cockpit_master_enabled: boolean;
  /** Resolved `cockpit.show_tool_durations` from the active profile's
   *  config. Drives the per-tool elapsed-time label in the cockpit
   *  web UI; cross-device since it lives in config.toml. */
  cockpit_show_tool_durations: boolean;
  /** Resolved `cockpit.queue_drain_mode` from the active profile's
   *  config. Selects how the composer drains client-side queued
   *  follow-up prompts on Stopped: `combined` (default) joins them
   *  with blank lines into a single prompt; `serial` fires one entry
   *  at a time. See #1031. */
  cockpit_queue_drain_mode: "combined" | "serial";
  /** Resolved `cockpit.max_concurrent_resumes` from the active
   *  profile's config. Upper bound on parallel cockpit worker
   *  spawns/attaches the reconciler runs on `aoe serve` cold start.
   *  See #1088. */
  cockpit_max_concurrent_resumes: number;
  /** Resolved `cockpit.force_end_turn_threshold_secs` from the active
   *  profile's config. Seconds of streaming inactivity after which
   *  the cockpit web UI offers a "Force end turn" button. See #1100. */
  cockpit_force_end_turn_threshold_secs: number;
  /** Resolved `cockpit.replay_events` from the active profile's
   *  config. Per-session retention cap on the cockpit event log;
   *  0 means unlimited. Mirrored onto the in-memory activity buffer
   *  so the rendered transcript matches the user's chosen ceiling
   *  instead of clipping at a hard-coded frontend constant. See #1111. */
  cockpit_replay_events: number;
}

export async function setCockpitMaster(
  enabled: boolean,
): Promise<{ master_enabled: boolean } | null> {
  try {
    const res = await fetch("/api/cockpit/master", {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ enabled }),
    });
    if (!res.ok) return null;
    return await res.json();
  } catch {
    return null;
  }
}

export function fetchAbout(): Promise<ServerAbout | null> {
  return fetchJson<ServerAbout>("/api/about");
}

export interface UpdateStatus {
  check_enabled: boolean;
  current_version: string;
  latest_version: string | null;
  update_available: boolean;
  release_url: string | null;
  web_poll_interval_minutes: number;
  error: string | null;
}

export function fetchUpdateStatus(): Promise<UpdateStatus | null> {
  return fetchJson<UpdateStatus>("/api/system/update-status");
}

// --- Branches ---

export interface BranchInfo {
  name: string;
  is_current: boolean;
  remote_only?: boolean;
}

/** Lists branches for a repo path. When `includeRemote` is true the
 *  response includes branches that only exist on the remote (with
 *  `remote_only: true`); selecting one bases the new worktree off the
 *  remote tip. See #948. */
export function fetchBranches(
  path: string,
  includeRemote = false,
): Promise<BranchInfo[] | null> {
  const params = new URLSearchParams({ path });
  if (includeRemote) params.set("include_remote", "true");
  return fetchJson<BranchInfo[]>(`/api/git/branches?${params.toString()}`);
}

// --- Cockpit context primer ---

export interface ContextPrimerResponse {
  primer: string;
  included_event_count: number;
  included_turn_count: number;
  truncated: boolean;
  max_chars: number;
}

/** Fetch a markdown primer built from events `seq < beforeSeq`. Used
 *  after a `session/load` failure: the agent's model context is empty
 *  but the transcript is intact in SQLite, so the user can opt in to
 *  pre-filling the composer with a compact recap. See #1004. */
export function fetchContextPrimer(
  sessionId: string,
  beforeSeq: number,
  signal?: AbortSignal,
): Promise<ContextPrimerResponse | null> {
  const params = new URLSearchParams({ before_seq: String(beforeSeq) });
  return fetchJson<ContextPrimerResponse>(
    `/api/sessions/${encodeURIComponent(sessionId)}/cockpit/context-primer?${params.toString()}`,
    signal ? { signal } : undefined,
  );
}

// --- Devices ---

export interface DeviceInfo {
  ip: string;
  user_agent: string;
  first_seen: string;
  last_seen: string;
  request_count: number;
}

export function fetchDevices(): Promise<DeviceInfo[] | null> {
  return fetchJson<DeviceInfo[]>("/api/devices");
}

// --- Wizard APIs ---

export async function fetchAgents(): Promise<AgentInfo[]> {
  return (await fetchJson<AgentInfo[]>("/api/agents")) ?? [];
}

/**
 * `GET /api/avk/agents[?role=...]` — FUR-3957 Adım 6 endpoint.
 *
 * Opsiyonel `role` filter director|senior|worker. Geçersiz role server
 * 400 döner; bu wrapper null'a indirgenmiş listeyi `[]` olarak verir.
 */
export async function fetchAvkAgents(role?: AvkAgentRole): Promise<AvkAgentInfo[]> {
  const url = role ? `/api/avk/agents?role=${role}` : "/api/avk/agents";
  return (await fetchJson<AvkAgentInfo[]>(url)) ?? [];
}

/**
 * `GET /api/avk/furkan-inbox?limit=N&unread_only=bool` — FUR-4170 inbox.
 *
 * Ajan→Furkan signal'leri (memory_signal_read agentId=furkan). Çağrı
 * delivered mesajları read'e işaretler (server tarafı). Hata null.
 */
export async function fetchAvkFurkanInbox(
  limit = 50,
  unreadOnly = false,
): Promise<FurkanInboxResponse | null> {
  const params = new URLSearchParams({ limit: String(limit) });
  if (unreadOnly) params.set("unread_only", "true");
  return await fetchJson<FurkanInboxResponse>(
    `/api/avk/furkan-inbox?${params.toString()}`,
  );
}

/**
 * `POST /api/avk/furkan-chat` — FUR-4164 agentmemory signal_send wrapper.
 *
 * Furkan'dan AVK ajanına chat mesajı yollar. Başarılı 200 →
 * `FurkanChatResponse` (signal_id + thread_id). Hata null döner.
 */
export async function postAvkFurkanChat(
  payload: FurkanChatRequest,
): Promise<FurkanChatResponse | null> {
  try {
    const res = await fetch("/api/avk/furkan-chat", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(payload),
    });
    if (!res.ok) return null;
    return (await res.json()) as FurkanChatResponse;
  } catch {
    return null;
  }
}

/**
 * `GET /api/avk/git-flow` — FUR-4162 gh CLI proxy (açık + son merged PR'lar).
 *
 * 200 → `GitFlowResponse`. 502 (`kind: "gh_unavailable"`) backend gh
 * eksik/unauthenticated. Diğer hata null döner.
 */
export async function fetchAvkGitFlow(): Promise<
  GitFlowResponse | GitFlowError | null
> {
  try {
    const res = await fetch("/api/avk/git-flow");
    if (res.ok) {
      return (await res.json()) as GitFlowResponse;
    }
    if (res.status === 502) {
      return (await res.json()) as GitFlowError;
    }
    return null;
  } catch {
    return null;
  }
}

/**
 * `GET /api/avk/pane-peek?slug=<slug>&lines=<N>` — FUR-4161 tmux capture-pane.
 *
 * 404 (bilinmeyen slug) / 502 (tmux capture hatası) durumunda null döner;
 * UI bunu "preview alınamadı" mesajı olarak gösterir.
 */
export async function fetchAvkPanePeek(
  slug: string,
  lines = 40,
): Promise<AvkPanePeekResponse | null> {
  return await fetchJson<AvkPanePeekResponse>(
    `/api/avk/pane-peek?slug=${encodeURIComponent(slug)}&lines=${lines}`,
  );
}

/**
 * `GET /api/avk/error-board` — FUR-4169 Hata Ajanı panosu.
 *
 * Aktif (started/unstarted/triage) + Son tamamlanmış (completed) bug issue'lar.
 */
export async function fetchAvkErrorBoard(): Promise<
  ErrorBoardResponse | ErrorBoardError | null
> {
  try {
    const res = await fetch("/api/avk/error-board");
    if (res.ok) {
      return (await res.json()) as ErrorBoardResponse;
    }
    if (res.status === 503 || res.status === 502) {
      return (await res.json()) as ErrorBoardError;
    }
    return null;
  } catch {
    return null;
  }
}

/**
 * `GET /api/avk/roadmap` — FUR-4165 Linear initiatives + projects.
 *
 * 200 → `RoadmapResponse`. 503/502 → `RoadmapError`. Fetch hata null.
 */
export async function fetchAvkRoadmap(): Promise<
  RoadmapResponse | RoadmapError | null
> {
  try {
    const res = await fetch("/api/avk/roadmap");
    if (res.ok) {
      return (await res.json()) as RoadmapResponse;
    }
    if (res.status === 503 || res.status === 502) {
      return (await res.json()) as RoadmapError;
    }
    return null;
  } catch {
    return null;
  }
}

/**
 * `GET /api/avk/linear-queue` — FUR-4160 Linear kuyruğu endpoint.
 *
 * 200 → `LinearQueueResponse`. 503 (`kind: "not_configured"`) veya 502
 * (`kind: "upstream_error"`) durumunda body error string ile `LinearQueueError`
 * döner; çağıran union'a göre branch'ler. fetch hatası null döner.
 */
export async function fetchAvkLinearQueue(): Promise<
  LinearQueueResponse | LinearQueueError | null
> {
  try {
    const res = await fetch("/api/avk/linear-queue");
    if (res.ok) {
      return (await res.json()) as LinearQueueResponse;
    }
    if (res.status === 503 || res.status === 502) {
      return (await res.json()) as LinearQueueError;
    }
    return null;
  } catch {
    return null;
  }
}

/**
 * `GET /api/avk/health` — FUR-4157 sistem sağlık endpoint.
 *
 * AoE daemon version + uptime + tmux durumu + canlı ajan oranı.
 * Hata durumunda null döner (UI fallback "bilinmiyor" gösterir).
 */
export async function fetchAvkHealth(): Promise<AvkHealthResponse | null> {
  return await fetchJson<AvkHealthResponse>("/api/avk/health");
}

/**
 * `GET /api/avk/memory-recall[?role=...&hours=...]` — FUR-4118 endpoint.
 *
 * Mock implementation; server static MOCK_FEED döner. Gerçek agentmemory MCP
 * proxy eklendiğinde aynı shape, UI değişikliği yok.
 */
export async function fetchAvkMemoryRecall(
  role?: string,
  hours?: number,
): Promise<AvkMemoryEntry[]> {
  const params = new URLSearchParams();
  if (role) params.set("role", role);
  if (hours) params.set("hours", String(hours));
  const qs = params.toString();
  const url = qs ? `/api/avk/memory-recall?${qs}` : "/api/avk/memory-recall";
  return (await fetchJson<AvkMemoryEntry[]>(url)) ?? [];
}

/**
 * `POST /api/avk/broadcast` — FUR-4121 endpoint.
 *
 * Server tier resolver `director`/`senior`/`worker`/`all` keyword'unu
 * AVK_AGENTS registry'sinden filtreler ve tmux pane'lere bracketed-paste
 * mesaj yollar. Tek pane fail durumunda yine 200 döner, `results` listesinde
 * bireysel hata kayıtları olur. 400 (boş mesaj), 404 (bilinmeyen tier),
 * 413 (8KB üstü mesaj) error response döner.
 */
export async function postAvkBroadcast(
  req: AvkBroadcastRequest,
): Promise<AvkBroadcastResponse | null> {
  try {
    const res = await fetch("/api/avk/broadcast", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(req),
    });
    if (!res.ok) return null;
    return (await res.json()) as AvkBroadcastResponse;
  } catch {
    return null;
  }
}

export async function fetchProfiles(): Promise<ProfileInfo[]> {
  return (await fetchJson<ProfileInfo[]>("/api/profiles")) ?? [];
}

export async function getHomePath(): Promise<string | null> {
  const data = await fetchJson<{ path?: string }>("/api/filesystem/home");
  return data?.path ?? null;
}

export async function browseFilesystem(
  path: string,
  limit?: number,
): Promise<BrowseResponse & { ok: boolean }> {
  const params = new URLSearchParams({ path });
  if (limit != null) params.set("limit", String(limit));
  const data = await fetchJson<BrowseResponse>(`/api/filesystem/browse?${params}`);
  if (!data) return { entries: [], has_more: false, ok: false };
  return { ...data, ok: true };
}

export async function fetchGroups(): Promise<GroupInfo[]> {
  return (await fetchJson<GroupInfo[]>("/api/groups")) ?? [];
}

export async function fetchProjects(scope?: "global" | "profile"): Promise<ProjectInfo[]> {
  const url = scope ? `/api/projects?scope=${scope}` : "/api/projects";
  return (await fetchJson<ProjectInfo[]>(url)) ?? [];
}

export async function createProject(body: {
  path: string;
  name?: string;
  scope?: "global" | "profile";
  allow_override?: boolean;
}): Promise<{ ok: boolean; error?: string; project?: ProjectInfo }> {
  try {
    const res = await fetch("/api/projects", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    if (!res.ok) {
      const text = await res.text();
      try {
        const data = JSON.parse(text);
        return { ok: false, error: data.message || `Server error (${res.status})` };
      } catch {
        return { ok: false, error: text || `Server error (${res.status})` };
      }
    }
    const project = (await res.json()) as ProjectInfo;
    return { ok: true, project };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : String(e) };
  }
}

export async function deleteProject(
  name: string,
  scope: "global" | "profile",
): Promise<{ ok: boolean; error?: string }> {
  try {
    const res = await fetch(
      `/api/projects/${encodeURIComponent(name)}?scope=${scope}`,
      { method: "DELETE" },
    );
    if (!res.ok) {
      const text = await res.text();
      try {
        const data = JSON.parse(text);
        return { ok: false, error: data.message || `Server error (${res.status})` };
      } catch {
        return { ok: false, error: text || `Server error (${res.status})` };
      }
    }
    return { ok: true };
  } catch (e) {
    return { ok: false, error: e instanceof Error ? e.message : String(e) };
  }
}

export async function fetchDockerStatus(): Promise<DockerStatusResponse> {
  return (
    (await fetchJson<DockerStatusResponse>("/api/docker/status")) ?? {
      available: false,
      runtime: null,
    }
  );
}

export async function createSession(
  body: CreateSessionRequest,
): Promise<{ ok: boolean; error?: string; session?: SessionResponse }> {
  try {
    const res = await fetch("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    if (!res.ok) {
      const text = await res.text();
      try {
        const data = JSON.parse(text);
        return {
          ok: false,
          error: data.message || `Server error (${res.status})`,
        };
      } catch {
        return {
          ok: false,
          error: `Server error (${res.status}): ${text.slice(0, 200)}`,
        };
      }
    }
    const data = await res.json();
    return { ok: true, session: data };
  } catch (e) {
    return {
      ok: false,
      error: `Network error: ${e instanceof Error ? e.message : "connection failed"}`,
    };
  }
}

// --- Clone ---

export async function cloneRepo(
  url: string,
  opts?: { destination?: string; shallow?: boolean },
): Promise<{ ok: boolean; path?: string; error?: string }> {
  try {
    const body: Record<string, unknown> = { url };
    if (opts?.destination) body.destination = opts.destination;
    if (opts?.shallow) body.shallow = true;
    const res = await fetch("/api/git/clone", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(body),
    });
    const data = await res.json().catch(() => ({}));
    if (!res.ok) {
      return {
        ok: false,
        error: data.message || `Clone failed (${res.status})`,
      };
    }
    return { ok: true, path: data.path };
  } catch (e) {
    return {
      ok: false,
      error: `Network error: ${e instanceof Error ? e.message : "connection failed"}`,
    };
  }
}

// --- Login ---

export async function loginStatus(): Promise<{
  required: boolean;
  authenticated: boolean;
}> {
  return (
    (await fetchJson<{ required: boolean; authenticated: boolean }>(
      "/api/login/status",
    )) ?? { required: false, authenticated: true }
  );
}

/** Verify the auth token via a session-exempt endpoint (`/api/login/status`).
 *  Returning `true` means the token authenticated; the caller still has to
 *  consult `loginStatus()` to decide between the main app and LoginPage.
 *  Used by the token entry page so a valid-token-but-needs-passphrase paste
 *  is accepted instead of being misread as a token rejection. */
export async function verifyToken(): Promise<boolean> {
  try {
    const res = await fetch("/api/login/status");
    return res.ok;
  } catch {
    return false;
  }
}

export async function login(
  passphrase: string,
): Promise<{ ok: boolean; error?: string }> {
  try {
    const res = await fetch("/api/login", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ passphrase }),
    });
    if (res.ok) return { ok: true };
    const data = await res.json().catch(() => null);
    return {
      ok: false,
      error: data?.message ?? `Login failed (${res.status})`,
    };
  } catch {
    return { ok: false, error: "Network error" };
  }
}

export async function logout(): Promise<void> {
  try {
    await fetch("/api/logout", { method: "POST" });
  } catch {
    // Best effort
  }
}

export async function renameSession(
  id: string,
  title: string,
): Promise<boolean> {
  try {
    const res = await fetch(`/api/sessions/${id}`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ title }),
    });
    return res.ok;
  } catch {
    return false;
  }
}

/** Three-preset helper for the sidebar context menu:
 *  - "off":     set all three overrides to false (silence this session)
 *  - "default": clear all three overrides (inherit server defaults)
 *  - "all":     set all three overrides to true (notify on any event)
 *  Sends all three fields in one PATCH to avoid multi-request ordering. */
export async function setSessionNotifications(
  id: string,
  preset: "off" | "default" | "all",
): Promise<boolean> {
  const value =
    preset === "off" ? false : preset === "all" ? true : null;
  try {
    const res = await fetch(`/api/sessions/${id}/notifications`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        notify_on_waiting: value,
        notify_on_idle: value,
        notify_on_error: value,
      }),
    });
    return res.ok;
  } catch {
    return false;
  }
}

export interface DeleteSessionOptions {
  delete_worktree?: boolean;
  delete_branch?: boolean;
  delete_sandbox?: boolean;
  force_delete?: boolean;
}

export interface DeleteSessionResult {
  ok: boolean;
  error?: string;
}

export async function deleteSession(
  id: string,
  options: DeleteSessionOptions = {},
): Promise<DeleteSessionResult> {
  try {
    const res = await fetch(`/api/sessions/${id}`, {
      method: "DELETE",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(options),
    });
    if (!res.ok) {
      const data = await res.json().catch(() => ({}));
      return {
        ok: false,
        error: data.message || `Server error (${res.status})`,
      };
    }
    return { ok: true };
  } catch (e) {
    return {
      ok: false,
      error: `Network error: ${e instanceof Error ? e.message : "connection failed"}`,
    };
  }
}
