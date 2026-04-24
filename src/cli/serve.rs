//! `aoe serve` command -- start a web dashboard for remote session access

use anyhow::{bail, Result};
use clap::Args;
use std::path::PathBuf;
use std::sync::Mutex;

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

    /// Expose the dashboard over a public HTTPS tunnel. Prefers Tailscale
    /// Funnel when `tailscale` is installed and logged in (stable
    /// `.ts.net` URL, installable PWAs survive restarts). Falls back to a
    /// Cloudflare quick tunnel otherwise (fresh URL on every restart).
    #[arg(long)]
    pub remote: bool,

    /// Use a named Cloudflare Tunnel (requires prior `cloudflared tunnel create`).
    /// Takes precedence over Tailscale auto-detection.
    #[arg(long, requires = "remote")]
    pub tunnel_name: Option<String>,

    /// Skip Tailscale Funnel auto-detection and go straight to Cloudflare.
    /// Useful if you have Tailscale installed for unrelated reasons.
    #[arg(long, requires = "remote")]
    pub no_tailscale: bool,

    /// Hostname for a named tunnel (e.g., aoe.example.com)
    #[arg(long, requires = "tunnel_name")]
    pub tunnel_url: Option<String>,

    /// Run as a background daemon (detach from terminal)
    #[arg(long)]
    pub daemon: bool,

    /// Stop a running daemon
    #[arg(long)]
    pub stop: bool,

    /// Require a passphrase for login (second-factor auth).
    /// Can also be set via AOE_SERVE_PASSPHRASE environment variable.
    #[arg(long, env = "AOE_SERVE_PASSPHRASE")]
    pub passphrase: Option<String>,
}

pub fn pid_file_path() -> Result<PathBuf> {
    let dir = crate::session::get_app_dir()?;
    Ok(dir.join("serve.pid"))
}

/// Cached read of `$APP_DIR/serve.mode`, keyed on the current daemon
/// PID. The status bar calls this on every render frame; without
/// caching, that's a syscall + file read per frame just to compute a
/// one-word label. We re-read the mode file only when the PID changes
/// (daemon restart, fresh spawn), which is exactly when the mode could
/// have changed.
///
/// Returns `None` when no daemon is running OR when the mode file is
/// missing/unparseable. Callers can treat both cases the same way:
/// "show the generic Serving label, no mode tag."
pub fn cached_serve_mode_label() -> Option<&'static str> {
    static CACHE: Mutex<Option<(u32, Option<&'static str>)>> = Mutex::new(None);

    let pid = daemon_pid()?;
    if let Ok(mut guard) = CACHE.lock() {
        if let Some((cached_pid, cached_label)) = *guard {
            if cached_pid == pid {
                return cached_label;
            }
        }
        let label = read_serve_mode_label();
        *guard = Some((pid, label));
        label
    } else {
        // Lock poisoned (only happens if a previous holder panicked
        // while reading the file); fall back to a fresh read so the
        // status bar still works.
        read_serve_mode_label()
    }
}

fn read_serve_mode_label() -> Option<&'static str> {
    let dir = crate::session::get_app_dir().ok()?;
    let raw = std::fs::read_to_string(dir.join("serve.mode")).ok()?;
    match raw.trim() {
        "local" => Some("local"),
        "tunnel" => Some("tunnel"),
        "tailscale" => Some("tailscale"),
        _ => None,
    }
}

/// Cross-platform check that `pid` belongs to an aoe / agent-of-empires
/// process. PIDs get recycled, so `kill(pid, 0) == Ok` is not enough on
/// its own — we also want to know it's actually *our* daemon.
///
/// Returns `true` if the process looks like ours, `false` otherwise.
/// If we can't determine either way (platform lacks the lookup, ps
/// missing), we return `true` so behavior matches the legacy Linux path
/// of trusting the PID file rather than falsely flagging a real daemon
/// as foreign.
fn verify_pid_is_aoe(pid: i32) -> bool {
    // Linux fast path: read /proc directly, no subprocess.
    let proc_path = format!("/proc/{}/cmdline", pid);
    if std::path::Path::new(&proc_path).exists() {
        if let Ok(cmdline) = std::fs::read_to_string(&proc_path) {
            return cmdline.contains("aoe") || cmdline.contains("agent-of-empires");
        }
    }

    // macOS / other: shell out to `ps`. `-o command=` prints the full
    // command (path + args) with no header.
    match std::process::Command::new("ps")
        .args(["-o", "command=", "-p", &pid.to_string()])
        .output()
    {
        Ok(out) if out.status.success() => {
            let s = String::from_utf8_lossy(&out.stdout);
            s.contains("aoe") || s.contains("agent-of-empires")
        }
        // ps failed or unavailable — we can't verify, so trust the PID
        // file rather than ghosting a real daemon.
        _ => true,
    }
}

/// Returns Some(pid) if the daemon's PID file exists AND the process is
/// still alive AND it looks like one of our aoe processes. Cleans up
/// stale PID files it finds. The TUI uses this both to jump straight to
/// the Active state when the Remote Access dialog opens and to render
/// the "● Remote on" status-bar indicator.
pub fn daemon_pid() -> Option<u32> {
    let path = pid_file_path().ok()?;
    let pid_str = std::fs::read_to_string(&path).ok()?;
    let pid: i32 = pid_str.trim().parse().ok()?;

    match nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None) {
        Ok(()) => {
            if verify_pid_is_aoe(pid) {
                Some(pid as u32)
            } else {
                // PID was recycled by an unrelated process — our daemon
                // is dead. Clean up the stale file so subsequent callers
                // don't keep false-positive-ing.
                let _ = std::fs::remove_file(&path);
                if let Ok(dir) = crate::session::get_app_dir() {
                    let _ = std::fs::remove_file(dir.join("serve.url"));
                    let _ = std::fs::remove_file(dir.join("serve.log"));
                    let _ = std::fs::remove_file(dir.join("serve.mode"));
                    let _ = std::fs::remove_file(dir.join("serve.passphrase"));
                }
                None
            }
        }
        Err(_) => {
            // Stale PID file; the ESRCH case is handled the same as any
            // other error — the process is not reachable.
            let _ = std::fs::remove_file(&path);
            None
        }
    }
}

pub async fn run(profile: &str, args: ServeArgs) -> Result<()> {
    if args.stop {
        return stop_daemon();
    }

    let is_localhost = args.host == "localhost"
        || args
            .host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback());

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

    // Passphrase strength check
    if let Some(ref passphrase) = args.passphrase {
        if let Some(warning) = crate::server::login::check_passphrase_strength(passphrase) {
            eprintln!("{}", warning);
            eprintln!();
        }
    }

    // Block remote mode without passphrase
    if args.remote && args.passphrase.is_none() {
        bail!(
            "Refusing to start in remote mode without a passphrase.\n\
             --remote exposes terminal access to the internet.\n\
             Add --passphrase <VALUE> or set AOE_SERVE_PASSPHRASE."
        );
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
        no_tailscale: args.no_tailscale,
        is_daemon: false,
        passphrase: args.passphrase.as_deref(),
    })
    .await;

    // Clean up PID and URL files on exit, but only if the PID file
    // still belongs to this process. A newer daemon spawn may have
    // overwritten it; removing their file would orphan them.
    if let Ok(path) = pid_file_path() {
        let is_ours = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| s.trim().parse::<u32>().ok())
            .is_some_and(|pid| pid == std::process::id());
        if is_ours {
            let _ = std::fs::remove_file(&path);
            if let Ok(dir) = crate::session::get_app_dir() {
                let _ = std::fs::remove_file(dir.join("serve.url"));
                let _ = std::fs::remove_file(dir.join("serve.mode"));
                let _ = std::fs::remove_file(dir.join("serve.passphrase"));
            }
        }
    }

    result
}

/// Path to the daemon's combined stdout/stderr log.
/// Kept alongside the PID file so the TUI can tail it while the
/// server runs without attaching to the process directly.
pub fn daemon_log_path() -> Result<PathBuf> {
    let dir = crate::session::get_app_dir()?;
    Ok(dir.join("serve.log"))
}

fn start_daemon(profile: &str, args: &ServeArgs) -> Result<()> {
    use std::process::{Command, Stdio};

    // Refuse to spawn if another daemon is already running. The TUI
    // dialog checks daemon_pid() before reaching here, but a CLI user
    // could call `aoe serve --daemon` twice, and a race between the
    // TUI check and this function could overwrite the PID file and
    // orphan the existing daemon.
    if let Some(existing) = daemon_pid() {
        bail!(
            "A serve daemon is already running (PID {}). \
             Stop it first with `aoe serve --stop`.",
            existing
        );
    }

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
    if args.no_tailscale {
        cmd.arg("--no-tailscale");
    }
    if let Some(ref passphrase) = args.passphrase {
        // Pass via env var to avoid exposing the passphrase in the process list
        cmd.env("AOE_SERVE_PASSPHRASE", passphrase);
    }
    if !profile.is_empty() {
        cmd.args(["--profile", profile]);
    }

    cmd.stdin(Stdio::null());

    // Create a new session so the daemon is not killed by SIGHUP when the
    // parent terminal closes. setsid() is async-signal-safe.
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid() is async-signal-safe per POSIX, which is the
        // only requirement for pre_exec closures.
        unsafe {
            cmd.pre_exec(|| {
                nix::unistd::setsid().map_err(std::io::Error::other)?;
                Ok(())
            });
        }
    }

    // Redirect stdout/stderr to a log file so controllers like the TUI can
    // tail the daemon's output. Truncate on each start so stale content from
    // a prior run doesn't confuse the UI.
    let log_path = daemon_log_path().ok();
    match log_path
        .as_ref()
        .and_then(|p| std::fs::File::create(p).ok().map(|f| (p.clone(), f)))
    {
        Some((_, log_file)) => {
            let stdout = log_file.try_clone()?;
            let stderr = log_file;
            cmd.stdout(Stdio::from(stdout)).stderr(Stdio::from(stderr));
        }
        None => {
            cmd.stdout(Stdio::null()).stderr(Stdio::null());
        }
    }

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

    // Verify PID belongs to an aoe process on all platforms
    if !verify_pid_is_aoe(pid) {
        std::fs::remove_file(&path)?;
        bail!(
            "PID {} belongs to a different process (stale PID file). Cleaned up.",
            pid
        );
    }

    // Send SIGTERM
    match nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(pid),
        nix::sys::signal::Signal::SIGTERM,
    ) {
        Ok(()) => {
            // Wait for the process to actually exit so the port is
            // released before a new daemon can be spawned. Without
            // this, closing the dialog and immediately reopening
            // races with the dying daemon and can orphan it.
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            loop {
                std::thread::sleep(std::time::Duration::from_millis(50));
                match nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None) {
                    Err(nix::errno::Errno::ESRCH) => break,
                    _ if std::time::Instant::now() >= deadline => {
                        // Still alive after timeout; escalate.
                        let _ = nix::sys::signal::kill(
                            nix::unistd::Pid::from_raw(pid),
                            nix::sys::signal::Signal::SIGKILL,
                        );
                        std::thread::sleep(std::time::Duration::from_millis(50));
                        break;
                    }
                    _ => {}
                }
            }
            // The daemon's own cleanup may have already removed some
            // of these; that's fine.
            let _ = std::fs::remove_file(&path);
            if let Ok(dir) = crate::session::get_app_dir() {
                let _ = std::fs::remove_file(dir.join("serve.url"));
                let _ = std::fs::remove_file(dir.join("serve.log"));
                let _ = std::fs::remove_file(dir.join("serve.mode"));
                let _ = std::fs::remove_file(dir.join("serve.passphrase"));
            }
            println!("Stopped aoe serve daemon (PID {})", pid);
        }
        Err(nix::errno::Errno::ESRCH) => {
            // Process doesn't exist; clean up stale PID file
            std::fs::remove_file(&path)?;
            if let Ok(dir) = crate::session::get_app_dir() {
                let _ = std::fs::remove_file(dir.join("serve.url"));
                let _ = std::fs::remove_file(dir.join("serve.log"));
                let _ = std::fs::remove_file(dir.join("serve.mode"));
                let _ = std::fs::remove_file(dir.join("serve.passphrase"));
            }
            println!("Daemon was not running (stale PID file cleaned up)");
        }
        Err(e) => bail!("Failed to stop daemon (PID {}): {}", pid, e),
    }

    Ok(())
}
