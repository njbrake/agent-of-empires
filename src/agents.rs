//! Centralized agent registry.
//!
//! All per-agent metadata lives here. Adding a new agent means adding one
//! `AgentDef` entry to `AGENTS` and writing a status detection function.

use crate::session::Status;
use crate::tmux::status_detection;

/// How to check whether an agent binary is installed on the host.
pub enum DetectionMethod {
    /// Run `which <binary>` and check exit code.
    Which(&'static str),
    /// Run `<binary> <arg>` and check that it doesn't error (e.g. `vibe --version`).
    RunWithArg(&'static str, &'static str),
}

/// How to enable YOLO / auto-approve mode for an agent.
pub enum YoloMode {
    /// Append a CLI flag (e.g. `--dangerously-skip-permissions`).
    CliFlag(&'static str),
    /// Set an environment variable (name, value).
    EnvVar(&'static str, &'static str),
    /// Agent always runs in YOLO mode with no opt-in needed (e.g. pi).
    AlwaysYolo,
}

/// How an agent resumes an existing session from the CLI.
pub enum ResumeStrategy {
    /// Append a flag (e.g. `--session <id>`). For agents where new and existing
    /// sessions use the same flag.
    Flag(&'static str),
    /// Two different flags depending on whether conversation data already exists.
    /// `existing` is used when there is prior conversation data (e.g. `--resume`),
    /// `new_session` when creating/attaching unconditionally (e.g. `--session-id`).
    FlagPair {
        existing: &'static str,
        new_session: &'static str,
    },
    /// Resume is a subcommand rather than a flag (e.g. `codex resume <id>`).
    /// The subcommand + id are inserted right after the binary name so that
    /// other flags land after it.
    Subcommand(&'static str),
    /// Agent does not support session resume.
    Unsupported,
}

/// A single hook event that AoE registers in an agent's settings file.
pub struct HookEvent {
    /// Event name as the agent expects it (e.g. `"PreToolUse"` for Claude Code).
    pub name: &'static str,
    /// Optional matcher pattern (e.g. `"permission_prompt|elicitation_dialog"`).
    pub matcher: Option<&'static str>,
    /// AoE status string written to the sidecar file when this event fires
    /// (e.g. `"running"`, `"idle"`, `"waiting"`).
    pub status: &'static str,
}

/// Configuration for installing status-detection hooks into an agent's settings file.
pub struct AgentHookConfig {
    /// Path relative to the home dir where the agent's settings live
    /// (e.g. `.claude/settings.json`).
    pub settings_rel_path: &'static str,
    /// Hook events to register. Each event maps to an AoE status.
    pub events: &'static [HookEvent],
}

/// Everything we know about a single agent CLI.
pub struct AgentDef {
    /// Canonical name: `"claude"`, `"opencode"`, etc.
    pub name: &'static str,
    /// Binary to invoke (usually same as name).
    pub binary: &'static str,
    /// Alternative substrings recognised by `resolve_tool_name` (e.g. `"open-code"`).
    pub aliases: &'static [&'static str],
    /// How to detect availability on the host.
    pub detection: DetectionMethod,
    /// YOLO/auto-approve configuration.
    pub yolo: Option<YoloMode>,
    /// CLI flag template for custom instruction injection.
    /// `{}` is replaced with the shell-escaped instruction text.
    pub instruction_flag: Option<&'static str>,
    /// If true, `builder.rs` sets `instance.command = binary` for this agent.
    pub set_default_command: bool,
    /// If true, the agent can be launched directly on the host (non-sandboxed).
    pub supports_host_launch: bool,
    /// Status detection function pointer. Takes raw (non-lowercased) pane content.
    pub detect_status: fn(&str) -> Status,
    /// Environment variables always injected into the container for this agent.
    pub container_env: &'static [(&'static str, &'static str)],
    /// Hook configuration for file-based status detection. If set, AoE installs
    /// hooks into the agent's settings file so status is written to a file instead
    /// of being parsed from tmux pane content.
    pub hook_config: Option<AgentHookConfig>,
    /// How this agent resumes a prior session.
    pub resume_strategy: ResumeStrategy,
}

pub const AGENTS: &[AgentDef] = &[
    AgentDef {
        name: "claude",
        binary: "claude",
        aliases: &[],
        detection: DetectionMethod::Which("claude"),
        yolo: Some(YoloMode::CliFlag("--dangerously-skip-permissions")),
        instruction_flag: Some("--append-system-prompt {}"),
        set_default_command: false,
        supports_host_launch: true,
        detect_status: status_detection::detect_claude_status,
        container_env: &[("CLAUDE_CONFIG_DIR", "/root/.claude")],
        hook_config: Some(AgentHookConfig {
            settings_rel_path: ".claude/settings.json",
            events: &[
                HookEvent {
                    name: "PreToolUse",
                    matcher: None,
                    status: "running",
                },
                HookEvent {
                    name: "UserPromptSubmit",
                    matcher: None,
                    status: "running",
                },
                HookEvent {
                    name: "Stop",
                    matcher: None,
                    status: "idle",
                },
                HookEvent {
                    name: "Notification",
                    matcher: Some("permission_prompt|elicitation_dialog"),
                    status: "waiting",
                },
                HookEvent {
                    name: "SessionStart",
                    matcher: None,
                    status: "running",
                },
                HookEvent {
                    name: "SessionEnd",
                    matcher: None,
                    status: "idle",
                },
            ],
        }),
        resume_strategy: ResumeStrategy::FlagPair {
            existing: "--resume",
            new_session: "--session-id",
        },
    },
    AgentDef {
        name: "opencode",
        binary: "opencode",
        aliases: &["open-code"],
        detection: DetectionMethod::Which("opencode"),
        yolo: Some(YoloMode::EnvVar("OPENCODE_PERMISSION", r#"{"*":"allow"}"#)),
        instruction_flag: None,
        set_default_command: true,
        supports_host_launch: false,
        detect_status: status_detection::detect_opencode_status,
        container_env: &[],
        hook_config: None,
        resume_strategy: ResumeStrategy::Flag("--session"),
    },
    AgentDef {
        name: "vibe",
        binary: "vibe",
        aliases: &["mistral-vibe"],
        detection: DetectionMethod::RunWithArg("vibe", "--version"),
        yolo: Some(YoloMode::CliFlag("--agent auto-approve")),
        instruction_flag: None,
        set_default_command: false,
        supports_host_launch: true,
        detect_status: status_detection::detect_vibe_status,
        container_env: &[],
        hook_config: None,
        resume_strategy: ResumeStrategy::Flag("--resume"),
    },
    AgentDef {
        name: "codex",
        binary: "codex",
        aliases: &[],
        detection: DetectionMethod::Which("codex"),
        yolo: Some(YoloMode::CliFlag(
            "--dangerously-bypass-approvals-and-sandbox",
        )),
        instruction_flag: Some("--config developer_instructions={}"),
        set_default_command: true,
        supports_host_launch: true,
        detect_status: status_detection::detect_codex_status,
        container_env: &[],
        hook_config: None,
        resume_strategy: ResumeStrategy::Subcommand("resume"),
    },
    AgentDef {
        name: "gemini",
        binary: "gemini",
        aliases: &[],
        detection: DetectionMethod::Which("gemini"),
        yolo: Some(YoloMode::CliFlag("--approval-mode yolo")),
        instruction_flag: None,
        set_default_command: false,
        supports_host_launch: true,
        detect_status: status_detection::detect_gemini_status,
        container_env: &[],
        hook_config: None,
        resume_strategy: ResumeStrategy::Flag("--resume"),
    },
    AgentDef {
        name: "cursor",
        binary: "agent",
        aliases: &["agent"],
        detection: DetectionMethod::Which("agent"),
        yolo: Some(YoloMode::CliFlag("--yolo")),
        instruction_flag: None,
        set_default_command: false,
        supports_host_launch: true,
        detect_status: status_detection::detect_cursor_status,
        container_env: &[("CURSOR_CONFIG_DIR", "/root/.cursor")],
        hook_config: Some(AgentHookConfig {
            settings_rel_path: ".cursor/settings.json",
            events: &[
                HookEvent {
                    name: "PreToolUse",
                    matcher: None,
                    status: "running",
                },
                HookEvent {
                    name: "UserPromptSubmit",
                    matcher: None,
                    status: "running",
                },
                HookEvent {
                    name: "Stop",
                    matcher: None,
                    status: "idle",
                },
                HookEvent {
                    name: "Notification",
                    matcher: Some("permission_prompt|elicitation_dialog"),
                    status: "waiting",
                },
                HookEvent {
                    name: "SessionStart",
                    matcher: None,
                    status: "running",
                },
                HookEvent {
                    name: "SessionEnd",
                    matcher: None,
                    status: "idle",
                },
            ],
        }),
        resume_strategy: ResumeStrategy::Unsupported,
    },
    AgentDef {
        name: "copilot",
        binary: "copilot",
        aliases: &["github-copilot"],
        detection: DetectionMethod::Which("copilot"),
        yolo: Some(YoloMode::CliFlag("--yolo")),
        instruction_flag: None,
        set_default_command: false,
        supports_host_launch: true,
        detect_status: status_detection::detect_copilot_status,
        container_env: &[("COPILOT_CONFIG_DIR", "/root/.copilot")],
        hook_config: None,
        resume_strategy: ResumeStrategy::Unsupported,
    },
    AgentDef {
        name: "pi",
        binary: "pi",
        aliases: &[],
        detection: DetectionMethod::Which("pi"),
        // Pi runs in full YOLO mode by default (no approval gates), so no flag needed.
        yolo: Some(YoloMode::AlwaysYolo),
        instruction_flag: None,
        set_default_command: false,
        supports_host_launch: true,
        detect_status: status_detection::detect_pi_status,
        container_env: &[("PI_CODING_AGENT_DIR", "/root/.pi/agent")],
        hook_config: None,
        resume_strategy: ResumeStrategy::Unsupported,
    },
];

/// Look up an agent by canonical name.
pub fn get_agent(name: &str) -> Option<&'static AgentDef> {
    AGENTS.iter().find(|a| a.name == name)
}

/// All canonical agent names in registry order.
pub fn agent_names() -> Vec<&'static str> {
    AGENTS.iter().map(|a| a.name).collect()
}

/// Given a command string (e.g. `"claude --resume xyz"` or `"open-code"`),
/// return the canonical agent name if one is recognised.
pub fn resolve_tool_name(cmd: &str) -> Option<&'static str> {
    let cmd_lower = cmd.to_lowercase();
    if cmd_lower.is_empty() {
        return Some("claude");
    }
    for agent in AGENTS {
        if cmd_lower.contains(agent.name) {
            return Some(agent.name);
        }
        for alias in agent.aliases {
            if cmd_lower.contains(alias) {
                return Some(agent.name);
            }
        }
    }
    None
}

/// Convert a tool name to a 1-based settings index (0 = Auto).
pub fn settings_index_from_name(name: Option<&str>) -> usize {
    match name {
        Some(n) => AGENTS
            .iter()
            .position(|a| a.name == n)
            .map(|i| i + 1)
            .unwrap_or(0),
        None => 0,
    }
}

/// Convert a 1-based settings index back to a tool name (0 = Auto/None).
pub fn name_from_settings_index(index: usize) -> Option<&'static str> {
    if index == 0 {
        None
    } else {
        AGENTS.get(index - 1).map(|a| a.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_agent_known() {
        assert_eq!(get_agent("claude").unwrap().binary, "claude");
        assert_eq!(get_agent("opencode").unwrap().binary, "opencode");
        assert_eq!(get_agent("vibe").unwrap().binary, "vibe");
        assert_eq!(get_agent("codex").unwrap().binary, "codex");
        assert_eq!(get_agent("gemini").unwrap().binary, "gemini");
        assert_eq!(get_agent("cursor").unwrap().binary, "agent");
        assert_eq!(get_agent("copilot").unwrap().binary, "copilot");
        assert_eq!(get_agent("pi").unwrap().binary, "pi");
    }

    #[test]
    fn test_get_agent_unknown() {
        assert!(get_agent("unknown").is_none());
    }

    #[test]
    fn test_agent_names() {
        let names = agent_names();
        assert_eq!(
            names,
            vec!["claude", "opencode", "vibe", "codex", "gemini", "cursor", "copilot", "pi"]
        );
    }

    #[test]
    fn test_resolve_tool_name() {
        assert_eq!(resolve_tool_name("claude"), Some("claude"));
        assert_eq!(resolve_tool_name("open-code"), Some("opencode"));
        assert_eq!(resolve_tool_name("mistral-vibe"), Some("vibe"));
        assert_eq!(resolve_tool_name("codex"), Some("codex"));
        assert_eq!(resolve_tool_name("gemini"), Some("gemini"));
        assert_eq!(resolve_tool_name("cursor"), Some("cursor"));
        assert_eq!(resolve_tool_name("github-copilot"), Some("copilot"));
        assert_eq!(resolve_tool_name("copilot"), Some("copilot"));
        assert_eq!(resolve_tool_name("pi"), Some("pi"));
        assert_eq!(resolve_tool_name(""), Some("claude"));
        assert_eq!(resolve_tool_name("agent"), Some("cursor"));
        assert_eq!(resolve_tool_name("unknown-tool"), None);
    }

    #[test]
    fn test_settings_index_roundtrip() {
        assert_eq!(settings_index_from_name(None), 0);
        assert_eq!(settings_index_from_name(Some("claude")), 1);
        assert_eq!(settings_index_from_name(Some("gemini")), 5);
        assert_eq!(settings_index_from_name(Some("cursor")), 6);
        assert_eq!(settings_index_from_name(Some("copilot")), 7);
        assert_eq!(settings_index_from_name(Some("pi")), 8);

        assert_eq!(name_from_settings_index(0), None);
        assert_eq!(name_from_settings_index(1), Some("claude"));
        assert_eq!(name_from_settings_index(5), Some("gemini"));
        assert_eq!(name_from_settings_index(6), Some("cursor"));
        assert_eq!(name_from_settings_index(7), Some("copilot"));
        assert_eq!(name_from_settings_index(8), Some("pi"));
        assert_eq!(name_from_settings_index(99), None);
    }

    #[test]
    fn test_all_agents_have_yolo_support() {
        for agent in AGENTS {
            assert!(
                agent.yolo.is_some(),
                "Agent '{}' should have YOLO mode configured",
                agent.name
            );
        }
    }
}
