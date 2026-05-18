//! `agent-of-empires send` subcommand implementation

use anyhow::{bail, Result};
use clap::Args;

use crate::cli::session::stale_history_suffix;
use crate::session::{EnsureReadyError, EnsureReadyOutcome, Storage};

#[derive(Args)]
pub struct SendArgs {
    /// Session ID or title
    identifier: String,

    /// Message to send to the agent
    message: String,

    /// Fail loud on dead/stopped sessions instead of auto-respawning. Default
    /// behavior is to revive the session so a `send` after a crash or stop
    /// just works; pass this for scripts that want the previous bail-out.
    #[arg(long = "no-revive")]
    no_revive: bool,
}

pub async fn run(profile: &str, args: SendArgs) -> Result<()> {
    let storage = Storage::new(profile)?;
    let (mut instances, _) = storage.load_with_groups()?;

    if args.message.trim().is_empty() {
        bail!("Message cannot be empty");
    }

    let inst = super::resolve_session(&args.identifier, &instances)?;
    let session_id = inst.id.clone();
    let session_title = inst.title.clone();
    let tool = inst.tool.clone();

    // Revive the pane if needed before delivering keystrokes. Without this,
    // a send to a dead pane silently writes to a corpse with no agent to
    // respond to it.
    if !args.no_revive {
        if let Some(target) = instances.iter_mut().find(|i| i.id == session_id) {
            match target.ensure_pane_ready() {
                Ok(EnsureReadyOutcome::Respawned { stale_sid: None }) => {
                    eprintln!("  (respawned dead pane before send)");
                }
                Ok(EnsureReadyOutcome::Respawned {
                    stale_sid: Some(sid),
                }) => {
                    eprintln!(
                        "  (respawned dead pane before send){}",
                        stale_history_suffix(&sid),
                    );
                }
                Ok(EnsureReadyOutcome::Started { stale_sid: None }) => {
                    eprintln!("  (started stopped session before send)");
                }
                Ok(EnsureReadyOutcome::Started {
                    stale_sid: Some(sid),
                }) => {
                    eprintln!(
                        "  (started stopped session before send){}",
                        stale_history_suffix(&sid),
                    );
                }
                Ok(EnsureReadyOutcome::AlreadyAlive) => {}
                Err(EnsureReadyError::Transient(status)) => {
                    bail!("Session is mid-lifecycle ({status:?}); cannot send right now")
                }
                Err(EnsureReadyError::CockpitMode) => {
                    bail!("Cockpit-mode sessions have no tmux pane; send is not supported")
                }
                Err(EnsureReadyError::Tmux(e)) => bail!("{}", e),
            }
        }
    }

    let tmux_session = crate::tmux::Session::new(&session_id, &session_title)?;
    if !tmux_session.exists() {
        bail!(
            "Session is not running. Start it first with: aoe session start {}",
            args.identifier
        );
    }

    let delay = crate::agents::send_keys_enter_delay(&tool);
    tmux_session.send_keys_with_delay(&args.message, delay)?;

    // Stamp last_accessed_at so the "last activity" column reflects user interaction
    if let Some(inst) = instances.iter_mut().find(|i| i.id == session_id) {
        inst.touch_last_accessed();
    }
    storage.save(&instances)?;

    println!("Sent message to '{}'", session_title);
    Ok(())
}
