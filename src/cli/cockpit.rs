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
    /// List running cockpit workers (detached or attached).
    Ps {
        /// Emit machine-readable JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Gracefully stop a cockpit worker (SIGTERM the runner, agent
    /// receives stdin EOF). Sessions can be reattached on the next
    /// `aoe serve` only if they are still alive afterward; `stop`
    /// destroys the worker.
    Stop {
        /// Session id to stop. Mutually exclusive with `--all`.
        session: Option<String>,
        /// Stop every running cockpit worker.
        #[arg(long, conflicts_with = "session")]
        all: bool,
        /// Seconds to wait after SIGTERM before escalating to SIGKILL.
        #[arg(long, default_value = "5")]
        timeout_secs: u64,
    },
    /// SIGKILL a worker immediately (use when `stop` doesn't take).
    Kill {
        /// Session id to kill.
        session: String,
    },
    /// Tail the runner's log file for a cockpit session.
    Logs {
        /// Session id whose worker logs to tail.
        #[arg(long)]
        session: Option<String>,
        /// Follow new lines as they arrive.
        #[arg(long)]
        follow: bool,
    },
    /// Restart a wedged cockpit worker: stop the existing runner, then
    /// let the daemon's reconciler spawn a fresh one on the next tick.
    Restart {
        /// Session id whose worker to restart.
        session: String,
    },
}

pub async fn run(command: CockpitCommands) -> Result<()> {
    match command {
        CockpitCommands::Doctor { json, fix } => doctor(json, fix).await,
        CockpitCommands::Agents => agents(),
        CockpitCommands::Ps { json } => ps(json),
        CockpitCommands::Stop {
            session,
            all,
            timeout_secs,
        } => stop(session, all, timeout_secs).await,
        CockpitCommands::Kill { session } => kill_now(&session),
        CockpitCommands::Logs { session, follow } => logs(session, follow),
        CockpitCommands::Restart { session } => restart(&session),
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

fn ps(json: bool) -> Result<()> {
    use crate::cockpit::worker_registry;
    let mut records = worker_registry::list().unwrap_or_default();
    records.sort_by_key(|r| r.started_at);
    if json {
        let value: Vec<serde_json::Value> = records
            .iter()
            .map(|r| {
                serde_json::json!({
                    "session_id": r.session_id,
                    "pid": r.pid,
                    "alive": worker_registry::is_record_live(r),
                    "agent": r.agent_name,
                    "socket": r.socket_path,
                    "cwd": r.cwd,
                    "started_at": r.started_at,
                    "last_attached_at": r.last_attached_at,
                    "detached_at": r.detached_at,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    if records.is_empty() {
        println!("No cockpit workers running.");
        return Ok(());
    }
    println!(
        "{:<24} {:<8} {:<14} {:<10} SOCKET",
        "SESSION", "PID", "AGENT", "STATE"
    );
    for r in &records {
        let state = if !worker_registry::is_record_live(r) {
            "dead"
        } else if r.detached_at.is_some()
            && r.last_attached_at.unwrap_or(0) <= r.detached_at.unwrap_or(0)
        {
            "detached"
        } else {
            "attached"
        };
        println!(
            "{:<24} {:<8} {:<14} {:<10} {}",
            truncate(&r.session_id, 24),
            r.pid,
            truncate(&r.agent_name, 14),
            state,
            r.socket_path.display()
        );
    }
    Ok(())
}

async fn stop(session: Option<String>, all: bool, timeout_secs: u64) -> Result<()> {
    use crate::cockpit::worker_registry;
    let targets: Vec<crate::cockpit::worker_registry::WorkerRecord> = if all {
        worker_registry::list().unwrap_or_default()
    } else {
        let id = match session {
            Some(s) => s,
            None => {
                anyhow::bail!("aoe cockpit stop requires <session> or --all");
            }
        };
        worker_registry::load(&id)?
            .map(|r| vec![r])
            .unwrap_or_default()
    };
    if targets.is_empty() {
        println!("No matching cockpit workers.");
        return Ok(());
    }
    for record in &targets {
        // Delete the registry entry BEFORE SIGTERM. The running daemon
        // (if any) uses the registry-gone signal in `restart_decision`
        // to distinguish a user-initiated stop from a crash; without
        // this ordering, the daemon's drain task sees socket EOF first,
        // observes the registry still present, and respawns the runner
        // — which immediately gets killed by our SIGTERM, racing into a
        // crash loop that burns the restart budget and surfaces the
        // "ACP agent crashed more than N times" banner.
        worker_registry::delete(&record.session_id).ok();
        signal_and_wait(record, timeout_secs).await;
        println!(
            "Stopped cockpit worker for {} (PID {}).",
            record.session_id, record.pid
        );
    }
    Ok(())
}

fn kill_now(session: &str) -> Result<()> {
    use crate::cockpit::worker_registry;
    let Some(record) = worker_registry::load(session)? else {
        anyhow::bail!("No cockpit worker registry entry for session {session}");
    };
    // Delete registry before SIGKILL for the same race reason described
    // on `stop`: the running daemon's drain task uses the registry-gone
    // signal to skip respawn on user-initiated termination.
    worker_registry::delete(session).ok();
    #[cfg(unix)]
    if worker_registry::is_pid_alive(record.pid) {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;
        let _ = kill(Pid::from_raw(record.pid as i32), Signal::SIGKILL);
    }
    println!(
        "Killed cockpit worker for {} (PID {}).",
        session, record.pid
    );
    Ok(())
}

async fn signal_and_wait(
    record: &crate::cockpit::worker_registry::WorkerRecord,
    timeout_secs: u64,
) {
    use crate::cockpit::worker_registry;
    #[cfg(unix)]
    {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;
        let pid = Pid::from_raw(record.pid as i32);
        if !worker_registry::is_pid_alive(record.pid) {
            return;
        }
        let _ = kill(pid, Signal::SIGTERM);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
        while std::time::Instant::now() < deadline {
            if !worker_registry::is_pid_alive(record.pid) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        let _ = kill(pid, Signal::SIGKILL);
    }
    #[cfg(not(unix))]
    let _ = (record, timeout_secs);
}

fn logs(session: Option<String>, follow: bool) -> Result<()> {
    use crate::cockpit::worker_registry;
    let id = match session {
        Some(s) => s,
        None => {
            let records = worker_registry::list().unwrap_or_default();
            if records.len() == 1 {
                records[0].session_id.clone()
            } else if records.is_empty() {
                println!("No cockpit workers running. Use `aoe cockpit ps` to inspect.");
                return Ok(());
            } else {
                println!("Multiple cockpit workers running; pass --session <id>:");
                for r in records {
                    println!("  {}", r.session_id);
                }
                return Ok(());
            }
        }
    };
    let log_path = worker_registry::log_path_for(&id)?;
    if !log_path.exists() {
        println!(
            "No log file at {} (worker may not have started yet).",
            log_path.display()
        );
        return Ok(());
    }
    if follow {
        // Use a simple busy-poll tail rather than depending on notify
        // crates; the runner appends a handful of lines per minute, so
        // the wasted wake-ups are negligible.
        use std::io::{BufRead, BufReader, Seek, SeekFrom};
        let mut file = std::fs::File::open(&log_path)?;
        // Seek to end so we only print *new* lines, like `tail -f`.
        file.seek(SeekFrom::End(0))?;
        let mut reader = BufReader::new(file);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => std::thread::sleep(std::time::Duration::from_millis(200)),
                Ok(_) => print!("{line}"),
                Err(e) => {
                    eprintln!("read error: {e}");
                    break;
                }
            }
        }
    } else {
        let content = std::fs::read_to_string(&log_path)?;
        print!("{content}");
    }
    Ok(())
}

fn restart(session: &str) -> Result<()> {
    use crate::cockpit::worker_registry;
    let Some(record) = worker_registry::load(session)? else {
        anyhow::bail!("No cockpit worker registry entry for session {session}");
    };
    // SIGTERM the runner; the next 2s reconciler tick on `aoe serve`
    // notices the session has no live worker and spawns a fresh one
    // (which calls session/load with the cached acp_session_id).
    // Delete registry first so the daemon's drain task observes a
    // user-initiated stop instead of racing the SIGTERM into a crash-
    // loop respawn.
    worker_registry::delete(session).ok();
    #[cfg(unix)]
    if worker_registry::is_pid_alive(record.pid) {
        use nix::sys::signal::{kill, Signal};
        use nix::unistd::Pid;
        let _ = kill(Pid::from_raw(record.pid as i32), Signal::SIGTERM);
    }
    println!(
        "Stopped runner for {} (PID {}). `aoe serve` will respawn on its next reconciler tick.",
        session, record.pid
    );
    Ok(())
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n.saturating_sub(1)).collect();
        out.push('…');
        out
    }
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
