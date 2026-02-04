//! `agent-of-empires session` subcommands implementation

use anyhow::{bail, Result};
use clap::{Args, Subcommand};
use serde::{Deserialize, Serialize};

use crate::session::{GroupTree, Instance, Storage};

#[derive(Subcommand)]
pub enum SessionCommands {
    /// Start a session's tmux process
    Start(SessionIdArgs),

    /// Stop session process
    Stop(SessionIdArgs),

    /// Restart session
    Restart(SessionIdArgs),

    /// Attach to session interactively
    Attach(SessionIdArgs),

    /// Show session details
    Show(ShowArgs),

    /// Auto-detect current session
    Current(CurrentArgs),

    /// Import sessions from external tools
    Import(ImportArgs),
}

#[derive(Args)]
pub struct SessionIdArgs {
    /// Session ID or title
    identifier: String,
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

#[derive(Args)]
pub struct ImportArgs {
    /// Import all sessions from the specified tool
    #[arg(long)]
    all: bool,

    /// Specific session ID to import
    identifier: Option<String>,

    /// Tool name (currently only 'opencode' is implemented)
    #[arg(long, default_value = "opencode")]
    tool: String,
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
        SessionCommands::Restart(args) => restart_session(profile, args).await,
        SessionCommands::Attach(args) => attach_session(profile, args).await,
        SessionCommands::Show(args) => show_session(profile, args).await,
        SessionCommands::Current(args) => current_session(args).await,
        SessionCommands::Import(args) => import_sessions(profile, args).await,
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

    instances[idx].start_with_size(crate::terminal::get_size())?;
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

#[derive(Deserialize)]
struct OpenCodeSession {
    id: String,
    title: String,
    #[serde(default)]
    directory: Option<String>,
    #[serde(default)]
    time: Option<OpenCodeTime>,
    #[serde(rename = "parentID")]
    parent_id: Option<String>,
}

#[derive(Deserialize)]
struct OpenCodeTime {
    #[serde(default)]
    created: Option<i64>,
    #[serde(default)]
    #[allow(dead_code)]
    updated: Option<i64>,
}

async fn import_sessions(profile: &str, args: ImportArgs) -> Result<()> {

    if args.tool != "opencode" {
        bail!(
            "Tool '{}' is not yet supported for import. Currently only 'opencode' is implemented.",
            args.tool
        );
    }

    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    let opencode_storage = home.join(".local/share/opencode/storage/session");

    if !opencode_storage.exists() {
        bail!(
            "OpenCode storage directory not found: {}. \
            Make sure OpenCode is installed and has been used at least once.",
            opencode_storage.display()
        );
    }

    let mut parsed_sessions: Vec<ParsedSession> = Vec::new();

    if args.all {
        for entry in std::fs::read_dir(&opencode_storage)? {
            let entry = entry?;
            let path = entry.path();

            if !path.is_dir() {
                continue;
            }

            for session_file in std::fs::read_dir(&path)? {
                let file = session_file?;
                if !file.path().extension().map(|e| e == "json").unwrap_or(false) {
                    continue;
                }

                if let Ok(session) = parse_opencode_session(&file.path()) {
                    // Skip subagent sessions (sessions with a parent) when importing all
                    if session.instance.parent_session_id.is_some() {
                        continue;
                    }
                    parsed_sessions.push(session);
                }
            }
        }
    } else if let Some(identifier) = &args.identifier {
        let session_path = find_opencode_session(&opencode_storage, identifier)?;
        if let Some(session) = session_path {
            parsed_sessions.push(session);
        } else {
            bail!("OpenCode session not found: {}", identifier);
        }
    } else {
        bail!("Either --all or a session identifier is required");
    }

    if parsed_sessions.is_empty() {
        println!("No sessions to import.");
        return Ok(());
    }

    let storage = Storage::new(profile)?;
    let (mut instances, groups) = storage.load_with_groups()?;

	let mut imported_count = 0;
	    for parsed in &parsed_sessions {
	        let already_exists = instances.iter().any(|inst| {
	            inst.command.contains(&parsed.opencode_id)
	                || inst.title == parsed.instance.title
	        });

	        if already_exists {
	            println!("Skipping duplicate: {}", parsed.instance.title);
	            continue;
	        }

	instances.push(parsed.instance.clone());
		imported_count += 1;
	        println!("✓ Imported: {}", parsed.instance.title);
	    }

    let group_tree = GroupTree::new_with_groups(&instances, &groups);
    storage.save_with_groups(&instances, &group_tree)?;

    println!("\nImported {} session(s)", imported_count);
    Ok(())
}

struct ParsedSession {
    instance: Instance,
    opencode_id: String,
}

fn parse_opencode_session(path: &std::path::Path) -> Result<ParsedSession> {
    let content = std::fs::read_to_string(path)?;
    let oc_session: OpenCodeSession = serde_json::from_str(&content)?;

    let project_path = oc_session
        .directory
        .ok_or_else(|| anyhow::anyhow!("Session missing directory field"))?;

    let title = if oc_session.title.is_empty() {
        &oc_session.id
    } else {
        &oc_session.title
    };

    let created_at = if let Some(time) = &oc_session.time {
        if let Some(created) = time.created {
            chrono::DateTime::from_timestamp_millis(created)
                .unwrap_or_else(chrono::Utc::now)
        } else {
            chrono::Utc::now()
        }
    } else {
        chrono::Utc::now()
    };

    let mut instance = Instance::new(title, &project_path);
    instance.command = format!("opencode --session {}", oc_session.id);
    instance.tool = "opencode".to_string();
    instance.group_path = "OpenCode Imports".to_string();
    instance.created_at = created_at;
    instance.parent_session_id = oc_session.parent_id;
    instance.update_search_cache();

    Ok(ParsedSession {
        instance,
        opencode_id: oc_session.id,
    })
}

fn find_opencode_session(
    storage_dir: &std::path::Path,
    identifier: &str,
) -> Result<Option<ParsedSession>> {
    for entry in std::fs::read_dir(storage_dir)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        for session_file in std::fs::read_dir(&path)? {
            let file = session_file?;
            if !file.path().extension().map(|e| e == "json").unwrap_or(false) {
                continue;
            }

            if let Ok(session) = parse_opencode_session(&file.path()) {
                if session.opencode_id == identifier || file.file_name().to_string_lossy().contains(identifier) {
                    return Ok(Some(session));
                }
            }
        }
    }

    Ok(None)
}
