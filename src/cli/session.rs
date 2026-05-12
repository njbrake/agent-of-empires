//! `agent-of-empires session` subcommands implementation

use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use serde::Serialize;

use crate::session::{GroupTree, Instance, Storage};

/// Refuse to act on archived sessions. Used by `restart` to honor the
/// "archived = sunk; do not touch without explicit unarchive" invariant.
/// Data-proven failure mode: without this guard, cx account-swap teardown
/// and aoe-restart-all.sh both iterate every live tmux session and call
/// `aoe session restart $title` for each — archived sessions were being
/// restarted (and sent a wake-up prompt) despite the user sinking them.
fn ensure_not_archived(inst: &Instance, identifier: &str) -> Result<()> {
    if inst.archived_at.is_some() {
        bail!(
            "Session '{}' is archived — refusing to restart. Run `aoe session unarchive {}` first.",
            inst.title,
            identifier
        );
    }
    Ok(())
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

    /// Archive a session (sinks it to the bottom of the Attention sort,
    /// rendered in italic+dim; remains visible). Default kills the tmux
    /// pane process so an archived session stops consuming resources;
    /// pass `--no-kill` to opt out and keep the pane running.
    Archive(ArchiveArgs),

    /// Unarchive a session (clears archived_at).
    Unarchive(SessionIdArgs),

    /// Favorite a session. While favorited AND in a "needs help" status
    /// (Waiting, Error, Idle, Unknown), it pins to the top of the Attention
    /// sort above all non-favorited peers. Rendered bold + underlined with
    /// a "* " prefix (ASCII, no emoji — avoids wide-width rendering
    /// artifacts on narrow iOS terminals). Opposite of archive.
    Favorite(SessionIdArgs),

    /// Unfavorite a session (clears favorited_at).
    Unfavorite(SessionIdArgs),

    /// Snooze a session (temporary archive). Sinks it to the bottom of
    /// the Attention sort and renders it italic+dim with a `z ` prefix
    /// and a remaining-time readout. Wakes automatically when the timer
    /// expires. Default duration is the profile's
    /// `session.snooze_duration_minutes` (default 30); override per-call
    /// with `--minutes`.
    Snooze(SnoozeArgs),

    /// Unsnooze a session (clears snoozed_until — wakes it immediately).
    Unsnooze(SessionIdArgs),

    /// Set agent session ID for a session
    SetSessionId(SetSessionIdArgs),
}

#[derive(Args)]
pub struct SessionIdArgs {
    /// Session ID or title
    identifier: String,
}

#[derive(Args)]
pub struct SnoozeArgs {
    /// Session ID or title
    identifier: String,

    /// Override snooze duration in minutes (1-1440). If omitted, uses the
    /// profile's configured `session.snooze_duration_minutes` (default 30).
    #[arg(short = 'm', long)]
    minutes: Option<u64>,
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
pub struct ArchiveArgs {
    /// Session ID or title
    pub identifier: String,
    /// Sink-only: skip killing the tmux pane on archive. Default behavior
    /// is to terminate the agent process (the pane stays as a remain-on-exit
    /// corpse) so an archived session stops consuming resources. Pass
    /// `--no-kill` for the rare case where you want the pane to keep
    /// running while sunk in the Attention sort.
    #[arg(long)]
    pub no_kill: bool,
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
        SessionCommands::Archive(args) => archive_session(profile, args).await,
        SessionCommands::Unarchive(args) => unarchive_session(profile, args).await,
        SessionCommands::Favorite(args) => set_session_favorited(profile, args, true).await,
        SessionCommands::Unfavorite(args) => set_session_favorited(profile, args, false).await,
        SessionCommands::Snooze(args) => snooze_session(profile, args).await,
        SessionCommands::Unsnooze(args) => unsnooze_session(profile, args).await,
        SessionCommands::SetSessionId(args) => set_session_id(profile, args).await,
    }
}

async fn snooze_session(profile: &str, args: SnoozeArgs) -> Result<()> {
    let storage = Storage::new(profile)?;
    let (mut instances, groups) = storage.load_with_groups()?;

    let config = crate::session::profile_config::resolve_config(profile)?;

    // `--minutes` overrides the profile default; otherwise use the
    // configured `snooze_duration_minutes`. Validate either way so the
    // on-disk config can't sneak in an out-of-range value.
    let raw_minutes = args
        .minutes
        .unwrap_or(config.session.snooze_duration_minutes as u64);
    crate::session::validate_snooze_duration(raw_minutes).map_err(|e| anyhow::anyhow!("{}", e))?;
    let minutes = raw_minutes as u32;

    let idx = instances
        .iter()
        .position(|i| {
            i.id == args.identifier
                || i.id.starts_with(&args.identifier)
                || i.title == args.identifier
        })
        .ok_or_else(|| anyhow::anyhow!("Session not found: {}", args.identifier))?;

    instances[idx].snooze(minutes);
    let title = instances[idx].title.clone();

    let group_tree = GroupTree::new_with_groups(&instances, &groups);
    storage.save_with_groups(&instances, &group_tree)?;

    println!("✓ Snoozed for {}m: {}", minutes, title);
    Ok(())
}

async fn unsnooze_session(profile: &str, args: SessionIdArgs) -> Result<()> {
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

    instances[idx].unsnooze();
    let title = instances[idx].title.clone();

    let group_tree = GroupTree::new_with_groups(&instances, &groups);
    storage.save_with_groups(&instances, &group_tree)?;

    println!("✓ Woke: {}", title);
    Ok(())
}

async fn set_session_favorited(profile: &str, args: SessionIdArgs, favorited: bool) -> Result<()> {
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

    if favorited {
        instances[idx].favorite();
    } else {
        instances[idx].unfavorite();
    }
    let title = instances[idx].title.clone();

    let group_tree = GroupTree::new_with_groups(&instances, &groups);
    storage.save_with_groups(&instances, &group_tree)?;

    let verb = if favorited {
        "Favorited"
    } else {
        "Unfavorited"
    };
    println!("✓ {} session: {}", verb, title);
    Ok(())
}

async fn archive_session(profile: &str, args: ArchiveArgs) -> Result<()> {
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

    let was_archived = instances[idx].is_archived();
    instances[idx].archive();
    let title = instances[idx].title.clone();
    let session_id = instances[idx].id.clone();

    // None → Some transition: kill the tmux pane process so the agent
    // stops consuming resources. With remain-on-exit on, the pane stays
    // around as a corpse and `aoe session unarchive` (Piece 2 below) or
    // `aoe session restart` (Piece 1) will respawn it.
    //
    // Already-archived → archive is a no-op for the kill: pane was
    // already killed on the first archive.
    if !was_archived && !args.no_kill {
        let tmux_session = crate::tmux::Session::new(&session_id, &title)?;
        if tmux_session.exists() {
            if let Err(e) = tmux_session.kill_pane() {
                eprintln!(
                    "Warning: failed to kill pane for archived session '{}': {}",
                    title, e
                );
            }
        }
    }

    let group_tree = GroupTree::new_with_groups(&instances, &groups);
    storage.save_with_groups(&instances, &group_tree)?;

    println!("✓ Archived session: {}", title);
    Ok(())
}

async fn unarchive_session(profile: &str, args: SessionIdArgs) -> Result<()> {
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

    let was_archived = instances[idx].is_archived();
    instances[idx].unarchive();
    let title = instances[idx].title.clone();
    let session_id = instances[idx].id.clone();
    let tool = instances[idx].tool.clone();

    // Some → None transition: respawn the pane and send the resume
    // message. Mirrors restart_session's pane_dead branch (Piece 1):
    // the on-archive kill leaves a remain-on-exit corpse, so unarchive
    // is the inverse — revive it. Cwd recovery uses the same three-step
    // chain (sidecar → session-name → $HOME).
    if was_archived {
        let tmux_session = crate::tmux::Session::new(&session_id, &title)?;
        if tmux_session.exists() && tmux_session.is_pane_dead() {
            let cwd = recover_cwd_for_session(&tmux_session, &title);
            tmux_session.respawn_dead_pane(&cwd, Some("zsh"))?;
            tmux_session.wait_for_shell_prompt(std::time::Duration::from_secs(5))?;
            let delay = crate::agents::send_keys_enter_delay(&tool);
            let resume_msg = "wake up — pick up what you were doing";
            if let Err(e) = tmux_session.send_keys_with_delay(resume_msg, delay) {
                eprintln!("Warning: failed to send resume message: {}", e);
            }
        }
    }

    let group_tree = GroupTree::new_with_groups(&instances, &groups);
    storage.save_with_groups(&instances, &group_tree)?;

    println!("✓ Unarchived session: {}", title);
    Ok(())
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
    storage.save_with_groups(&instances, &group_tree)?;

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
    ensure_not_archived(&instances[idx], &args.identifier)?;

    instances[idx].restart_with_size(crate::terminal::get_size())?;
    let title = instances[idx].title.clone();
    let session_id = instances[idx].id.clone();
    let tool = instances[idx].tool.clone();

    // Wait for the agent CLI to render its prompt before injecting input.
    // Without this, keystrokes land in the shell before claude/opencode
    // takes over the TTY and get lost.
    std::thread::sleep(std::time::Duration::from_millis(2000));

    let tmux_session = crate::tmux::Session::new(&session_id, &title)?;
    if tmux_session.exists() {
        // remain-on-exit panes survive their child process and tmux reports
        // them as existing-but-dead. The naive send-keys path would target a
        // corpse and silently no-op. Respawn-pane brings the pane back via
        // zsh; the wake message that follows lands in a live shell.
        if tmux_session.is_pane_dead() {
            let cwd = recover_cwd_for_session(&tmux_session, &title);
            tmux_session.respawn_dead_pane(&cwd, Some("zsh"))?;
            tmux_session.wait_for_shell_prompt(std::time::Duration::from_secs(5))?;
        }
        let delay = crate::agents::send_keys_enter_delay(&tool);
        let wake_msg = "wake up — pick up what you were doing";
        match tmux_session.send_keys_with_delay(wake_msg, delay) {
            Ok(()) => {
                if let Some(inst) = instances.iter_mut().find(|i| i.id == session_id) {
                    inst.touch_last_accessed();
                }
            }
            Err(e) => {
                eprintln!("Warning: failed to send wake-up message: {}", e);
            }
        }
    }

    let group_tree = GroupTree::new_with_groups(&instances, &groups);
    storage.save_with_groups(&instances, &group_tree)?;

    println!("✓ Restarted session: {}", title);
    Ok(())
}

/// Recover a usable cwd for a session whose pane was respawned. Mirrors
/// cx-revive's three-step lookup so the on-revive UX matches whether the
/// pane was respawned by `cxr` from the shell or `aoe session restart`
/// from inside aoe.
///
/// Order: pane sidecar at /tmp/cx-panes/<pane_id> (cxr's SessionStart
/// hook writes `<sid> <cfg> <cwd>` per launch) → ~/GitProjects/<project>
/// derived from the tmux session name (`aoe_<project>_<8hex>`) → $HOME
/// with a stderr warning. Always returns something usable rather than
/// erroring, so the respawn keeps making progress on edge cases.
fn recover_cwd_for_session(tmux_session: &crate::tmux::Session, title: &str) -> String {
    use std::path::Path;

    if let Some(pane_id) = tmux_session.pane_id() {
        let sidecar = format!("/tmp/cx-panes/{}", pane_id);
        if let Ok(content) = std::fs::read_to_string(&sidecar) {
            if let Some(last) = content.lines().rev().find(|l| !l.trim().is_empty()) {
                let parts: Vec<&str> = last.splitn(3, ' ').collect();
                if parts.len() >= 3 {
                    let cwd = parts[2].trim();
                    if !cwd.is_empty() && Path::new(cwd).is_dir() {
                        return cwd.to_string();
                    }
                }
            }
        }
    }

    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());

    // session-name fallback: aoe_<project>_<8hex> → ~/GitProjects/<project>
    let session_name = tmux_session.name();
    if let Some(rest) = session_name.strip_prefix("aoe_") {
        if let Some((project, _id)) = rest.rsplit_once('_') {
            let project_path = format!("{}/GitProjects/{}", home, project);
            if Path::new(&project_path).is_dir() {
                return project_path;
            }
        }
    }

    // title-derived fallback (rare: when session_name doesn't parse, e.g.
    // legacy or hand-renamed sessions).
    let title_path = format!("{}/GitProjects/{}", home, title);
    if Path::new(&title_path).is_dir() {
        return title_path;
    }

    eprintln!(
        "Warning: could not recover cwd for session '{}', falling back to $HOME",
        title
    );
    home
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
mod tests {
    use super::*;

    #[test]
    fn ensure_not_archived_allows_live_session() {
        let inst = Instance::new("forit-Avatics", "/tmp/x");
        assert!(ensure_not_archived(&inst, "forit-Avatics").is_ok());
    }

    #[test]
    fn ensure_not_archived_refuses_archived_session() {
        let mut inst = Instance::new("forit-Avatics", "/tmp/x");
        inst.archive();
        let err = ensure_not_archived(&inst, "forit-Avatics").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("archived"), "expected 'archived' in: {msg}");
        assert!(msg.contains("forit-Avatics"), "expected title in: {msg}");
        assert!(
            msg.contains("unarchive"),
            "expected recovery hint in: {msg}"
        );
    }

    #[test]
    fn ensure_not_archived_allows_after_unarchive() {
        let mut inst = Instance::new("forit-Avatics", "/tmp/x");
        inst.archive();
        inst.unarchive();
        assert!(ensure_not_archived(&inst, "forit-Avatics").is_ok());
    }
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
