# Web Dashboard (experimental)

> This feature is experimental and subject to major changes.

The web dashboard lets you monitor and interact with agent sessions from any browser, including your phone, tablet, or another computer. It runs as an embedded web server inside the `aoe` binary.

## Availability

The web dashboard is included in release binaries downloaded from [GitHub Releases](https://github.com/njbrake/agent-of-empires/releases) and the [quick install script](../installation.md#quick-install-recommended). No extra build steps needed, just run `aoe serve`.

> **Homebrew:** The Homebrew formula (`brew install aoe`) does not yet include the web dashboard since the feature is still experimental. Use the install script or build from source to get `aoe serve`.

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

# Remote access via Cloudflare Tunnel (HTTPS, QR code pairing)
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

This starts a [Cloudflare Tunnel](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/) that gives you a public HTTPS URL with a QR code. No account, DNS, or certificate setup needed.

**Requirements:** `cloudflared` must be installed on the host:
- macOS: `brew install cloudflared`
- Linux: `sudo apt install cloudflared`
- Other: https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/

**Named tunnels** provide a stable domain (useful for bookmarks and future passkey support):

```bash
# One-time setup
cloudflared tunnel create my-tunnel
# Add a CNAME record: aoe.example.com -> <tunnel-id>.cfargotunnel.com

# Run with stable URL
aoe serve --remote --tunnel-name my-tunnel --tunnel-url aoe.example.com
```

## Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--port` | 8080 | Port to listen on |
| `--host` | 127.0.0.1 | Bind address. Use `0.0.0.0` for LAN/VPN access |
| `--remote` | off | Expose via Cloudflare Tunnel (HTTPS, QR code) |
| `--tunnel-name` | | Use a named tunnel (requires `--remote`) |
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
