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
    /// Check logging consistency: every tracing! call carries an explicit
    /// target:, and Rust/TS/docs target lists stay in sync.
    CheckLogging,
    /// Bulk-rewrite untagged `tracing::xxx!(...)` calls in a file to carry
    /// the given target as the first macro argument. Use to backfill
    /// historical untagged calls in bulk; review the diff and adjust
    /// per-call where a different target is more appropriate.
    AutoTagLogging {
        /// File to rewrite.
        path: String,
        /// Target to insert, e.g. `tui.home`, `session.create`.
        target: String,
    },
}

fn main() {
    let args = Xtask::parse();
    match args.command {
        Commands::GenDocs => generate_cli_docs(),
        Commands::CheckSkill => check_skill(),
        Commands::CheckLogging => check_logging(),
        Commands::AutoTagLogging { path, target } => auto_tag_logging(&path, &target),
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

/// Walk src/, find every `tracing::(trace|debug|info|warn|error)!(...)`
/// invocation, and flag any whose argument list does not contain `target:`.
/// Untagged calls fall under `agent_of_empires` and cannot be dialed via
/// per-target filters, so they're treated as a regression unless explicitly
/// allow-listed with `// allow-untagged-trace` on the previous line.
///
/// Then sanity-check that KNOWN_SUB_TARGETS in src/logging.rs, KNOWN_TARGETS
/// in web/src/components/settings/LoggingSettings.tsx, and the targets table
/// in docs/development/logging.md cover the same set of sub-targets. Drift
/// here causes the settings dropdown and docs to disagree with what the
/// backend actually filters on.
fn check_logging() {
    let mut has_error = false;
    has_error |= !check_untagged_tracing();
    has_error |= !check_target_list_sync();
    if has_error {
        std::process::exit(1);
    }
    println!("Logging consistency check passed.");
}

fn check_untagged_tracing() -> bool {
    let mut ok = true;
    // Match both level macros (`tracing::info!`, ...) and span macros
    // (`tracing::info_span!`, `tracing::span!`, ...). Both accept `target:`
    // as the first macro argument; both should carry it so the user can
    // dial coverage from settings without falling back to the crate root.
    let macro_re = regex::Regex::new(
        r"tracing::(trace_span|debug_span|info_span|warn_span|error_span|trace|debug|info|warn|error|event|span)!\s*\(",
    )
    .expect("regex");

    for entry in walk_rust_files(Path::new("src")) {
        let content = match fs::read_to_string(&entry) {
            Ok(c) => c,
            Err(_) => continue,
        };
        // Skip the logging module itself: it defines plumbing for tracing
        // and emits a few diagnostic events under `log.runtime` via raw
        // macros. The check would false-positive on those.
        if entry.ends_with("logging.rs") {
            continue;
        }
        for m in macro_re.find_iter(&content) {
            let start = m.start();
            // Find the matching `)` for this `tracing::xxx!(`.
            let open = m.end() - 1; // points at `(`
            let Some(close) = find_matching_paren(&content, open) else {
                continue;
            };
            let body = &content[open..=close];
            if body.contains("target:") {
                continue;
            }
            // Allow-list: a line `// allow-untagged-trace` immediately
            // above the call opts out (e.g. tests, log_runtime audit).
            let line_start = content[..start].rfind('\n').map(|p| p + 1).unwrap_or(0);
            let prev_line_end = line_start.saturating_sub(1);
            let prev_line_start = content[..prev_line_end]
                .rfind('\n')
                .map(|p| p + 1)
                .unwrap_or(0);
            let prev_line = &content[prev_line_start..prev_line_end];
            if prev_line.trim().contains("allow-untagged-trace") {
                continue;
            }
            let line_no = content[..start].bytes().filter(|&b| b == b'\n').count() + 1;
            eprintln!(
                "ERROR: untagged tracing call at {}:{} (add `target: \"<root>.<sub>\"` or `// allow-untagged-trace` above)",
                entry.display(),
                line_no
            );
            ok = false;
        }
    }
    ok
}

fn walk_rust_files(root: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    walk_rust_files_into(root, &mut out);
    out
}

fn walk_rust_files_into(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk_rust_files_into(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn find_matching_paren(s: &str, open_idx: usize) -> Option<usize> {
    let bytes = s.as_bytes();
    if bytes.get(open_idx) != Some(&b'(') {
        return None;
    }
    let mut depth = 0i32;
    let mut in_str = false;
    let mut in_char = false;
    let mut in_line_comment = false;
    let mut in_block_comment = 0i32;
    let mut i = open_idx;
    while i < bytes.len() {
        let b = bytes[i];
        let next = bytes.get(i + 1).copied();
        if in_line_comment {
            if b == b'\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }
        if in_block_comment > 0 {
            if b == b'*' && next == Some(b'/') {
                in_block_comment -= 1;
                i += 2;
                continue;
            } else if b == b'/' && next == Some(b'*') {
                in_block_comment += 1;
                i += 2;
                continue;
            }
            i += 1;
            continue;
        }
        if in_str {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if in_char {
            if b == b'\\' {
                i += 2;
                continue;
            }
            if b == b'\'' {
                in_char = false;
            }
            i += 1;
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'\'' => {
                // Char or lifetime. Disambiguate by looking ahead.
                // For our use case (find_matching_paren on a macro call),
                // treating `'` as char literal is fine because lifetimes
                // contain no parens.
                in_char = true;
            }
            b'/' if next == Some(b'/') => in_line_comment = true,
            b'/' if next == Some(b'*') => in_block_comment = 1,
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

fn check_target_list_sync() -> bool {
    use agent_of_empires::logging::KNOWN_SUB_TARGETS;

    let rust_set: BTreeSet<String> = KNOWN_SUB_TARGETS.iter().map(|s| s.to_string()).collect();

    let ts_path = Path::new("web/src/components/settings/LoggingSettings.tsx");
    let ts_content = match fs::read_to_string(ts_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ERROR: could not read {}: {e}", ts_path.display());
            return false;
        }
    };
    let ts_re = regex::Regex::new(r#"\{\s*value:\s*"([^"]+)"\s*,\s*group:"#).unwrap();
    let ts_set: BTreeSet<String> = ts_re
        .captures_iter(&ts_content)
        .map(|c| c[1].to_string())
        .collect();

    let docs_path = Path::new("docs/development/logging.md");
    let docs_content = match fs::read_to_string(docs_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ERROR: could not read {}: {e}", docs_path.display());
            return false;
        }
    };
    // Sub-targets in the docs table appear inside backticks: `<root>.<sub>`.
    // Drop bare root names (no dot) and the meta `log.runtime` is included.
    let docs_re = regex::Regex::new(r"`([a-z_]+(?:\.[a-z_]+)+)`").unwrap();
    let docs_set: BTreeSet<String> = docs_re
        .captures_iter(&docs_content)
        .map(|c| c[1].to_string())
        .filter(|s| !s.starts_with("logging.") && !s.starts_with("config."))
        .collect();

    let mut ok = true;

    let in_rust_only: Vec<_> = rust_set.difference(&ts_set).collect();
    if !in_rust_only.is_empty() {
        eprintln!("ERROR: sub-targets in src/logging.rs but missing from web/src/components/settings/LoggingSettings.tsx:");
        for t in in_rust_only {
            eprintln!("  {t}");
        }
        ok = false;
    }
    let in_ts_only: Vec<_> = ts_set.difference(&rust_set).collect();
    if !in_ts_only.is_empty() {
        eprintln!("ERROR: sub-targets in LoggingSettings.tsx but missing from KNOWN_SUB_TARGETS in src/logging.rs:");
        for t in in_ts_only {
            eprintln!("  {t}");
        }
        ok = false;
    }

    // Docs check is best-effort: the table phrasing may not match exactly.
    // Only flag sub-targets present in Rust but NOT mentioned anywhere in the
    // docs file.
    let missing_from_docs: Vec<_> = rust_set.iter().filter(|t| !docs_set.contains(*t)).collect();
    if !missing_from_docs.is_empty() {
        eprintln!(
            "ERROR: sub-targets in KNOWN_SUB_TARGETS but not mentioned in docs/development/logging.md:"
        );
        for t in missing_from_docs {
            eprintln!("  {t}");
        }
        ok = false;
    }
    ok
}

/// Bulk-rewrite untagged `tracing::xxx!(...)` calls in `path` to carry
/// `target: "<target>"` as the first macro argument. Idempotent: calls
/// that already have `target:` are skipped. Backslash-style escapes are
/// preserved through string literals.
fn auto_tag_logging(path: &str, target: &str) {
    // Match the same set of macros as `check_untagged_tracing` so a
    // `check-logging` failure can always be backfilled by re-running this.
    let macro_re = regex::Regex::new(
        r"tracing::(trace_span|debug_span|info_span|warn_span|error_span|trace|debug|info|warn|error|event|span)!\s*\(",
    )
    .expect("regex");
    let content = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ERROR: could not read {path}: {e}");
            std::process::exit(1);
        }
    };
    let mut out = String::with_capacity(content.len() + 256);
    let mut cursor = 0usize;
    let mut rewritten = 0usize;
    for m in macro_re.find_iter(&content) {
        let open = m.end() - 1;
        let Some(close) = find_matching_paren(&content, open) else {
            continue;
        };
        let body = &content[open..=close];
        if body.contains("target:") {
            continue;
        }
        // Emit up to and including the `(`.
        out.push_str(&content[cursor..=open]);
        // Inject `target: "<target>",` (with a trailing space for readability
        // when the existing first arg is on the same line).
        out.push_str(&format!("target: \"{target}\", "));
        cursor = open + 1;
        rewritten += 1;
    }
    out.push_str(&content[cursor..]);
    if rewritten == 0 {
        println!("{path}: no untagged calls");
        return;
    }
    if let Err(e) = fs::write(path, out) {
        eprintln!("ERROR: could not write {path}: {e}");
        std::process::exit(1);
    }
    println!("{path}: tagged {rewritten} calls with target: \"{target}\"");
}
