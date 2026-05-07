//! `agent-of-empires session` subcommands implementation

use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use serde::Serialize;

use crate::session::{GroupTree, Storage};

#[derive(Subcommand)]
pub enum SessionCommands {
    /// Start a session's tmux process
    Start(SessionIdArgs),

    /// Stop session process
    Stop(SessionIdArgs),

    /// Restart session (or all sessions with `--all`)
    Restart(RestartArgs),

    /// Attach to session interactively
    Attach(SessionIdArgs),

    /// Show session details
    Show(ShowArgs),

    /// Rename a session
    Rename(RenameArgs),

    /// Capture tmux pane output
    Capture(CaptureArgs),

    /// Auto-detect current session
    Current(CurrentArgs),

    /// Set agent session ID for a session
    SetSessionId(SetSessionIdArgs),
}

#[derive(Args)]
pub struct SessionIdArgs {
    /// Session ID or title
    identifier: String,
}

#[derive(Args)]
pub struct RestartArgs {
    /// Session ID or title (required unless `--all` is passed)
    pub identifier: Option<String>,

    /// Restart every session in the active profile. Useful after
    /// `aoe update`, after editing `sandbox.environment`, after a
    /// Docker hiccup, or after changing a hook. Mutually exclusive
    /// with `identifier`.
    #[arg(long, conflicts_with = "identifier")]
    pub all: bool,

    /// Concurrency cap for `--all`. Restarting many sandboxed
    /// sessions in parallel pressures dockerd, so the default is
    /// intentionally modest. Ignored when `--all` is not set.
    #[arg(long, default_value_t = 3)]
    pub parallel: usize,
}

#[derive(Args)]
pub struct RenameArgs {
    /// Session ID or title (optional, auto-detects in tmux)
    identifier: Option<String>,

    /// New title for the session
    #[arg(short, long)]
    title: Option<String>,

    /// New group for the session (empty string to ungroup)
    #[arg(short, long)]
    group: Option<String>,
}

#[derive(Args)]
pub struct ShowArgs {
    /// Session ID or title (optional, auto-detects in tmux)
    identifier: Option<String>,

    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
pub struct CaptureArgs {
    /// Session ID or title (auto-detects in tmux if omitted)
    identifier: Option<String>,

    /// Number of lines to capture
    #[arg(short = 'n', long, default_value = "50")]
    lines: usize,

    /// Strip ANSI escape codes
    #[arg(long)]
    strip_ansi: bool,

    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Args)]
pub struct CurrentArgs {
    /// Just session name (for scripting)
    #[arg(short = 'q', long)]
    quiet: bool,

    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Serialize)]
struct CaptureOutput {
    id: String,
    title: String,
    status: String,
    tool: String,
    content: String,
    lines: usize,
}

#[derive(Args)]
pub struct SetSessionIdArgs {
    /// Session ID or title
    identifier: String,
    /// Agent session ID to set (pass empty string to clear)
    session_id: String,
}

#[derive(Serialize)]
struct SessionDetails {
    id: String,
    title: String,
    path: String,
    group: String,
    tool: String,
    command: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_session_id: Option<String>,
    profile: String,
}

pub async fn run(profile: &str, command: SessionCommands) -> Result<()> {
    match command {
        SessionCommands::Start(args) => start_session(profile, args).await,
        SessionCommands::Stop(args) => stop_session(profile, args).await,
        SessionCommands::Restart(args) => restart_session_dispatch(profile, args).await,
        SessionCommands::Attach(args) => attach_session(profile, args).await,
        SessionCommands::Show(args) => show_session(profile, args).await,
        SessionCommands::Capture(args) => capture_session(profile, args).await,
        SessionCommands::Rename(args) => rename_session(profile, args).await,
        SessionCommands::Current(args) => current_session(args).await,
        SessionCommands::SetSessionId(args) => set_session_id(profile, args).await,
    }
}

async fn start_session(profile: &str, args: SessionIdArgs) -> Result<()> {
    let storage = Storage::new(profile)?;
    let (mut instances, groups) = storage.load_with_groups()?;

    let idx = instances
        .iter()
        .position(|i| {
            i.id == args.identifier
                || i.id.starts_with(&args.identifier)
                || i.title == args.identifier
        })
        .ok_or_else(|| anyhow::anyhow!("Session not found: {}", args.identifier))?;

    // `source_profile` is runtime-only (skip_serializing) so storage-loaded
    // instances always come back blank; rehydrate it from the storage profile
    // so start-time config resolution honors the right profile's overrides.
    instances[idx].source_profile = profile.to_string();
    instances[idx].start_with_size(crate::terminal::get_size())?;
    let title = instances[idx].title.clone();

    let group_tree = GroupTree::new_with_groups(&instances, &groups);
    storage.save_with_groups(&instances, &group_tree)?;

    println!("✓ Started session: {}", title);
    Ok(())
}

async fn stop_session(profile: &str, args: SessionIdArgs) -> Result<()> {
    let storage = Storage::new(profile)?;
    let (mut instances, groups) = storage.load_with_groups()?;

    let inst = super::resolve_session(&args.identifier, &instances)?;
    let session_id = inst.id.clone();
    let title = inst.title.clone();
    let tmux_session = crate::tmux::Session::new(&inst.id, &inst.title)?;
    let was_running = tmux_session.exists();
    let had_container = inst.is_sandboxed()
        && crate::containers::DockerContainer::from_session_id(&inst.id)
            .is_running()
            .unwrap_or(false);

    if !was_running && !had_container {
        println!("Session is not running: {}", title);
        return Ok(());
    }

    inst.stop()?;

    // Persist Stopped status to disk so it survives TUI restarts
    if let Some(stored) = instances.iter_mut().find(|i| i.id == session_id) {
        stored.status = crate::session::Status::Stopped;
    }
    let group_tree = crate::session::GroupTree::new_with_groups(&instances, &groups);
    storage.save_with_groups(&instances, &group_tree)?;

    if had_container {
        println!("✓ Stopped session and container: {}", title);
    } else {
        println!("✓ Stopped session: {}", title);
    }

    Ok(())
}

async fn restart_session_dispatch(profile: &str, args: RestartArgs) -> Result<()> {
    if args.all {
        return restart_all_sessions(profile, args.parallel).await;
    }
    let identifier = args
        .identifier
        .ok_or_else(|| anyhow::anyhow!("session identifier required (or pass --all)"))?;
    restart_session(profile, SessionIdArgs { identifier }).await
}

async fn restart_all_sessions(profile: &str, parallel: usize) -> Result<()> {
    let storage = Storage::new(profile)?;
    let (mut instances, groups) = storage.load_with_groups()?;

    let target_ids = pick_targets_for_restart_all(&instances);
    if target_ids.is_empty() {
        println!("No sessions to restart in profile '{}'.", profile);
        return Ok(());
    }

    let total = target_ids.len();
    let size = crate::terminal::get_size();
    let parallel = parallel.max(1);

    // Clone each target into its worker; we'll write the (mutated) copy back
    // by index after the worker returns. Workers never touch the shared Vec.
    // `source_profile` is runtime-only (skip_serializing) so storage-loaded
    // instances always come back blank; rehydrate it from the storage profile
    // so start-time config resolution honors the right profile's overrides
    // (sandbox.environment, on_launch hooks, etc.).
    let mut targets: Vec<(usize, crate::session::Instance)> = Vec::with_capacity(total);
    for id in &target_ids {
        if let Some(idx) = instances.iter().position(|i| &i.id == id) {
            let mut clone = instances[idx].clone();
            clone.source_profile = profile.to_string();
            targets.push((idx, clone));
        }
    }

    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(parallel));
    let mut join_set: tokio::task::JoinSet<(
        usize,
        String,
        Option<crate::session::Instance>,
        Result<()>,
    )> = tokio::task::JoinSet::new();

    for (idx, mut inst) in targets {
        let permit_sem = semaphore.clone();
        join_set.spawn(async move {
            let _permit = permit_sem
                .acquire_owned()
                .await
                .expect("semaphore not closed");
            let title = inst.title.clone();
            let res = tokio::task::spawn_blocking(move || {
                let result = inst.restart_with_size(size);
                (inst, result)
            })
            .await;
            match res {
                Ok((inst, result)) => (idx, title, Some(inst), result),
                Err(join_err) => (
                    idx,
                    title,
                    None,
                    Err(anyhow::anyhow!("worker panicked: {}", join_err)),
                ),
            }
        });
    }

    let mut succeeded: Vec<String> = Vec::new();
    let mut failed: Vec<(String, String)> = Vec::new();

    while let Some(joined) = join_set.join_next().await {
        let (idx, title, inst_opt, result) =
            joined.expect("JoinSet shouldn't panic on join itself");
        if let Some(inst) = inst_opt {
            instances[idx] = inst;
        }
        match result {
            Ok(()) => succeeded.push(title),
            Err(e) => failed.push((title, e.to_string())),
        }
    }

    let group_tree = GroupTree::new_with_groups(&instances, &groups);
    storage.save_with_groups(&instances, &group_tree)?;

    println!("✓ Restarted {}/{} sessions:", succeeded.len(), total);
    for title in &succeeded {
        println!("  · {}", title);
    }
    if !failed.is_empty() {
        println!("✗ {} failed:", failed.len());
        for (title, err) in &failed {
            println!("  · {}: {}", title, err);
        }
        bail!("{} session(s) failed to restart", failed.len());
    }

    Ok(())
}

/// Sessions in `Deleting` or `Creating` are mid-transition; restarting them
/// would race the deletion/boot path. Everything else is fair game; agents
/// have their own resume-or-restart logic on the next start.
fn pick_targets_for_restart_all(instances: &[crate::session::Instance]) -> Vec<String> {
    use crate::session::Status;
    instances
        .iter()
        .filter(|i| !matches!(i.status, Status::Deleting | Status::Creating))
        .map(|i| i.id.clone())
        .collect()
}

async fn restart_session(profile: &str, args: SessionIdArgs) -> Result<()> {
    let storage = Storage::new(profile)?;
    let (mut instances, groups) = storage.load_with_groups()?;

    let idx = instances
        .iter()
        .position(|i| {
            i.id == args.identifier
                || i.id.starts_with(&args.identifier)
                || i.title == args.identifier
        })
        .ok_or_else(|| anyhow::anyhow!("Session not found: {}", args.identifier))?;

    // `source_profile` is runtime-only (skip_serializing) so storage-loaded
    // instances always come back blank; rehydrate it from the storage profile
    // so restart-time config resolution honors the right profile's overrides.
    instances[idx].source_profile = profile.to_string();
    instances[idx].restart_with_size(crate::terminal::get_size())?;
    let title = instances[idx].title.clone();

    let group_tree = GroupTree::new_with_groups(&instances, &groups);
    storage.save_with_groups(&instances, &group_tree)?;

    println!("✓ Restarted session: {}", title);
    Ok(())
}

async fn attach_session(profile: &str, args: SessionIdArgs) -> Result<()> {
    let storage = Storage::new(profile)?;
    let (instances, _) = storage.load_with_groups()?;

    let inst = super::resolve_session(&args.identifier, &instances)?;
    let tmux_session = crate::tmux::Session::new(&inst.id, &inst.title)?;

    if !tmux_session.exists() {
        bail!(
            "Session is not running. Start it first with: aoe session start {}",
            args.identifier
        );
    }

    tmux_session.attach()?;
    Ok(())
}

async fn show_session(profile: &str, args: ShowArgs) -> Result<()> {
    let storage = Storage::new(profile)?;
    let (instances, _) = storage.load_with_groups()?;

    let mut inst = if let Some(id) = &args.identifier {
        super::resolve_session(id, &instances)?.clone()
    } else {
        // Auto-detect from tmux
        let current_session = std::env::var("TMUX_PANE")
            .ok()
            .and_then(|_| crate::tmux::get_current_session_name());

        if let Some(session_name) = current_session {
            instances
                .iter()
                .find(|i| {
                    let tmux_name = crate::tmux::Session::generate_name(&i.id, &i.title);
                    tmux_name == session_name
                })
                .ok_or_else(|| {
                    anyhow::anyhow!("Current tmux session is not an Agent of Empires session")
                })?
                .clone()
        } else {
            bail!("Not in a tmux session. Specify a session ID or run inside tmux.");
        }
    };

    // Refresh status from tmux so the output reflects current state
    // rather than the stale persisted value.
    crate::tmux::refresh_session_cache();
    inst.update_status();

    if args.json {
        let details = SessionDetails {
            id: inst.id.clone(),
            title: inst.title.clone(),
            path: inst.project_path.clone(),
            group: inst.group_path.clone(),
            tool: inst.tool.clone(),
            command: inst.command.clone(),
            status: format!("{:?}", inst.status).to_lowercase(),
            parent_session_id: inst.parent_session_id.clone(),
            profile: storage.profile().to_string(),
        };
        super::output::print_json(&details)?;
    } else {
        println!("Session: {}", inst.title);
        println!("  ID:      {}", inst.id);
        println!("  Path:    {}", inst.project_path);
        println!("  Group:   {}", inst.group_path);
        println!("  Tool:    {}", inst.tool);
        println!("  Command: {}", inst.command);
        println!("  Status:  {:?}", inst.status);
        println!("  Profile: {}", storage.profile());
        if let Some(parent_id) = &inst.parent_session_id {
            println!("  Parent:  {}", parent_id);
        }
    }

    Ok(())
}

async fn capture_session(profile: &str, args: CaptureArgs) -> Result<()> {
    let storage = Storage::new(profile)?;
    let (instances, _) = storage.load_with_groups()?;

    let inst = if let Some(id) = &args.identifier {
        super::resolve_session(id, &instances)?
    } else {
        let current_session = std::env::var("TMUX_PANE")
            .ok()
            .and_then(|_| crate::tmux::get_current_session_name());

        if let Some(session_name) = current_session {
            instances
                .iter()
                .find(|i| {
                    let tmux_name = crate::tmux::Session::generate_name(&i.id, &i.title);
                    tmux_name == session_name
                })
                .ok_or_else(|| {
                    anyhow::anyhow!("Current tmux session is not an Agent of Empires session")
                })?
        } else {
            bail!("Not in a tmux session. Specify a session ID or run inside tmux.");
        }
    };

    let tmux_session = crate::tmux::Session::new(&inst.id, &inst.title)?;

    let (content, status) = if !tmux_session.exists() {
        (String::new(), "stopped".to_string())
    } else {
        let raw = tmux_session.capture_pane(args.lines)?;
        let content = if args.strip_ansi {
            crate::tmux::utils::strip_ansi(&raw)
        } else {
            raw
        };
        let status = crate::hooks::read_hook_status(&inst.id)
            .unwrap_or_else(|| tmux_session.detect_status(&inst.tool).unwrap_or_default());
        (content, format!("{:?}", status).to_lowercase())
    };

    if args.json {
        let output = CaptureOutput {
            id: inst.id.clone(),
            title: inst.title.clone(),
            status,
            tool: inst.tool.clone(),
            content,
            lines: args.lines,
        };
        super::output::print_json(&output)?;
    } else {
        print!("{}", content);
    }

    Ok(())
}

async fn rename_session(profile: &str, args: RenameArgs) -> Result<()> {
    if args.title.is_none() && args.group.is_none() {
        bail!("At least one of --title or --group must be specified");
    }

    let storage = Storage::new(profile)?;
    let (mut instances, groups) = storage.load_with_groups()?;

    let inst = if let Some(id) = &args.identifier {
        super::resolve_session(id, &instances)?
    } else {
        // Auto-detect from tmux
        let current_session = std::env::var("TMUX_PANE")
            .ok()
            .and_then(|_| crate::tmux::get_current_session_name());

        if let Some(session_name) = current_session {
            instances
                .iter()
                .find(|i| {
                    let tmux_name = crate::tmux::Session::generate_name(&i.id, &i.title);
                    tmux_name == session_name
                })
                .ok_or_else(|| {
                    anyhow::anyhow!("Current tmux session is not an Agent of Empires session")
                })?
        } else {
            bail!("Not in a tmux session. Specify a session ID or run inside tmux.");
        }
    };

    let id = inst.id.clone();
    let old_title = inst.title.clone();

    let effective_title = args.title.unwrap_or(old_title.clone());
    let effective_title = effective_title.trim().to_string();

    let idx = instances
        .iter()
        .position(|i| i.id == id)
        .ok_or_else(|| anyhow::anyhow!("Session not found"))?;

    // Rename tmux session if title changed
    if instances[idx].title != effective_title {
        let tmux_session = crate::tmux::Session::new(&id, &instances[idx].title)?;
        if tmux_session.exists() {
            let new_tmux_name = crate::tmux::Session::generate_name(&id, &effective_title);
            if let Err(e) = tmux_session.rename(&new_tmux_name) {
                eprintln!("Warning: failed to rename tmux session: {}", e);
            } else {
                crate::tmux::refresh_session_cache();
            }
        }
    }

    instances[idx].title = effective_title.clone();

    if let Some(group) = args.group {
        instances[idx].group_path = group.trim().to_string();
    }

    let mut group_tree = GroupTree::new_with_groups(&instances, &groups);
    if !instances[idx].group_path.is_empty() {
        group_tree.create_group(&instances[idx].group_path);
    }
    storage.save_with_groups(&instances, &group_tree)?;

    if old_title != effective_title {
        println!("✓ Renamed session: {} → {}", old_title, effective_title);
    } else {
        println!("✓ Updated session: {}", effective_title);
    }

    Ok(())
}

async fn current_session(args: CurrentArgs) -> Result<()> {
    // Auto-detect profile and session from tmux
    let current_session = std::env::var("TMUX_PANE")
        .ok()
        .and_then(|_| crate::tmux::get_current_session_name());

    let session_name = current_session.ok_or_else(|| anyhow::anyhow!("Not in a tmux session"))?;

    // Search all profiles for this session
    let profiles = crate::session::list_profiles()?;

    for profile_name in &profiles {
        if let Ok(storage) = Storage::new(profile_name) {
            if let Ok((instances, _)) = storage.load_with_groups() {
                if let Some(inst) = instances.iter().find(|i| {
                    let tmux_name = crate::tmux::Session::generate_name(&i.id, &i.title);
                    tmux_name == session_name
                }) {
                    if args.json {
                        #[derive(Serialize)]
                        struct CurrentInfo {
                            session: String,
                            profile: String,
                            id: String,
                        }
                        let info = CurrentInfo {
                            session: inst.title.clone(),
                            profile: profile_name.clone(),
                            id: inst.id.clone(),
                        };
                        super::output::print_json(&info)?;
                    } else if args.quiet {
                        println!("{}", inst.title);
                    } else {
                        println!("Session: {}", inst.title);
                        println!("Profile: {}", profile_name);
                        println!("ID:      {}", inst.id);
                    }
                    return Ok(());
                }
            }
        }
    }

    bail!("Current tmux session is not an Agent of Empires session")
}

async fn set_session_id(profile: &str, args: SetSessionIdArgs) -> Result<()> {
    let storage = Storage::new(profile)?;
    let (mut instances, groups) = storage.load_with_groups()?;

    let idx = instances
        .iter()
        .position(|i| {
            i.id == args.identifier
                || i.id.starts_with(&args.identifier)
                || i.title == args.identifier
        })
        .ok_or_else(|| anyhow::anyhow!("Session not found: {}", args.identifier))?;

    let new_id = if args.session_id.trim().is_empty() {
        None
    } else {
        let trimmed = args.session_id.trim().to_string();
        if !crate::session::is_valid_session_id(&trimmed) {
            bail!(
                "Invalid session ID {:?}: must be 1-256 ASCII alphanumeric, dash, underscore, or dot characters",
                trimmed
            );
        }
        Some(trimmed)
    };

    instances[idx].agent_session_id = new_id.clone();
    let title = instances[idx].title.clone();

    let group_tree = GroupTree::new_with_groups(&instances, &groups);
    storage.save_with_groups(&instances, &group_tree)?;

    match new_id {
        Some(ref id) => {
            println!("✓ Set session ID for '{}': {}", title, id);
            let tool = &instances[idx].tool;
            if let Some(agent) = crate::agents::get_agent(tool) {
                if matches!(
                    agent.resume_strategy,
                    crate::agents::ResumeStrategy::Unsupported
                ) {
                    eprintln!("Warning: {} does not support session resume; this ID will be stored but not used.", tool);
                }
            }
        }
        None => println!("✓ Cleared session ID for '{}'", title),
    }
    Ok(())
}

#[cfg(test)]
mod restart_args_tests {
    use super::SessionCommands;
    use clap::Parser;

    #[derive(Parser)]
    struct Cli {
        #[command(subcommand)]
        cmd: SessionCommands,
    }

    #[test]
    fn restart_with_identifier_still_parses() {
        let cli = Cli::try_parse_from(["aoe", "restart", "claude-3"])
            .expect("identifier-only must parse");
        match cli.cmd {
            SessionCommands::Restart(args) => {
                assert!(!args.all);
                assert_eq!(args.identifier.as_deref(), Some("claude-3"));
                assert_eq!(args.parallel, 3);
            }
            _ => panic!("wrong subcommand"),
        }
    }

    #[test]
    fn restart_all_alone_parses() {
        let cli = Cli::try_parse_from(["aoe", "restart", "--all"]).expect("--all alone must parse");
        match cli.cmd {
            SessionCommands::Restart(args) => {
                assert!(args.all);
                assert!(args.identifier.is_none());
                assert_eq!(args.parallel, 3);
            }
            _ => panic!("wrong subcommand"),
        }
    }

    #[test]
    fn restart_all_with_parallel_parses() {
        let cli = Cli::try_parse_from(["aoe", "restart", "--all", "--parallel", "5"])
            .expect("--all --parallel must parse");
        match cli.cmd {
            SessionCommands::Restart(args) => {
                assert!(args.all);
                assert_eq!(args.parallel, 5);
            }
            _ => panic!("wrong subcommand"),
        }
    }

    #[test]
    fn restart_identifier_and_all_conflicts() {
        let result = Cli::try_parse_from(["aoe", "restart", "claude-3", "--all"]);
        assert!(
            result.is_err(),
            "passing both identifier and --all should error"
        );
    }
}

#[cfg(test)]
mod target_filter_tests {
    use super::pick_targets_for_restart_all;
    use crate::session::{Instance, Status};

    fn instance_with_status(id: &str, status: Status) -> Instance {
        let mut inst = Instance::new(id, "/tmp");
        inst.id = id.to_string();
        inst.status = status;
        inst
    }

    #[test]
    fn skips_deleting_and_creating() {
        let instances = vec![
            instance_with_status("running", Status::Running),
            instance_with_status("idle", Status::Idle),
            instance_with_status("stopped", Status::Stopped),
            instance_with_status("error", Status::Error),
            instance_with_status("waiting", Status::Waiting),
            instance_with_status("starting", Status::Starting),
            instance_with_status("unknown", Status::Unknown),
            instance_with_status("deleting", Status::Deleting),
            instance_with_status("creating", Status::Creating),
        ];
        let mut picked = pick_targets_for_restart_all(&instances);
        picked.sort();
        let mut expected = vec![
            "error".to_string(),
            "idle".to_string(),
            "running".to_string(),
            "starting".to_string(),
            "stopped".to_string(),
            "unknown".to_string(),
            "waiting".to_string(),
        ];
        expected.sort();
        assert_eq!(picked, expected);
    }

    #[test]
    fn empty_input_yields_empty_targets() {
        assert!(pick_targets_for_restart_all(&[]).is_empty());
    }
}
