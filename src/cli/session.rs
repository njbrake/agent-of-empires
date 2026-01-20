//! `agent-of-empires session` subcommands implementation

use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use crossterm::terminal;
use serde::Serialize;

use crate::session::{GroupTree, Storage};

/// Get current terminal size for tmux session creation.
/// This ensures sessions start at the correct size instead of the 80x24 default.
fn get_terminal_size() -> Option<(u16, u16)> {
    terminal::size().ok()
}

#[derive(Subcommand)]
pub enum SessionCommands {
    /// Start a session's tmux process
    Start(SessionIdArgs),

    /// Stop session process
    Stop(SessionIdArgs),

    /// Restart session
    Restart(SessionIdArgs),

    /// Fork Claude session with context
    Fork(ForkArgs),

    /// Attach to session interactively
    Attach(SessionIdArgs),

    /// Show session details
    Show(ShowArgs),

    /// Auto-detect current session
    Current(CurrentArgs),
}

#[derive(Args)]
pub struct SessionIdArgs {
    /// Session ID or title
    identifier: String,
}

#[derive(Args)]
pub struct ForkArgs {
    /// Session ID or title to fork
    identifier: String,

    /// Custom title for forked session
    #[arg(short = 't', long)]
    title: Option<String>,

    /// Target group for forked session
    #[arg(short = 'g', long)]
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
pub struct CurrentArgs {
    /// Just session name (for scripting)
    #[arg(short = 'q', long)]
    quiet: bool,

    /// Output as JSON
    #[arg(long)]
    json: bool,
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
    claude_session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    parent_session_id: Option<String>,
    profile: String,
}

pub async fn run(profile: &str, command: SessionCommands) -> Result<()> {
    match command {
        SessionCommands::Start(args) => start_session(profile, args).await,
        SessionCommands::Stop(args) => stop_session(profile, args).await,
        SessionCommands::Restart(args) => restart_session(profile, args).await,
        SessionCommands::Fork(args) => fork_session(profile, args).await,
        SessionCommands::Attach(args) => attach_session(profile, args).await,
        SessionCommands::Show(args) => show_session(profile, args).await,
        SessionCommands::Current(args) => current_session(args).await,
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

    instances[idx].start_with_size(get_terminal_size())?;
    let title = instances[idx].title.clone();

    let group_tree = GroupTree::new_with_groups(&instances, &groups);
    storage.save_with_groups(&instances, &group_tree)?;

    println!("✓ Started session: {}", title);
    Ok(())
}

async fn stop_session(profile: &str, args: SessionIdArgs) -> Result<()> {
    let storage = Storage::new(profile)?;
    let (instances, _) = storage.load_with_groups()?;

    let inst = super::resolve_session(&args.identifier, &instances)?;
    let tmux_session = crate::tmux::Session::new(&inst.id, &inst.title)?;

    if tmux_session.exists() {
        tmux_session.kill()?;
        println!("✓ Stopped session: {}", inst.title);
    } else {
        println!("Session is not running: {}", inst.title);
    }

    Ok(())
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

    instances[idx].restart_with_size(get_terminal_size())?;
    let title = instances[idx].title.clone();

    let group_tree = GroupTree::new_with_groups(&instances, &groups);
    storage.save_with_groups(&instances, &group_tree)?;

    println!("✓ Restarted session: {}", title);
    Ok(())
}

async fn fork_session(profile: &str, args: ForkArgs) -> Result<()> {
    let storage = Storage::new(profile)?;
    let (mut instances, groups) = storage.load_with_groups()?;

    let source_inst = super::resolve_session(&args.identifier, &instances)?;

    if source_inst.tool != "claude" {
        bail!("Fork is only supported for Claude sessions");
    }

    if source_inst.claude_session_id.is_none() {
        bail!("No Claude session ID detected. Start the session first and wait for Claude to initialize.");
    }

    // Clone source data we need before mutating instances
    let source_title = source_inst.title.clone();
    let source_id = source_inst.id.clone();
    let source_project_path = source_inst.project_path.clone();
    let source_group_path = source_inst.group_path.clone();

    let fork_title = args.title.unwrap_or_else(|| {
        let base = format!("{} (fork)", source_title);
        super::add::generate_unique_title(&instances, &base, &source_project_path)
    });

    let fork_group = args.group.unwrap_or_else(|| source_group_path.clone());

    let forked = source_inst.fork(&fork_title, &fork_group)?;
    let forked_id = forked.id.clone();
    instances.push(forked);

    let mut group_tree = GroupTree::new_with_groups(&instances, &groups);
    if !fork_group.is_empty() {
        group_tree.create_group(&fork_group);
    }

    storage.save_with_groups(&instances, &group_tree)?;

    println!("✓ Forked session: {}", fork_title);
    println!("  Source:  {} ({})", source_title, &source_id[..8]);
    println!("  ID:      {}", forked_id);
    println!("  Group:   {}", fork_group);

    Ok(())
}

async fn attach_session(profile: &str, args: SessionIdArgs) -> Result<()> {
    let storage = Storage::new(profile)?;
    let (instances, _) = storage.load_with_groups()?;

    let inst = super::resolve_session(&args.identifier, &instances)?;
    let tmux_session = crate::tmux::Session::new(&inst.id, &inst.title)?;

    if !tmux_session.exists() {
        bail!(
            "Session is not running. Start it first with: agent-of-empires session start {}",
            args.identifier
        );
    }

    tmux_session.attach()?;
    Ok(())
}

async fn show_session(profile: &str, args: ShowArgs) -> Result<()> {
    let storage = Storage::new(profile)?;
    let (instances, _) = storage.load_with_groups()?;

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

    if args.json {
        let details = SessionDetails {
            id: inst.id.clone(),
            title: inst.title.clone(),
            path: inst.project_path.clone(),
            group: inst.group_path.clone(),
            tool: inst.tool.clone(),
            command: inst.command.clone(),
            status: format!("{:?}", inst.status).to_lowercase(),
            claude_session_id: inst.claude_session_id.clone(),
            parent_session_id: inst.parent_session_id.clone(),
            profile: storage.profile().to_string(),
        };
        println!("{}", serde_json::to_string_pretty(&details)?);
    } else {
        println!("Session: {}", inst.title);
        println!("  ID:      {}", inst.id);
        println!("  Path:    {}", inst.project_path);
        println!("  Group:   {}", inst.group_path);
        println!("  Tool:    {}", inst.tool);
        println!("  Command: {}", inst.command);
        println!("  Status:  {:?}", inst.status);
        println!("  Profile: {}", storage.profile());
        if let Some(claude_id) = &inst.claude_session_id {
            println!("  Claude Session: {}", claude_id);
        }
        if let Some(parent_id) = &inst.parent_session_id {
            println!("  Parent:  {}", parent_id);
        }
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
                        println!("{}", serde_json::to_string_pretty(&info)?);
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
