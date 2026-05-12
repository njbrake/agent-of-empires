// Recognise MCP-server tool calls (Anthropic's `mcp__<server>__<verb>`
// namespacing) so the cockpit can render them in a dedicated card
// instead of the generic-tool fallback. The adapter (claude-agent-acp)
// ships these calls through as `kind: "other"` with the raw underscore
// name as the title; the frontend reclassifies on title alone.

import type { ToolCall } from "./cockpitTypes";
import { parseJsonObject, pickStr } from "./cockpitArgs";

export interface McpHit {
  isMcp: true;
  server: string;
  verb: string;
}

export interface NotMcp {
  isMcp: false;
}

/** Pull the canonical name out of a tool call: prefer the wire `name`
 *  field, but fall back to `_aoe_title` from args (the cockpit runtime
 *  forwards the ACP title there when it's distinct from the kind). */
function nameOf(tool: ToolCall): string {
  if (tool.name) return tool.name;
  const t = pickStr(parseJsonObject(tool.args_preview), "_aoe_title");
  return t ?? "";
}

/** Split `mcp__<server>__<verb>` into its parts. Server names can
 *  contain underscores (e.g. `claude_ai_HubSpot`,
 *  `db-toolbox-preprod`), but the separators between mcp/server/verb
 *  are always the double-underscore `__`. */
function parseMcpName(
  name: string,
): { server: string; verb: string } | null {
  if (!name.startsWith("mcp__")) return null;
  const parts = name.split("__");
  if (parts.length < 3) return null;
  if (parts[0] !== "mcp") return null;
  const server = parts[1];
  const verb = parts.slice(2).join("__");
  if (!server || !verb) return null;
  return { server, verb };
}

export function classifyMcp(tool: ToolCall): McpHit | NotMcp {
  const hit = parseMcpName(nameOf(tool));
  if (!hit) return { isMcp: false };
  return { isMcp: true, server: hit.server, verb: hit.verb };
}

/** Turn a server slug into a display label. Preserves existing mixed
 *  case (so `claude_ai_HubSpot` keeps the `HubSpot` chunk verbatim)
 *  but title-cases all-lowercase chunks. */
export function humanizeServer(server: string): string {
  return server
    .split(/[-_]/)
    .filter(Boolean)
    .map((c) =>
      /[A-Z]/.test(c) ? c : c.charAt(0).toUpperCase() + c.slice(1),
    )
    .join(" ");
}

/** Turn a snake_case verb into sentence case: `get_sentry_resource` →
 *  `Get sentry resource`. Sentence case (not Title Case) reads better
 *  when later words are often proper nouns the user typed themselves. */
export function humanizeVerb(verb: string): string {
  const spaced = verb.replace(/_/g, " ").trim();
  if (!spaced) return "";
  return spaced.charAt(0).toUpperCase() + spaced.slice(1);
}
