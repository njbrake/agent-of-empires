//! `aoe update` command - self-update by detected install method.

use anyhow::{Context, Result};
use clap::Args;
use std::io::{self, IsTerminal, Write};

use crate::update::check_for_update;
use crate::update::install::{
    detect_install_method, format_prompt_block, parent_is_writable, perform_update, InstallMethod,
};

#[derive(Args, Debug)]
pub struct UpdateArgs {
    /// Skip confirmation prompt
    #[arg(short = 'y', long)]
    pub yes: bool,

    /// Print update status and exit (no install)
    #[arg(long)]
    pub check: bool,

    /// Detect install method and print what would happen, no download
    #[arg(long)]
    pub dry_run: bool,
}

pub async fn run(args: UpdateArgs) -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");

    // Force-fresh check; the user explicitly asked.
    let info = check_for_update(current_version, true)
        .await
        .context("checking for updates")?;

    if args.check {
        println!("current: {}", info.current_version);
        println!("latest:  {}", info.latest_version);
        println!("available: {}", info.available);
        return Ok(());
    }

    if !info.available {
        println!(
            "You're on v{} (latest). Nothing to do.",
            info.current_version
        );
        return Ok(());
    }

    let method = detect_install_method()?;
    let needs_sudo = matches!(&method, InstallMethod::Tarball { binary_path } if !parent_is_writable(binary_path));

    let prompt = format_prompt_block(
        &info.current_version,
        &info.latest_version,
        &method,
        needs_sudo,
    );
    println!("{prompt}\n");

    if args.dry_run {
        println!("(dry run; not downloading)");
        return Ok(());
    }

    if !args.yes {
        if !io::stdin().is_terminal() {
            anyhow::bail!("stdin is not a TTY; pass `-y` to confirm.");
        }
        print!("Proceed? [Y/n] ");
        io::stdout().flush()?;
        let mut answer = String::new();
        io::stdin().read_line(&mut answer)?;
        let answer = answer.trim().to_lowercase();
        if !(answer.is_empty() || answer == "y" || answer == "yes") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    let mut last_pct: i64 = -1;
    let mut on_progress = |bytes: u64, total: Option<u64>| {
        if let Some(total) = total {
            let pct = (bytes as f64 / total as f64 * 100.0) as i64;
            if pct != last_pct && pct % 5 == 0 {
                eprint!("\rDownloading… {pct}%");
                let _ = io::stderr().flush();
                last_pct = pct;
            }
        }
    };
    perform_update(&method, &info.latest_version, Some(&mut on_progress)).await?;
    if matches!(&method, InstallMethod::Tarball { .. }) {
        eprintln!();
        println!(
            "✓ Updated to v{}. Restart `aoe` to use the new version.",
            info.latest_version
        );
    } else if matches!(&method, InstallMethod::Homebrew) {
        println!("✓ brew upgrade complete.");
    }
    Ok(())
}
