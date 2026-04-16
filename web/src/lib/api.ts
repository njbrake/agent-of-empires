import type {
  SessionResponse,
  RichDiffFilesResponse,
  RichFileDiffResponse,
  AgentInfo,
  ProfileInfo,
  BrowseResponse,
  BranchInfo,
  GroupInfo,
  DockerStatusResponse,
  CreateSessionRequest,
} from "./types";

// --- Sessions ---

export async function fetchSessions(): Promise<SessionResponse[] | null> {
  try {
    const res = await fetch("/api/sessions");
    if (!res.ok) return null;
    return await res.json();
  } catch {
    return null;
  }
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

export async function getSessionDiffFiles(
  id: string,
): Promise<RichDiffFilesResponse | null> {
  try {
    const res = await fetch(`/api/sessions/${id}/diff/files`);
    if (!res.ok) return null;
    return await res.json();
  } catch {
    return null;
  }
}

export async function getSessionFileDiff(
  id: string,
  filePath: string,
): Promise<RichFileDiffResponse | null> {
  try {
    const res = await fetch(
      `/api/sessions/${id}/diff/file?path=${encodeURIComponent(filePath)}`,
    );
    if (!res.ok) return null;
    return await res.json();
  } catch {
    return null;
  }
}

// --- Settings ---

export async function getSettings(profile?: string): Promise<Record<string, unknown> | null> {
  try {
    const params = profile ? `?profile=${encodeURIComponent(profile)}` : "";
    const res = await fetch(`/api/settings${params}`);
    if (!res.ok) return null;
    return await res.json();
  } catch {
    return null;
  }
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

export async function fetchAbout(): Promise<ServerAbout | null> {
  try {
    const res = await fetch("/api/about");
    if (!res.ok) return null;
    return await res.json();
  } catch {
    return null;
  }
}

// --- Devices ---

export interface DeviceInfo {
  ip: string;
  user_agent: string;
  first_seen: string;
  last_seen: string;
  request_count: number;
}

export async function fetchDevices(): Promise<DeviceInfo[] | null> {
  try {
    const res = await fetch("/api/devices");
    if (!res.ok) return null;
    return await res.json();
  } catch {
    return null;
  }
}

// --- Themes ---

export async function fetchThemes(): Promise<string[]> {
  try {
    const res = await fetch("/api/themes");
    if (!res.ok) return [];
    return await res.json();
  } catch {
    return [];
  }
}

// --- Wizard APIs ---

export async function fetchAgents(): Promise<AgentInfo[]> {
  try {
    const res = await fetch("/api/agents");
    if (!res.ok) return [];
    return await res.json();
  } catch {
    return [];
  }
}

export async function fetchProfiles(): Promise<ProfileInfo[]> {
  try {
    const res = await fetch("/api/profiles");
    if (!res.ok) return [];
    return await res.json();
  } catch {
    return [];
  }
}

export async function getHomePath(): Promise<string | null> {
  try {
    const res = await fetch("/api/filesystem/home");
    if (!res.ok) return null;
    const data = await res.json();
    return data.path ?? null;
  } catch {
    return null;
  }
}

export async function browseFilesystem(
  path: string,
  limit?: number,
): Promise<BrowseResponse & { ok: boolean }> {
  try {
    const params = new URLSearchParams({ path });
    if (limit != null) params.set("limit", String(limit));
    const res = await fetch(`/api/filesystem/browse?${params}`);
    if (!res.ok) return { entries: [], has_more: false, ok: false };
    const data = await res.json();
    return { ...data, ok: true };
  } catch {
    return { entries: [], has_more: false, ok: false };
  }
}

export async function fetchBranches(path: string): Promise<BranchInfo[]> {
  try {
    const res = await fetch(
      `/api/git/branches?path=${encodeURIComponent(path)}`,
    );
    if (!res.ok) return [];
    return await res.json();
  } catch {
    return [];
  }
}

export async function fetchGroups(): Promise<GroupInfo[]> {
  try {
    const res = await fetch("/api/groups");
    if (!res.ok) return [];
    return await res.json();
  } catch {
    return [];
  }
}

export async function fetchDockerStatus(): Promise<DockerStatusResponse> {
  try {
    const res = await fetch("/api/docker/status");
    if (!res.ok) return { available: false, runtime: null };
    return await res.json();
  } catch {
    return { available: false, runtime: null };
  }
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

// --- Login ---

export async function loginStatus(): Promise<{
  required: boolean;
  authenticated: boolean;
}> {
  try {
    const res = await fetch("/api/login/status");
    if (!res.ok) return { required: false, authenticated: true };
    return await res.json();
  } catch {
    return { required: false, authenticated: true };
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
