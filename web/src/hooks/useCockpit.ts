// Cockpit subscription hook.
//
// Connects to /sessions/{id}/cockpit/ws, receives CockpitBroadcastFrame
// JSON, and reduces them into a CockpitState. On `lagged` notices the
// hook hits the snapshot endpoint to recover any missed frames before
// resuming live broadcast. Errors from sendPrompt / resolveApproval /
// cancelPrompt are surfaced via state.lastError so the user gets a
// dismissible banner instead of a silently-lost action.

import { useCallback, useEffect, useReducer, useRef, useState } from "react";
import {
  applyEvent,
  emptyCockpitState,
  type ApprovalDecision,
  type CockpitFrame,
  type CockpitState,
  type QueuedPrompt,
} from "../lib/cockpitTypes";
import { useCockpitPrefs } from "../lib/cockpitPrefs";
import { getToken } from "../lib/token";

export type Action =
  | { kind: "frame"; frame: CockpitFrame }
  | { kind: "frames"; frames: CockpitFrame[] }
  | { kind: "lagged"; skipped: number }
  | { kind: "user_prompt"; text: string }
  | { kind: "error"; message: string }
  | { kind: "clear_error" }
  | { kind: "lagged_resolved" }
  | { kind: "reset" }
  | { kind: "hydrate"; state: CockpitState }
  | { kind: "enqueue_prompt"; text: string }
  | { kind: "dequeue_prompt"; id: string }
  | { kind: "edit_queued_prompt"; id: string; text: string }
  | { kind: "clear_queue" };

// LRU-capped module cache keyed by cockpit session id. Survives
// component unmount within the same page lifetime so the user can
// navigate between cockpit sessions (or away from the dashboard and
// back) and keep seeing the in-memory transcript while the
// WebSocket reconnects in the background. Lost on full page reload
// by design.
//
// The cap prevents long-running dashboards from accumulating state
// for every cockpit session ever opened (Map.set with an existing
// key is a no-op for ordering, so we delete-then-set to refresh the
// LRU position). `clearCockpitCache(id?)` is exported so the
// session-delete handler and logout flow can drop stale entries
// instead of waiting for them to age out.
const STATE_CACHE_CAP = 32;
const stateCache = new Map<string, CockpitState>();

function cacheGet(sessionId: string): CockpitState | undefined {
  const value = stateCache.get(sessionId);
  if (value !== undefined) {
    // Touch the LRU position by re-inserting at the back of the Map's
    // insertion order.
    stateCache.delete(sessionId);
    stateCache.set(sessionId, value);
  }
  return value;
}

function cacheSet(sessionId: string, value: CockpitState): void {
  stateCache.delete(sessionId);
  stateCache.set(sessionId, value);
  while (stateCache.size > STATE_CACHE_CAP) {
    const oldest = stateCache.keys().next().value;
    if (oldest === undefined) break;
    stateCache.delete(oldest);
  }
}

/** Drop a session's cached state (or the entire cache when called
 *  with no argument). Call from the session-delete handler so the
 *  next session created with the same id doesn't briefly show the
 *  prior transcript on remount. */
export function clearCockpitCache(sessionId?: string): void {
  if (sessionId === undefined) {
    stateCache.clear();
  } else {
    stateCache.delete(sessionId);
  }
}

function initialState(sessionId: string | null): CockpitState {
  if (!sessionId) return emptyCockpitState();
  return cacheGet(sessionId) ?? emptyCockpitState();
}

export function cockpitHookReducer(
  state: CockpitState,
  action: Action,
): CockpitState {
  return reducer(state, action);
}

/** Build the single combined prompt fired when
 *  `cockpit.queue_drain_mode = combined` and the agent transitions to
 *  idle with a non-empty queue. Joins every queued entry's text with a
 *  blank line so the agent sees them as one batch follow-up. Extracted
 *  for testability; consumed by the drain effect below. See #1031. */
export function combineQueuedPrompts(
  queue: ReadonlyArray<QueuedPrompt>,
): string {
  return queue.map((q) => q.text).join("\n\n");
}

function reducer(state: CockpitState, action: Action): CockpitState {
  if (action.kind === "frame") {
    return applyEvent(state, action.frame);
  }
  if (action.kind === "frames") {
    return action.frames.reduce(applyEvent, state);
  }
  if (action.kind === "lagged") {
    return { ...state, lagged: true };
  }
  if (action.kind === "lagged_resolved") {
    return { ...state, lagged: false };
  }
  if (action.kind === "error") {
    return { ...state, lastError: action.message };
  }
  if (action.kind === "clear_error") {
    return { ...state, lastError: null };
  }
  if (action.kind === "hydrate") {
    return action.state;
  }
  if (action.kind === "user_prompt") {
    return {
      ...state,
      activity: state.activity.concat({
        id: `user-${Date.now()}-${state.activity.length}`,
        kind: "user_prompt",
        text: action.text,
        at: new Date().toISOString(),
      }),
      assistantMessage: "",
      // A fresh prompt clears stale errors: the user has indicated
      // they're trying again, so don't keep nagging them.
      startupError: null,
      lastError: null,
      turnActive: true,
    };
  }
  if (action.kind === "enqueue_prompt") {
    const entry: QueuedPrompt = {
      id: `q-${Date.now()}-${state.queuedPrompts.length}`,
      text: action.text,
      queuedAt: new Date().toISOString(),
    };
    return { ...state, queuedPrompts: state.queuedPrompts.concat(entry) };
  }
  if (action.kind === "dequeue_prompt") {
    return {
      ...state,
      queuedPrompts: state.queuedPrompts.filter((q) => q.id !== action.id),
    };
  }
  if (action.kind === "edit_queued_prompt") {
    return {
      ...state,
      queuedPrompts: state.queuedPrompts.map((q) =>
        q.id === action.id ? { ...q, text: action.text } : q,
      ),
    };
  }
  if (action.kind === "clear_queue") {
    return { ...state, queuedPrompts: [] };
  }
  return emptyCockpitState();
}

export type ConnectionStatus =
  | "connecting"
  | "open"
  | "closed"
  | "error";

export function useCockpit(
  sessionId: string | null,
  /** Live cockpit worker lifecycle from `SessionResponse.cockpit_worker_state`.
   *  When not `"running"`, the drain effect parks queued prompts so they
   *  don't dispatch into a worker that isn't online yet. Defaults to
   *  `"running"` so non-cockpit / pre-#1088 call sites keep working. */
  workerState: "absent" | "resuming" | "running" = "running",
) {
  const [state, dispatch] = useReducer(reducer, sessionId, initialState);
  const [status, setStatus] = useState<ConnectionStatus>("connecting");
  // Mirror the worker state into a ref so the drain effect always sees
  // the latest value without re-running on every poll.
  const workerStateRef = useRef(workerState);
  useEffect(() => {
    workerStateRef.current = workerState;
  }, [workerState]);
  // Drain mode is sourced from the daemon's resolved `[cockpit]` config
  // and republished via `CockpitPrefsProvider` (App.tsx). Held in a ref
  // so the drain effect's pop logic always sees the latest value
  // without re-running the effect on every toggle. See #1031.
  const { queueDrainMode } = useCockpitPrefs();
  const drainModeRef = useRef(queueDrainMode);
  useEffect(() => {
    drainModeRef.current = queueDrainMode;
  }, [queueDrainMode]);
  // Mirror status into a ref so sendPrompt's stable callback can short
  // circuit when the WS is closed without re-creating the callback on
  // every status flip (which would invalidate downstream memoised
  // handlers).
  const statusRef = useRef<ConnectionStatus>("connecting");
  useEffect(() => {
    statusRef.current = status;
  }, [status]);

  // Mirror every state change into the module-level cache so that on
  // remount (e.g. user navigates back to the cockpit tab) we hydrate
  // from the last-known state instead of staring at an empty chat
  // until the WS connection completes.
  useEffect(() => {
    if (sessionId) cacheSet(sessionId, state);
  }, [sessionId, state]);
  const wsRef = useRef<WebSocket | null>(null);
  // Track lastSeq in a ref so the snapshot fetcher always sees the
  // latest value without re-running the effect when it changes.
  // The ref is updated inside an effect (not during render) to keep
  // the react-hooks linter happy; fetchReplay only ever runs from
  // an event handler or another effect, so the one-tick lag is fine.
  const lastSeqRef = useRef(0);
  useEffect(() => {
    lastSeqRef.current = state.lastSeq;
  }, [state.lastSeq]);

  const fetchReplay = useCallback(
    async (sid: string) => {
      try {
        const since = lastSeqRef.current;
        const res = await fetch(
          `/api/sessions/${encodeURIComponent(sid)}/cockpit/replay?since=${since}`,
          { credentials: "same-origin" },
        );
        if (!res.ok) return;
        const data = (await res.json()) as {
          frames: CockpitFrame[];
          lost: boolean;
          highest_seq: number;
        };
        // Detect a server-side seq reset: the supervisor's per-session
        // counter has been forgotten (cockpit_disable → cockpit_enable,
        // or session delete+recreate with the same id), so the new
        // conversation is starting fresh from seq=1. Without this reset
        // the client-side dedupe would drop the new events because
        // `frame.seq <= state.lastSeq` is true.
        if (data.highest_seq < since) {
          dispatch({ kind: "reset" });
        }
        if (data.lost) {
          // The buffer doesn't go back far enough; surface this via
          // the existing `lagged` flag (the UI shows a "history
          // truncated" notice) and let the user reload if they want
          // the full transcript back.
          dispatch({ kind: "lagged", skipped: data.highest_seq });
          return;
        }
        if (data.frames.length > 0) {
          dispatch({ kind: "frames", frames: data.frames });
        }
        dispatch({ kind: "lagged_resolved" });
      } catch {
        // Network failure: leave the lagged flag set so the user
        // sees something is wrong rather than silently dropping
        // frames.
      }
    },
    [],
  );

  useEffect(() => {
    if (!sessionId) {
      statusRef.current = "closed";
      setStatus("closed");
      return;
    }
    // Hydrate the reducer from the per-session cache rather than
    // resetting to empty. fetchReplay will then top up anything that
    // happened on the server while this component was unmounted using
    // the cached lastSeq as the `since` cursor.
    dispatch({
      kind: "hydrate",
      state: cacheGet(sessionId) ?? emptyCockpitState(),
    });
    statusRef.current = "connecting";
    setStatus("connecting");
    // On reconnect, replay anything we may have missed.
    fetchReplay(sessionId);

    const token = getToken();
    const protocol = window.location.protocol === "https:" ? "wss" : "ws";
    // Pass `?since=<lastSeq>` so the server's on-connect drain only
    // resends events newer than what we already have. Without this,
    // a long-running session resends its full transcript on every
    // reconnect (page refresh / mobile flap), which can be tens of
    // MB at the retention cap. lastSeqRef stays current via the
    // effect below.
    const since = lastSeqRef.current;
    const url = `${protocol}://${window.location.host}/sessions/${encodeURIComponent(sessionId)}/cockpit/ws?since=${since}`;

    const ws = new WebSocket(url, token ? ["aoe-auth", token] : ["aoe-auth"]);
    wsRef.current = ws;

    // Set the ref synchronously alongside setState so sendPrompt's
    // gate (which reads the ref) doesn't race the next render. Without
    // this, a click landing in the same event-loop tick as `onclose`
    // could see statusRef.current === "open" and dispatch an
    // optimistic prompt against a closed socket.
    ws.onopen = () => {
      statusRef.current = "open";
      setStatus("open");
    };
    ws.onerror = () => {
      statusRef.current = "error";
      setStatus("error");
    };
    ws.onclose = () => {
      statusRef.current = "closed";
      setStatus("closed");
    };
    ws.onmessage = (ev) => {
      try {
        const data = JSON.parse(ev.data) as
          | CockpitFrame
          | { kind: "lagged"; skipped?: number };
        if (
          typeof data === "object" &&
          data !== null &&
          "kind" in data &&
          (data as { kind?: unknown }).kind === "lagged"
        ) {
          const skipped =
            ((data as unknown) as { skipped?: number }).skipped ?? 0;
          dispatch({ kind: "lagged", skipped });
          // Try to recover via the snapshot endpoint.
          fetchReplay(sessionId);
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
  }, [sessionId, fetchReplay]);

  const resolveApproval = useCallback(
    async (nonce: string, decision: ApprovalDecision) => {
      if (!sessionId) return;
      try {
        const res = await fetch(
          `/api/sessions/${encodeURIComponent(sessionId)}/cockpit/approvals/${encodeURIComponent(nonce)}`,
          {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ decision }),
          },
        );
        if (!res.ok) {
          const detail = await safeText(res);
          dispatch({
            kind: "error",
            message: `Could not resolve approval (${res.status}). ${detail}`.trim(),
          });
        } else {
          dispatch({ kind: "clear_error" });
        }
      } catch (e) {
        dispatch({
          kind: "error",
          message: `Network error resolving approval: ${describeError(e)}`,
        });
      }
    },
    [sessionId],
  );

  // Dispatch a prompt immediately, no queueing. Internal helper used by
  // both sendPrompt (when the turn is idle) and the drain effect below
  // (when popping the head of queuedPrompts on Stopped).
  const dispatchPromptNow = useCallback(
    async (text: string) => {
      if (!sessionId) return;
      if (statusRef.current !== "open") {
        dispatch({
          kind: "error",
          message: "Cockpit disconnected; message not sent. Reconnect to retry.",
        });
        return;
      }
      // Optimistically echo the user's message; the agent reply
      // streams back as session/update events on the WS. If the POST
      // fails we'll surface a banner and the user can retry; the
      // optimistic row stays so they see what they tried to send.
      dispatch({ kind: "user_prompt", text });
      try {
        const res = await fetch(
          `/api/sessions/${encodeURIComponent(sessionId)}/cockpit/prompt`,
          {
            method: "POST",
            headers: { "Content-Type": "application/json" },
            body: JSON.stringify({ text }),
          },
        );
        if (!res.ok) {
          const detail = await safeText(res);
          dispatch({
            kind: "error",
            message: `Could not send prompt (${res.status}). ${detail}`.trim(),
          });
        }
      } catch (e) {
        dispatch({
          kind: "error",
          message: `Network error sending prompt: ${describeError(e)}`,
        });
      }
    },
    [sessionId],
  );

  // Public sendPrompt. When the agent is already working we enqueue
  // client-side; the drain effect dispatches the head once the current
  // turn ends. When idle we dispatch immediately. Background: #1031.
  const sendPrompt = useCallback(
    async (text: string) => {
      if (!sessionId) return;
      // The reducer sees the same `turnActive` the UI does. statusRef
      // covers the WS-closed case separately so disconnected users
      // still get the existing error banner even when typing into a
      // running session.
      if (statusRef.current !== "open") {
        dispatch({
          kind: "error",
          message: "Cockpit disconnected; message not sent. Reconnect to retry.",
        });
        return;
      }
      if (state.turnActive) {
        dispatch({ kind: "enqueue_prompt", text });
        return;
      }
      // Worker still resuming (e.g. cold-start parallel reconciler
      // hasn't reached this session yet). Queue locally rather than
      // POSTing into a worker that isn't online; the drain effect
      // fires once `workerState` transitions to `"running"`. See #1088.
      if (workerStateRef.current !== "running") {
        dispatch({ kind: "enqueue_prompt", text });
        return;
      }
      await dispatchPromptNow(text);
    },
    [sessionId, state.turnActive, dispatchPromptNow],
  );

  // Drain effect: when the agent transitions to idle and the queue is
  // non-empty, dispatch follow-ups per `cockpit.queue_drain_mode`:
  //   - combined (default): join every queued entry with `\n\n` and
  //     fire one prompt; the agent's single response covers the batch.
  //   - serial: pop the head only; the next Stopped re-runs this effect
  //     and fires the following entry. One response per entry.
  // Guarded by `drainingRef` so a re-render between the dequeue
  // dispatch and the next state tick doesn't fire the same head twice.
  // Skipped while a worker-stopped / restarting banner is showing; a
  // fresh `AcpSessionAssigned` (which clears both flags) re-runs this
  // effect and drains then. See #1031.
  const drainingRef = useRef(false);
  useEffect(() => {
    if (drainingRef.current) return;
    if (!sessionId) return;
    if (state.turnActive) return;
    if (state.workerStopped || state.workerRestarting) return;
    // Worker still mid-resume from a daemon cold start (or it never
    // came online). Park queued prompts so they don't POST into a
    // worker that's not online yet; the next REST poll flips
    // workerState to "running" and re-runs this effect. See #1088.
    if (workerStateRef.current !== "running") return;
    if (state.queuedPrompts.length === 0) return;
    drainingRef.current = true;
    if (drainModeRef.current === "combined") {
      const combined = combineQueuedPrompts(state.queuedPrompts);
      dispatch({ kind: "clear_queue" });
      void dispatchPromptNow(combined).finally(() => {
        drainingRef.current = false;
      });
    } else {
      const head = state.queuedPrompts[0]!;
      dispatch({ kind: "dequeue_prompt", id: head.id });
      void dispatchPromptNow(head.text).finally(() => {
        drainingRef.current = false;
      });
    }
  }, [
    sessionId,
    workerState,
    state.turnActive,
    state.workerStopped,
    state.workerRestarting,
    state.queuedPrompts,
    dispatchPromptNow,
  ]);

  const removeQueuedPrompt = useCallback((id: string) => {
    dispatch({ kind: "dequeue_prompt", id });
  }, []);

  const editQueuedPrompt = useCallback((id: string, text: string) => {
    dispatch({ kind: "edit_queued_prompt", id, text });
  }, []);

  const clearQueue = useCallback(() => {
    dispatch({ kind: "clear_queue" });
  }, []);

  // Cancels the in-flight agent turn (ACP session/cancel). Must only
  // fire on an explicit user gesture against a dedicated cancel/stop
  // affordance; never bind this to the Escape key. Claude Code CLI
  // hijacks Escape for cancel and accidental presses lose work the
  // user did not mean to abort; the cockpit deliberately keeps Escape
  // for closing local UI surfaces (palette, dialogs, popovers) only.
  // If a future Escape binding is added, route it through
  // useKeyboardShortcuts.onEscape's local-UI dismissal, not here.
  const cancelPrompt = useCallback(async () => {
    if (!sessionId) return;
    try {
      const res = await fetch(
        `/api/sessions/${encodeURIComponent(sessionId)}/cockpit/cancel`,
        { method: "POST" },
      );
      if (!res.ok) {
        const detail = await safeText(res);
        dispatch({
          kind: "error",
          message: `Could not cancel (${res.status}). ${detail}`.trim(),
        });
      }
    } catch (e) {
      dispatch({
        kind: "error",
        message: `Network error cancelling: ${describeError(e)}`,
      });
    }
  }, [sessionId]);

  const dismissError = useCallback(() => {
    dispatch({ kind: "clear_error" });
  }, []);

  return {
    state,
    status,
    resolveApproval,
    sendPrompt,
    cancelPrompt,
    dismissError,
    removeQueuedPrompt,
    editQueuedPrompt,
    clearQueue,
  };
}

async function safeText(res: Response): Promise<string> {
  try {
    return (await res.text()).slice(0, 200);
  } catch {
    return "";
  }
}

function describeError(e: unknown): string {
  if (e instanceof Error) return e.message;
  return String(e);
}
