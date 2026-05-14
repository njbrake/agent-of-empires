import type { DiffComment } from "./types";

interface BuildOpts {
  /** When true, prefix each heading with `[repoName]`. Tests pass this
   *  explicitly; the UI infers it from the session's workspace_repos. */
  isMultiRepo: boolean;
}

const DEFAULT_OUTRO = "Please address these comments.";

/** Pure assembly of the comments section. Stable sort, single-line
 *  vs range wording, multi-repo prefix, and a dynamically-sized code
 *  fence per snippet. */
export function buildCommentsMarkdown(
  comments: DiffComment[],
  opts: BuildOpts,
): string {
  if (comments.length === 0) return "";
  const sorted = [...comments].sort(compareComments);
  const sections = sorted.map((c) => renderComment(c, opts.isMultiRepo));
  return sections.join("\n\n---\n\n");
}

/** Assemble the full prompt body: intro + comments preview + outro.
 *  `outro` falls back to a default if blank so the agent sees an
 *  actionable nudge. */
export function buildFullPrompt(
  comments: DiffComment[],
  intro: string,
  outro: string,
  opts: BuildOpts,
): string {
  const introText = intro.trim();
  const outroText = (outro.trim() || DEFAULT_OUTRO).trim();
  const sections: string[] = [];
  if (introText) sections.push(introText);
  const commentsBlock = buildCommentsMarkdown(comments, opts);
  if (commentsBlock) {
    sections.push("## Diff comments");
    sections.push(commentsBlock);
  }
  sections.push(outroText);
  return sections.join("\n\n") + "\n";
}

function renderComment(c: DiffComment, isMultiRepo: boolean): string {
  const repo = isMultiRepo && c.repoName ? `[${c.repoName}] ` : "";
  const range =
    c.startLine === c.endLine
      ? `line ${c.startLine}`
      : `lines ${c.startLine}-${c.endLine}`;
  const heading = `### ${repo}\`${c.filePath}\` ${range} (${c.side})`;
  const fence = makeFence(c.capturedSnippet);
  const lang = c.language ?? "";
  const codeBlock = `${fence}${lang}\n${c.capturedSnippet}\n${fence}`;
  const body = c.body.trim();
  return `${heading}\n\n${codeBlock}\n\n${body}`;
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

/** Pick a code-fence length longer than the longest backtick run in the
 *  snippet so a markdown-content snippet doesn't terminate the fence
 *  early. Minimum 3 backticks. */
function makeFence(snippet: string): string {
  const re = /`+/g;
  let longest = 0;
  let match: RegExpExecArray | null;
  while ((match = re.exec(snippet)) !== null) {
    if (match[0].length > longest) longest = match[0].length;
  }
  return "`".repeat(Math.max(3, longest + 1));
}
