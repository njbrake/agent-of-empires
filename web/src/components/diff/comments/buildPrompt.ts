import type { DiffComment } from "./types";
import { isWellFormed } from "./storage";

interface BuildOpts {
  /** When true, prefix each heading with `[repoName]`. Tests pass this
   *  explicitly; the UI infers it from the session's workspace_repos. */
  isMultiRepo: boolean;
}

const DEFAULT_OUTRO = "Please address these comments.";

const SENTINEL_PREFIX = "<!-- aoe:diff-comments:v1 ";
const SENTINEL_SUFFIX = " -->";

/** Structured payload embedded as an HTML comment at the top of the
 *  prompt body. The cockpit `UserMessage` parses it back out and
 *  renders a structured card; the agent ignores the HTML comment and
 *  reads the assembled markdown below. Base64-encoded JSON avoids any
 *  `-->` collision with snippet/body content. */
export interface DiffCommentsSentinelPayload {
  intro: string;
  outro: string;
  isMultiRepo: boolean;
  comments: DiffComment[];
}

function encodeSentinel(payload: DiffCommentsSentinelPayload): string {
  const json = JSON.stringify(payload);
  const bytes = new TextEncoder().encode(json);
  let bin = "";
  for (let i = 0; i < bytes.length; i++) {
    bin += String.fromCharCode(bytes[i]!);
  }
  return SENTINEL_PREFIX + btoa(bin) + SENTINEL_SUFFIX;
}

/** Returns the structured payload when `text` begins with our sentinel,
 *  or `null` otherwise. Malformed payloads return `null` so the caller
 *  falls back to plain-text rendering. */
export function parseDiffCommentsSentinel(
  text: string,
): DiffCommentsSentinelPayload | null {
  if (!text.startsWith(SENTINEL_PREFIX)) return null;
  const end = text.indexOf(SENTINEL_SUFFIX, SENTINEL_PREFIX.length);
  if (end < 0) return null;
  const b64 = text.slice(SENTINEL_PREFIX.length, end);
  try {
    const bin = atob(b64);
    const bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    const json = new TextDecoder().decode(bytes);
    const parsed = JSON.parse(json) as unknown;
    if (!parsed || typeof parsed !== "object") return null;
    const obj = parsed as Record<string, unknown>;
    if (!Array.isArray(obj.comments)) return null;
    if (typeof obj.intro !== "string") return null;
    if (typeof obj.outro !== "string") return null;
    if (typeof obj.isMultiRepo !== "boolean") return null;
    // Drop malformed inner comments rather than crashing the card.
    // A future producer may add fields we don't recognize yet, but
    // missing required fields means the renderer can't render the
    // entry safely. Keeping only well-formed entries also matches
    // how `loadComments` cleans the localStorage envelope.
    const comments = obj.comments.filter(isWellFormed);
    return {
      intro: obj.intro,
      outro: obj.outro,
      isMultiRepo: obj.isMultiRepo,
      comments,
    };
  } catch {
    return null;
  }
}

/** Strip the sentinel prefix from a prompt body, returning the visible
 *  markdown the agent reads. Used by the cockpit renderer so the
 *  structured card doesn't show the raw HTML comment line. */
export function stripDiffCommentsSentinel(text: string): string {
  if (!text.startsWith(SENTINEL_PREFIX)) return text;
  const end = text.indexOf(SENTINEL_SUFFIX, SENTINEL_PREFIX.length);
  if (end < 0) return text;
  const rest = text.slice(end + SENTINEL_SUFFIX.length);
  return rest.replace(/^\n+/, "");
}

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
  const body = sections.join("\n\n") + "\n";
  if (comments.length === 0) return body;
  const sentinel = encodeSentinel({
    intro: introText,
    outro: outroText,
    isMultiRepo: opts.isMultiRepo,
    comments,
  });
  return `${sentinel}\n${body}`;
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
