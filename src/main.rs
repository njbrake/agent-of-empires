//! Agent of Empires - Terminal session manager for AI coding agents

use agent_of_empires::cli::{self, Cli, Commands};
use agent_of_empires::migrations;
use agent_of_empires::tui;
use anyhow::Result;
use clap::{CommandFactory, Parser};
use clap_complete::generate;

/// Did the user invoke `aoe serve`? Feature-gated because `Commands::Serve`
/// only exists when the `serve` feature is on; in TUI-only builds we
/// always return false so the tracing-init branch below compiles.
#[cfg(feature = "serve")]
fn is_serve_command(cli: &Cli) -> bool {
    matches!(cli.command, Some(Commands::Serve(_)))
}

#[cfg(not(feature = "serve"))]
fn is_serve_command(_cli: &Cli) -> bool {
    false
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let mut debug_log_warning: Option<String> = None;
    if std::env::var("AGENT_OF_EMPIRES_DEBUG").is_ok() {
        // Log to file to avoid corrupting the TUI on stderr.
        let log_path = agent_of_empires::session::get_app_dir().map(|d| d.join("debug.log"));
        let log_file = log_path
            .as_ref()
            .ok()
            .and_then(|p| std::fs::File::create(p).ok());
        if let Some(file) = log_file {
            tracing_subscriber::fmt()
                .with_env_filter("agent_of_empires=debug")
                .with_writer(std::sync::Mutex::new(file))
                .with_ansi(false)
                .init();
            tracing::info!("Debug logging to {}", log_path.unwrap().display());
        } else {
            debug_log_warning = Some(
                "AGENT_OF_EMPIRES_DEBUG is set but the debug log file could not be created. Debug logging is disabled.".to_string(),
            );
        }
    } else if is_serve_command(&cli) {
        // `aoe serve` writes info-level tracing to stdout so the daemon
        // path (which redirects child stdout/stderr into serve.log) can
        // capture progress for the TUI's Starting-screen log tail.
        // Without this, serve.log would be empty and the user would
        // stare at "(waiting for daemon output...)" for 30-60s during
        // cert provisioning. Foreground `aoe serve` just prints to
        // the user's terminal; that's fine and matches other CLIs.
        tracing_subscriber::fmt()
            .with_env_filter("agent_of_empires=info")
            .with_ansi(false)
            .try_init()
            .ok();
    }

    // Handle commands that don't need app data or migrations.
    // These work in read-only/sandboxed environments (e.g. Nix builds).
    match cli.command {
        Some(Commands::Completion { shell }) => {
            generate(shell, &mut Cli::command(), "aoe", &mut std::io::stdout());
            return Ok(());
        }
        Some(Commands::Init(args)) => return cli::init::run(args).await,
        Some(Commands::Tmux { command }) => {
            use cli::tmux::TmuxCommands;
            return match command {
                TmuxCommands::Status(args) => cli::tmux::run_status(args),
            };
        }
        Some(Commands::Sounds { command }) => return cli::sounds::run(command).await,
        Some(Commands::Theme { command }) => {
            use cli::theme::ThemeCommands;
            return match command {
                ThemeCommands::List => {
                    cli::theme::run_list();
                    Ok(())
                }
                ThemeCommands::Export { name, output } => {
                    cli::theme::run_export(&name, output.as_deref())
                }
                ThemeCommands::Dir => cli::theme::run_dir(),
            };
        }
        Some(Commands::Uninstall(args)) => return cli::uninstall::run(args).await,
        _ => {}
    }

    let profile = cli.profile.unwrap_or_default();

    // TUI mode handles migrations with a spinner; CLI runs them silently
    if cli.command.is_some() {
        migrations::run_migrations()?;
    }

    match cli.command {
        Some(Commands::Add(args)) => cli::add::run(&profile, args).await,
        Some(Commands::List(args)) => cli::list::run(&profile, args).await,
        Some(Commands::Remove(args)) => cli::remove::run(&profile, args).await,
        Some(Commands::Send(args)) => cli::send::run(&profile, args).await,
        Some(Commands::Status(args)) => cli::status::run(&profile, args).await,
        Some(Commands::Session { command }) => cli::session::run(&profile, command).await,
        Some(Commands::Group { command }) => cli::group::run(&profile, command).await,
        Some(Commands::Profile { command }) => cli::profile::run(command).await,
        Some(Commands::Worktree { command }) => cli::worktree::run(&profile, command).await,
        #[cfg(feature = "serve")]
        Some(Commands::Serve(args)) => cli::serve::run(&profile, args).await,
        None => tui::run(&profile, debug_log_warning).await,
        _ => unreachable!(),
    }
}
