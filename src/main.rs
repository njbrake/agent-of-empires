//! Agent of Empires - Terminal session manager for AI coding agents

use agent_of_empires::cli::{self, Cli, Commands};
use agent_of_empires::migrations;
use agent_of_empires::tui;
use anyhow::Result;
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    if std::env::var("AGENT_OF_EMPIRES_DEBUG").is_ok() {
        tracing_subscriber::fmt()
            .with_env_filter("agent_of_empires=debug")
            .init();
    }

    migrations::run_migrations()?;

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
        Some(Commands::Tmux { command }) => {
            use cli::tmux::TmuxCommands;
            match command {
                TmuxCommands::Status(args) => cli::tmux::run_status(args),
            }
        }
        Some(Commands::Uninstall(args)) => cli::uninstall::run(args).await,
        None => tui::run(&profile).await,
    }
}
