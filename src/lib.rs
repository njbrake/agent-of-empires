//! Agent of Empires library - Core functionality for the terminal session manager

pub mod agents;
pub mod claude_settings;
pub mod cli;
#[cfg(feature = "serve")]
pub mod cockpit;
pub mod containers;
pub mod git;
pub mod hooks;
pub mod logging;
pub mod migrations;
pub mod process;
#[cfg(feature = "serve")]
pub mod server;
pub mod session;
pub mod sound;
pub mod task_util;
pub mod terminal;
pub mod tmux;
pub mod tui;
pub mod update;
