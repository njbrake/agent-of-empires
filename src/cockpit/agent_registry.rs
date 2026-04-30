//! Named agent registry: maps an agent name (e.g. `claude-code`,
//! `aoe-agent`, `gemini`) to a spawn command + args. Users add agents via
//! the settings TUI; this module is the in-memory model.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSpec {
    /// Executable to run, e.g. `npx` or `/usr/local/bin/aoe-agent`.
    pub command: String,
    pub args: Vec<String>,
    /// Human-readable description shown in the settings TUI and
    /// `aoe cockpit agents`.
    pub description: String,
    /// Optional: which env vars from aoe to forward to this agent. If
    /// `None`, only `PATH`, `HOME`, `LANG`, `TERM`, and provider auth env
    /// (e.g. `ANTHROPIC_API_KEY`) are forwarded.
    pub env_allowlist: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AgentRegistry {
    pub agents: HashMap<String, AgentSpec>,
}

impl AgentRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a registry seeded with one entry per aoe tool that has
    /// a published ACP server, plus our own `aoe-agent` as a generic
    /// multi-provider fallback. Each entry is keyed on the same name
    /// the tmux substrate uses (claude / opencode / gemini / codex /
    /// vibe / pi) so the spawn path can map `instance.tool` directly
    /// to a registry key.
    ///
    /// Sources verified against
    /// https://agentclientprotocol.com/get-started/agents.md
    /// (Jan 2026):
    ///
    ///   claude   → claude-agent-acp     (Zed adapter for Claude SDK)
    ///   opencode → `opencode acp`       (native, SST)
    ///   gemini   → `gemini --acp`       (native, Google)
    ///   codex    → codex-acp            (Zed adapter, OpenAI Codex CLI)
    ///   vibe     → vibe-acp             (native, Mistral)
    ///   pi       → pi-acp               (adapter, Hermes coding agent)
    ///
    /// We deliberately don't use `npx -y` for these. First-run
    /// downloads can hang for tens of seconds with no output, which
    /// used to leave the cockpit worker silently wedged before the
    /// handshake. `aoe cockpit doctor --fix` can install missing
    /// binaries on demand.
    pub fn with_defaults() -> Self {
        let mut reg = Self::new();

        reg.agents.insert(
            "claude".into(),
            AgentSpec {
                command: "claude-agent-acp".into(),
                args: vec![],
                description:
                    "Anthropic Claude via the official ACP adapter (npm i -g @agentclientprotocol/claude-agent-acp)"
                        .into(),
                env_allowlist: None,
            },
        );
        // Legacy alias used by older session records before the
        // tool-keyed naming. Kept so persisted sessions with
        // cockpit_agent="claude-code" still resolve.
        reg.agents.insert(
            "claude-code".into(),
            AgentSpec {
                command: "claude-agent-acp".into(),
                args: vec![],
                description: "Alias for `claude` (legacy name)".into(),
                env_allowlist: None,
            },
        );
        reg.agents.insert(
            "opencode".into(),
            AgentSpec {
                command: "opencode".into(),
                args: vec!["acp".into()],
                description: "OpenCode (SST) — native ACP via `opencode acp`".into(),
                env_allowlist: None,
            },
        );
        reg.agents.insert(
            "gemini".into(),
            AgentSpec {
                command: "gemini".into(),
                args: vec!["--acp".into()],
                description: "Google Gemini CLI — native ACP via `gemini --acp`".into(),
                env_allowlist: None,
            },
        );
        reg.agents.insert(
            "codex".into(),
            AgentSpec {
                command: "codex-acp".into(),
                args: vec![],
                description:
                    "OpenAI Codex CLI via Zed adapter (npm i -g @zed-industries/codex-acp)".into(),
                env_allowlist: None,
            },
        );
        reg.agents.insert(
            "vibe".into(),
            AgentSpec {
                command: "vibe-acp".into(),
                args: vec![],
                description: "Mistral Vibe — native ACP via the bundled `vibe-acp` binary".into(),
                env_allowlist: None,
            },
        );
        reg.agents.insert(
            "pi".into(),
            AgentSpec {
                command: "pi-acp".into(),
                args: vec![],
                description: "Hermes coding agent (`pi`) via the pi-acp adapter (npm i -g pi-acp)"
                    .into(),
                env_allowlist: None,
            },
        );
        reg.agents.insert(
            "aoe-agent".into(),
            AgentSpec {
                command: "${aoe_data_dir}/cockpit-worker/dist/aoe-agent".into(),
                args: vec![],
                description: "aoe's bundled multi-provider agent (Vercel AI SDK 6)".into(),
                env_allowlist: None,
            },
        );
        reg
    }

    pub fn get(&self, name: &str) -> Option<&AgentSpec> {
        self.agents.get(name)
    }

    pub fn upsert(&mut self, name: String, spec: AgentSpec) {
        self.agents.insert(name, spec);
    }

    pub fn remove(&mut self, name: &str) -> Option<AgentSpec> {
        self.agents.remove(name)
    }

    pub fn list(&self) -> Vec<(&String, &AgentSpec)> {
        let mut entries: Vec<_> = self.agents.iter().collect();
        entries.sort_by_key(|(n, _)| n.as_str());
        entries
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_include_claude_code_and_aoe_agent() {
        let reg = AgentRegistry::with_defaults();
        assert!(reg.get("claude-code").is_some());
        assert!(reg.get("aoe-agent").is_some());
    }

    #[test]
    fn list_is_sorted() {
        let mut reg = AgentRegistry::new();
        reg.upsert(
            "zeta".into(),
            AgentSpec {
                command: "z".into(),
                args: vec![],
                description: "z".into(),
                env_allowlist: None,
            },
        );
        reg.upsert(
            "alpha".into(),
            AgentSpec {
                command: "a".into(),
                args: vec![],
                description: "a".into(),
                env_allowlist: None,
            },
        );
        let names: Vec<&str> = reg.list().iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, vec!["alpha", "zeta"]);
    }
}
