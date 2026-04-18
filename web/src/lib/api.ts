import type {
  SessionResponse,
  RichDiffFilesResponse,
  RichFileDiffResponse,
  AgentInfo,
  ProfileInfo,
  BrowseResponse,
  GroupInfo,
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
): Promise<RichFileDiffResponse | null> {
  return fetchJson<RichFileDiffResponse>(
    `/api/sessions/${id}/diff/file?path=${encodeURIComponent(filePath)}`,
  );
}

// --- Settings ---

export function getSettings(profile?: string): Promise<Record<string, unknown> | null> {
  const params = profile ? `?profile=${encodeURIComponent(profile)}` : "";
  return fetchJson<Record<string, unknown>>(`/api/settings${params}`);
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

// --- About / server info ---

export interface ServerAbout {
  version: string;
  auth_required: boolean;
  passphrase_enabled: boolean;
  read_only: boolean;
  behind_tunnel: boolean;
  profile: string;
}

export function fetchAbout(): Promise<ServerAbout | null> {
  return fetchJson<ServerAbout>("/api/about");
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
