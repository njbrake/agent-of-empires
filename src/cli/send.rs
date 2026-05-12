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

    log_aoe_message(&session_id, &session_title, &args.message);

    // Stamp last_accessed_at so the "last activity" column reflects user interaction
    if let Some(inst) = instances.iter_mut().find(|i| i.id == session_id) {
        inst.touch_last_accessed();
    }
    storage.save(&instances)?;

    println!("Sent message to '{}'", session_title);
    Ok(())
}

fn log_aoe_message(dst_id: &str, dst_title: &str, message: &str) {
    use std::io::Write;
    let Some(home) = std::env::var_os("HOME") else {
        return;
    };
    let dir = std::path::PathBuf::from(home).join("logs");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("aoe-messages.jsonl");
    let snippet: String = message.chars().take(80).collect();
    let src_pwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_string))
        .unwrap_or_default();
    let entry = serde_json::json!({
        "ts": chrono::Utc::now().to_rfc3339(),
        "path": "cli",
        "src_pwd": src_pwd,
        "src_pid": std::process::id(),
        "dst_id": dst_id,
        "dst_title": dst_title,
        "msg_len": message.chars().count(),
        "msg_snippet": snippet,
    });
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{}", entry);
    }
}
