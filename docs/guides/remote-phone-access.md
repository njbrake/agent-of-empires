# Remote Access from Your Phone

> Experimental — part of the web dashboard, which is still evolving.

You can start agents on your laptop, go about your day, and check on them from your phone by scanning a QR code from the TUI. This guide takes about 60 seconds to walk through.

## Prerequisite

Install [`cloudflared`](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/):

```bash
# macOS
brew install cloudflared

# Debian/Ubuntu
sudo apt install cloudflared
```

No Cloudflare account needed — `aoe` uses a Cloudflare *quick tunnel*, which is anonymous and ephemeral.

## Enable from the TUI

1. Launch `aoe` and press `R`.
2. Read the security confirmation and press `Y` to enable.
3. Wait 5–15 seconds for the tunnel to come up.
4. Scan the QR code with your phone camera.
5. When the browser opens, enter the displayed passphrase.

That's it. Your phone is now inside the web dashboard. Any agent sessions you had running on your laptop are visible and interactive.

## While it's running

- The tunnel runs as a background daemon (the same one as `aoe serve --daemon`). It **survives TUI quits** — you can close the TUI and the tunnel keeps going.
- Press `R` in the TUI any time to reopen the dialog; it reattaches to the running daemon and shows the URL again.
- Press `S` inside the dialog to stop the daemon (equivalent to `aoe serve --stop`).
- `Esc` just closes the dialog without touching the daemon.
- If the daemon has been open for 8+ hours, the dialog header highlights as a passive nudge — nothing is killed.
- The dialog shows the last 200 lines of the daemon's log so you can see what's happening under the hood.

## Enabling via CLI instead

If you prefer the command line, the same behavior is available via `aoe serve --remote --passphrase <value>`. The TUI toggle is a friendlier wrapper that generates the passphrase and URL for you and shows the QR code in-place. See [Web Dashboard](web-dashboard.md) for the full `aoe serve` reference.

## Security notes

- The tunnel URL alone is useless — the web dashboard requires the passphrase to log in.
- If the passphrase leaks, someone else can run commands as you on this machine. Don't paste it into screenshots, don't post it on social, and stop the tunnel when you're done.
- The quick tunnel URL rotates every time you re-enable it. Phones paired with an old URL will need to re-scan the new QR.
- Each tunnel session gets a freshly generated passphrase. Nothing is persisted to disk.

## Troubleshooting

**"Cloudflare tunnel did not announce a URL within 60s."** Usually means `cloudflared` isn't installed or isn't on `$PATH`. Open a fresh shell, run `cloudflared --version`, and retry. The dialog leaves the daemon running — use `aoe serve --stop` if you want to fully reset before retrying.

**"`aoe serve --remote --daemon` exited before the tunnel came up."** The daemon crashed at startup, most often because the chosen port was taken or `cloudflared` is missing. The dialog shows the last few log lines so you can see why; reopen the dialog to get a new port.

**QR code looks garbled.** Widen your terminal window — the QR uses text cells and needs room. You can also scan the plain URL shown beneath the QR.

**I started `aoe serve` from the CLI — does `R` still work?** Yes. The dialog detects the existing daemon and jumps straight to Active, showing the URL. Since it doesn't know the passphrase you typed at the CLI, the passphrase field shows "(set when the daemon started)" instead of the value.
