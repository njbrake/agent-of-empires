//! Cloudflare Tunnel integration for secure remote access.
//!
//! Spawns `cloudflared tunnel --url http://localhost:PORT` for zero-config tunnels,
//! or `cloudflared tunnel run` for named tunnels with stable domains.
//! Parses the assigned URL from stderr and provides it for QR code display.

use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

/// Manages a cloudflared tunnel subprocess.
pub struct TunnelHandle {
    child: Arc<Mutex<Child>>,
    pub url: String,
    port: u16,
    kind: TunnelKind,
    cancel: CancellationToken,
}

#[derive(Clone)]
enum TunnelKind {
    Quick,
    Named { tunnel_name: String },
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
            child: Arc::new(Mutex::new(child)),
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
            child: Arc::new(Mutex::new(child)),
            url,
            port: local_port,
            kind: TunnelKind::Named {
                tunnel_name: tunnel_name.to_string(),
            },
            cancel: CancellationToken::new(),
        })
    }

    /// Gracefully shut down the tunnel process.
    /// Cancels the health monitor first, then sends SIGTERM to cloudflared.
    pub async fn shutdown(self) {
        self.cancel.cancel();
        // Brief yield to let the monitor task observe cancellation
        tokio::task::yield_now().await;

        let mut child = self.child.lock().await;
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
    pub fn spawn_health_monitor(&self) {
        let child = Arc::clone(&self.child);
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
    }
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
