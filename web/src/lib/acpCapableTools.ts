// Tools known to have a published ACP server. Anything not in this
// set falls back to tmux automatically; when the server has
// AOE_EXPERIMENTAL_COCKPIT set, the wizard creates cockpit sessions
// only for tools listed here. Kept in sync with the default registry
// in src/cockpit/agent_registry.rs.
export const ACP_CAPABLE_TOOLS: ReadonlySet<string> = new Set([
  "claude",
  "opencode",
  "gemini",
  "codex",
  "vibe",
  "pi",
]);
