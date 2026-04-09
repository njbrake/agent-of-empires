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
