import type {
  SessionResponse,
  RichDiffFilesResponse,
  RichFileDiffResponse,
  AgentInfo,
  ProfileInfo,
  DirEntry,
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

export async function getSettings(): Promise<Record<string, unknown> | null> {
  try {
    const res = await fetch("/api/settings");
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

export async function browseFilesystem(path: string): Promise<DirEntry[]> {
  try {
    const res = await fetch(
      `/api/filesystem/browse?path=${encodeURIComponent(path)}`,
    );
    if (!res.ok) return [];
    return await res.json();
  } catch {
    return [];
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
