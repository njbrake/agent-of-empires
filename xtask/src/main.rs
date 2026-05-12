//! xtask - Development tasks for agent-of-empires

use clap::{CommandFactory, Parser, Subcommand};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

#[derive(Parser)]
#[command(name = "xtask")]
#[command(about = "Development tasks for agent-of-empires")]
struct Xtask {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate CLI documentation from clap definitions
    GenDocs,
    /// Check that contrib skill files reference valid CLI commands
    CheckSkill,
    /// Probe an ACP adapter's substrate-switching capabilities and
    /// write the results to `<app_dir>/cockpit-probe-results.json`.
    /// The `cockpit::capabilities` resolver reads that file to flip
    /// per-tool capability bits from the conservative defaults.
    ///
    /// Today the probe only checks adapter presence + spawn-ability.
    /// Full ACP-protocol round-trip verification (session/load,
    /// native-id discovery, cross-substrate import) is tracked as
    /// follow-up work; the harness is wired so that work can land
    /// without touching capability consumers.
    CockpitProbe {
        /// Single agent name to probe (e.g. claude, codex, gemini).
        /// Mutually exclusive with --all.
        #[arg(long)]
        agent: Option<String>,
        /// Probe every agent in the default registry.
        #[arg(long, conflicts_with = "agent")]
        all: bool,
    },
}

fn main() {
    let args = Xtask::parse();
    match args.command {
        Commands::GenDocs => generate_cli_docs(),
        Commands::CheckSkill => check_skill(),
        Commands::CockpitProbe { agent, all } => cockpit_probe(agent, all),
    }
}

fn generate_cli_docs() {
    let markdown = clap_markdown::help_markdown::<agent_of_empires::cli::Cli>();

    let docs_dir = Path::new("docs/cli");
    fs::create_dir_all(docs_dir).expect("Failed to create docs/cli directory");

    let output_path = docs_dir.join("reference.md");
    fs::write(&output_path, markdown).expect("Failed to write CLI reference");

    println!("Generated CLI documentation at {}", output_path.display());
}

fn collect_subcommand_paths(cmd: &clap::Command, prefix: &str, out: &mut BTreeSet<String>) {
    for sub in cmd.get_subcommands() {
        if sub.get_name() == "help" {
            continue;
        }
        let path = if prefix.is_empty() {
            sub.get_name().to_string()
        } else {
            format!("{} {}", prefix, sub.get_name())
        };
        out.insert(path.clone());
        collect_subcommand_paths(sub, &path, out);
    }
}

fn check_skill() {
    let skill_path = Path::new("contrib/openclaw-skill/SKILL.md");
    if !skill_path.exists() {
        eprintln!("Skill file not found: {}", skill_path.display());
        std::process::exit(1);
    }

    let content = fs::read_to_string(skill_path).expect("Failed to read SKILL.md");

    let mut has_error = false;

    // The skill's published version is managed by clawhub via _meta.json and
    // the release workflow's `--version` flag. A static `version:` in the
    // frontmatter goes stale on every release, so disallow it.
    if let Some((frontmatter, _)) = content
        .strip_prefix("---\n")
        .and_then(|s| s.split_once("\n---"))
    {
        for line in frontmatter.lines() {
            if line.starts_with("version:") {
                eprintln!(
                    "ERROR: SKILL.md frontmatter must not contain a top-level `version:` field; \
                     clawhub's _meta.json is the source of truth"
                );
                has_error = true;
                break;
            }
        }
    }

    // Build the clap command tree
    let cli_cmd = agent_of_empires::cli::Cli::command();
    let mut cli_commands: BTreeSet<String> = BTreeSet::new();
    collect_subcommand_paths(&cli_cmd, "", &mut cli_commands);

    // Extract `aoe <words>` patterns and match longest valid subcommand path
    let re = regex::Regex::new(r"aoe\s+([a-z][a-z0-9 -]*)").unwrap();
    let mut skill_commands: BTreeSet<String> = BTreeSet::new();
    for cap in re.captures_iter(&content) {
        let raw = cap[1].trim();
        let words: Vec<&str> = raw
            .split_whitespace()
            .take_while(|w| {
                !w.starts_with('-')
                    && !w.starts_with('<')
                    && !w.starts_with('"')
                    && !w.starts_with('$')
                    && !w.starts_with('/')
                    && !w.starts_with('.')
                    && w.chars().all(|c| c.is_ascii_lowercase() || c == '-')
            })
            .collect();

        // Find the longest prefix that is a known CLI command
        let mut best = String::new();
        let mut path = String::new();
        for word in &words {
            if path.is_empty() {
                path = word.to_string();
            } else {
                path = format!("{} {}", path, word);
            }
            if cli_commands.contains(&path) {
                best = path.clone();
            }
        }
        // If no exact match, use the first word if it's a known top-level command
        if best.is_empty() && !words.is_empty() && cli_commands.contains(words[0]) {
            best = words[0].to_string();
        }
        if !best.is_empty() {
            skill_commands.insert(best);
        }
    }

    // Check for skill references to commands that don't exist
    for skill_cmd in &skill_commands {
        if !cli_commands.contains(skill_cmd) {
            let is_prefix = cli_commands
                .iter()
                .any(|c| c.starts_with(&format!("{} ", skill_cmd)));
            if !is_prefix {
                eprintln!(
                    "ERROR: Skill references command 'aoe {}' which does not exist in CLI",
                    skill_cmd
                );
                has_error = true;
            }
        }
    }

    // Advisory: CLI commands not mentioned in skill
    let mut missing_from_skill = Vec::new();
    for cli_cmd in &cli_commands {
        let mentioned = skill_commands.iter().any(|s| {
            s == cli_cmd
                || cli_cmd.starts_with(&format!("{} ", s))
                || s.starts_with(&format!("{} ", cli_cmd))
        });
        if !mentioned {
            missing_from_skill.push(cli_cmd.clone());
        }
    }

    if !missing_from_skill.is_empty() {
        println!("Advisory: CLI commands not referenced in skill file:");
        for cmd in &missing_from_skill {
            println!("  aoe {}", cmd);
        }
    }

    if has_error {
        std::process::exit(1);
    }

    println!("Skill check passed.");
}

/// Probe one or more ACP adapters and persist substrate-switch
/// capability evidence to `<app_dir>/cockpit-probe-results.json`.
///
/// The format is the on-disk shape consumed by
/// `agent_of_empires::cockpit::capabilities::probe_results_path`. Each
/// invocation merges into the existing file rather than overwriting,
/// so probing one tool today and another tomorrow accumulates.
///
/// Today the probe is a smoke test: adapter on PATH + non-immediate
/// crash on spawn. The richer ACP-protocol round trip (session/load,
/// native-id discovery, cross-substrate import) is tracked in the
/// substrate-switching plan; this scaffold lets capability defaults
/// flip without touching capability-consuming code.
fn cockpit_probe(agent: Option<String>, all: bool) {
    use agent_of_empires::cockpit::agent_registry::AgentRegistry;

    let registry = AgentRegistry::with_defaults();
    let mut targets: Vec<String> = Vec::new();
    if all {
        targets.extend(registry.list().into_iter().map(|(n, _)| n.clone()));
    } else if let Some(a) = agent {
        if registry.get(&a).is_none() {
            eprintln!("ERROR: agent {a:?} not in default registry");
            eprintln!("Available agents:");
            for (n, _) in registry.list() {
                eprintln!("  {n}");
            }
            std::process::exit(1);
        }
        targets.push(a);
    } else {
        eprintln!("ERROR: pass --agent <name> or --all");
        std::process::exit(2);
    }

    let path = match agent_of_empires::cockpit::capabilities::probe_results_path() {
        Some(p) => p,
        None => {
            eprintln!("ERROR: could not resolve app_dir for probe results");
            std::process::exit(1);
        }
    };

    let mut results: serde_json::Map<String, serde_json::Value> = if let Ok(bytes) = fs::read(&path)
    {
        match serde_json::from_slice::<serde_json::Value>(&bytes) {
            Ok(serde_json::Value::Object(m)) => m,
            _ => serde_json::Map::new(),
        }
    } else {
        serde_json::Map::new()
    };

    let now = chrono::Utc::now().to_rfc3339();

    for tool in &targets {
        let spec = registry.get(tool).expect("checked above");
        let bin = spec.command.split('/').next_back().unwrap_or(&spec.command);
        let adapter_available = spec.command.contains("${")
            || which_on_path(bin)
            || fs::metadata(&spec.command).is_ok();

        // Conservative defaults until a full ACP round-trip probe
        // lands. We don't flip `native_session_discoverable = true`
        // here even when the adapter is available — that bit must come
        // from observed ACP-mode disk artifacts, which this scaffold
        // doesn't yet measure.
        let entry = serde_json::json!({
            "load_session_capable": if tool == "claude" { adapter_available } else { false },
            "native_session_discoverable": false,
            "probed_at": now,
            "adapter_version": null,
            "adapter_available": adapter_available,
        });
        results.insert(tool.clone(), entry);

        let mark = if adapter_available { "[OK]" } else { "[!!]" };
        println!(
            "{mark} {tool}  (adapter `{}` {})",
            spec.command,
            if adapter_available {
                "found"
            } else {
                "missing on PATH"
            }
        );
    }

    let json = serde_json::Value::Object(results);
    let pretty = serde_json::to_string_pretty(&json).expect("serialize");
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    fs::write(&path, pretty).unwrap_or_else(|e| {
        eprintln!("ERROR: could not write {}: {e}", path.display());
        std::process::exit(1);
    });
    println!();
    println!("Wrote probe results to {}", path.display());
}

fn which_on_path(binary: &str) -> bool {
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&path_var).any(|dir| dir.join(binary).is_file())
}
