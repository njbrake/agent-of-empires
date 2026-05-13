// Helper for extracting the adapter's failure reason out of a
// tool_error completion row. claude-agent-acp wraps Claude's tool
// errors in `<tool_use_error>…</tool_use_error>` markers; older adapters
// emit the inner string directly. Treat both shapes uniformly: peel the
// tag when present and surface its name as a small label outside the
// error body so users still see where the message came from. See
// issue #1090.

export interface ParsedToolError {
  /** The unwrapped error message (whitespace-trimmed). Empty when the
   *  adapter sent `is_error: true` with no body. */
  body: string;
  /** Tag name when the original payload was wrapped in
   *  `<tag>...</tag>`. The cockpit renders this as a small label
   *  outside the error body so the source is clear without polluting
   *  the message itself. Null when the payload was a bare string. */
  tag: string | null;
}

// Non-anchored: claude-agent-acp sometimes joins multiple
// `ContentBlock::Text` entries with `\n` before the wrapper, or appends
// trailing prose, so an `^…$` regex misses the common shape and the
// wrapper leaks into the rendered body. Match the FIRST wrapper anywhere
// in the text and treat its inner contents as the body; any prose
// outside the wrapper is appended below the wrapper body (rare in
// practice, but lossless when it does happen). Non-greedy `*?` so we
// stop at the first matching close tag, not the last one.
const WRAPPER_RE = /<([a-zA-Z_][a-zA-Z0-9_-]*)>([\s\S]*?)<\/\1>/;

export function parseToolError(text: string | undefined | null): ParsedToolError {
  const raw = (text ?? "").trim();
  if (!raw) return { body: "", tag: null };
  const m = WRAPPER_RE.exec(raw);
  if (m && m[1] && m[2] !== undefined) {
    const inner = m[2].trim();
    const before = raw.slice(0, m.index).trim();
    const after = raw.slice(m.index + m[0].length).trim();
    const extra = [before, after].filter((s) => s.length > 0).join("\n");
    const body = extra ? `${inner}\n\n${extra}` : inner;
    return { body, tag: m[1] };
  }
  return { body: raw, tag: null };
}

/** Human-readable rendering of a wrapper tag, mapping the few common
 *  shapes claude-agent-acp emits to friendlier labels. Unknown tags
 *  pass through verbatim. */
export function describeToolErrorTag(tag: string | null): string | null {
  if (!tag) return null;
  switch (tag.toLowerCase()) {
    case "tool_use_error":
      return "agent-reported error";
    case "tool_result_error":
      return "agent-reported error";
    case "error":
      return "error";
    default:
      return tag;
  }
}
