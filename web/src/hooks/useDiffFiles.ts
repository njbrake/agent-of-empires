import { useCallback, useEffect, useRef, useState } from "react";
import { getSessionDiffFiles } from "../lib/api";
import type { RichDiffFile } from "../lib/types";

const POLL_INTERVAL = 10_000;

interface UseDiffFilesResult {
  files: RichDiffFile[];
  baseBranch: string;
  warning: string | null;
  loading: boolean;
  /** Monotonically increasing revision counter; bumps when the file list changes. */
  revision: number;
  refresh: () => void;
}

export function useDiffFiles(
  sessionId: string | null,
  enabled: boolean,
): UseDiffFilesResult {
  const [files, setFiles] = useState<RichDiffFile[]>([]);
  const [baseBranch, setBaseBranch] = useState("main");
  const [warning, setWarning] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [revision, setRevision] = useState(0);
  const lastFingerprintRef = useRef("");
  const intervalRef = useRef<ReturnType<typeof setInterval> | null>(null);
  const requestIdRef = useRef(0);

  const fetchFiles = useCallback(async () => {
    if (!sessionId) return;
    const reqId = ++requestIdRef.current;
    const capturedSessionId = sessionId;
    const resp = await getSessionDiffFiles(capturedSessionId);
    // Drop stale responses: another fetch started, or session changed mid-flight
    if (reqId !== requestIdRef.current || capturedSessionId !== sessionId) return;
    if (resp) {
      const fingerprint = JSON.stringify(resp.files);
      if (fingerprint !== lastFingerprintRef.current) {
        lastFingerprintRef.current = fingerprint;
        setFiles(resp.files);
        setBaseBranch(resp.base_branch);
        setWarning(resp.warning ?? null);
        setRevision((r) => r + 1);
      }
    }
    setLoading(false);
  }, [sessionId]);

  // Fetch on session change; invalidate any in-flight requests.
  useEffect(() => {
    requestIdRef.current += 1;
    if (!sessionId) {
      setFiles([]);
      setLoading(false);
      lastFingerprintRef.current = "";
      setRevision(0);
      return;
    }
    setLoading(true);
    lastFingerprintRef.current = "";
    void fetchFiles();
  }, [sessionId, fetchFiles]);

  // Poll when enabled
  useEffect(() => {
    if (intervalRef.current) {
      clearInterval(intervalRef.current);
      intervalRef.current = null;
    }
    if (enabled && sessionId) {
      intervalRef.current = setInterval(() => void fetchFiles(), POLL_INTERVAL);
    }
    return () => {
      if (intervalRef.current) clearInterval(intervalRef.current);
    };
  }, [enabled, sessionId, fetchFiles]);

  return { files, baseBranch, warning, loading, revision, refresh: fetchFiles };
}
