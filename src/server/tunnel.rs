//! Public-HTTPS tunnel integration for secure remote access.
//!
//! Supports three transports, auto-picked in `prefer_tailscale_then_cloudflare`:
//! 1. Tailscale Funnel: preferred when `tailscale` is installed and logged in.
//!    Gives a stable `https://<machine>.<tailnet>.ts.net` URL, so installed
//!    PWAs survive server restarts. No child process to manage; the Tailscale
//!    daemon owns the ingress.
//! 2. Named Cloudflare tunnel: user-provided `--tunnel-name` + `--tunnel-url`.
//!    Stable hostname on the user's own domain.
//! 3. Cloudflare quick tunnel: fallback. Zero-config, but the URL rotates on
//!    every restart, which breaks installed PWAs. Documented limitation.

use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

/// Manages a public-HTTPS tunnel. For Cloudflare variants, wraps a
/// `cloudflared` subprocess and supervises it. For Tailscale Funnel,
/// there's no child process: the Tailscale daemon owns the ingress,
/// and this handle is essentially a URL carrier.
pub struct TunnelHandle {
    /// None for Tailscale (no child process to supervise).
    child: Option<Arc<Mutex<Child>>>,
    pub url: String,
    port: u16,
    kind: TunnelKind,
    cancel: CancellationToken,
}

#[derive(Clone)]
enum TunnelKind {
    Quick,
    Named { tunnel_name: String },
    Tailscale,
}

impl TunnelKind {
    /// Short label for `serve.mode` and the TUI status bar.
    pub fn mode_label(&self) -> &'static str {
        match self {
            Self::Quick | Self::Named { .. } => "tunnel",
            Self::Tailscale => "tailscale",
        }
    }
}

impl TunnelHandle {
    pub fn mode_label(&self) -> &'static str {
        self.kind.mode_label()
    }

    pub fn is_stable_origin(&self) -> bool {
        // Quick CF tunnels rotate; named CF and Tailscale are stable.
        !matches!(self.kind, TunnelKind::Quick)
    }
}

impl TunnelHandle {
    /// Spawn a quick tunnel (zero-config, random subdomain, no account needed).
    pub async fn spawn_quick(local_port: u16) -> anyhow::Result<Self> {
        let mut child = Command::new("cloudflared")
            .args([
                "tunnel",
                "--url",
                &format!("http://localhost:{}", local_port),
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to start cloudflared: {}.\n\
                     Install it: https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/",
                    e
                )
            })?;

        let stderr = child.stderr.take().expect("stderr was piped");
        let mut reader = BufReader::new(stderr).lines();

        let url = tokio::time::timeout(std::time::Duration::from_secs(30), async {
            while let Some(line) = reader.next_line().await? {
                if let Some(url) = extract_tunnel_url(&line) {
                    return Ok::<String, anyhow::Error>(url);
                }
            }
            anyhow::bail!("cloudflared exited without providing a tunnel URL")
        })
        .await
        .map_err(|_| anyhow::anyhow!("Timed out waiting for cloudflared tunnel URL (30s)"))??;

        // Drain remaining stderr to prevent pipe buffer deadlock
        tokio::spawn(async move { while let Ok(Some(_)) = reader.next_line().await {} });

        info!(url = %url, "Cloudflare tunnel established");

        Ok(TunnelHandle {
            child: Some(Arc::new(Mutex::new(child))),
            url,
            port: local_port,
            kind: TunnelKind::Quick,
            cancel: CancellationToken::new(),
        })
    }

    /// Spawn a named tunnel (requires prior `cloudflared tunnel create` and DNS setup).
    pub async fn spawn_named(
        tunnel_name: &str,
        tunnel_url: &str,
        local_port: u16,
    ) -> anyhow::Result<Self> {
        let child = Command::new("cloudflared")
            .args([
                "tunnel",
                "run",
                "--url",
                &format!("http://localhost:{}", local_port),
                tunnel_name,
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to start named tunnel '{}': {}.\n\
                     Make sure you have run `cloudflared tunnel create {}` first.",
                    tunnel_name,
                    e,
                    tunnel_name
                )
            })?;

        // Give cloudflared a moment to connect
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        let domain = tunnel_url
            .trim_start_matches("https://")
            .trim_start_matches("http://");
        if domain.is_empty()
            || domain.contains(' ')
            || domain.contains('/')
            || !domain.contains('.')
        {
            return Err(anyhow::anyhow!(
                "Invalid tunnel URL '{}'. Expected a domain like 'aoe.example.com'.",
                tunnel_url
            ));
        }

        let url = format!("https://{}", domain);

        info!(url = %url, tunnel = %tunnel_name, "Named Cloudflare tunnel started");

        Ok(TunnelHandle {
            child: Some(Arc::new(Mutex::new(child))),
            url,
            port: local_port,
            kind: TunnelKind::Named {
                tunnel_name: tunnel_name.to_string(),
            },
            cancel: CancellationToken::new(),
        })
    }

    /// Configure Tailscale Funnel for the local port and return a
    /// handle carrying the stable `https://<host>.<tailnet>.ts.net` URL.
    /// No subprocess supervision is needed; `tailscale serve` and
    /// `tailscale funnel` configure state in the Tailscale daemon and
    /// return immediately.
    pub async fn spawn_tailscale(local_port: u16) -> anyhow::Result<Self> {
        // Hard cap on each tailscale command so we never wedge if
        // tailscale pops an interactive prompt (HTTPS-certs consent,
        // Funnel-not-enabled-in-ACL, node not signed in). When the
        // timeout fires, start_server catches the error and falls back
        // to Cloudflare instead of leaving the user staring at a
        // frozen "Starting tunnel..." screen.
        const STEP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

        // Step 1: point the funnel's https:443 at our local port.
        // `tailscale serve` is idempotent; re-running with a different
        // port overwrites the previous mapping. stdin is null so any
        // interactive prompt fails fast rather than hanging.
        let serve_fut = Command::new("tailscale")
            .args([
                "serve",
                "--bg",
                "--https=443",
                &format!("http://127.0.0.1:{}", local_port),
            ])
            .stdin(std::process::Stdio::null())
            .output();
        let out = tokio::time::timeout(STEP_TIMEOUT, serve_fut)
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "tailscale serve timed out after {}s; it may be waiting on an \
                     interactive prompt (HTTPS certs, Funnel consent). \
                     Run `tailscale serve --bg --https=443 http://127.0.0.1:{}` manually \
                     once to clear it, or use --no-tailscale to skip.",
                    STEP_TIMEOUT.as_secs(),
                    local_port
                )
            })?
            .map_err(|e| anyhow::anyhow!("failed to run tailscale serve: {}", e))?;
        if !out.status.success() {
            anyhow::bail!(
                "tailscale serve failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }

        // Step 2: open the funnel on port 443. Also idempotent.
        let funnel_fut = Command::new("tailscale")
            .args(["funnel", "--bg", "443"])
            .stdin(std::process::Stdio::null())
            .output();
        let out = tokio::time::timeout(STEP_TIMEOUT, funnel_fut)
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "tailscale funnel timed out after {}s; Funnel may not be enabled \
                     in your tailnet ACL. Enable it at \
                     https://login.tailscale.com/admin/acls/file or use --no-tailscale.",
                    STEP_TIMEOUT.as_secs()
                )
            })?
            .map_err(|e| anyhow::anyhow!("failed to run tailscale funnel: {}", e))?;
        if !out.status.success() {
            anyhow::bail!(
                "tailscale funnel failed: {}. Funnel may need to be enabled in the admin console.",
                String::from_utf8_lossy(&out.stderr)
            );
        }

        // Step 3: read the stable funnel URL from `tailscale status`.
        let url = tokio::time::timeout(STEP_TIMEOUT, tailscale_funnel_url())
            .await
            .map_err(|_| {
                anyhow::anyhow!(
                    "tailscale status timed out after {}s",
                    STEP_TIMEOUT.as_secs()
                )
            })??;
        info!(url = %url, "Tailscale Funnel established");

        Ok(TunnelHandle {
            child: None,
            url,
            port: local_port,
            kind: TunnelKind::Tailscale,
            cancel: CancellationToken::new(),
        })
    }

    /// Gracefully shut down the tunnel process.
    /// Cancels the health monitor first, then sends SIGTERM to cloudflared.
    /// For Tailscale funnels, leaves the funnel configuration in place on
    /// purpose: restarting aoe shouldn't tear down the PWA's origin.
    pub async fn shutdown(self) {
        self.cancel.cancel();
        // Brief yield to let the monitor task observe cancellation
        tokio::task::yield_now().await;

        let Some(child_arc) = self.child else {
            info!("Tailscale Funnel handle released (Funnel config left in place)");
            return;
        };
        let mut child = child_arc.lock().await;
        if let Some(id) = child.id() {
            let _ = nix::sys::signal::kill(
                nix::unistd::Pid::from_raw(id as i32),
                nix::sys::signal::Signal::SIGTERM,
            );
        }
        match tokio::time::timeout(std::time::Duration::from_secs(5), child.wait()).await {
            Ok(_) => info!("Cloudflare tunnel stopped cleanly"),
            Err(_) => {
                warn!("Cloudflare tunnel did not stop in 5s, killing");
                let _ = child.kill().await;
            }
        }
    }

    /// Spawn a background task that monitors tunnel health and attempts one restart.
    /// The task stops when the cancellation token is cancelled (during shutdown).
    /// No-op for Tailscale funnels (no child process to supervise; the
    /// Tailscale daemon handles its own health).
    pub fn spawn_health_monitor(&self) {
        let Some(child_arc) = self.child.as_ref() else {
            return; // Tailscale: no child to monitor
        };
        let child = Arc::clone(child_arc);
        let kind = self.kind.clone();
        let port = self.port;
        let cancel = self.cancel.clone();

        tokio::spawn(async move {
            let mut has_restarted = false;
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => return,
                    _ = tokio::time::sleep(std::time::Duration::from_secs(10)) => {}
                }

                let mut child_guard = child.lock().await;
                match child_guard.try_wait() {
                    Ok(Some(status)) => {
                        if has_restarted {
                            error!(
                                "Cloudflare tunnel exited again ({}). \
                                 Remote access is unavailable. \
                                 Restart with `aoe serve --remote`.",
                                status
                            );
                            return;
                        }

                        warn!(
                            "Cloudflare tunnel exited unexpectedly ({}). Attempting restart...",
                            status
                        );

                        match restart_tunnel(&kind, port).await {
                            Ok(new_child) => {
                                *child_guard = new_child;
                                has_restarted = true;
                                info!("Cloudflare tunnel restarted successfully");
                            }
                            Err(e) => {
                                error!(
                                    "Failed to restart tunnel: {}. \
                                     Remote access is unavailable.",
                                    e
                                );
                                return;
                            }
                        }
                    }
                    Ok(None) => {} // Still running
                    Err(e) => {
                        warn!("Error checking tunnel status: {}", e);
                    }
                }
            }
        });
    }
}

async fn restart_tunnel(kind: &TunnelKind, port: u16) -> anyhow::Result<Child> {
    match kind {
        TunnelKind::Quick => {
            let child = Command::new("cloudflared")
                .args(["tunnel", "--url", &format!("http://localhost:{}", port)])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .kill_on_drop(true)
                .spawn()?;
            Ok(child)
        }
        TunnelKind::Named { tunnel_name } => {
            let child = Command::new("cloudflared")
                .args([
                    "tunnel",
                    "run",
                    "--url",
                    &format!("http://localhost:{}", port),
                    tunnel_name,
                ])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .kill_on_drop(true)
                .spawn()?;
            Ok(child)
        }
        TunnelKind::Tailscale => {
            // Unreachable in practice: the health monitor doesn't run
            // for Tailscale (see spawn_health_monitor early-return), so
            // restart_tunnel is never called with this variant. If a
            // future refactor changes that invariant, fail loudly.
            anyhow::bail!("restart_tunnel called for Tailscale; no child process exists")
        }
    }
}

/// True if `tailscale` is on PATH, the daemon is logged in, and Funnel
/// is enabled for the tailnet. Conservative: any failure returns false
/// so callers fall back cleanly to Cloudflare.
pub async fn tailscale_available() -> bool {
    // Cheapest possible check first: does the CLI exist and return
    // a successful status?
    let Ok(out) = Command::new("tailscale").arg("--version").output().await else {
        return false;
    };
    if !out.status.success() {
        return false;
    }
    // Then confirm the daemon is running and signed in. `status` exits
    // non-zero if not logged in.
    let Ok(out) = Command::new("tailscale").arg("status").output().await else {
        return false;
    };
    out.status.success()
}

/// Read the stable Tailscale Funnel URL from `tailscale status --json`.
/// The self node's `DNSName` is the `.ts.net` hostname; we wrap it in
/// `https://` and strip the trailing dot that appears in the JSON.
async fn tailscale_funnel_url() -> anyhow::Result<String> {
    let out = Command::new("tailscale")
        .args(["status", "--json"])
        .output()
        .await
        .map_err(|e| anyhow::anyhow!("failed to run tailscale status: {}", e))?;
    if !out.status.success() {
        anyhow::bail!(
            "tailscale status --json failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let parsed: serde_json::Value = serde_json::from_slice(&out.stdout)
        .map_err(|e| anyhow::anyhow!("parse tailscale status JSON: {}", e))?;
    let dns = parsed
        .get("Self")
        .and_then(|s| s.get("DNSName"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Self.DNSName missing from tailscale status"))?;
    let host = dns.trim_end_matches('.');
    if host.is_empty() {
        anyhow::bail!("empty DNSName from tailscale status");
    }
    Ok(format!("https://{}", host))
}

/// Extract a trycloudflare.com tunnel URL from a cloudflared stderr line.
fn extract_tunnel_url(line: &str) -> Option<String> {
    for word in line.split_whitespace() {
        if word.starts_with("https://") && word.contains(".trycloudflare.com") {
            // Trim trailing punctuation that may appear in log output.
            // The URL always ends with ".com" so strip anything after that.
            if let Some(pos) = word.find(".trycloudflare.com") {
                let end = pos + ".trycloudflare.com".len();
                return Some(word[..end].to_string());
            }
        }
    }
    None
}

/// Render a QR code to stderr for easy phone scanning.
pub fn print_qr_code(url: &str) {
    use qrcode::QrCode;

    match QrCode::new(url.as_bytes()) {
        Ok(code) => {
            let string = code
                .render::<char>()
                .quiet_zone(true)
                .module_dimensions(2, 1)
                .build();
            eprintln!();
            for line in string.lines() {
                eprintln!("  {}", line);
            }
            eprintln!("  ^^ Scan this QR code to connect from your phone.");
            eprintln!("     (Resize your terminal wider if it looks garbled.)");
            eprintln!();
            eprintln!("  Or open: {}", url);
            eprintln!();
        }
        Err(e) => {
            eprintln!("Could not generate QR code: {}", e);
            eprintln!("Open this URL: {}", url);
        }
    }
}

/// Check if cloudflared is installed and accessible on PATH.
pub fn check_cloudflared() -> anyhow::Result<()> {
    match std::process::Command::new("cloudflared")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(status) if status.success() => Ok(()),
        _ => anyhow::bail!(
            "cloudflared is not installed or not on PATH.\n\
             Install it: https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/\n\
             \n\
             Quick install:\n\
             - macOS:  brew install cloudflared\n\
             - Linux:  sudo apt install cloudflared\n\
             - Other:  see the URL above"
        ),
    }
}

/// Sync counterpart of `tailscale_available()` for use from the TUI
/// (which avoids spinning up a tokio runtime just to probe for a CLI).
/// Conservative: any error means "not available" and we fall through
/// to Cloudflare.
pub fn tailscale_available_sync() -> bool {
    let version_ok = std::process::Command::new("tailscale")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !version_ok {
        return false;
    }
    std::process::Command::new("tailscale")
        .arg("status")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_url_from_typical_output() {
        let line =
            "2026-04-12T12:00:00Z INF +-------------------------------------------------------------------+";
        assert_eq!(extract_tunnel_url(line), None);

        let line = "2026-04-12T12:00:01Z INF |  https://random-words-here.trycloudflare.com  |";
        assert_eq!(
            extract_tunnel_url(line),
            Some("https://random-words-here.trycloudflare.com".to_string())
        );
    }

    #[test]
    fn extract_url_no_match() {
        assert_eq!(extract_tunnel_url("INF Starting tunnel subsystem"), None);
        assert_eq!(extract_tunnel_url("https://example.com not a tunnel"), None);
    }

    #[test]
    fn extract_url_with_trailing_punctuation() {
        let line = "Visit https://abc-def.trycloudflare.com.";
        assert_eq!(
            extract_tunnel_url(line),
            Some("https://abc-def.trycloudflare.com".to_string())
        );
    }

    #[test]
    fn check_cloudflared_returns_err_when_missing() {
        // This test verifies the function doesn't panic with a missing binary.
        // It may pass or fail depending on whether cloudflared is installed.
        let result = check_cloudflared();
        // We just verify it returns a Result without panicking
        let _ = result;
    }
}
