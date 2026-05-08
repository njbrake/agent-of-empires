// Tools known to have a published ACP server. Anything not in this
// set falls back to tmux automatically; when the server has
// AOE_EXPERIMENTAL_COCKPIT set, the wizard creates cockpit sessions
// only for tools listed here.
//
// SOURCE OF TRUTH: src/cockpit/agent_registry.rs. If you add a new
// ACP adapter to that registry, also add it here; otherwise the web
// wizard will silently fall back to tmux for it. (Long-term we should
// expose this list via /api/about and drop the JS-side copy.)
export const ACP_CAPABLE_TOOLS: ReadonlySet<string> = new Set([
  "claude",
  "opencode",
  "gemini",
  "codex",
  "vibe",
  "pi",
]);
