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

const WRAPPER_RE = /^\s*<([a-zA-Z_][a-zA-Z0-9_-]*)>([\s\S]*)<\/\1>\s*$/;

export function parseToolError(text: string | undefined | null): ParsedToolError {
  const raw = (text ?? "").trim();
  if (!raw) return { body: "", tag: null };
  const m = raw.match(WRAPPER_RE);
  if (m && m[1] && m[2] !== undefined) {
    return { body: m[2].trim(), tag: m[1] };
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
