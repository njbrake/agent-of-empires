import { useCallback, useEffect, useState } from "react";
import type { SessionResponse } from "../lib/types";
import { fetchSessions } from "../lib/api";

const POLL_INTERVAL = 3000;

export function useSessions() {
  const [sessions, setSessions] = useState<SessionResponse[]>([]);
  const [error, setError] = useState(false);

  const refresh = useCallback(async () => {
    const data = await fetchSessions();
    if (data !== null) {
      setSessions(data);
      setError(false);
    } else {
      setError(true);
    }
  }, []);

  useEffect(() => {
    refresh();
    const interval = setInterval(refresh, POLL_INTERVAL);
    return () => clearInterval(interval);
  }, [refresh]);

  return { sessions, error, refresh };
}
