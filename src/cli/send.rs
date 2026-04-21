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
    let (mut instances, _) = storage.load_with_groups()?;

    if args.message.trim().is_empty() {
        bail!("Message cannot be empty");
    }

    let inst = super::resolve_session(&args.identifier, &instances)?;
    let session_id = inst.id.clone();
    let session_title = inst.title.clone();
    let tool = inst.tool.clone();
    let tmux_session = crate::tmux::Session::new(&inst.id, &inst.title)?;

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
