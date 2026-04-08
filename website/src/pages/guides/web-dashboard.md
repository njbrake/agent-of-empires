---
layout: ../../layouts/Docs.astro
title: "Web Dashboard (Experimental)"
description: Remote access to AI coding agent sessions from any browser with Agent of Empires.
---

> **Experimental**: This feature is under active development and subject to major changes.

The web dashboard lets you monitor and interact with agent sessions from any browser -- your phone, tablet, or another computer. It runs as an embedded web server inside the `aoe` binary.

## Prerequisites

The web dashboard requires the `serve` Cargo feature, which adds a Node.js build dependency:

- [Node.js](https://nodejs.org/) (v18+) with npm

TUI-only builds (`cargo build` without `--features serve`) do not need Node.js.

## Building

```bash
cargo build --release --features serve
```

The build automatically runs `npm install && npm run build` in the `web/` directory. The output is embedded in the binary.

## Starting the server

```bash
# Localhost only (default, safe)
aoe serve

# Accessible from other devices on your network
aoe serve --host 0.0.0.0

# Run in background
aoe serve --daemon

# Stop a background server
aoe serve --stop

# Read-only monitoring (no terminal input)
aoe serve --host 0.0.0.0 --read-only
```

The server prints a URL with an auth token. Open it in any browser.

## Flags

| Flag | Default | Description |
|------|---------|-------------|
| `--port` | 8080 | Port to listen on |
| `--host` | 127.0.0.1 | Bind address. Use `0.0.0.0` for LAN/VPN access |
| `--no-auth` | off | Disable token auth (localhost only) |
| `--read-only` | off | View terminals without input capability |
| `--daemon` | off | Run in background |
| `--stop` | - | Stop a running background server |

## Security

The web dashboard exposes terminal access over HTTP. Anyone with the auth token can interact with your agent sessions.

**Recommended setup**: Use a VPN like [Tailscale](https://tailscale.com/) or WireGuard for remote access. The VPN encrypts traffic so the auth token cannot be intercepted.

**Safety rules**:
- `--no-auth` is blocked when binding to non-localhost addresses
- `--read-only` disables all terminal input and session control
- Default binding is `127.0.0.1` (localhost only)

## Installing as a PWA

The dashboard is a Progressive Web App. Install it for an app-like experience:

- **macOS (Chrome)**: Menu > "Install Agent of Empires"
- **macOS (Safari)**: File > Add to Dock
- **iOS**: Share > Add to Home Screen
- **Android**: Chrome prompts "Add to Home Screen"

Use `--daemon` so the server stays running when you close the terminal.

## Features

- Session list with live status (Running, Waiting, Idle, Error)
- Full terminal access via PTY relay (all key sequences work)
- Stop and restart sessions from the browser
- Mobile-responsive layout
- Multi-profile session loading
