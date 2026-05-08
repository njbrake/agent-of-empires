# Cockpit (Native Agent Rendering, Beta)

> **Beta, opt-in.** Cockpit ships disabled by default behind two
> independent gates:
>
> 1. `cockpit.enabled = true` in `config.toml` (persistent master
>    switch; default `false` from migration v005). Editable via the
>    settings TUI.
> 2. `AOE_EXPERIMENTAL_COCKPIT=1` env var on the process running
>    `aoe serve` (and the CLI for `aoe add --cockpit`). Per-process
>    opt-in for *new* sessions while the feature stabilises.
>
> While either gate is off:
>
> - the web wizard auto-routes new sessions through tmux,
> - `aoe add --cockpit` refuses with an actionable error.
>
> Existing cockpit sessions still load and run when the env var is
> unset (the env-var gate is for *new* sessions only); when the
> master switch is off, the reconciler doesn't auto-spawn workers
> for any session.
>
> The data model (`cockpit_mode: bool` per session) is stable; the
> UI and reliability story are still evolving — see "What's deferred".

Cockpit is aoe's native rendering surface for AI coding agents. Instead
of viewing the agent through a terminal pane (PTY bytes piped through
xterm.js), cockpit renders the agent's structured state directly: plan,
tool calls, diffs, and approvals. It's mobile-first, with a desktop
layout that scales the same components into a richer multi-pane view.

Cockpit speaks the [Agent Client Protocol](https://agentclientprotocol.com/)
(ACP), a JSON-RPC standard for editor-agent communication. aoe is the
*client*; the agent (Anthropic's Claude Code, our `aoe-agent`, Google's
Gemini CLI, etc.) is the *server*. Any ACP-conformant agent works.

## Supported agents

aoe ships a registry entry for each tool whose ACP server we've verified
against [agentclientprotocol.com](https://agentclientprotocol.com/get-started/agents.md).
The wizard greys out the cockpit option for tools not in this set.

| aoe tool   | Substrate B (cockpit)                                      | Auth                                   |
|------------|------------------------------------------------------------|----------------------------------------|
| `claude`   | `claude-agent-acp` (Zed adapter for the Claude SDK)        | `claude /login` writes `~/.claude/credentials`; or `ANTHROPIC_API_KEY` |
| `opencode` | `opencode acp` (native, SST)                               | `OPENCODE_API_KEY` env var; or provider-specific env (set up via `opencode auth`) |
| `gemini`   | `gemini --acp` (native, Google)                            | `GEMINI_API_KEY` env var, OAuth via `gemini auth`, or Vertex `GOOGLE_API_KEY` |
| `codex`    | `codex-acp` (Zed adapter, npm `@zed-industries/codex-acp`) | `OPENAI_API_KEY` env var, or ChatGPT login (local-only) |
| `vibe`     | `vibe-acp` (native, Mistral)                               | Mistral API key; set up via `vibe` first |
| `pi`       | `pi-acp` (adapter, requires `@mariozechner/pi-coding-agent`) | `pi-acp --terminal-login` for OAuth, or env vars per provider |
| `aoe-agent`| Bundled multi-provider agent (Vercel AI SDK 6)             | Whatever provider env vars Vercel AI SDK expects |
| *aider, cursor, copilot, droid, settl, hermes* | not yet wired into the cockpit registry — fall back to terminal mode |

The four env vars cockpit always forwards to the agent process are
`ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, `CLAUDE_CODE_OAUTH_TOKEN`,
`CLAUDE_CONFIG_DIR`. For the others, set them in the env that runs
`aoe serve` (or use the per-session `extra_env` field) and the agent's
own auth path will pick them up via the forwarded `HOME`.

## Quickstart

```bash
# 1. Confirm prerequisites: aoe, Node.js >= 20, claude login.
aoe cockpit doctor

# 2. Create a Claude Code session in cockpit mode.
aoe add . --cmd claude --cockpit

# 3. Open the dashboard, pick the session, and you should see the
#    structured plan + tool-call cards instead of a terminal.
aoe serve
```

A first-time mobile user pointed at a remote `aoe serve` will install
the PWA, tap the session, and see the plan panel render the moment the
agent emits its first plan event.

## Requirements

- aoe 1.5.0 or newer, built with `--features serve` (cockpit ships
  alongside the web dashboard).
- Node.js 20 or newer on `PATH`. Cockpit spawns an ACP agent
  subprocess; for the bundled `aoe-agent` runtime it uses Vercel AI
  SDK 6, which requires Node 20+.
- For Claude Code via the official ACP adapter, you also need a
  `claude login` session.

If Node.js is missing or too old, cockpit refuses to start and prints
an actionable error pointing at the install path for your OS.

### Verify

```bash
aoe cockpit doctor
```

Sample output on a machine where Claude is installed but the others
aren't:

```
Cockpit doctor  (Beta)
======================

Cockpit is the structured-rendering substrate (ACP-based).
Tmux passthrough remains the default for tool sessions; cockpit
is opt-in per session via `aoe add --cockpit` or the web wizard.

[OK] Node runtime  v22.21.0
    path: /opt/homebrew/bin/node

Configured agents:
[!! ] aoe-agent  (aoe's bundled multi-provider agent (Vercel AI SDK 6))
[OK] claude  (Anthropic Claude via the official ACP adapter …)
[OK] claude-code  (Alias for `claude` (legacy name))
[!! ] codex  (OpenAI Codex CLI via Zed adapter …)
    install: npm install -g @zed-industries/codex-acp
[!! ] gemini  (Google Gemini CLI — native ACP via `gemini --acp`)
    install: npm install -g @google/gemini-cli  (then `gemini --acp`)
[!! ] opencode  (OpenCode (SST) — native ACP via `opencode acp`)
    install: curl -fsSL https://opencode.ai/install | bash  (then `opencode acp`)
[!! ] pi  (Hermes coding agent (`pi`) via the pi-acp adapter …)
    install: npm install -g pi-acp  (also requires `npm i -g @mariozechner/pi-coding-agent`)
[!! ] vibe  (Mistral Vibe — native ACP via the bundled `vibe-acp` binary)
    install: follow https://github.com/mistralai/mistral-vibe (ships the `vibe-acp` binary)

Overall: partial
```

`aoe cockpit doctor --fix` will `npm install -g` the npm-distributed
adapters (claude / codex / pi). The native CLIs (opencode / gemini /
vibe) you install through their own channels.

If Node is missing the report exits 1; if some agents are unreachable
it exits 2; otherwise 0. Pass `--json` for machine-readable output.

## Enabling cockpit

### Per session

```bash
# Force cockpit on for this session, regardless of defaults.
aoe add . --cmd claude --cockpit

# Force terminal/PTY on, regardless of defaults.
aoe add . --cmd claude --no-cockpit

# Pick a specific cockpit agent + model.
aoe add . --cockpit --agent aoe-agent --model gpt-5
aoe add . --cockpit --agent aoe-agent --model llama3.3:ollama
aoe add . --cockpit --agent gemini
```

### Globally

The settings live in `config.toml` under `[cockpit]`:

```toml
[cockpit]
enabled = true
default_for_claude = true
default_agent = "aoe-agent"
approval_timeout_secs = 300
destructive_require_double_confirm = true
max_concurrent_workers = 5
replay_events = 500
replay_bytes = 5_242_880
node_path = ""
```

`enabled = false` is a master kill switch; cockpit refuses to spawn
even if a session has `--cockpit`. `default_for_claude = true` makes
new Claude sessions cockpit-mode by default on mobile clients.

Migration v005 seeds these defaults on upgrade so the section already
exists if you came from 1.4.x.

## Disabling / escape hatches

- `--no-cockpit` per session (CLI).
- `cockpit.enabled = false` in `config.toml` (persistent master). The
  reconciler short-circuits, REST endpoints return 503, and the CLI
  refuses `--cockpit`. Snapshotted at startup, so toggling requires
  `aoe serve --stop` then a fresh start to take effect.
- Don't set `AOE_EXPERIMENTAL_COCKPIT` (per-process). With the master
  switch on but the env var unset, *new* browser sessions still get
  tmux; existing cockpit sessions keep running with a one-time warn
  log on startup.
- `AOE_COCKPIT_NODE=/path/to/node` overrides Node discovery for one
  process (useful when the host's PATH-side Node is the wrong version
  and you can't change PATH).

### Fully turn cockpit off

```bash
# 1. Stop the daemon.
aoe serve --stop

# 2. Set the master switch off in config.toml.
$EDITOR ~/.config/agent-of-empires/config.toml  # [cockpit] enabled = false

# 3. Make sure AOE_EXPERIMENTAL_COCKPIT is NOT in your shell init
#    (.zshrc/.bashrc), systemd unit, launchd plist, etc.

# 4. Start serve again.
aoe serve
```

`aoe cockpit doctor` reports the gate state up front. `aoe cockpit
doctor --fix` will install missing ACP tooling but **will not** flip
`cockpit.enabled` on for you; toggling that is always an explicit
operator action.

## TUI vs web dashboard

Cockpit is a **web-dashboard surface**. The TUI does not render the
structured cockpit view today.

- **Sessions started in cockpit mode** appear in the TUI session list
  with a `[web]` badge. Pressing Enter opens an info dialog telling
  the user to switch to the dashboard; it does *not* attach to a tmux
  pane (cockpit sessions don't have one).
- **Sessions started in tmux mode** work in both surfaces as before.
  The TUI attaches to the pane; the dashboard renders the pane via
  xterm.js.
- **Switching substrates** (web wizard or the per-session "Switch to
  cockpit" / "Switch to tmux" action) destroys the in-memory
  conversation history for that session. The git worktree, files on
  disk, and any commits remain. The next prompt starts a fresh
  conversation under the new substrate.
- **TUI status indicators**: a cockpit session that's healthy shows
  as Idle/Active in the TUI session list, since cockpit health is
  observed via the ACP event stream rather than tmux pane probing.

A future release will either render a read-only cockpit transcript
inside the TUI, or grow a richer "open this in the dashboard"
affordance. Both are tracked as deferred work below.

## Tool compatibility

| Tool          | Cockpit?     | Notes                                              |
|---------------|--------------|----------------------------------------------------|
| Claude Code   | yes          | via the official ACP adapter (`claude-code`)        |
| aoe-agent     | yes          | bundled multi-provider runtime (Vercel AI SDK 6)   |
| Gemini CLI    | yes          | `gemini acp` (Google reference impl)               |
| OpenCode      | optional     | requires `opencode` with ACP support               |
| Codex CLI     | optional     | tracking upstream ACP support                      |
| Cursor CLI    | terminal only| no ACP support today                               |
| Factory Droid | terminal only| no ACP support today                               |
| OpenClaw      | terminal only| no ACP support today                               |

Tools without ACP support continue to work exactly as they do today
(tmux + PTY); cockpit is additive, not a replacement.

## Approvals

When the agent wants to run a tool that requires approval, the cockpit
shows an approval card:

- **Benign tools** (read, search, list): single tap on a primary
  button.
- **Destructive tools** (`rm -rf`, `git push --force`, writes to
  system paths): long-press 800ms with a progress ring and a haptic
  confirmation. Single tap is reserved for the deny button.

You can configure how cockpit classifies destructive operations and
the timeout before a pending approval auto-cancels:

```toml
[cockpit]
approval_timeout_secs = 300
destructive_require_double_confirm = true
```

## Security

- File system access uses ACP's `fs/read_text_file` and
  `fs/write_text_file`. Agents do **not** access the disk directly; aoe
  reads/writes on their behalf and enforces sandbox roots (the
  session's worktree + any explicit `--repo` paths).
- Terminal commands use ACP's `terminal/*`. The shell command runs in
  aoe's process, in the session's worktree (or sandboxed Docker
  container if applicable).
- Approval nonces are server-generated and single-use. A compromised
  agent process cannot synthesise approvals; aoe never reveals the
  nonce to the agent.
- Auth tokens (`AOE_TOKEN`) are explicitly *not* forwarded to the
  agent subprocess.

## Troubleshooting

### `aoe cockpit doctor` says Node is missing

Install Node.js 20 or newer:

- macOS: `brew install node`
- Linux: `apt install nodejs` or `nvm install 20`
- Windows: download from <https://nodejs.org/>

Then re-run `aoe cockpit doctor` to verify. If you have Node installed
in a non-standard location, set `AOE_COCKPIT_NODE=/path/to/node` or
configure `cockpit.node_path` in `config.toml`.

### `aoe cockpit doctor` says aoe-agent is missing

`aoe-agent` ships with the aoe binary. If the doctor reports it
missing, your install is incomplete. Reinstall aoe via your package
manager (e.g., `brew reinstall aoe`).

### `aoe cockpit doctor` says claude-code adapter is missing

Install the official adapter once:

```bash
npm install -g @agentclientprotocol/claude-agent-acp
```

Then run `claude login` if you haven't already.

### Cockpit feels "stuck" with no events

- Check `aoe cockpit logs --follow` (when the worker supervisor lands)
  to see worker stderr.
- Check the dashboard's connection chrome at the top of the cockpit
  view; it shows reconnect status if the WebSocket is degraded.
- The supervisor watchdog respawns the agent up to 3 times in 60s
  after a crash; if all three burn, the cockpit shows a red
  "session parked" banner. Refresh the page to retry from scratch.
- On reconnect the client calls
  `GET /api/sessions/{id}/cockpit/replay?since={lastSeq}` to recover
  any frames it missed during a brief network blip. If the buffer no
  longer holds events that far back, you'll see a `History
  truncated` notice and reloading is the cleanest way to resync.

### Approval card vanished without resolving

Approvals expire after `approval_timeout_secs` (default 300). The
agent receives a structured cancellation; you'll typically see a
follow-up message asking again. Bump the timeout if you're in a
context where approvals legitimately take longer.

## CLI reference

```
aoe cockpit doctor [--json] [--fix]
aoe cockpit agents
aoe cockpit logs [--session <id>] [--follow]
aoe cockpit restart <session>
```

`logs` and `restart` are reserved for the worker supervisor wiring
(landing in a follow-up release); they print a clear "not yet
available" message today so scripts can check the surface stably.

## What's deferred

These are tracked for follow-up releases:

- Mid-token interrupt (waiting on Anthropic's stable feature).
- Plan-mode and elicitation event mappings (the SDK supports them; the
  cockpit's typed schema covers the common path).
- Cross-agent handoff and unified search across cockpit sessions.
- Voice input/output on mobile.
- A read-only cockpit transcript view inside the TUI (today the TUI
  shows a `[web]` badge and an "open in dashboard" hint).
- Promotion out of `AOE_EXPERIMENTAL_COCKPIT`: once the
  default-cockpit-on-web flow has burned in for one release,
  `default_cockpit_for_web()` flips back to `true` for browser
  clients and the wizard shows the substrate picker by default.
- Docker sandbox unix-socket transport for cockpit sessions running
  inside containers.
