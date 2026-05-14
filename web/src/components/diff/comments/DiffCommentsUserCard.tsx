import { CommentMarkdown } from "./CommentMarkdown";
import type { DiffCommentsSentinelPayload } from "./buildPrompt";
import type { DiffComment } from "./types";

interface Props {
  payload: DiffCommentsSentinelPayload;
}

/** Rich rendering of a diff-comments prompt in the cockpit user-message
 *  slot. Built from the sentinel payload that `buildFullPrompt`
 *  prepends to the prompt body. Falls back to the raw text rendering
 *  upstream when the sentinel is missing or malformed. */
export function DiffCommentsUserCard({ payload }: Props) {
  const { intro, outro, isMultiRepo, comments } = payload;
  const sorted = [...comments].sort(compareComments);
  return (
    <div className="w-full max-w-3xl rounded-2xl rounded-br-sm border border-surface-700 bg-surface-800/70 px-4 py-3 text-sm">
      <div className="mb-2 flex items-center gap-2 text-[11px] uppercase tracking-wider text-text-dim">
        <span className="rounded bg-brand-600/15 px-1.5 py-0.5 font-mono text-brand-300">
          diff review
        </span>
        <span>
          {comments.length} comment{comments.length === 1 ? "" : "s"}
        </span>
      </div>
      {intro && (
        <div className="mb-3 border-l-2 border-surface-700 pl-3 text-text-secondary">
          <CommentMarkdown text={intro} />
        </div>
      )}
      <ul className="flex flex-col gap-3">
        {sorted.map((c) => (
          <li
            key={c.id}
            className="rounded-lg border border-surface-700/60 bg-surface-900/60"
          >
            <CommentHeader comment={c} isMultiRepo={isMultiRepo} />
            <pre className="overflow-x-auto border-b border-surface-700/40 bg-surface-950 px-3 py-2 font-mono text-[12px] text-text-primary">
              {c.capturedSnippet}
            </pre>
            <div className="px-3 py-2 text-text-primary">
              <CommentMarkdown text={c.body} />
            </div>
          </li>
        ))}
      </ul>
      {outro && (
        <div className="mt-3 border-l-2 border-surface-700 pl-3 text-text-secondary">
          <CommentMarkdown text={outro} />
        </div>
      )}
    </div>
  );
}

function CommentHeader({
  comment,
  isMultiRepo,
}: {
  comment: DiffComment;
  isMultiRepo: boolean;
}) {
  const range =
    comment.startLine === comment.endLine
      ? `line ${comment.startLine}`
      : `lines ${comment.startLine}-${comment.endLine}`;
  return (
    <div className="flex flex-wrap items-center gap-1.5 border-b border-surface-700/40 px-3 py-1.5 text-[11px] font-mono text-text-dim">
      {isMultiRepo && comment.repoName && (
        <span className="rounded bg-surface-800 px-1.5 py-0.5 text-text-secondary">
          {comment.repoName}
        </span>
      )}
      <span className="text-text-secondary">{comment.filePath}</span>
      <span>·</span>
      <span>{range}</span>
      <span>·</span>
      <span>{comment.side}</span>
    </div>
  );
}

function compareComments(a: DiffComment, b: DiffComment): number {
  const ra = a.repoName ?? "";
  const rb = b.repoName ?? "";
  if (ra !== rb) return ra.localeCompare(rb);
  if (a.filePath !== b.filePath) return a.filePath.localeCompare(b.filePath);
  if (a.startLine !== b.startLine) return a.startLine - b.startLine;
  if (a.side !== b.side) return a.side === "old" ? -1 : 1;
  return a.createdAt.localeCompare(b.createdAt);
}
