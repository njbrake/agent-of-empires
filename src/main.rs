//! Agent of Empires - Terminal session manager for AI coding agents

use agent_of_empires::cli::{self, Cli, Commands};
use agent_of_empires::logging::{self, LogConfig, SubscriberTarget};
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

    // Detect drift between release-build state and dev-build state BEFORE
    // anything below calls `get_app_dir()` (which would auto-create the dev
    // dir and silently flip the trigger condition for the rest of this
    // process). Compiled away in release builds.
    let debug_namespace_drift = agent_of_empires::session::debug_namespace_drift();

    let mut debug_log_warning: Option<String> = None;
    // Logging gate. Env-var matrix lives in `LogConfig::from_env`:
    //   AOE_LOG_LEVEL=trace|debug|info|warn|error   (preferred)
    //   AGENT_OF_EMPIRES_DEBUG=1                    (legacy alias for debug)
    //   AOE_ACP_TRACE=1                             (raw JSON-RPC firehose)
    //   AOE_TERMINAL_TRACE=1                        (per-byte WS firehose)
    //
    // Level set → file logging to ~/.agent-of-empires/debug.log.
    // Else `aoe serve` → stdout (captured into serve.log by the daemon
    // redirect) so the TUI Starting-screen log tail isn't empty during
    // cert provisioning.
    // Else no subscriber installed.
    let env_cfg = LogConfig::from_env();
    let (init, log_path_for_msg) = if let Some(filter) = env_cfg.filter_string() {
        let log_path = agent_of_empires::session::get_app_dir().map(|d| d.join("debug.log"));
        match log_path.as_ref() {
            Ok(path) => {
                let res = logging::init_subscriber(SubscriberTarget::File(path.clone()), filter);
                (res, Some(path.clone()))
            }
            Err(_) => (
                logging::InitResult {
                    controller: None,
                    warning: Some(
                        "Log level requested but app dir unavailable; file logging disabled."
                            .to_string(),
                    ),
                },
                None,
            ),
        }
    } else if is_serve_command(&cli) {
        let filter = LogConfig::serve_default()
            .filter_string()
            .expect("serve_default sets a level");
        (
            logging::init_subscriber(SubscriberTarget::Stdout, filter),
            None,
        )
    } else {
        (
            logging::InitResult {
                controller: None,
                warning: None,
            },
            None,
        )
    };
    if let Some(c) = init.controller.clone() {
        logging::install_controller(c);
    }
    if let Some(msg) = init.warning {
        debug_log_warning = Some(msg);
    }
    if let (Some(_), Some(path), Some(lvl)) = (
        init.controller.as_ref(),
        log_path_for_msg.as_ref(),
        env_cfg.level,
    ) {
        tracing::info!("Debug logging at {} to {}", lvl.as_str(), path.display());
    }

    // CLI invocations get the dev-namespace drift warning on stderr right
    // away. TUI mode handles it via the existing startup-warning popup
    // pipeline below — we don't print here for TUI because ratatui's
    // alt-screen would clobber the message.
    if cli.command.is_some() {
        if let Some((release, dev)) = debug_namespace_drift.as_ref() {
            eprintln!(
                "\n{}\n",
                agent_of_empires::session::format_debug_namespace_warning(release, dev),
            );
        }
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
        Some(Commands::Agents) => return cli::agents::run(),
        Some(Commands::Logs(args)) => return cli::logs::run(args).await,
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
        Some(Commands::Update(args)) => return cli::update::run(args).await,
        _ => {}
    }

    let profile_explicit = cli.profile.is_some();
    let profile = cli.profile.unwrap_or_default();

    // TUI mode handles migrations with a spinner; CLI runs them silently
    if cli.command.is_some() {
        migrations::run_migrations()?;
    }

    match cli.command {
        Some(Commands::Add(args)) => cli::add::run(&profile, *args).await,
        Some(Commands::List(args)) => cli::list::run(&profile, args).await,
        Some(Commands::Remove(args)) => cli::remove::run(&profile, args).await,
        Some(Commands::Send(args)) => cli::send::run(&profile, args).await,
        Some(Commands::Status(args)) => cli::status::run(&profile, args).await,
        Some(Commands::Session { command }) => cli::session::run(&profile, command).await,
        Some(Commands::Group { command }) => cli::group::run(&profile, command).await,
        Some(Commands::Profile { command }) => cli::profile::run(command).await,
        Some(Commands::Project { command }) => {
            cli::project::run(&profile, profile_explicit, command).await
        }
        Some(Commands::Worktree { command }) => cli::worktree::run(&profile, command).await,
        #[cfg(feature = "serve")]
        Some(Commands::Serve(args)) => cli::serve::run(&profile, args).await,
        #[cfg(feature = "serve")]
        Some(Commands::Url(args)) => cli::url::run(args),
        #[cfg(feature = "serve")]
        Some(Commands::Cockpit { command }) => cli::cockpit::run(command).await,
        #[cfg(feature = "serve")]
        Some(Commands::CockpitRunner(args)) => agent_of_empires::cockpit::runner::run(*args).await,
        None => {
            // Fold the drift notice into the existing startup-warning channel
            // so the TUI surfaces both (debug-log + drift, if both fire) in a
            // single modal instead of stacking two dialogs.
            let drift_msg = debug_namespace_drift.as_ref().map(|(release, dev)| {
                agent_of_empires::session::format_debug_namespace_warning(release, dev)
            });
            let combined = match (debug_log_warning, drift_msg) {
                (Some(a), Some(b)) => Some(format!("{a}\n\n{b}")),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            };
            tui::run(&profile, combined).await
        }
        _ => unreachable!(),
    }
}
