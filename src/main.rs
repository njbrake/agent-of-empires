//! Agent of Empires - Terminal session manager for AI coding agents

use anyhow::Result;
use clap::{Parser, Subcommand};

mod cli;
mod git;
mod process;
mod session;
mod tmux;
mod tui;
mod update;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(name = "agent-of-empires")]
#[command(about = "Terminal session manager for AI coding agents")]
#[command(version = VERSION)]
struct Cli {
    /// Profile to use
    #[arg(short = 'p', long, global = true, env = "AGENT_OF_EMPIRES_PROFILE")]
    profile: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Add a new session
    Add(cli::add::AddArgs),

    /// List all sessions
    #[command(alias = "ls")]
    List(cli::list::ListArgs),

    /// Remove a session
    #[command(alias = "rm")]
    Remove(cli::remove::RemoveArgs),

    /// Show session status summary
    Status(cli::status::StatusArgs),

    /// Manage session lifecycle
    Session {
        #[command(subcommand)]
        command: cli::session::SessionCommands,
    },

    /// Manage groups
    Group {
        #[command(subcommand)]
        command: cli::group::GroupCommands,
    },

    /// Manage profiles
    Profile {
        #[command(subcommand)]
        command: Option<cli::profile::ProfileCommands>,
    },

    /// Manage git worktrees
    Worktree {
        #[command(subcommand)]
        command: cli::worktree::WorktreeCommands,
    },

    /// Uninstall Agent of Empires
    Uninstall(cli::uninstall::UninstallArgs),
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    if std::env::var("AGENT_OF_EMPIRES_DEBUG").is_ok() {
        tracing_subscriber::fmt()
            .with_env_filter("agent_of_empires=debug")
            .init();
    }

    let cli = Cli::parse();
    let profile = cli.profile.unwrap_or_default();

    match cli.command {
        Some(Commands::Add(args)) => cli::add::run(&profile, args).await,
        Some(Commands::List(args)) => cli::list::run(&profile, args).await,
        Some(Commands::Remove(args)) => cli::remove::run(&profile, args).await,
        Some(Commands::Status(args)) => cli::status::run(&profile, args).await,
        Some(Commands::Session { command }) => cli::session::run(&profile, command).await,
        Some(Commands::Group { command }) => cli::group::run(&profile, command).await,
        Some(Commands::Profile { command }) => cli::profile::run(command).await,
        Some(Commands::Worktree { command }) => cli::worktree::run(&profile, command).await,
        Some(Commands::Uninstall(args)) => cli::uninstall::run(args).await,
        None => {
            // Launch TUI
            tui::run(&profile).await
        }
    }
}
