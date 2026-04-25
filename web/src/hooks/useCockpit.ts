// Cockpit subscription hook.
//
// Connects to /sessions/{id}/cockpit/ws, receives CockpitBroadcastFrame
// JSON, and reduces them into a CockpitState. Exposes a
// resolveApproval helper that POSTs the user's decision back to the
// server. The REST endpoint that resolveApproval targets is wired up
// when the worker supervisor lands; today the call is made
// optimistically so the UI can be developed against the WS surface.

import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import {
  applyEvent,
  emptyCockpitState,
  type ApprovalDecision,
  type CockpitFrame,
  type CockpitState,
  type LaggedFrame,
} from "../lib/cockpitTypes";
import { getToken } from "../lib/token";

type Action =
  | { kind: "frame"; frame: CockpitFrame }
  | { kind: "lagged"; skipped: number }
  | { kind: "reset" };

function reducer(state: CockpitState, action: Action): CockpitState {
  if (action.kind === "frame") {
    return applyEvent(state, action.frame);
  }
  if (action.kind === "lagged") {
    return { ...state, lagged: true };
  }
  return emptyCockpitState();
}

export type ConnectionStatus =
  | "connecting"
  | "open"
  | "closed"
  | "error";

export function useCockpit(sessionId: string | null) {
  const [state, dispatch] = useReducer(reducer, emptyCockpitState());
  const [status, setStatus] = useState<ConnectionStatus>("connecting");
  const wsRef = useRef<WebSocket | null>(null);

  useEffect(() => {
    if (!sessionId) {
      setStatus("closed");
      return;
    }
    dispatch({ kind: "reset" });
    setStatus("connecting");

    const token = getToken();
    const protocol = window.location.protocol === "https:" ? "wss" : "ws";
    const url = `${protocol}://${window.location.host}/sessions/${encodeURIComponent(sessionId)}/cockpit/ws`;

    // The auth middleware accepts the token via the
    // sec-websocket-protocol subprotocol header so the WS handshake
    // can be authenticated without a custom Sec-* extension.
    const ws = new WebSocket(url, token ? ["aoe-auth", token] : ["aoe-auth"]);
    wsRef.current = ws;

    ws.onopen = () => setStatus("open");
    ws.onerror = () => setStatus("error");
    ws.onclose = () => setStatus("closed");
    ws.onmessage = (ev) => {
      try {
        const data = JSON.parse(ev.data) as CockpitFrame | LaggedFrame;
        if (
          typeof data === "object" &&
          data !== null &&
          "kind" in data &&
          (data as { kind?: unknown }).kind === "lagged"
        ) {
          const skipped =
            ((data as unknown) as { skipped?: number }).skipped ?? 0;
          dispatch({ kind: "lagged", skipped });
          return;
        }
        if (
          typeof data === "object" &&
          data !== null &&
          "session_id" in data &&
          "event" in data
        ) {
          dispatch({ kind: "frame", frame: data as CockpitFrame });
        }
      } catch {
        // Ignore malformed frames; the server should never send them.
      }
    };

    return () => {
      try {
        ws.close();
      } catch {
        // ignore
      }
      wsRef.current = null;
    };
  }, [sessionId]);

  const resolveApproval = useCallback(
    async (nonce: string, decision: ApprovalDecision) => {
      if (!sessionId) return;
      // POST the resolution. The endpoint will be wired up alongside
      // the worker supervisor; today we issue the request so the UI
      // can be developed against the wire.
      try {
        await fetch(
          `/api/sessions/${encodeURIComponent(sessionId)}/cockpit/approvals/${encodeURIComponent(nonce)}`,
          {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ decision }),
          },
        );
      } catch {
        // Network errors are surfaced via the connection status; the
        // UI stays optimistic until the server confirms by removing
        // the approval from `pendingApprovals` via a broadcast frame.
      }
    },
    [sessionId],
  );

  const sendPrompt = useCallback(
    async (text: string) => {
      if (!sessionId) return;
      await fetch(
        `/api/sessions/${encodeURIComponent(sessionId)}/cockpit/prompt`,
        {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ text }),
        },
      );
    },
    [sessionId],
  );

  return { state, status, resolveApproval, sendPrompt };
}
