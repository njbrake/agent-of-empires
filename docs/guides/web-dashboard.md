# Web Dashboard

The web dashboard lets you monitor and interact with agent sessions from any browser, including your phone, tablet, or another computer. It runs as an embedded web server inside the `aoe` binary.

## Availability

The web dashboard is included in all release binaries: [GitHub Releases](https://github.com/njbrake/agent-of-empires/releases), the [quick install script](../installation.md#quick-install-recommended), and Homebrew (`brew install aoe`). No extra build steps needed, just run `aoe serve`.

## Building from source

If building from source, you need the `serve` Cargo feature and Node.js/npm:

```bash
cargo build --release --features serve
```

The build automatically runs `npm install && npm run build` in the `web/` directory to compile the React frontend. The output is embedded in the binary, so there are no separate files to deploy.

## Starting the server

```bash
# Localhost only (safe, default)
aoe serve

# Remote access over HTTPS (Tailscale Funnel if available, else Cloudflare quick tunnel)
aoe serve --remote

# Accessible from other devices on your LAN/VPN (HTTP, requires VPN)
aoe serve --host 0.0.0.0

# Run in background
aoe serve --daemon

# Read-only monitoring (no terminal input)
aoe serve --remote --read-only
```

The server prints a URL with an auth token:

```
aoe web dashboard running at:
  http://localhost:8080/?token=a1b2c3...
```

Open this URL in any browser to access the dashboard. The token is set as a cookie on first visit so you don't need to keep it in the URL.

In `--remote` mode, a QR code is also printed for easy phone pairing.

## Remote access

The `--remote` flag is the recommended way to access the dashboard from your phone or another device:

```bash
aoe serve --remote
```

aoe picks a transport automatically in this order:

### 1. Tailscale Funnel (preferred when available)

If `tailscale` is on the host's PATH and the daemon is logged in, aoe runs `tailscale serve` + `tailscale funnel` and exposes the dashboard at your stable `https://<machine>.<tailnet>.ts.net` URL. No domain, no Cloudflare account, no rotating URLs. **This is the only option where a PWA installed on your phone keeps working across server restarts** (the URL is stable).

Setup:
1. Install Tailscale on the host ([tailscale.com/download](https://tailscale.com/download))
2. `tailscale up`
3. Enable Funnel once in the admin console or tailnet ACL: [login.tailscale.com/admin/acls/file](https://login.tailscale.com/admin/acls/file)
4. `aoe serve --remote`

### 2. Named Cloudflare tunnel

Stable hostname on your own Cloudflare-managed domain. Takes precedence over Tailscale auto-detection when you pass the flags:

```bash
# One-time setup
cloudflared tunnel create my-tunnel
# Add a CNAME record: aoe.example.com -> <tunnel-id>.cfargotunnel.com

# Run with stable URL
aoe serve --remote --tunnel-name my-tunnel --tunnel-url aoe.example.com
```

### 3. Cloudflare quick tunnel (fallback)

Zero-config but the URL rotates on every restart. Fine for one-off remote sessions, **bad for installed PWAs**: the home-screen app is bound to the URL it was installed from, so every restart costs you a delete-and-reinstall.

```bash
aoe serve --remote
```

Requires `cloudflared` on the host:
- macOS: `brew install cloudflared`
- Linux: `sudo apt install cloudflared`
- Other: [Cloudflare's downloads page](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/)

aoe prints a notice when it falls back to this path so you don't accidentally install a PWA from a rotating URL.

## Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--port` | 8080 | Port to listen on |
| `--host` | 127.0.0.1 | Bind address. Use `0.0.0.0` for LAN/VPN access |
| `--remote` | off | Expose over HTTPS tunnel (Tailscale Funnel if available, else Cloudflare quick tunnel) |
| `--tunnel-name` | | Use a named Cloudflare tunnel (requires `--remote`; overrides Tailscale auto-detection) |
| `--tunnel-url` | | Hostname for a named tunnel (requires `--tunnel-name`) |
| `--no-auth` | off | Disable token auth (localhost only) |
| `--read-only` | off | View terminals but cannot send keystrokes |
| `--daemon` | off | Fork to background and detach from terminal |
| `--stop` | | Stop a running daemon |

## Security

**The web dashboard exposes terminal access.** Anyone who authenticates can send keystrokes to your agent sessions, which run as your user.

### Authentication

- **Token auth:** A random 256-bit token is generated on startup and stored at `~/.config/agent-of-empires/serve.token` (Linux) or `~/.agent-of-empires/serve.token` (macOS). The token is passed via URL on first visit, then stored as an `HttpOnly; SameSite=Strict` cookie.
- **Rate limiting:** 5 failed auth attempts from an IP trigger a 15-minute lockout. Uses `Cf-Connecting-IP` when behind a Cloudflare tunnel to prevent IP spoofing.
- **Token rotation:** In `--remote` mode, the token rotates every 4 hours with a 5-minute grace period for active sessions.
- **Device tracking:** Connected devices (IP, browser, last seen) are visible in Settings > Security.

### Security headers

The server sets `X-Frame-Options: DENY` (prevents clickjacking), `X-Content-Type-Options: nosniff`, and `Referrer-Policy: no-referrer` (prevents token leaking via Referer headers).

### Safe usage patterns

- **Localhost** (`aoe serve`): Same security as the TUI. Fine.
- **Remote via tunnel** (`aoe serve --remote`): Encrypted via HTTPS. Recommended for phone access.
- **Over Tailscale/WireGuard** (`aoe serve --host 0.0.0.0`): The VPN encrypts traffic.
- **Read-only** (`aoe serve --remote --read-only`): Monitor sessions without input capability.

### Dangerous

- `aoe serve --host 0.0.0.0` on public WiFi without a VPN: traffic is unencrypted HTTP
- `aoe serve --no-auth --host 0.0.0.0`: blocked (refuses to start)
- `aoe serve --no-auth --remote`: blocked (refuses to start)

## Installing as a PWA

The dashboard supports Progressive Web App (PWA) installation for an app-like experience:

**macOS (Chrome):** Three-dot menu > "Install Agent of Empires" -- creates a standalone window with a Dock icon.

**macOS (Safari):** File > Add to Dock.

**iOS:** Share > Add to Home Screen.

**Android:** Chrome will prompt "Add to Home Screen" or show an install banner.

The PWA requires the server to be running. Use `--daemon` to keep it running in the background:

```bash
aoe serve --daemon
# Server runs in background, prints PID
# Stop with: aoe serve --stop
```

## Features

- **Session list** with live status updates (Running, Waiting, Idle, Error)
- **Live terminal** via PTY relay, full terminal experience with all key sequences
- **Stop/restart** sessions from the browser
- **Mobile-responsive** layout (sidebar collapses on small screens)
- **Multi-profile** support (shows sessions from all profiles)
- **Connected Devices** view in Settings > Security

## Architecture

The server embeds an axum web server that serves a React frontend and provides:

- REST API for session listing and control (`/api/sessions`)
- WebSocket PTY relay for terminal streaming (`/sessions/:id/ws`)
- Token-based authentication via cookie, query parameter, or WebSocket protocol header
- Rate limiting, token rotation, and device tracking
- Security headers (X-Frame-Options, Referrer-Policy)

Each terminal connection spawns `tmux attach-session` inside a PTY and relays the raw byte stream bidirectionally over WebSocket. This gives the browser a real terminal experience identical to SSH.

## Frontend development

The React frontend lives in `web/`:

```bash
cd web
npm install
npm run dev     # Vite dev server with HMR on port 5173
```

For API/WebSocket requests, run the Rust server simultaneously:

```bash
cargo run --features serve -- serve
```

The Vite dev server proxies API requests to the Rust server (configure in `vite.config.ts` if needed).
