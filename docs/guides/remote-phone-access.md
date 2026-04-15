# Remote Access from Your Phone

Start agents on your laptop. Check on them from your phone.

## Four steps

1. **Install `aoe`** — see [Installation](../installation.md). You'll also need [`cloudflared`](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/) (`brew install cloudflared` on macOS, `sudo apt install cloudflared` on Debian/Ubuntu). No Cloudflare account required.
2. **Launch the TUI**: `aoe`.
3. **Press `R`**, confirm, and wait ~10 seconds for the Cloudflare tunnel to come up.
4. **Scan the QR code** with your phone camera, then type the displayed four-word passphrase.

You're in. Tap **Share → Add to Home Screen** (iOS) or **three-dot menu → Install** (Android Chrome) and the dashboard installs as a PWA — launches from your home screen, standalone window, no browser chrome.

## How it's protected

- **HTTPS end-to-end** via Cloudflare.
- **Two factors**: the auth token embedded in the QR URL, plus the passphrase typed on the login page. Either alone is useless.
- Tunnel stays up as a background daemon after you close the TUI. Press `R` again anytime to reattach, press `S` to stop, or run `aoe serve --stop` from a shell.

Don't screenshot the QR and passphrase together, and stop the tunnel when you're done.

## Troubleshooting

- **401 or "missing auth token"** — scan the QR, not a screenshot of the URL without the `?token=...` query.
- **QR never appears** — `cloudflared --version` should work from the same shell you launched `aoe` from.
- **Started `aoe serve` from the CLI instead** — press `R` in the TUI; it attaches to the running daemon.
