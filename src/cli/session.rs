//! `agent-of-empires session` subcommands implementation

use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use serde::Serialize;

use crate::session::{GroupTree, StartOutcome, Storage};

/// Wording used by both single-session and `--all` restart paths when the
/// resume-fallback cascade cleared a stale agent_session_id. Centralized so
/// drift between the two surfaces cannot happen.
pub(crate) fn stale_history_suffix(stale_sid: &str) -> String {
    format!(" (resume failed for sid {stale_sid}; started fresh, prior history not loaded)")
}

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

    /// Set or clear the per-session diff base branch. The diff view
    /// compares the worktree against this ref instead of the
    /// auto-detected default. Useful when the PR target differs from
    /// the project default (stacked PRs, hotfix off `release/*`,
    /// renamed default branch). See #970.
    SetBase(SetBaseArgs),
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

#[derive(Args)]
pub struct SetBaseArgs {
    /// Session ID or title
    pub identifier: String,
    /// Branch ref to diff against (short name like `main` or
    /// remote-qualified like `upstream/main`). Required unless
    /// `--clear` is passed.
    pub branch: Option<String>,
    /// Clear the override and fall back to the profile default /
    /// auto-detected base.
    #[arg(long, conflicts_with = "branch")]
    pub clear: bool,
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

#[tracing::instrument(target = "cli.session", skip_all, fields(profile = %profile))]
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
        SessionCommands::SetBase(args) => set_base(profile, args).await,
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
    bail_if_cockpit(&instances[idx], "start")?;
    instances[idx].start_with_size(crate::terminal::get_size())?;
    let title = instances[idx].title.clone();

    let group_tree = GroupTree::new_with_groups(&instances, &groups);
    storage.commit(&instances, &group_tree)?;

    println!("✓ Started session: {}", title);
    Ok(())
}

/// Cockpit-mode sessions are not backed by tmux; their ACP worker is owned
/// by `aoe serve`'s supervisor (auto-spawned by the reconciler within ~2s
/// of the session appearing on disk). Calling `start`/`stop`/`restart`
/// from the CLI silently no-ops, which previously misled users into
/// thinking the session was up. Bail loudly with the actual remediation.
///
/// `cockpit_mode` is gated behind the `serve` feature; without it the
/// field doesn't exist on `Instance` and no session can be in cockpit
/// mode, so this is a no-op shim.
#[cfg(feature = "serve")]
fn bail_if_cockpit(inst: &crate::session::Instance, verb: &str) -> Result<()> {
    if inst.cockpit_mode {
        bail!(
            "cockpit sessions are managed by `aoe serve`; \
             cannot `aoe session {verb}` from the CLI.\n\
             The ACP worker is auto-spawned within ~2s of `aoe add --cockpit` \
             while serve is running, or on next `aoe serve` startup.\n\
             To control a cockpit session, use the web dashboard or the REST API."
        );
    }
    Ok(())
}

#[cfg(not(feature = "serve"))]
fn bail_if_cockpit(_inst: &crate::session::Instance, _verb: &str) -> Result<()> {
    Ok(())
}

async fn stop_session(profile: &str, args: SessionIdArgs) -> Result<()> {
    let storage = Storage::new(profile)?;
    let (mut instances, groups) = storage.load_with_groups()?;

    let inst = super::resolve_session(&args.identifier, &instances)?;
    bail_if_cockpit(inst, "stop")?;
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
    storage.commit(&instances, &group_tree)?;

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
        Result<StartOutcome>,
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

    let mut succeeded: Vec<(String, Option<String>)> = Vec::new();
    let mut failed: Vec<(String, String)> = Vec::new();
    while let Some(joined) = join_set.join_next().await {
        let (idx, title, inst_opt, result) =
            joined.expect("JoinSet shouldn't panic on join itself");
        if let Some(inst) = inst_opt {
            instances[idx] = inst;
        }
        match result {
            Ok(StartOutcome::Restarted { stale_sid }) => succeeded.push((title, Some(stale_sid))),
            Ok(StartOutcome::Resumed | StartOutcome::Fresh) => succeeded.push((title, None)),
            Err(e) => failed.push((title, e.to_string())),
        }
    }

    let group_tree = GroupTree::new_with_groups(&instances, &groups);
    storage.commit(&instances, &group_tree)?;

    let stale_count = succeeded.iter().filter(|(_, s)| s.is_some()).count();
    if stale_count == 0 {
        println!("✓ Restarted {}/{} sessions:", succeeded.len(), total);
    } else {
        println!(
            "✓ Restarted {}/{} sessions ({} without prior history):",
            succeeded.len(),
            total,
            stale_count,
        );
    }
    for (title, stale) in &succeeded {
        match stale {
            Some(sid) => println!("  · {}{}", title, stale_history_suffix(sid)),
            None => println!("  · {}", title),
        }
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
/// would race the deletion/boot path. Cockpit-mode sessions are skipped
/// because their lifecycle is owned by `aoe serve`'s supervisor, not
/// tmux: a CLI-side restart would no-op silently and (with the explicit
/// bail in `restart_session`) flood `--all` with per-session errors.
/// Everything else is fair game; agents have their own resume-or-restart
/// logic on the next start.
fn pick_targets_for_restart_all(instances: &[crate::session::Instance]) -> Vec<String> {
    use crate::session::Status;
    instances
        .iter()
        .filter(|i| !matches!(i.status, Status::Deleting | Status::Creating))
        .filter(|_i| {
            #[cfg(feature = "serve")]
            {
                !_i.cockpit_mode
            }
            #[cfg(not(feature = "serve"))]
            {
                true
            }
        })
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
    bail_if_cockpit(&instances[idx], "restart")?;
    let outcome = instances[idx].restart_with_size(crate::terminal::get_size())?;
    let title = instances[idx].title.clone();

    let group_tree = GroupTree::new_with_groups(&instances, &groups);
    storage.commit(&instances, &group_tree)?;

    match outcome {
        StartOutcome::Restarted { stale_sid } => {
            println!(
                "✓ Restarted session: {}{}",
                title,
                stale_history_suffix(&stale_sid),
            );
        }
        StartOutcome::Resumed | StartOutcome::Fresh => {
            println!("✓ Restarted session: {}", title);
        }
    }
    Ok(())
}

async fn attach_session(profile: &str, args: SessionIdArgs) -> Result<()> {
    let storage = Storage::new(profile)?;
    let (instances, _) = storage.load_with_groups()?;

    let inst = super::resolve_session(&args.identifier, &instances)?;
    bail_if_cockpit(inst, "attach")?;
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
        let detection_tool = if inst.detect_as.is_empty() {
            &inst.tool
        } else {
            &inst.detect_as
        };
        let status = if let Some(hook_status) = crate::hooks::read_hook_status(&inst.id) {
            if detection_tool == "codex" && hook_status == crate::session::Status::Running {
                let status_raw;
                let status_content = if args.lines >= 50 {
                    raw.as_str()
                } else {
                    status_raw = tmux_session
                        .capture_pane(50)
                        .unwrap_or_else(|_| raw.clone());
                    status_raw.as_str()
                };
                crate::tmux::reconcile_codex_hook_status(hook_status, status_content)
            } else {
                hook_status
            }
        } else {
            tmux_session
                .detect_status(detection_tool)
                .unwrap_or_default()
        };
        let content = if args.strip_ansi {
            crate::tmux::utils::strip_ansi(&raw)
        } else {
            raw
        };
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
    storage.commit(&instances, &group_tree)?;

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
    storage.commit(&instances, &group_tree)?;

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

async fn set_base(profile: &str, args: SetBaseArgs) -> Result<()> {
    if !args.clear && args.branch.is_none() {
        bail!("Provide a branch ref or pass --clear to remove the override.");
    }
    let storage = Storage::new(profile)?;
    let instances = storage.load()?;

    let idx = instances
        .iter()
        .position(|i| {
            i.id == args.identifier
                || i.id.starts_with(&args.identifier)
                || i.title == args.identifier
        })
        .ok_or_else(|| anyhow::anyhow!("Session not found: {}", args.identifier))?;

    let new_value = if args.clear {
        None
    } else {
        let trimmed = args.branch.as_deref().unwrap_or("").trim().to_string();
        if trimmed.is_empty() {
            bail!("Branch name is empty. Pass --clear to remove the override.");
        }
        // Validate the ref against the same resolution chain the diff
        // resolver uses, so users see a clear error at set-time rather
        // than a silent fallback when the diff is next computed. For
        // workspace sessions, validate against the first repo's
        // worktree (each repo will resolve the ref the same way).
        let validate_path = instances[idx]
            .workspace_info
            .as_ref()
            .and_then(|w| w.repos.first().map(|r| r.worktree_path.clone()))
            .unwrap_or_else(|| instances[idx].project_path.clone());
        if let Err(e) =
            crate::git::diff::validate_ref(std::path::Path::new(&validate_path), &trimmed)
        {
            bail!(
                "Branch '{}' does not resolve in {}: {}",
                trimmed,
                validate_path,
                e
            );
        }
        Some(trimmed)
    };

    let title = instances[idx].title.clone();
    let id_for_save = instances[idx].id.clone();

    storage.update(|instances, _groups| {
        let inst = instances
            .iter_mut()
            .find(|i| i.id == id_for_save)
            .ok_or_else(|| anyhow::anyhow!("Session not found: {}", args.identifier))?;
        inst.base_branch_override = new_value.clone();
        Ok(())
    })?;

    match new_value {
        Some(ref v) => println!("✓ Set diff base for '{}': {}", title, v),
        None => println!("✓ Cleared diff base override for '{}'", title),
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

    #[test]
    fn set_base_with_branch_parses() {
        let cli = Cli::try_parse_from(["aoe", "set-base", "claude-3", "upstream/main"])
            .expect("set-base with branch must parse");
        match cli.cmd {
            SessionCommands::SetBase(args) => {
                assert_eq!(args.identifier, "claude-3");
                assert_eq!(args.branch.as_deref(), Some("upstream/main"));
                assert!(!args.clear);
            }
            _ => panic!("wrong subcommand"),
        }
    }

    #[test]
    fn set_base_with_clear_parses() {
        let cli = Cli::try_parse_from(["aoe", "set-base", "claude-3", "--clear"])
            .expect("set-base --clear must parse");
        match cli.cmd {
            SessionCommands::SetBase(args) => {
                assert_eq!(args.identifier, "claude-3");
                assert!(args.branch.is_none());
                assert!(args.clear);
            }
            _ => panic!("wrong subcommand"),
        }
    }

    #[test]
    fn set_base_branch_and_clear_conflicts() {
        let result = Cli::try_parse_from(["aoe", "set-base", "claude-3", "main", "--clear"]);
        assert!(
            result.is_err(),
            "passing both branch and --clear should error"
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

#[cfg(test)]
mod stale_history_suffix_tests {
    use super::stale_history_suffix;

    #[test]
    fn matches_single_session_wording() {
        let suffix = stale_history_suffix("11111111-1111-1111-1111-111111111111");
        assert_eq!(
            suffix,
            " (resume failed for sid 11111111-1111-1111-1111-111111111111; \
             started fresh, prior history not loaded)"
        );
    }

    #[test]
    fn renders_inline_with_title_correctly() {
        let line = format!(
            "  · {}{}",
            "alpha",
            stale_history_suffix("22222222-2222-2222-2222-222222222222"),
        );
        assert_eq!(
            line,
            "  · alpha (resume failed for sid 22222222-2222-2222-2222-222222222222; \
             started fresh, prior history not loaded)"
        );
    }
}
