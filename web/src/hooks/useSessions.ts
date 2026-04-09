import { useCallback, useEffect, useRef, useState } from "react";
import type { SessionResponse } from "../lib/types";
import { fetchSessions } from "../lib/api";

const POLL_INTERVAL = 3000;

export function useSessions() {
  const [sessions, setSessions] = useState<SessionResponse[]>([]);
  const [error, setError] = useState(false);
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);

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
    // Initial fetch
    void fetchSessions().then((data) => {
      if (data !== null) {
        setSessions(data);
        setError(false);
      } else {
        setError(true);
      }
    });

    // Polling
    intervalRef.current = setInterval(() => {
      void fetchSessions().then((data) => {
        if (data !== null) {
          setSessions(data);
          setError(false);
        } else {
          setError(true);
        }
      });
    }, POLL_INTERVAL);

    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, []);

  return { sessions, error, refresh };
}
