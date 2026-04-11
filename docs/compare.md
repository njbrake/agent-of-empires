# Which version is right for me?

Agent of Empires is, and always has been, a terminal-first session manager for AI coding agents. The TUI (`aoe`) is the original interface and the way most people use it. There are now also a few optional shells around the same core: a macOS desktop app, a web dashboard, and an embeddable Rust library. This page explains the differences so you can pick the right starting point — but you can switch between them at any time, since they all share the same session storage.

## TL;DR

**If you're not sure, install the TUI/CLI.** It's the original interface, the most stable, the most documented, and works on both macOS and Linux. Everything else is optional.

| If you... | Install | Surface |
|---|---|---|
| Want the original AoE experience (recommended) | [`brew install aoe` or the install script](installation.md#install-the-tui--cli-most-users) | TUI + CLI commands, optional web dashboard |
| Use a Mac and prefer a native window + menu bar | [Download the desktop app](installation.md#optional-macos-desktop-app) | Native window + menu bar + phone QR (built on top of the TUI/CLI install) |
| Want a browser/phone interface without the desktop app | TUI/CLI install + run [`aoe serve`](installation.md#optional-web-dashboard-aoe-serve) | Headless web server, accessible over LAN/VPN |
| Want to embed agent management in your own Rust code | [`cargo add agent-of-empires`](installation.md#optional-rust-library) | Crate, no UI |

## Full feature comparison

| | Desktop app | CLI / TUI | CLI + `aoe serve` | Library crate |
|---|---|---|---|---|
| **Best for** | Mac users new to terminals | Terminal natives | Team servers, remote access | Building tools on top |
| **Platforms** | macOS only (v1) | macOS, Linux | macOS, Linux | macOS, Linux, any Rust target |
| **Native window** | ✅ | — | — | — |
| **Terminal UI (TUI)** | — | ✅ | ✅ | — |
| **Web dashboard in browser** | ✅ (built-in) | — | ✅ | — |
| **Phone access via QR code** | ✅ (one click) | — | manual URL | — |
| **Menu bar tray** | ✅ | — | — | — |
| **macOS notifications** | ✅ | — | — | — |
| **Headless / SSH** | — | ✅ | ✅ | ✅ |
| **Background daemon** | ✅ (built-in) | `--daemon` flag | `--daemon` flag | implement yourself |
| **Multiple agents in parallel** | ✅ | ✅ | ✅ | ✅ |
| **Git worktrees** | ✅ | ✅ | ✅ | ✅ |
| **Docker sandboxing** | ✅ | ✅ | ✅ | ✅ |
| **Per-repo config & hooks** | ✅ | ✅ | ✅ | ✅ |
| **Same session storage** | ✅ | ✅ | ✅ | ✅ |
| **Install size (approx)** | ~30 MB | ~15 MB | ~25 MB | depends on features |
| **Requires tmux on host** | bundled | yes | yes | yes (for tmux features) |
| **Requires Node.js to build** | yes (frontend) | no | yes (frontend) | only with `serve` feature |

## They're the same product underneath

The desktop app is not a separate product. Under the hood:

- The desktop app **runs the same web server** as `aoe serve` does, just inside a native macOS window with extra glue for the menu bar and QR pairing.
- All four surfaces **read and write the same session storage** at `~/.agent-of-empires/` (or `$XDG_CONFIG_HOME/agent-of-empires/` on Linux).
- All four use the **same tmux sessions**, so a session you start in the CLI shows up in the desktop app's window and vice versa.
- All four respect the **same per-repo config** in `.agent-of-empires/config.toml`.

This means you can:

- Install the desktop app for daily local use, then SSH into a remote machine and run `aoe` in the terminal — same workflow.
- Run `aoe serve --daemon` on a server, install the desktop app on your laptop, and access the same sessions from both.
- Use the library to build a custom CLI for your team, then have your team install the desktop app for the GUI.

## When to pick which

### Pick the CLI / TUI (recommended starting point)

- You want the original AoE experience, the way most people use it.
- You're on Linux, or you live in a terminal on macOS.
- You want the smallest possible install and the most stable surface.
- You want to use it over SSH on a remote machine.
- You want vim-style keyboard navigation in the terminal.
- You're using it in CI or scripts.

**Trade-offs:** No phone access without manually running `aoe serve` and copying the URL. No system tray or notifications. (You can layer on `aoe serve` or the desktop app later if you want those.)

### Pick the desktop app if...

- You're on a Mac and you'd rather not work in a terminal.
- You want phone access to your sessions without copying a URL into Safari yourself.
- You want a menu bar icon and macOS notifications.
- You want to install via .dmg drag-and-drop instead of `brew` or `cargo`.

**Trade-offs:** macOS only and currently experimental. Bigger install size (~30 MB vs ~15 MB). The desktop window is a WKWebView wrapping the same web dashboard, not a uniquely native UI. The same `aoe` CLI is also installed alongside it, so you can drop into the terminal anytime — installing the desktop app is additive, not exclusive.

### Pick `aoe serve` if...

- You want to run a shared dashboard for a team on a server.
- You want to access your sessions from a phone or tablet without the desktop app.
- You're on Linux and want the web UI.

**Trade-offs:** You're responsible for securing the network exposure (use a VPN like Tailscale, or run behind an SSH tunnel). The auth token travels in plaintext over HTTP, so don't expose this directly to the internet without TLS.

### Pick the library if...

- You're building your own CLI, GUI, or service on top of agent management.
- You want to script session creation, status polling, or worktree operations from Rust.
- You don't want any HTTP, web, or GUI dependencies in your binary.

**Trade-offs:** You write your own UI. The library is the building blocks, not a finished product.

## Switching between surfaces

You don't have to commit to one. Common patterns:

- **Start with the TUI, add the web dashboard later.** Run `aoe serve` whenever you want browser/phone access to the same sessions you've been managing from the terminal. No reinstall needed if you installed via the script or built with `--features serve`.
- **Use the TUI on a remote dev box, the desktop app on your laptop.** Tailscale-connect them and use the desktop app's QR pairing to access the remote sessions from your phone.
- **Run `aoe serve --daemon` on your dev machine, open the desktop app as your local viewer.** The desktop app detects a running server and connects to it instead of starting a new one.
- **Use the desktop app for daily local work, drop into `aoe` in Terminal.app when you need a power-user shortcut.** Same sessions, same data, just a different surface.

## See also

- [Installation guide](installation.md) — install instructions for all four surfaces
- [Quick start](quick-start.md) — first session in 30 seconds
- [Web Dashboard guide](guides/web-dashboard.md) — `aoe serve` details
