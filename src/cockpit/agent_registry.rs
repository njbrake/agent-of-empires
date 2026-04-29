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

    /// Returns a registry seeded with the day-one defaults from the v4
    /// design doc: `claude-code` (Anthropic via the official ACP adapter)
    /// and `aoe-agent` (our Node binary).
    pub fn with_defaults() -> Self {
        let mut reg = Self::new();
        // Default to a global install (`npm install -g
        // @agentclientprotocol/claude-agent-acp`). The doctor surfaces a
        // clear remediation hint when the binary isn't on PATH. We
        // deliberately don't use `npx -y @agentclientprotocol/claude-agent-acp`
        // here because the first-run download can hang for tens of
        // seconds with no output, which used to leave the cockpit
        // worker silently wedged before the handshake.
        reg.agents.insert(
            "claude-code".into(),
            AgentSpec {
                command: "claude-agent-acp".into(),
                args: vec![],
                description:
                    "Anthropic Claude via the official ACP adapter (npm i -g @agentclientprotocol/claude-agent-acp)"
                        .into(),
                env_allowlist: None,
            },
        );
        reg.agents.insert(
            "aoe-agent".into(),
            AgentSpec {
                command: "${aoe_data_dir}/cockpit-worker/dist/aoe-agent".into(),
                args: vec![],
                description: "aoe's multi-provider agent (Vercel AI SDK 6)".into(),
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
