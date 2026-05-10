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

    // Detect drift between release-build state and dev-build state BEFORE
    // anything below calls `get_app_dir()` (which would auto-create the dev
    // dir and silently flip the trigger condition for the rest of this
    // process). Compiled away in release builds.
    let debug_namespace_drift = agent_of_empires::session::debug_namespace_drift();

    let mut debug_log_warning: Option<String> = None;
    // File-logging gate. Two env vars are accepted:
    //
    //   AOE_LOG_LEVEL=trace|debug|info|warn|error  (preferred)
    //   AGENT_OF_EMPIRES_DEBUG=1                   (legacy alias for
    //                                               AOE_LOG_LEVEL=debug)
    //
    // Either var on its own creates ~/.agent-of-empires-dev/debug.log
    // (release: ~/.agent-of-empires/debug.log) and configures
    // tracing-subscriber to write to it. The level is applied to
    // `agent_of_empires=*` and `cockpit=*` together; the ACP
    // framework's own tracing is OFF by default at every level so
    // even AOE_LOG_LEVEL=trace stays focused on our code rather
    // than dumping every JSON-RPC frame.
    //
    //   AOE_ACP_TRACE=1                            (orthogonal opt-in
    //                                               for raw JSON-RPC
    //                                               firehose)
    //
    // Adds `agent_client_protocol=debug` plus the JSON-RPC
    // transport_actor at TRACE so every raw inbound/outbound frame
    // lands in the log. Useful for chasing schema mismatches against
    // newer adapter versions, but extremely chatty.
    let log_level: Option<&'static str> = std::env::var("AOE_LOG_LEVEL")
        .ok()
        .and_then(|v| match v.to_ascii_lowercase().as_str() {
            "trace" => Some("trace"),
            "debug" => Some("debug"),
            "info" => Some("info"),
            "warn" | "warning" => Some("warn"),
            "error" => Some("error"),
            _ => None,
        })
        .or_else(|| {
            if std::env::var("AGENT_OF_EMPIRES_DEBUG").is_ok() {
                Some("debug")
            } else {
                None
            }
        });
    if let Some(level) = log_level {
        // Log to file to avoid corrupting the TUI on stderr.
        let log_path = agent_of_empires::session::get_app_dir().map(|d| d.join("debug.log"));
        let log_file = log_path
            .as_ref()
            .ok()
            .and_then(|p| std::fs::File::create(p).ok());
        if let Some(file) = log_file {
            // Cockpit code uses custom log targets like `cockpit.acp`,
            // `cockpit.supervisor`, `cockpit.acp.stderr`, etc., which
            // don't match the `agent_of_empires` crate prefix. The web
            // terminal WS handler uses `terminal.ws` and the per-byte
            // firehose uses `terminal.ws.bytes`. List them explicitly so
            // debug.log captures the full picture when chasing a crashed
            // agent. Add new top-level targets here when introducing them.
            let mut filter = format!("agent_of_empires={level},cockpit={level},terminal={level}");
            if std::env::var("AOE_ACP_TRACE").is_ok() {
                filter.push_str(
                    ",agent_client_protocol=debug,\
                     agent_client_protocol::jsonrpc::transport_actor=trace",
                );
            }
            // Per-WS-message byte firehose for the terminal relay, hidden
            // behind its own opt-in. The bytes target (`terminal.ws.bytes`)
            // logs at trace, so we only need to bump the `terminal` target
            // up to trace when the user explicitly wants the firehose;
            // a busy claude session emits thousands of frames/min and
            // would drown the lifecycle signal otherwise. The duplicate
            // directive overrides the baseline `terminal={level}` above
            // (EnvFilter is last-wins per target).
            if std::env::var("AOE_TERMINAL_TRACE").is_ok() {
                filter.push_str(",terminal=trace");
            }
            tracing_subscriber::fmt()
                .with_env_filter(filter)
                .with_writer(std::sync::Mutex::new(file))
                .with_ansi(false)
                .init();
            tracing::info!(
                "Debug logging at {} to {}",
                level,
                log_path.unwrap().display()
            );
        } else {
            debug_log_warning = Some(
                "Log level requested but debug log file could not be created. File logging is disabled.".to_string(),
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
        //
        // `terminal=info` mirrors the file logger's target list so a
        // default serve (no AOE_LOG_LEVEL set) still captures
        // `terminal.ws` warn/error lines in serve.log. Without this,
        // dead-pane warnings, idle-reaper firings, and ws send/recv
        // errors would be silently dropped in production.
        tracing_subscriber::fmt()
            .with_env_filter("agent_of_empires=info,cockpit=info,terminal=info")
            .with_ansi(false)
            .try_init()
            .ok();
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
