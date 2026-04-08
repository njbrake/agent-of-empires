//! `aoe serve` command -- start a web dashboard for remote session access

use anyhow::{bail, Result};
use clap::Args;

#[derive(Args)]
pub struct ServeArgs {
    /// Port to listen on
    #[arg(long, default_value = "8080")]
    pub port: u16,

    /// Host/IP to bind to (use 0.0.0.0 for LAN/VPN access)
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Disable authentication (only allowed with localhost binding)
    #[arg(long)]
    pub no_auth: bool,

    /// Read-only mode: view terminals but cannot send keystrokes
    #[arg(long)]
    pub read_only: bool,

    /// Run as a background daemon (detach from terminal)
    #[arg(long)]
    pub daemon: bool,
}

pub async fn run(profile: &str, args: ServeArgs) -> Result<()> {
    let is_localhost = args.host == "127.0.0.1" || args.host == "localhost" || args.host == "::1";

    // Block dangerous combination: no auth on a network-accessible server
    if args.no_auth && !is_localhost {
        bail!(
            "Refusing to start without authentication on {}.\n\
             --no-auth is only allowed with localhost (127.0.0.1).\n\
             For remote access, use token auth (the default) over a VPN like Tailscale.",
            args.host
        );
    }

    // Warn about security implications of network binding
    if !is_localhost {
        eprintln!("==========================================================");
        eprintln!("  SECURITY WARNING: Binding to {}", args.host);
        eprintln!("==========================================================");
        eprintln!();
        eprintln!("  This exposes terminal access to your network.");
        eprintln!("  Anyone with the auth token can execute commands");
        eprintln!("  as your user on this machine.");
        eprintln!();
        eprintln!("  Traffic is NOT encrypted (HTTP, not HTTPS).");
        eprintln!("  Use a VPN (Tailscale, WireGuard) or SSH tunnel");
        eprintln!("  for remote access. Do NOT expose this to the");
        eprintln!("  public internet without TLS termination.");
        eprintln!();
        if args.read_only {
            eprintln!("  Read-only mode is ON: terminal input is disabled.");
            eprintln!();
        }
        eprintln!("==========================================================");
        eprintln!();
    }

    if args.daemon {
        return start_daemon(profile, &args);
    }

    crate::server::start_server(profile, &args.host, args.port, args.no_auth, args.read_only).await
}

fn start_daemon(profile: &str, args: &ServeArgs) -> Result<()> {
    use std::process::Command;

    let exe = std::env::current_exe()?;
    let mut cmd = Command::new(exe);
    cmd.args([
        "serve",
        "--port",
        &args.port.to_string(),
        "--host",
        &args.host,
    ]);

    if args.no_auth {
        cmd.arg("--no-auth");
    }
    if args.read_only {
        cmd.arg("--read-only");
    }

    // Pass profile if set
    if !profile.is_empty() {
        cmd.args(["--profile", profile]);
    }

    // Detach: redirect stdio to /dev/null and spawn
    use std::process::Stdio;
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let child = cmd.spawn()?;
    println!("aoe serve started as daemon (PID {})", child.id());
    println!("Stop with: kill {}", child.id());
    Ok(())
}
