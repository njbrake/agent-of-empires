// Shared helpers for parsing the JSON-shaped `args_preview` field on
// cockpit tool calls (and other JSON blobs the cockpit UI displays).
// The Rust side ships a string preview that's USUALLY a JSON object
// but sometimes truncated or non-object — these helpers handle both.

/** Parse a JSON object payload. Returns null when the input doesn't
 *  parse, isn't an object, or is an array (callers want
 *  field-by-field access, not array indexing). */
export function parseJsonObject(s: string): Record<string, unknown> | null {
  try {
    const v = JSON.parse(s);
    return v && typeof v === "object" && !Array.isArray(v)
      ? (v as Record<string, unknown>)
      : null;
  } catch {
    return null;
  }
}

/** Return the first key whose value is a string. Used to surface a
 *  tool's primary argument (path, command, query) when the agent
 *  uses different field names across versions. */
export function pickStr(
  o: Record<string, unknown> | null,
  ...keys: string[]
): string | null {
  if (!o) return null;
  for (const k of keys) {
    const v = o[k];
    if (typeof v === "string") return v;
  }
  return null;
}

/** Return the first non-empty string in the chain, or null. */
export function pickFirst(
  ...candidates: Array<string | null | undefined>
): string | null {
  for (const c of candidates) {
    if (typeof c === "string" && c.trim() !== "") return c;
  }
  return null;
}
