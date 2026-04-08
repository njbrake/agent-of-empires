import type { SessionResponse } from "./types";

export async function fetchSessions(): Promise<SessionResponse[] | null> {
  try {
    const res = await fetch("/api/sessions");
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
