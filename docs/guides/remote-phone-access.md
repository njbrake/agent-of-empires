# Remote Access from Your Phone

Start agents on your laptop. Check on them from your phone.

## Four steps

1. **Install `aoe`** (see [Installation](../installation.md)) and one of the two supported tunnel tools on the host:
   - **Tailscale (preferred):** install from [tailscale.com/download](https://tailscale.com/download), run `tailscale up`, then two one-time clicks to unblock Funnel: enable it for the tailnet at [login.tailscale.com/f/funnel](https://login.tailscale.com/f/funnel), and grant the `funnel` nodeAttr to this node in your ACL at [login.tailscale.com/admin/acls/file](https://login.tailscale.com/admin/acls/file). Free, stable URL, no Cloudflare account, **required if you want to install the dashboard as a PWA and have it survive server restarts**.
   - **cloudflared (fallback):** `brew install cloudflared` on macOS, `sudo apt install cloudflared` on Debian/Ubuntu, no Cloudflare account needed. Gives a working URL but it rotates on every restart, which breaks installed PWAs.
2. **Launch the TUI**: `aoe`.
3. **Press `R`**, pick a transport on the Confirm screen (Tailscale Funnel vs Cloudflare Tunnel, cards show each one's readiness), and wait ~10 seconds for the tunnel to come up.
4. **Scan the QR code** with your phone camera, then type the displayed four-word passphrase.

You're in. Tap **Share → Add to Home Screen** (iOS) or **three-dot menu → Install** (Android Chrome) and the dashboard installs as a PWA: launches from your home screen, standalone window, no browser chrome.

**Important if you install the PWA:** use Tailscale for the tunnel. A PWA installed from a Cloudflare quick-tunnel URL will stop working the next time aoe restarts because the URL changes. aoe prints a warning when falling back to the quick tunnel.

## How it's protected

- **HTTPS end-to-end** via Tailscale or Cloudflare.
- **Two factors**: the auth token embedded in the QR URL, plus the passphrase typed on the login page. Either alone is useless.
- Tunnel stays up as a background daemon after you close the TUI. Press `R` again anytime to reattach, press `S` to stop, or run `aoe serve --stop` from a shell.

Don't screenshot the QR and passphrase together, and stop the tunnel when you're done.

## Troubleshooting

- **401 or "missing auth token"**: scan the QR, not a screenshot of the URL without the `?token=...` query.
- **QR never appears**: either `tailscale status` should report the daemon is logged in, or `cloudflared --version` should work from the same shell you launched `aoe` from.
- **Tailscale card shows "Funnel not enabled for this node"**: the tailnet ACL doesn't grant the `funnel` nodeAttr to this device. If your node is tagged, `autogroup:member` rules don't apply to it — target the tag instead, or add a rule targeting `*`. Save the ACL and press `[R]` on the Confirm screen to re-check.
- **"Tailscale Funnel is not enabled for this tailnet"**: click the node-specific URL shown in the error to flip the tailnet-wide switch at [login.tailscale.com/f/funnel](https://login.tailscale.com/f/funnel). aoe detects this condition in seconds via `tailscale funnel` stderr, so you won't wait out a 60s timeout.
- **"port 443 is already configured on this node"**: a non-loopback Funnel from another tool is using port 443. Press `[R]` on the Error dialog to run `tailscale funnel reset`, then retry. Stale configs from a prior aoe run are fine and get overwritten automatically.
- **Started `aoe serve` from the CLI instead**: press `R` in the TUI; it attaches to the running daemon.
- **Installed PWA stopped working after aoe restart**: you were on a Cloudflare quick tunnel and the URL rotated. Switch to Tailscale Funnel (or a named Cloudflare tunnel with a stable domain), delete the installed PWA, and reinstall from the new stable URL.
