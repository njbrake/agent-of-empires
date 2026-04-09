import type {
  SessionResponse,
  AgentInfo,
  GroupInfo,
  DiffResponse,
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

export async function getSession(
  id: string,
): Promise<SessionResponse | null> {
  try {
    const res = await fetch(`/api/sessions/${id}`);
    if (!res.ok) return null;
    return await res.json();
  } catch {
    return null;
  }
}

export async function createSession(data: {
  title?: string;
  path: string;
  tool: string;
  group?: string;
  yolo_mode?: boolean;
  worktree_branch?: string;
  create_new_branch?: boolean;
  sandbox?: boolean;
  extra_args?: string;
}): Promise<SessionResponse | null> {
  try {
    const res = await fetch("/api/sessions", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(data),
    });
    if (!res.ok) {
      const err = await res.json().catch(() => null);
      throw new Error(err?.message || `HTTP ${res.status}`);
    }
    return await res.json();
  } catch {
    return null;
  }
}

export async function stopSession(id: string): Promise<boolean> {
  try {
    const res = await fetch(`/api/sessions/${id}/stop`, { method: "POST" });
    return res.ok;
  } catch {
    return false;
  }
}

export async function restartSession(id: string): Promise<boolean> {
  try {
    const res = await fetch(`/api/sessions/${id}/restart`, { method: "POST" });
    return res.ok;
  } catch {
    return false;
  }
}

export async function deleteSession(id: string): Promise<boolean> {
  try {
    const res = await fetch(`/api/sessions/${id}`, { method: "DELETE" });
    return res.ok;
  } catch {
    return false;
  }
}

export async function updateSession(
  id: string,
  updates: { title?: string; group_path?: string },
): Promise<SessionResponse | null> {
  try {
    const res = await fetch(`/api/sessions/${id}`, {
      method: "PATCH",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify(updates),
    });
    if (!res.ok) return null;
    return await res.json();
  } catch {
    return null;
  }
}

export async function getSessionDiff(
  id: string,
): Promise<DiffResponse | null> {
  try {
    const res = await fetch(`/api/sessions/${id}/diff`);
    if (!res.ok) return null;
    return await res.json();
  } catch {
    return null;
  }
}

// --- Agents ---

export async function fetchAgents(): Promise<AgentInfo[]> {
  try {
    const res = await fetch("/api/agents");
    if (!res.ok) return [];
    return await res.json();
  } catch {
    return [];
  }
}

// --- Groups ---

export async function fetchGroups(): Promise<GroupInfo[]> {
  try {
    const res = await fetch("/api/groups");
    if (!res.ok) return [];
    return await res.json();
  } catch {
    return [];
  }
}

// --- Profiles ---

export async function fetchProfiles(): Promise<string[]> {
  try {
    const res = await fetch("/api/profiles");
    if (!res.ok) return [];
    return await res.json();
  } catch {
    return [];
  }
}

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
    const res = await fetch(`/api/profiles/${name}`, { method: "DELETE" });
    return res.ok;
  } catch {
    return false;
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

// --- Worktrees ---

export interface WorktreeInfo {
  session_id: string;
  session_title: string;
  branch: string;
  main_repo_path: string;
  managed_by_aoe: boolean;
}

export async function fetchWorktrees(): Promise<WorktreeInfo[]> {
  try {
    const res = await fetch("/api/worktrees");
    if (!res.ok) return [];
    return await res.json();
  } catch {
    return [];
  }
}
