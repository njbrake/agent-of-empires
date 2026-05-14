import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import type {
  DiffComment,
  DiffCommentDraft,
  DiffCommentsStorageV1,
} from "../components/diff/comments/types";
import {
  EMPTY_STORAGE,
  loadComments,
  saveComments,
} from "../components/diff/comments/storage";

export interface UseDiffCommentsResult {
  comments: DiffComment[];
  count: number;
  clearAfterSend: boolean;
  setClearAfterSend(v: boolean): void;
  introDraft: string;
  outroDraft: string;
  setIntroDraft(v: string): void;
  setOutroDraft(v: string): void;
  addComment(draft: DiffCommentDraft): DiffComment;
  updateComment(id: string, body: string): void;
  deleteComment(id: string): void;
  clearComments(): void;
}

/** Session-scoped comments store backed by localStorage. Comments
 *  persist across page reloads inside the same session and are wiped
 *  when the user explicitly clears them or after a successful send
 *  (when `clearAfterSend` is true). State only switches when the
 *  session id changes; if the active session changes we reload from
 *  storage so each session sees its own list. See #928. */
export function useDiffComments(
  sessionId: string | null,
): UseDiffCommentsResult {
  const [state, setState] = useState<DiffCommentsStorageV1>(() =>
    sessionId ? loadComments(sessionId) : { ...EMPTY_STORAGE },
  );

  // Skip the first write-through after a session change; the initial
  // load already mirrors disk and re-saving would no-op anyway. The
  // ref guards against also writing on mount.
  const initialMountRef = useRef(true);
  const lastSessionRef = useRef<string | null>(sessionId);

  useEffect(() => {
    if (lastSessionRef.current !== sessionId) {
      lastSessionRef.current = sessionId;
      initialMountRef.current = true;
      setState(sessionId ? loadComments(sessionId) : { ...EMPTY_STORAGE });
    }
  }, [sessionId]);

  useEffect(() => {
    if (!sessionId) return;
    if (initialMountRef.current) {
      initialMountRef.current = false;
      return;
    }
    saveComments(sessionId, state);
  }, [sessionId, state]);

  const addComment = useCallback(
    (draft: DiffCommentDraft): DiffComment => {
      const created: DiffComment = {
        id: cryptoRandomId(),
        createdAt: new Date().toISOString(),
        ...draft,
      };
      setState((s) => ({ ...s, comments: [...s.comments, created] }));
      return created;
    },
    [],
  );

  const updateComment = useCallback((id: string, body: string) => {
    const ts = new Date().toISOString();
    setState((s) => ({
      ...s,
      comments: s.comments.map((c) =>
        c.id === id ? { ...c, body, updatedAt: ts } : c,
      ),
    }));
  }, []);

  const deleteComment = useCallback((id: string) => {
    setState((s) => ({
      ...s,
      comments: s.comments.filter((c) => c.id !== id),
    }));
  }, []);

  const clearComments = useCallback(() => {
    setState((s) => ({ ...s, comments: [] }));
  }, []);

  const setClearAfterSend = useCallback((v: boolean) => {
    setState((s) => ({ ...s, clearAfterSend: v }));
  }, []);

  const setIntroDraft = useCallback((v: string) => {
    setState((s) => ({ ...s, introDraft: v }));
  }, []);

  const setOutroDraft = useCallback((v: string) => {
    setState((s) => ({ ...s, outroDraft: v }));
  }, []);

  return useMemo(
    () => ({
      comments: state.comments,
      count: state.comments.length,
      clearAfterSend: state.clearAfterSend,
      setClearAfterSend,
      introDraft: state.introDraft,
      outroDraft: state.outroDraft,
      setIntroDraft,
      setOutroDraft,
      addComment,
      updateComment,
      deleteComment,
      clearComments,
    }),
    [
      state,
      addComment,
      updateComment,
      deleteComment,
      clearComments,
      setClearAfterSend,
      setIntroDraft,
      setOutroDraft,
    ],
  );
}

function cryptoRandomId(): string {
  const c = globalThis.crypto;
  if (c && typeof c.randomUUID === "function") return c.randomUUID();
  // Fallback for environments without crypto.randomUUID (older Safari, jsdom).
  return `dc_${Date.now().toString(36)}_${Math.random()
    .toString(36)
    .slice(2, 10)}`;
}
