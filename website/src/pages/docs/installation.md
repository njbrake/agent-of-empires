# Installation

AoE is the same tool it's always been: a terminal-first session manager built on tmux, with `aoe` as the main interface. There are now also optional shells around it (a macOS desktop app, a web dashboard, and an embeddable Rust library), but the TUI/CLI is the original and still the recommended way to use AoE for most users. You can install just the TUI and ignore everything else, or layer on the optional surfaces later. They all share the same session storage, so you can switch between them at any time.

If you came here from a recommendation that AoE is "a TUI for managing AI coding agents," you're in the right place. Start with [Install the TUI / CLI](#install-the-tui--cli-most-users) below.

For a side-by-side of all four surfaces, see [Which version is right for me?](compare.md).

## Before you install: prerequisites

### tmux (required for everything except the desktop app)

AoE wraps tmux. Sessions are tmux sessions, so they survive disconnects and crashes.

- **macOS:** `brew install tmux`
- **Ubuntu/Debian:** `sudo apt install tmux`
- **Fedora/RHEL:** `sudo dnf install tmux`
- **Arch:** `sudo pacman -S tmux`

> The macOS desktop app bundles its own tmux, so you don't need to install it separately. Every other surface (CLI, `aoe serve`, library) needs tmux on the host.

### At least one AI coding agent

AoE is a session manager, not an agent itself. You need to install at least one supported agent CLI before sessions are useful. AoE auto-detects which are installed and only shows those in the new session dialog.

| Agent | Install command | Docs |
|---|---|---|
| **Claude Code** | `npm install -g @anthropic-ai/claude-code` | [docs](https://docs.claude.com/en/docs/claude-code) |
| **OpenCode** | `curl -fsSL https://opencode.ai/install \| bash` | [opencode.ai](https://opencode.ai/) |
| **Codex CLI** | `npm install -g @openai/codex` | [openai/codex](https://github.com/openai/codex) |
| **Gemini CLI** | `npm install -g @google/gemini-cli` | [google-gemini/gemini-cli](https://github.com/google-gemini/gemini-cli) |
| **Cursor CLI** | See [cursor.com/cli](https://cursor.com/cli) | [cursor.com](https://cursor.com/cli) |
| **Copilot CLI** | `npm install -g @github/copilot` | [github/copilot-cli](https://github.com/github/copilot-cli) |
| **Mistral Vibe** | See [Mistral docs](https://docs.mistral.ai/) | [mistral.ai](https://mistral.ai/) |
| **Pi.dev** | See [pi.dev docs](https://pi.dev/) | [pi.dev](https://pi.dev/) |
| **Factory Droid** | See [factory.ai docs](https://factory.ai/) | [factory.ai](https://factory.ai/) |

You only need to install the agents you actually plan to use. Most people start with one (Claude Code or Codex) and add more later. AoE will work with zero agents installed but you won't be able to create any sessions until you install at least one.

### Docker (optional)

Required only if you want to [sandbox agents in containers](guides/sandbox.md). Skip this for local-only use.

- macOS / Windows: [Docker Desktop](https://www.docker.com/products/docker-desktop/)
- Linux: [Docker Engine](https://docs.docker.com/engine/install/)

---

## Install the TUI / CLI (most users)

This is the original AoE: a terminal-first session manager. It's what most people install, what's covered in the demo videos, and the only thing you need for the full feature set. Everything else on this page is optional.

### macOS

```bash
brew install aoe
```

Or use the install script (gets you the latest, includes the experimental web dashboard):

```bash
curl -fsSL https://raw.githubusercontent.com/njbrake/agent-of-empires/main/scripts/install.sh | bash
```

> The Homebrew formula does not yet include the web dashboard (`aoe serve`). If you want the web dashboard from a Homebrew install, use the install script or [build from source](#optional-build-from-source) with `--features serve`.

### Linux

```bash
curl -fsSL https://raw.githubusercontent.com/njbrake/agent-of-empires/main/scripts/install.sh | bash
```

Or with Nix:

```bash
nix run github:njbrake/agent-of-empires
```

### What you get

- The `aoe` binary on your `$PATH`
- TUI dashboard via `aoe`
- CLI commands (`aoe add`, `aoe list`, `aoe attach`, etc.)
- Web dashboard via `aoe serve` (if installed via the script or built with `--features serve`)

After installing, run `aoe` to launch the TUI. See [Quick Start](quick-start.md) for your first session.

---

## Optional: macOS desktop app

> **Experimental.** The desktop app is new and macOS-only. The TUI/CLI install above gives you the full feature set on its own — you don't need this. The desktop app is for Mac users who'd prefer a native window and menu bar experience over a terminal.

What you get on top of the TUI/CLI:

- Native macOS window rendering the dashboard
- Menu bar tray with quick toggles
- One-click QR code pairing for accessing the dashboard from your phone
- Bundled tmux (no separate install needed)
- The same `aoe` CLI is also installed alongside the app for terminal use

### Install

1. Download `Agent of Empires.app` from the [latest release](https://github.com/njbrake/agent-of-empires/releases/latest).
2. Open the `.dmg`, drag the app to Applications.
3. Right-click the app the first time you open it and choose "Open" (the app is not yet code-signed, so macOS Gatekeeper will warn on first launch).

After the first launch, double-click works as normal. You'll still need at least one agent CLI installed (see prerequisites above) — the desktop app doesn't change that.

---

## Optional: web dashboard (`aoe serve`)

> **Experimental.** This is bundled with the install script and source builds with `--features serve`. If you installed via Homebrew, you don't have it (yet).

If you want a browser/phone interface to your sessions, run:

```bash
aoe serve                         # localhost only
aoe serve --host 0.0.0.0          # accessible from other devices on your network
aoe serve --daemon                # run in background
```

Open the printed URL in any browser (laptop, phone, tablet). You get the same session list, live terminal streaming, and session controls as the TUI.

> **Security:** `aoe serve` binds to `127.0.0.1` by default. If you bind to `0.0.0.0` for remote access, the auth token travels in plaintext over HTTP. Use a VPN like [Tailscale](https://tailscale.com/) or an SSH tunnel — never expose this directly to the internet without TLS.

See the [Web Dashboard guide](guides/web-dashboard.md) for details.

---

## Optional: Rust library

> For tool builders only. If you just want to use AoE, install the TUI/CLI above.

If you're building your own tool on top of AoE in Rust:

```toml
# Cargo.toml
[dependencies]
agent-of-empires = "1.1"
```

This pulls in only the core: `session`, `tmux`, `git`, `agents`, `containers`. No web server, no axum, no Node.js required.

If you want to embed the web server in your own binary:

```toml
agent-of-empires = { version = "1.1", features = ["serve"] }
```

This adds axum, the embedded web frontend, and the `start_server_with_config()` API. Building this requires Node.js (the frontend is built at compile time).

See the [crate docs on docs.rs](https://docs.rs/agent-of-empires) for the full API.

---

## Optional: build from source

```bash
git clone https://github.com/njbrake/agent-of-empires
cd agent-of-empires
cargo build --release                   # TUI/CLI only, no Node.js needed
cargo build --release --features serve  # TUI/CLI + web dashboard, requires Node.js
cd desktop && cargo tauri build         # Desktop app, macOS only, requires Tauri CLI
```

The binary lands at `target/release/aoe`. See [development.md](development.md) for the full contributor setup.

---

## Verify your installation

```bash
aoe --version
```

For the desktop app, just open it from Launchpad or Applications.

## What's next?

- New here? Try the [Quick start](quick-start.md) to create your first session.
- Comparing options? See [Which version is right for me?](compare.md)
- Want to use it with your phone? See the [Web Dashboard guide](guides/web-dashboard.md).

## Uninstall

For the CLI:

```bash
aoe uninstall
```

This will guide you through removing the binary, configuration, and tmux settings.

For the desktop app: drag `Agent of Empires.app` from `/Applications` to the Trash. To also remove session data and config, run `aoe uninstall` first.
