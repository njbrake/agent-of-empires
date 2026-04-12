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

    /// Expose via Cloudflare Tunnel for secure remote access
    #[arg(long)]
    pub remote: bool,

    /// Use a named Cloudflare Tunnel (requires prior `cloudflared tunnel create`)
    #[arg(long, requires = "remote")]
    pub tunnel_name: Option<String>,

    /// Hostname for a named tunnel (e.g., aoe.example.com)
    #[arg(long, requires = "tunnel_name")]
    pub tunnel_url: Option<String>,

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

    // Block --no-auth with --remote (tunnel makes localhost publicly accessible)
    if args.no_auth && args.remote {
        bail!(
            "Refusing to start without authentication in remote mode.\n\
             --no-auth with --remote would expose unauthenticated shell access to the internet."
        );
    }

    // Named tunnel requires --tunnel-url
    if args.tunnel_name.is_some() && args.tunnel_url.is_none() {
        bail!(
            "Named tunnels require --tunnel-url to specify the hostname.\n\
             Example: aoe serve --remote --tunnel-name my-tunnel --tunnel-url aoe.example.com\n\
             \n\
             Setup steps:\n\
             1. cloudflared tunnel create my-tunnel\n\
             2. Add a CNAME record: aoe.example.com -> <tunnel-id>.cfargotunnel.com\n\
             3. aoe serve --remote --tunnel-name my-tunnel --tunnel-url aoe.example.com"
        );
    }

    // Remote mode: check cloudflared and force localhost binding
    let host = if args.remote {
        crate::server::tunnel::check_cloudflared()?;
        // Force localhost since cloudflared connects to localhost
        "127.0.0.1".to_string()
    } else {
        args.host.clone()
    };

    // Warn about security implications of network binding (non-remote, non-localhost)
    if !is_localhost && !args.remote {
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
        eprintln!("  Or use: aoe serve --remote");
        eprintln!("  for automatic HTTPS via Cloudflare Tunnel.");
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

    let result = crate::server::start_server(crate::server::ServerConfig {
        profile,
        host: &host,
        port: args.port,
        no_auth: args.no_auth,
        read_only: args.read_only,
        remote: args.remote,
        tunnel_name: args.tunnel_name.as_deref(),
        tunnel_url: args.tunnel_url.as_deref(),
        is_daemon: false,
    })
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
    if args.remote {
        cmd.arg("--remote");
    }
    if let Some(ref name) = args.tunnel_name {
        cmd.args(["--tunnel-name", name]);
    }
    if let Some(ref url) = args.tunnel_url {
        cmd.args(["--tunnel-url", url]);
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
            // Process doesn't exist; clean up stale PID file
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
