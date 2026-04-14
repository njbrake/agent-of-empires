import { useCallback, useEffect, useState } from "react";
import { getSessionFileDiff } from "../lib/api";
import type { RichFileDiffResponse } from "../lib/types";

interface UseFileDiffResult {
  diff: RichFileDiffResponse | null;
  loading: boolean;
  error: string | null;
  refresh: () => void;
}

export function useFileDiff(
  sessionId: string | null,
  filePath: string | null,
  /** Triggers a re-fetch when bumped (e.g. from useDiffFiles.revision). */
  externalRevision?: number,
): UseFileDiffResult {
  const [diff, setDiff] = useState<RichFileDiffResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const fetchDiff = useCallback(async () => {
    if (!sessionId || !filePath) {
      setDiff(null);
      return;
    }
    setLoading(true);
    setError(null);
    const resp = await getSessionFileDiff(sessionId, filePath);
    if (resp) {
      setDiff(resp);
    } else {
      setError("Failed to load diff");
    }
    setLoading(false);
  }, [sessionId, filePath]);

  useEffect(() => {
    void fetchDiff();
  }, [fetchDiff, externalRevision]);

  return { diff, loading, error, refresh: fetchDiff };
}
