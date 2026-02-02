//! CLI argument definitions for documentation generation
//!
//! This module contains the CLI struct definitions used by clap.
//! They're separated from main.rs so xtask can generate documentation.

use clap::{Parser, Subcommand};

use super::add::AddArgs;
use super::group::GroupCommands;
use super::init::InitArgs;
use super::list::ListArgs;
use super::profile::ProfileCommands;
use super::remove::RemoveArgs;
use super::session::SessionCommands;
use super::sounds::SoundsCommands;
use super::status::StatusArgs;
use super::tmux::TmuxCommands;
use super::uninstall::UninstallArgs;
use super::worktree::WorktreeCommands;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(name = "aoe")]
#[command(about = "Terminal session manager for AI coding agents")]
#[command(version = VERSION)]
#[command(
    long_about = "Agent of Empires (aoe) is a terminal session manager that uses tmux to help \
    you manage and monitor AI coding agents like Claude Code and OpenCode.\n\n\
    Run without arguments to launch the TUI dashboard."
)]
pub struct Cli {
    /// Profile to use (separate workspace with its own sessions)
    #[arg(short = 'p', long, global = true, env = "AGENT_OF_EMPIRES_PROFILE")]
    pub profile: Option<String>,

    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Add a new session
    Add(AddArgs),

    /// Initialize .aoe/config.toml in a repository
    Init(InitArgs),

    /// List all sessions
    #[command(alias = "ls")]
    List(ListArgs),

    /// Remove a session
    #[command(alias = "rm")]
    Remove(RemoveArgs),

    /// Show session status summary
    Status(StatusArgs),

    /// Manage session lifecycle (start, stop, attach, etc.)
    Session {
        #[command(subcommand)]
        command: SessionCommands,
    },

    /// Manage groups for organizing sessions
    Group {
        #[command(subcommand)]
        command: GroupCommands,
    },

    /// Manage profiles (separate workspaces)
    Profile {
        #[command(subcommand)]
        command: Option<ProfileCommands>,
    },

    /// Manage git worktrees for parallel development
    Worktree {
        #[command(subcommand)]
        command: WorktreeCommands,
    },

    /// tmux integration utilities
    Tmux {
        #[command(subcommand)]
        command: TmuxCommands,
    },

    /// Manage sound effects for agent state transitions
    Sounds {
        #[command(subcommand)]
        command: SoundsCommands,
    },

    /// Uninstall Agent of Empires
    Uninstall(UninstallArgs),
}
