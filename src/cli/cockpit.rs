//! Cockpit CLI subcommands.
//!
//! `aoe cockpit doctor` runs preflight checks (Node runtime, agent
//! binaries, claude auth). `aoe cockpit agents` lists configured
//! cockpit agents. Logs/restart are deferred until the worker
//! supervisor is wired into `aoe serve`.

use anyhow::Result;
use clap::Subcommand;

use crate::cockpit::agent_registry::AgentRegistry;
use crate::cockpit::node;

#[derive(Subcommand)]
pub enum CockpitCommands {
    /// Verify the cockpit can start: Node runtime, configured agents,
    /// provider auth (claude login).
    Doctor {
        /// Emit machine-readable JSON instead of a human report.
        #[arg(long)]
        json: bool,
        /// Attempt safe remediations: install missing claude-code-acp
        /// adapter, verify aoe-agent presence, etc. (Reserved for future
        /// release; the flag exists so scripts can opt in early.)
        #[arg(long)]
        fix: bool,
    },
    /// List configured cockpit agents (claude-code, aoe-agent, etc.).
    Agents,
    /// Tail the worker stderr for a running cockpit session. Requires
    /// `aoe serve` to be running and is deferred until the worker
    /// supervisor lands.
    Logs {
        /// Session id whose worker logs to tail.
        #[arg(long)]
        session: Option<String>,
        /// Follow new lines as they arrive.
        #[arg(long)]
        follow: bool,
    },
    /// Restart a wedged cockpit worker. Reserved for the supervisor
    /// slice.
    Restart {
        /// Session id whose worker to restart.
        session: String,
    },
}

pub async fn run(command: CockpitCommands) -> Result<()> {
    match command {
        CockpitCommands::Doctor { json, fix } => doctor(json, fix).await,
        CockpitCommands::Agents => agents(),
        CockpitCommands::Logs { session, follow } => logs(session, follow),
        CockpitCommands::Restart { session } => restart(session),
    }
}

#[derive(Debug, serde::Serialize)]
struct DoctorReport {
    node: NodeStatus,
    agents: Vec<AgentDoctorEntry>,
    overall: &'static str,
}

#[derive(Debug, serde::Serialize)]
struct NodeStatus {
    found: bool,
    path: Option<String>,
    version: Option<String>,
    meets_minimum: Option<bool>,
}

#[derive(Debug, serde::Serialize)]
struct AgentDoctorEntry {
    name: String,
    command_present: bool,
    description: String,
}

/// ACP adapters that ship as npm packages (binary name → package id).
/// The doctor's `--fix` path runs `npm install -g <package>` for each
/// entry whose binary isn't already on PATH.
const NPM_INSTALLABLE_ACP: &[(&str, &str)] = &[
    ("claude-agent-acp", "@agentclientprotocol/claude-agent-acp"),
    ("codex-acp", "@zed-industries/codex-acp"),
    ("pi-acp", "pi-acp"),
];

/// Native CLIs whose ACP server is shipped as part of the agent
/// itself, not as a separate npm adapter. These get a one-line
/// install hint in the doctor output instead of an `npm i -g`.
pub(crate) fn install_hint_for(binary: &str) -> Option<&'static str> {
    Some(match binary {
        "claude-agent-acp" => "npm install -g @agentclientprotocol/claude-agent-acp",
        "codex-acp" => "npm install -g @zed-industries/codex-acp",
        "pi-acp" => {
            "npm install -g pi-acp  (also requires `npm i -g @mariozechner/pi-coding-agent`)"
        }
        "opencode" => "curl -fsSL https://opencode.ai/install | bash  (then `opencode acp`)",
        "gemini" => "npm install -g @google/gemini-cli  (then `gemini --acp`)",
        "vibe-acp" => {
            "follow https://github.com/mistralai/mistral-vibe (ships the `vibe-acp` binary)"
        }
        _ => return None,
    })
}

async fn doctor(json: bool, fix: bool) -> Result<()> {
    if fix {
        // Auto-remediate: download the bundled Node runtime if Node is
        // missing or the wrong version on PATH.
        if let Ok(app_dir) = crate::session::get_app_dir() {
            match node::resolve("", &app_dir) {
                Ok(_) => println!("Node already available; skipping download."),
                Err(node::NodeError::NoNode(_)) | Err(node::NodeError::TooOld { .. }) => {
                    println!("Downloading Node {} runtime...", node::PINNED_NODE_VERSION);
                    match node::download(&app_dir).await {
                        Ok(resolved) => {
                            println!(
                                "Installed Node {} at {}",
                                resolved.version,
                                resolved.path.display()
                            );
                        }
                        Err(e) => {
                            println!("Download failed: {e}");
                        }
                    }
                }
                Err(e) => println!("Cannot probe Node: {e}"),
            }
        }
        // Auto-install npm-distributed ACP adapters that aren't on
        // PATH. Native CLIs (opencode / gemini / vibe) have to be
        // installed via their own channels; we only print a hint for
        // those.
        for (binary, npm_pkg) in NPM_INSTALLABLE_ACP {
            if find_in_path(binary).is_some() {
                continue;
            }
            println!("Installing {npm_pkg} globally via npm...");
            let status = std::process::Command::new("npm")
                .args(["install", "-g", npm_pkg])
                .status();
            match status {
                Ok(s) if s.success() => println!("Installed {npm_pkg}."),
                Ok(s) => println!("npm install {npm_pkg} exited with status {s}"),
                Err(e) => {
                    println!("Could not run npm: {e}. Install Node.js + npm first.");
                    break;
                }
            }
        }
    }
    let registry = AgentRegistry::with_defaults();

    let node_status = check_node();
    let agent_entries: Vec<AgentDoctorEntry> = registry
        .list()
        .into_iter()
        .map(|(name, spec)| AgentDoctorEntry {
            name: name.clone(),
            command_present: command_present(&spec.command),
            description: spec.description.clone(),
        })
        .collect();

    let any_agent_ok = agent_entries.iter().any(|e| e.command_present);
    let node_ok = node_status.meets_minimum.unwrap_or(false);
    let overall = if node_ok && any_agent_ok {
        "ok"
    } else if node_ok || any_agent_ok {
        "partial"
    } else {
        "fail"
    };
    let report = DoctorReport {
        node: node_status,
        agents: agent_entries,
        overall,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("Cockpit doctor  (Beta)");
    println!("======================");
    println!();
    // Surface the gate state up front so users investigating "why
    // doesn't cockpit work for me" don't have to read the docs to
    // notice that they need an env var.
    if !crate::cockpit::experimental_enabled() {
        println!("[!! ] AOE_EXPERIMENTAL_COCKPIT is not set.");
        println!("    Cockpit is gated behind this env var while it stabilises.");
        println!("    Set AOE_EXPERIMENTAL_COCKPIT=1 in the env that runs `aoe serve`");
        println!("    (and the CLI for `aoe add --cockpit`) to opt in.");
        println!();
    }
    println!("Cockpit is the structured-rendering substrate (ACP-based).");
    println!("Tmux passthrough remains the default for tool sessions; cockpit");
    println!("is opt-in per session via `aoe add --cockpit` or the web wizard.");
    println!();
    let node = &report.node;
    let node_mark = if node.meets_minimum.unwrap_or(false) {
        "[OK]"
    } else {
        "[!! ]"
    };
    println!(
        "{} Node runtime  {}",
        node_mark,
        node.version.as_deref().unwrap_or("not found"),
    );
    if let Some(path) = &node.path {
        println!("    path: {}", path);
    }
    println!();
    println!("Configured agents:");
    let registry_for_hints = AgentRegistry::with_defaults();
    for entry in &report.agents {
        let mark = if entry.command_present {
            "[OK]"
        } else {
            "[!! ]"
        };
        println!("{} {}  ({})", mark, entry.name, entry.description);
        if !entry.command_present {
            // Look up the binary name via the registry so we can
            // print a tailored install hint instead of generic
            // "missing".
            if let Some(spec) = registry_for_hints.get(&entry.name) {
                let bin = spec.command.split('/').next_back().unwrap_or(&spec.command);
                if let Some(hint) = install_hint_for(bin) {
                    println!("    install: {hint}");
                }
            }
        }
    }
    println!();
    println!("Overall: {}", overall);

    if overall != "ok" {
        std::process::exit(if overall == "partial" { 2 } else { 1 });
    }
    Ok(())
}

fn check_node() -> NodeStatus {
    let path = match find_in_path("node") {
        Some(p) => p,
        None => {
            return NodeStatus {
                found: false,
                path: None,
                version: None,
                meets_minimum: None,
            };
        }
    };
    let output = std::process::Command::new(&path).arg("--version").output();
    let (version, meets_minimum) = match output {
        Ok(out) if out.status.success() => {
            let raw = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let meets = parse_node_major(&raw).map(|m| m >= 20);
            (Some(raw), meets)
        }
        _ => (None, None),
    };
    NodeStatus {
        found: true,
        path: Some(path),
        version,
        meets_minimum,
    }
}

fn parse_node_major(raw: &str) -> Option<u32> {
    let trimmed = raw.trim_start_matches('v');
    let major_str = trimmed.split('.').next()?;
    major_str.parse::<u32>().ok()
}

fn find_in_path(binary: &str) -> Option<String> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(binary);
        if candidate.is_file() {
            return Some(candidate.to_string_lossy().into_owned());
        }
    }
    None
}

pub(crate) fn command_present(command: &str) -> bool {
    // Placeholders like `${aoe_data_dir}/cockpit-worker/...` resolve at
    // runtime against the app data dir, so the literal string contains
    // both `${` and `/`. Check the placeholder branch FIRST — otherwise
    // the `/`-branch tries to stat a literal path containing `${...}`
    // and reports "missing" for every placeholder-based agent
    // (notably `aoe-agent`, our bundled multi-provider fallback).
    if command.contains("${") {
        true
    } else if command.contains('/') || command.contains('\\') {
        std::path::Path::new(command).exists()
    } else {
        find_in_path(command).is_some()
    }
}

fn agents() -> Result<()> {
    let registry = AgentRegistry::with_defaults();
    println!("Configured cockpit agents:");
    println!();
    for (name, spec) in registry.list() {
        let present = command_present(&spec.command);
        let mark = if present { "[OK]" } else { "[!! ]" };
        println!("{} {:<14}  {}", mark, name, spec.description);
        let args = if spec.args.is_empty() {
            String::new()
        } else {
            format!(" {}", spec.args.join(" "))
        };
        println!("        spawn: {}{}", spec.command, args);
    }
    Ok(())
}

fn logs(session: Option<String>, _follow: bool) -> Result<()> {
    match session {
        Some(id) => {
            println!(
                "aoe cockpit logs --session {id} is not yet wired (requires the worker supervisor; tracked for a follow-up release)."
            );
        }
        None => {
            println!(
                "aoe cockpit logs has no active workers to tail (worker supervisor wiring is the next slice)."
            );
        }
    }
    Ok(())
}

fn restart(session: String) -> Result<()> {
    println!(
        "aoe cockpit restart {session} is not yet wired (requires the worker supervisor; tracked for a follow-up release)."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_node_major_works() {
        assert_eq!(parse_node_major("v22.21.0"), Some(22));
        assert_eq!(parse_node_major("v20.0.0"), Some(20));
        assert_eq!(parse_node_major("18.17.1"), Some(18));
        assert_eq!(parse_node_major("not a version"), None);
    }
}
