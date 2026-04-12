import type { SessionResponse, DiffResponse } from "./types";

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
