import { safeGetItem, safeSetItem } from "../../../lib/safeStorage";
import type { DiffComment, DiffCommentsStorageV1 } from "./types";

const KEY_PREFIX = "aoe:diff-comments:v1:";

export function storageKey(sessionId: string): string {
  return `${KEY_PREFIX}${sessionId}`;
}

export const EMPTY_STORAGE: DiffCommentsStorageV1 = {
  version: 1,
  comments: [],
  clearAfterSend: true,
  introDraft: "",
  outroDraft: "",
};

/** Load comments for a session. Tolerates corruption, missing keys,
 *  and version mismatches by falling back to an empty state. localStorage
 *  is browser-local; data corruption shouldn't kill the feature. */
export function loadComments(sessionId: string): DiffCommentsStorageV1 {
  const raw = safeGetItem(storageKey(sessionId));
  if (!raw) return { ...EMPTY_STORAGE };
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (
      !parsed ||
      typeof parsed !== "object" ||
      (parsed as { version?: number }).version !== 1 ||
      !Array.isArray((parsed as { comments?: unknown }).comments)
    ) {
      return { ...EMPTY_STORAGE };
    }
    const v = parsed as DiffCommentsStorageV1;
    return {
      version: 1,
      comments: v.comments.filter(isWellFormed),
      clearAfterSend: typeof v.clearAfterSend === "boolean" ? v.clearAfterSend : true,
      introDraft: typeof v.introDraft === "string" ? v.introDraft : "",
      outroDraft: typeof v.outroDraft === "string" ? v.outroDraft : "",
    };
  } catch {
    return { ...EMPTY_STORAGE };
  }
}

export function saveComments(
  sessionId: string,
  state: DiffCommentsStorageV1,
): void {
  safeSetItem(storageKey(sessionId), JSON.stringify(state));
}

export function isWellFormed(c: unknown): c is DiffComment {
  if (!c || typeof c !== "object") return false;
  const o = c as Record<string, unknown>;
  return (
    typeof o.id === "string" &&
    typeof o.filePath === "string" &&
    (o.side === "old" || o.side === "new") &&
    typeof o.startLine === "number" &&
    typeof o.endLine === "number" &&
    typeof o.body === "string" &&
    typeof o.capturedSnippet === "string" &&
    typeof o.createdAt === "string"
  );
}
