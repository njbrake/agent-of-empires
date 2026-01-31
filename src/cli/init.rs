//! `agent-of-empires init` command implementation

use anyhow::{bail, Result};
use clap::Args;
use std::fs;
use std::path::PathBuf;

use crate::session::repo_config::INIT_TEMPLATE;

#[derive(Args)]
pub struct InitArgs {
    /// Directory to initialize (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,
}

pub async fn run(args: InitArgs) -> Result<()> {
    let path = if args.path.as_os_str() == "." {
        std::env::current_dir()?
    } else {
        args.path.canonicalize()?
    };

    let aoe_dir = path.join(".aoe");
    let config_path = aoe_dir.join("config.toml");

    if config_path.exists() {
        bail!(
            ".aoe/config.toml already exists at {}\nEdit it directly to make changes.",
            config_path.display()
        );
    }

    fs::create_dir_all(&aoe_dir)?;
    fs::write(&config_path, INIT_TEMPLATE)?;

    println!("Created .aoe/config.toml at {}", path.display());
    println!("Edit the file to configure hooks and session defaults for this repo.");

    Ok(())
}
