//! `aoe serve` command -- start a web dashboard for remote session access

use anyhow::{bail, Result};
use clap::Args;
use std::path::PathBuf;

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

    /// Stop a running daemon
    #[arg(long)]
    pub stop: bool,
}

fn pid_file_path() -> Result<PathBuf> {
    let dir = crate::session::get_app_dir()?;
    Ok(dir.join("serve.pid"))
}

pub async fn run(profile: &str, args: ServeArgs) -> Result<()> {
    if args.stop {
        return stop_daemon();
    }

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

    // Write PID file for non-daemon mode too (so --stop works either way)
    if let Ok(path) = pid_file_path() {
        let _ = std::fs::write(&path, std::process::id().to_string());
    }

    let result =
        crate::server::start_server(profile, &args.host, args.port, args.no_auth, args.read_only)
            .await;

    // Clean up PID and URL files on exit
    if let Ok(path) = pid_file_path() {
        let _ = std::fs::remove_file(path);
    }
    if let Ok(dir) = crate::session::get_app_dir() {
        let _ = std::fs::remove_file(dir.join("serve.url"));
    }

    result
}

fn start_daemon(profile: &str, args: &ServeArgs) -> Result<()> {
    use std::process::{Command, Stdio};

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
    if !profile.is_empty() {
        cmd.args(["--profile", profile]);
    }

    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    let child = cmd.spawn()?;
    let pid = child.id();

    // Write PID file
    if let Ok(path) = pid_file_path() {
        std::fs::write(&path, pid.to_string())?;
    }

    println!("aoe serve started as daemon (PID {})", pid);
    println!("Stop with: aoe serve --stop");
    Ok(())
}

fn stop_daemon() -> Result<()> {
    let path = pid_file_path()?;

    if !path.exists() {
        bail!(
            "No running daemon found (no PID file at {})",
            path.display()
        );
    }

    let pid_str = std::fs::read_to_string(&path)?;
    let pid: i32 = pid_str
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid PID in {}: {}", path.display(), pid_str.trim()))?;

    // Send SIGTERM
    match nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(pid),
        nix::sys::signal::Signal::SIGTERM,
    ) {
        Ok(()) => {
            std::fs::remove_file(&path)?;
            if let Ok(dir) = crate::session::get_app_dir() {
                let _ = std::fs::remove_file(dir.join("serve.url"));
            }
            println!("Stopped aoe serve daemon (PID {})", pid);
        }
        Err(nix::errno::Errno::ESRCH) => {
            // Process doesn't exist -- clean up stale PID file
            std::fs::remove_file(&path)?;
            if let Ok(dir) = crate::session::get_app_dir() {
                let _ = std::fs::remove_file(dir.join("serve.url"));
            }
            println!("Daemon was not running (stale PID file cleaned up)");
        }
        Err(e) => bail!("Failed to stop daemon (PID {}): {}", pid, e),
    }

    Ok(())
}
