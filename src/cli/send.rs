//! `agent-of-empires send` subcommand implementation

use anyhow::{bail, Result};
use clap::Args;

use crate::session::Storage;

#[derive(Args)]
pub struct SendArgs {
    /// Session ID or title
    identifier: String,

    /// Message to send to the agent
    message: String,
}

pub async fn run(profile: &str, args: SendArgs) -> Result<()> {
    let storage = Storage::new(profile)?;
    let (instances, _) = storage.load_with_groups()?;

    if args.message.trim().is_empty() {
        bail!("Message cannot be empty");
    }

    let inst = super::resolve_session(&args.identifier, &instances)?;
    let tmux_session = crate::tmux::Session::new(&inst.id, &inst.title)?;

    if !tmux_session.exists() {
        bail!(
            "Session is not running. Start it first with: aoe session start {}",
            args.identifier
        );
    }

    tmux_session.send_keys(&args.message)?;
    println!("Sent message to '{}'", inst.title);
    Ok(())
}
