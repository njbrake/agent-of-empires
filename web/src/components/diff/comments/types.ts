/** Which side of the diff a comment is anchored to. */
export type DiffSide = "old" | "new";

/** A user-authored review comment on a range of lines in a diff. The
 *  range is anchored to one side; snippet extraction filters out rows
 *  belonging to the opposite side (a new-side range skips deleted rows,
 *  and vice versa). */
export interface DiffComment {
  id: string;
  /** Workspace member name. Undefined for single-repo sessions. */
  repoName?: string;
  filePath: string;
  side: DiffSide;
  /** Inclusive 1-based line number on the chosen side. */
  startLine: number;
  /** Inclusive. >= startLine. */
  endLine: number;
  /** Markdown body. */
  body: string;
  /** Code captured at authoring time so the assembled prompt remains
   *  meaningful even after the diff changes underneath. The agent gets
   *  to see what the human reviewer actually read. */
  capturedSnippet: string;
  /** Code-fence language inferred at authoring time. */
  language?: string;
  /** ISO 8601 timestamp. */
  createdAt: string;
  /** Set when the body is edited. */
  updatedAt?: string;
}

/** Input for creating a comment. The hook fills in id/createdAt. */
export type DiffCommentDraft = Omit<DiffComment, "id" | "createdAt">;

/** Versioned localStorage envelope. Future schema changes bump the
 *  version and trigger a clean drop in the loader (no migration
 *  helpers needed yet, schema is fresh). */
export interface DiffCommentsStorageV1 {
  version: 1;
  comments: DiffComment[];
  /** Whether the send dialog clears comments on success by default. */
  clearAfterSend: boolean;
  /** Persisted draft text for the intro / outro fields so the user
   *  doesn't lose framing text when re-opening the dialog. */
  introDraft: string;
  outroDraft: string;
}

/** Anchor status of a comment against the currently loaded diff. */
export type AnchorStatus = "active" | "stale";

/** A comment paired with its current-diff anchor result. */
export interface AnchoredComment {
  comment: DiffComment;
  status: AnchorStatus;
  /** True when the current snippet for the comment's range no longer
   *  matches `comment.capturedSnippet`. Not surfaced in v1 UI but
   *  computed in `anchor.ts` so a future "[changed]" chip can be added
   *  without refactoring the matching logic. */
  contentChanged: boolean;
  /** Index of the matching hunk when status === "active". */
  hunkIndex?: number;
  /** Row index inside the hunk where the comment card should be
   *  rendered (last line of the range). */
  endRowIndex?: number;
}
