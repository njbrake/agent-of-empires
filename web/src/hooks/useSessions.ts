import { useCallback, useEffect, useRef, useState } from "react";
import type { SessionResponse } from "../lib/types";
import { fetchSessions } from "../lib/api";
import { setServerDown } from "../lib/connectionState";

const POLL_INTERVAL = 3000;

export function useSessions() {
  const [sessions, setSessions] = useState<SessionResponse[]>([]);
  const [error, setError] = useState(false);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

  const injectSession = useCallback((session: SessionResponse) => {
    setSessions((prev) => {
      if (prev.some((s) => s.id === session.id)) return prev;
      return [session, ...prev];
    });
  }, []);

  const applyResult = useCallback((data: SessionResponse[] | null) => {
    if (data !== null) {
      setSessions(data);
      setError(false);
      setServerDown(false);
    } else {
      setError(true);
      setServerDown(true);
    }
  }, []);

  const refresh = useCallback(async () => {
    applyResult(await fetchSessions());
  }, [applyResult]);

  useEffect(() => {
    void fetchSessions().then(applyResult);
    intervalRef.current = setInterval(
      () => void fetchSessions().then(applyResult),
      POLL_INTERVAL,
    );
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, [applyResult]);

  const setSessionStatus = useCallback((id: string, status: SessionResponse["status"]) => {
    setSessions((prev) => prev.map((s) => s.id === id ? { ...s, status } : s));
  }, []);

  return { sessions, error, refresh, injectSession, setSessionStatus };
}
