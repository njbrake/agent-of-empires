# Cockpit (Native Agent Rendering, Beta)

> **Beta, opt-in.** Cockpit ships disabled by default behind a single
> master switch: `cockpit.enabled = true` in `config.toml` (default
> `false` from migration v005). Toggle it from the web settings
> (Cockpit tab) or by editing `config.toml` directly.
>
> While the switch is off:
>
> - the web wizard auto-routes new sessions through tmux,
> - `aoe add --cockpit` refuses with an actionable error,
> - the reconciler doesn't auto-spawn workers for any session.
>
> The data model (`cockpit_mode: bool` per session) is stable; the
> UI and reliability story are still evolving; see "What's deferred".

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
| *aider, cursor, copilot, droid, settl, hermes* | not yet wired into the cockpit registry; fall back to terminal mode |

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
[!! ] gemini  (Google Gemini CLI; native ACP via `gemini --acp`)
    install: npm install -g @google/gemini-cli  (then `gemini --acp`)
[!! ] opencode  (OpenCode (SST); native ACP via `opencode acp`)
    install: curl -fsSL https://opencode.ai/install | bash  (then `opencode acp`)
[!! ] pi  (Hermes coding agent (`pi`) via the pi-acp adapter …)
    install: npm install -g pi-acp  (also requires `npm i -g @mariozechner/pi-coding-agent`)
[!! ] vibe  (Mistral Vibe; native ACP via the bundled `vibe-acp` binary)
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
max_concurrent_resumes = 4  # cap on parallel cold-start spawns/attaches (#1088)
replay_events = 0  # 0 = unlimited history; set a positive value to cap per-session rows (also caps the web client's in-memory activity buffer, #1111)
replay_bytes = 5_242_880
node_path = ""
show_tool_durations = true  # per-tool elapsed-time label in the web UI
queue_drain_mode = "combined"  # how the composer drains client-side queued prompts: "combined" | "serial" (#1031)
force_end_turn_threshold_secs = 30  # seconds of streaming silence before the spinner offers a "Force end turn" button (#1100)
```

`max_concurrent_resumes` bounds how many cockpit workers the reconciler
spawns/attaches in parallel on `aoe serve` cold start. Default 4 keeps
Node.js bootup memory bounded for laptops/Pis; raise on beefier hosts.
Clamped at runtime by `min(this, max_concurrent_workers).max(1)`. The
supervisor's per-agent install gate serialises only the first spawn of
each agent per daemon lifetime, so the claude-agent-acp lazy-install
race is safe even at high parallelism (#1088).

`enabled = false` is a master kill switch; cockpit refuses to spawn
even if a session has `--cockpit`. `default_for_claude = true` makes
new Claude sessions cockpit-mode by default on mobile clients.

Migration v005 seeds these defaults on upgrade so the section already
exists if you came from 1.4.x. Migration v006 then flips the v005-seeded
`replay_events = 500` to `0` so upgraders pick up the new unlimited
default; any user who has explicitly chosen a different cap is left
alone.

## Disabling / escape hatches

- `--no-cockpit` per session (CLI).
- `cockpit.enabled = false` in `config.toml` (persistent master). The
  reconciler short-circuits, REST endpoints return 503, and the CLI
  refuses `--cockpit`. The web settings panel toggles this live;
  flipping the switch shuts down running workers within a couple of
  seconds and respawns them when re-enabled, no `aoe serve --stop`
  required.
- `AOE_COCKPIT_NODE=/path/to/node` overrides Node discovery for one
  process (useful when the host's PATH-side Node is the wrong version
  and you can't change PATH).

### Fully turn cockpit off

The fastest path: open the web settings, go to the Cockpit tab, and
flip the master switch off. Workers exit within a couple of seconds.

Or edit `config.toml` directly and restart:

```bash
aoe serve --stop
$EDITOR ~/.config/agent-of-empires/config.toml  # [cockpit] enabled = false
aoe serve
```

`aoe cockpit doctor --fix` will install missing ACP tooling but **will
not** flip `cockpit.enabled` on for you; toggling that is always an
explicit operator action.

## TUI vs web dashboard

Cockpit renders natively in the TUI alongside the web dashboard.
Both consume the same `aoe serve` daemon over the same HTTP/WS
surface, so the conversation log, pending approvals, and worker
state are always in sync.

- **Sessions started in cockpit mode** appear in the TUI session list
  with a `[cockpit]` badge. Pressing Enter opens the native cockpit
  view, which requires an `aoe serve` daemon to be already running.
  If one isn't, the view renders an actionable error pointing at
  `aoe serve --daemon` (localhost), `aoe serve --daemon --remote`
  (Tailscale/Cloudflare), or `AOE_DAEMON_URL` (attach to a remote
  daemon you already have running). The TUI intentionally does not
  start a daemon on your behalf, so you keep the choice between
  localhost, tunnel, and named tunnel explicit.
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

### TUI cockpit view keybinds

The TUI cockpit view has three focusable regions: composer (where
you type prompts), transcript (the activity feed), and approval
cards (one per pending tool authorization). Tab cycles focus; the
status banner at the bottom of the screen shows the current focus.

| Focus       | Key             | Action                                                |
| ----------- | --------------- | ----------------------------------------------------- |
| Composer    | `Enter`         | Send the buffered text as a prompt                    |
| Composer    | `Shift+Enter`   | Insert a newline (multi-line prompts)                 |
| Composer    | `Esc`           | Return focus to the transcript                        |
| Transcript  | `j` / `↓`       | Scroll down one line                                  |
| Transcript  | `k` / `↑`       | Scroll up one line                                    |
| Transcript  | `PgDn` / `PgUp` | Scroll ten lines                                      |
| Transcript  | `g` / `G`       | Jump to top / bottom                                  |
| Transcript  | `i`             | Focus the composer                                    |
| Transcript  | `Tab`           | Cycle to the approval card (if any pending)           |
| Transcript  | `o`             | Open this session in the web dashboard                |
| Transcript  | `Esc`           | Close the cockpit view and return to the session list |
| Approval    | `a`             | Allow once                                            |
| Approval    | `Shift+A`       | Allow always (session-scoped allow-list entry)        |
| Approval    | `d`             | Deny                                                  |
| Approval    | `Esc`           | Return focus to the transcript                        |
| Any         | `Ctrl+C`        | Cancel the in-flight prompt                           |
| Any         | `Ctrl+O`        | Open the session in the web dashboard                 |

**Focus isolation.** Approval keys (`a`/`Shift+A`/`d`) only resolve
when the approval card itself has focus. Typing "always allow" into
the composer will never silently approve a pending tool; the
composer captures every keystroke, including those letters.

### Web composer Enter behavior

On desktop, Enter sends the prompt and Shift+Enter inserts a
newline, matching the TUI convention above.

On touch-primary devices (phones, tablets without an attached
keyboard), plain Enter inserts a newline and the explicit Send
button on the right of the composer is the only path to dispatch.
This matches the conventions of WhatsApp, Slack, ChatGPT mobile,
and Claude.ai mobile, and avoids the common foot-gun of accidentally
firing a partial multi-line prompt by reaching for a line break.
An iPad with a Bluetooth keyboard (or any device that reports both
`(pointer: coarse)` and `(any-pointer: fine)` to the browser) keeps
the desktop Enter-to-send convention so hardware-keyboard typing
feels natural. See #1129.

### Cross-machine attach

Set `AOE_DAEMON_URL` (and optionally `AOE_DAEMON_TOKEN`) to point at
a remote `aoe serve` daemon, then either:

```sh
# Browse the remote daemon's cockpit sessions and pick one.
AOE_DAEMON_URL=https://aoe.example.com AOE_DAEMON_TOKEN=… aoe

# Or jump straight into a known session id.
aoe cockpit attach <session_id> --daemon-url https://aoe.example.com
```

When `AOE_DAEMON_URL` is set, the TUI swaps the local home view for
a remote-cockpit picker. Local-only operations (tmux attach,
`aoe stop`, file edit) aren't available against a remote; for
those, use the web dashboard or SSH into the host machine.

The env override also retargets `aoe serve --status` and the
`aoe cockpit *` verbs: with `AOE_DAEMON_URL` set, `--status` pings
the remote endpoint and reports its reachability instead of inspecting
the local `serve.pid` file. Unset the variable (or run `env -u
AOE_DAEMON_URL aoe serve --status`) to fall back to local introspection.

### Headless CLI verbs

For scripting and quick checks, every cockpit operation has a
matching `aoe cockpit <verb>` that talks to the same daemon:

| Verb                              | What it does                                                |
| --------------------------------- | ----------------------------------------------------------- |
| `aoe cockpit history <id>`        | Dump the persisted transcript                               |
| `aoe cockpit status <id>`         | Print highest/lowest seq and the daemon source              |
| `aoe cockpit prompt <id> <text>`  | Send a prompt (`-` reads from stdin)                        |
| `aoe cockpit approve <id> <nonce> [--always\|--deny]` | Resolve a pending approval        |
| `aoe cockpit cancel <id>`         | Cancel the in-flight prompt                                 |
| `aoe cockpit tail <id>`           | Stream broadcast frames to stdout as JSON lines             |
| `aoe cockpit attach <id>`         | Open the TUI cockpit view directly for this session id      |

Every verb (including `attach`) requires an `aoe serve` daemon to be
already running, and exits with an actionable hint if none is found.
Start one with `aoe serve --daemon` (localhost) or
`aoe serve --daemon --remote` (Tailscale/Cloudflare), or set
`AOE_DAEMON_URL` to attach to a remote daemon. The CLI deliberately
does not spawn a daemon on your behalf so the localhost-vs-tunnel
choice stays explicit.

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

## Worker persistence across `aoe serve` restart

> **Behavior change (cockpit-only).** Prior releases tore down every
> cockpit ACP worker on `aoe serve --stop` (and any other daemon
> shutdown). As of this release, the daemon detaches without killing
> the runner: in-flight turns survive `aoe serve --stop`, `aoe update`,
> daemon crashes, and host suspend/wake. To actually terminate
> workers, use `aoe cockpit stop <session>` or `aoe cockpit stop --all`
> (graceful), or `aoe cockpit kill <session>` (force). tmux-based
> (non-cockpit) sessions are unaffected.

Cockpit workers run as detached `aoe __cockpit-runner` processes that
outlive the daemon. `aoe serve --stop` drops the daemon's connection
to each worker but does **not** terminate the runner: the agent
keeps running, in-flight turns continue, and a subsequent `aoe serve`
reattaches via the worker's unix socket.

Each runner registers itself at
`<app_dir>/cockpit-workers/<session_id>.json` with its PID, socket
path, and cached ACP session id. The same directory holds the
per-session `.sock` (unix socket) and `.log` (runner stderr drain)
files. `aoe cockpit ps` lists running workers.

Practical implications:

- `aoe update` followed by `aoe serve --stop` + `aoe serve` keeps
  every cockpit agent's in-flight turn alive.
- Closing the laptop or restarting the host with `aoe serve` running:
  the daemon dies on suspend, but the runner continues. On wake the
  next `aoe serve` reattaches.
- To actually terminate a worker, run `aoe cockpit stop <session>` or
  `aoe cockpit stop --all`. To force-kill, `aoe cockpit kill <session>`.
- During the detach window (between `aoe serve --stop` and the next
  `aoe serve`), the runner buffers up to 256 agent → daemon
  notification lines so per-stream chunks emitted while the daemon was
  down get replayed on reattach. Permission requests issued while
  detached block the agent's turn until reattach.
- **Mid-turn reattach.** When the daemon comes back up against a
  session that was actively streaming a prompt, the new daemon resumes
  the existing ACP session id directly (no `session/new` or
  `session/load` is sent; the agent process never died, so its in-
  memory session is still addressable). The agent's eventual response
  to the orphaned in-flight `session/prompt` is dropped silently by
  the transport because its request id was issued by the previous
  daemon; to keep the UI from staying stuck on "thinking" forever,
  the daemon arms a resume-idle watchdog that emits a synthetic
  `Stopped { reason: "reattach_idle" }` event after 10s of inbound
  silence. Sessions that the runner cannot reattach to (dead PID,
  missing socket, etc.) fall through to a fresh spawn; if the on-disk
  event log shows that fresh spawn's session was mid-prompt at the
  moment the daemon died, the reconciler publishes a
  `Stopped { reason: "orphaned_at_restart" }` event before the new
  agent starts so the UI clears immediately. The same path covers the
  `main`-branch case where there is no runner at all and every cockpit
  session takes the fresh-spawn branch on restart.

## Conversation persistence

Cockpit transcripts survive page reloads, session switches, and
`aoe serve --stop`/restart cycles. For agents that support session
restoration (Claude today), the model itself also retains conversation
context across restarts; so a follow-up like "what did we just
decide?" still works after a daemon restart.

The web dashboard mirrors each session's reduced state into
`localStorage` under the `aoe:cockpit-state:v1:<session_id>` key so a
full page reload (mobile OS evicting the tab, Cloudflare tunnel
re-auth, PWA cold start) hydrates the chat surface instantly from the
last-known state and only fetches the seq-delta from the server. Entries
expire after seven days; an oversized session that exceeds the
per-origin quota falls back to the full server replay path without
warning. `clearCockpitCache` and the session-delete handler drop the
matching entry so a freshly-recreated session id doesn't briefly show
the prior transcript.

If context restoration fails (e.g., the agent's stored session is no
longer available), cockpit falls back to a fresh session and renders
an amber "Conversation context reset" callout in the transcript so
you know prior turns are no longer in the model's context window.

After that callout, an inline "Resume with prior context" banner
appears above the composer. Clicking it calls
`GET /api/sessions/{id}/cockpit/context-primer?before_seq=<reset-seq>`,
which walks the SQLite event log and returns a compact markdown
recap of the last ~20 turns (capped at ~24k characters, bulky tool
inputs/outputs elided, tool calls collapsed to one-liners). The
primer is pre-filled into the composer so you can review, trim, or
extend it before sending; nothing is sent silently. The banner is
one-shot per reset: dismiss it or submit any prompt and it stays
gone until the next `session/load` failure. See #1004.

The bundled `aoe-agent` doesn't yet support context restoration; its
transcript still replays from disk, but the model starts fresh on each
spawn. Tracked in
[#1005](https://github.com/njbrake/agent-of-empires/issues/1005).

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

### Notifications and sound

When an approval lands, the cockpit fires two channels so a user away
from the dashboard still sees the agent is blocked:

- **Web push.** If the PWA is installed and notifications are
  enabled, the daemon sends an OS-level push tagged
  `cockpit-approval-<session>`. Tapping the notification deep-links
  back to the cockpit. Unlike status-change pushes, approval pushes
  are not suppressed when the dashboard or TUI is active; the service
  worker routes focused clients to an in-app toast instead of an OS
  banner. See [Push notifications](push-notifications.md).
- **Browser sound.** The cockpit plays a chime in the dashboard tab
  whenever pending approvals go from zero to non-zero. Configure the
  file via `[sound] on_approval` in the daemon config or the Sound
  category of the Settings TUI. The chime is independent of the
  host-side audio used by the tmux status flow; the cockpit case
  often runs `aoe serve` on a remote box and the host speaker would
  be on the wrong side of the wire.

## Security

- File system access uses ACP's `fs/read_text_file` and
  `fs/write_text_file`. Agents do **not** access the disk directly; aoe
  reads/writes on their behalf and enforces sandbox roots (the
  session's worktree + any explicit `--repo` paths).
- Terminal commands use ACP's `terminal/*`. The shell command runs in
  aoe's process, in the session's worktree (or inside the sandbox
  container when sandbox is enabled, via `docker exec`).
- Approval nonces are server-generated and single-use. A compromised
  agent process cannot synthesise approvals; aoe never reveals the
  nonce to the agent.
- Auth tokens (`AOE_TOKEN`) are explicitly *not* forwarded to the
  agent subprocess.

### Sandbox containers

Cockpit sessions honor the wizard's **Run in a safe container** toggle.
When enabled, the ACP agent runs inside the same `aoe-sandbox-<id>`
Docker container the tmux substrate uses. The daemon stays on the host
and wraps the agent argv in `docker exec`, so the agent never sees host
paths. `fs/*` requests are translated from container paths (e.g.
`/workspace/proj/foo.rs`) back to host paths before the inside-roots
check; `terminal/*` commands run via `docker exec`, so a `pwd` from the
agent returns the container's working directory, not the host's.

The unix socket between the daemon and the per-session runner stays on
the host. The runner proxies the agent's stdio across the container
boundary, so there is no bind-mount of the daemon's socket into the
container. That path is reserved for a future agent that natively
speaks the socket transport.

Known limitations:

- `fs/*` path translation only covers the workspace mount(s) the
  container was built with. Agent-config mounts (`/root/.claude`),
  bind-mounted credentials, and user-configured `extra_volumes` are
  not in the path map. In practice the inside-roots check (worktree-
  only) already rejects those paths, so the safety property holds;
  the failure mode is just a generic "outside session roots" error.

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

### "Failed to start cockpit agent" while the adapter is installed

`aoe serve` captures the launching shell's PATH at startup and keeps it for the daemon's lifetime. If the adapter is installed under a node-version-manager dir (`~/.nvm/versions/node/v<ver>/bin`, `~/.fnm/node-versions/.../installation/bin`, mise/asdf equivalents) and the active node version on the daemon's PATH doesn't match, the spawn fails with `agent spawn failed: No such file or directory`.

The spawn path scans common node-manager bin dirs (nvm, fnm, mise, asdf, Volta, `~/.npm-global/bin`, `~/.local/bin`, `/usr/local/bin`, `/opt/homebrew/bin`) per spawn, so a `nvm use <other-version>` after the daemon started is picked up on the next worker respawn without a daemon restart. If the binary lives somewhere else, either restart `aoe serve` from a shell where `which claude-agent-acp` resolves, or symlink it into one of those dirs.

### "Project path no longer exists" banner

The session's working directory was renamed, moved, or deleted out from under `aoe serve`. The most common trigger is a `git worktree move` or a manual `mv` on a worktree dir the session was bound to. The cockpit pre-flights `project_path` before spawning, so this fails fast with a typed banner instead of a generic ENOENT (which is indistinguishable on POSIX from "the adapter is missing"). Two ways to recover:

1. **Restore the directory at the path the banner shows** (e.g. `git worktree move <new> <old>`, or recreate the dir), then click **Retry** on the banner. Cockpit transcript continuity is preserved.
2. **Stop `aoe serve`**, edit `project_path` for this session in `~/.agent-of-empires/profiles/<profile>/sessions.json` to point at the new location, then start `aoe serve` again. If the worktree's branch was also renamed, update `worktree_info.branch` in the same file. Cockpit history + `cockpit_acp_session_id` are preserved; the conversation resumes against the new path.

Reinstalling the adapter does not help here; the adapter is fine, the cwd is gone.

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

### Editing settings asks for the passphrase again

When passphrase login is configured, the daily-use cockpit flows
(sending prompts, cancelling turns, resolving approvals, switching
mode, restarting workers, attaching terminals) do NOT prompt for the
passphrase again. Your session cookie plus the device-binding
secret are sufficient, the same way an SSH session stays open after
the initial authentication. See #1137.

Editing the persisted config IS gated. Saving the global settings
panel, creating / deleting / renaming a profile, editing a profile's
settings, or changing the default profile requires that your login
session has been "elevated" within the last 15 minutes via `POST
/api/login/elevate`. The first such action after a fresh page load
surfaces an inline passphrase prompt; subsequent edits inside the
same 15-minute window go through without re-prompting. The narrow
scope catches the persisted-tamper attack (an attacker with stolen
session + binding plants a malicious Docker image, worktree
template, or profile, then waits for the owner to spawn a session
that runs it) without putting friction on the conversation surface.

### WebSocket auto-reconnect and keepalive

Mobile browsers and Cloudflare tunnels both close idle WebSocket
connections aggressively (Chrome / Safari at ~30 to 60 seconds in the
background, Cloudflare at 100 seconds), so the cockpit pairs an
application-level keepalive with a client-side reconnect envelope.
The server sends a Ping every 30 seconds and reaps any socket that
goes 90 seconds without a Pong reply. On the client, the
`useCockpit` hook re-dials the WebSocket on close with exponential
backoff (1s, 2s, 4s, 8s, 16s, 30s, 30s), reset on the next successful
`onopen`. The reconnect resumes from `?since={lastSeq}` so the
transcript stays continuous. The cockpit banner shows
`Reconnecting (N/7) in Xs...` while the auto-retry is armed, and a
manual **Reconnect** button after the seven attempts exhaust.
`visibilitychange`, `online`, and `pageshow` listeners trigger an
immediate reconnect when the tab returns to the foreground.

### Approval card vanished without resolving

Approvals expire after `approval_timeout_secs` (default 300). The
agent receives a structured cancellation; you'll typically see a
follow-up message asking again. Bump the timeout if you're in a
context where approvals legitimately take longer.

### `/clear` collapsed earlier turns

When you run `/clear` in a cockpit session, the model's context is
wiped on the adapter side but the visible transcript is preserved.
The cockpit appends a "Conversation cleared" divider, resets the
active plan, the current mode, any in-flight approvals, and the
session usage snapshot, then folds every row above the divider
behind a disclosure banner: `Show N earlier turns (cleared, not in
the model's memory)`. Click the banner to expand the older transcript
for your own reference; the model still won't see those turns. See
#1101.

The slash-command palette and mode picker stay populated across a
`/clear`. `claude-agent-sdk` caches the supported command surface at
Query init and does not rotate it when conversation context is reset,
so the cached list stays authoritative for the lifetime of the
cockpit's underlying agent process. See #1128.

### "Force end turn" button under the spinner

If the agent finished a turn but the cockpit's working spinner is
still rattling (no streaming chunks landed for a while), a small
"Force end turn" button appears beneath it. Clicking it clears the
local spinner immediately and asks the daemon to publish a synthetic
`Stopped` plus a best-effort `session/cancel` to the agent. Pure
recovery affordance for a missed-event race (#1100); during a healthy
turn it never shows. Configure the inactivity threshold with
`cockpit.force_end_turn_threshold_secs` (default 30s).

### Sharing debug logs

`AOE_LOG_LEVEL=debug` (or the legacy `AGENT_OF_EMPIRES_DEBUG=1`) writes
agent stderr verbatim to `debug.log` under the app data dir. We scrub
common API-key prefixes (Anthropic `sk-...`, GitHub `ghp_...`, AWS
`AKIA...`, `Bearer <token>`, etc.) before they hit disk, but the scrub
is best-effort; a hand-rolled secret with no recognisable shape will
pass through. Before attaching `debug.log` to a bug report, skim it
for anything that looks like a credential, and replace it with
`<redacted>` if needed.

## CLI reference

```
aoe cockpit doctor [--json] [--fix]
aoe cockpit agents
aoe cockpit ps [--json]
aoe cockpit stop <session>            # graceful: SIGTERM the runner
aoe cockpit stop --all
aoe cockpit kill <session>            # immediate: SIGKILL the runner
aoe cockpit logs [--session <id>] [--follow]
aoe cockpit restart <session>         # stop + let daemon respawn
```

## What's deferred

These are tracked for follow-up releases:

- Mid-token interrupt (waiting on Anthropic's stable feature).
- Plan-mode and elicitation event mappings (the SDK supports them; the
  cockpit's typed schema covers the common path).
- Cross-agent handoff and unified search across cockpit sessions.
- Voice input/output on mobile.
- A read-only cockpit transcript view inside the TUI (today the TUI
  shows a `[web]` badge and an "open in dashboard" hint).
- Default `cockpit.enabled = true`: once the default-cockpit-on-web
  flow has burned in for one release, the master switch flips on by
  default and the wizard shows the substrate picker out of the box.
- Native unix-socket transport for in-container agents that natively
  speak the socket protocol. Today the sandbox path uses `docker exec`
  to keep stdio-only agents working without upstream changes.
