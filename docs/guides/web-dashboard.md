# Web Dashboard (experimental)

> This feature is experimental and subject to major changes.

The web dashboard lets you monitor and interact with agent sessions from any browser -- your phone, tablet, or another computer. It runs as an embedded web server inside the `aoe` binary.

## Prerequisites

The web dashboard requires the `serve` Cargo feature, which adds a Node.js/npm build dependency:

- [Node.js](https://nodejs.org/) (v18+) with npm

TUI-only builds (`cargo build` without `--features serve`) do not need Node.js.

## Building

```bash
cargo build --release --features serve
```

The build automatically runs `npm install && npm run build` in the `web/` directory to compile the React frontend. The output is embedded in the binary -- no separate files to deploy.

## Starting the server

```bash
# Localhost only (safe, default)
aoe serve

# Accessible from other devices on your network
aoe serve --host 0.0.0.0

# Run in background (for PWA use)
aoe serve --daemon

# Read-only monitoring (no terminal input)
aoe serve --host 0.0.0.0 --read-only
```

The server prints a URL with an auth token:

```
aoe web dashboard running at:
  http://localhost:8080/?token=abc123def456
```

Open this URL in any browser to access the dashboard.

## Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--port` | 8080 | Port to listen on |
| `--host` | 127.0.0.1 | Bind address. Use `0.0.0.0` for LAN/VPN access |
| `--no-auth` | off | Disable token auth (localhost only -- blocked with `0.0.0.0`) |
| `--read-only` | off | View terminals but cannot send keystrokes or stop/restart sessions |
| `--daemon` | off | Fork to background and detach from terminal |

## Security

**The web dashboard exposes terminal access over HTTP.** Anyone with the auth token can send keystrokes to your agent sessions, which run as your user.

**Safe usage patterns:**

- **Localhost** (`aoe serve`): Same security as the TUI. Fine.
- **Over Tailscale/WireGuard** (`aoe serve --host 0.0.0.0`): The VPN encrypts traffic. This is the recommended way to access remotely.
- **Read-only over LAN** (`aoe serve --host 0.0.0.0 --read-only`): Monitor sessions from your phone without input capability.

**Dangerous:**

- `aoe serve --host 0.0.0.0` on public WiFi without a VPN -- the token is transmitted in cleartext HTTP
- `aoe serve --no-auth --host 0.0.0.0` -- this is blocked and will refuse to start

**Blocked combinations:**

The server refuses to start with `--no-auth` and a non-localhost `--host`. This prevents accidental exposure of unauthenticated terminal access to the network.

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
# Stop with: kill <PID>
```

## Features

- **Session list** with live status updates (Running, Waiting, Idle, Error)
- **Live terminal** via PTY relay -- full terminal experience with all key sequences
- **Stop/restart** sessions from the browser
- **Mobile-responsive** layout (sidebar collapses on small screens)
- **Multi-profile** support (shows sessions from all profiles)

## Architecture

The server embeds an axum web server that serves a React frontend and provides:

- REST API for session listing and control (`/api/sessions`)
- WebSocket PTY relay for terminal streaming (`/sessions/:id/ws`)
- Token-based authentication via cookie, query parameter, or WebSocket protocol header

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
