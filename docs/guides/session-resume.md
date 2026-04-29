# Session Resume (Claude)

Agent of Empires can persist Claude Code conversation IDs so sessions resume their prior context after a reboot, an `aoe` upgrade, or a `kill-server`. No more hunting through `/resume` to find the right session.

## How it works

When you launch a Claude session through AoE, AoE generates a UUID and passes it to `claude --session-id <uuid>`. Claude uses that UUID for the conversation; AoE records it in `sessions.json`. On every subsequent launch of the same instance, AoE invokes `claude --resume <uuid>` so the conversation picks up where it left off.

A background poller checks `~/.claude/projects/<project>/` every 2 seconds for the most recently modified session file. If the session ID rotates at runtime — for example after `/clear`, `--fork-session`, or starting a fresh `claude` invocation in the same tmux pane — the poller catches the new ID and AoE persists it transparently. Next launch resumes the new conversation, not the stale one.

For sandboxed (Docker) sessions, the poller runs the same scan inside the container via `docker exec`, since Claude writes its session files to the container's filesystem rather than the host's.

## What's covered

- Launch, store, resume across reboots and `aoe` upgrades, in both host and sandboxed modes.
- Runtime rotation via `/clear`, `--fork-session`, or fresh `claude` invocation in the same pane.
- Manual override via the CLI when you want to point a session at a specific conversation.

## What's not covered (yet)

- Other agents (OpenCode, Codex, Gemini, Vibe, Pi). They're tracked behind separate follow-up PRs; their sessions still launch fresh until then.
- Web dashboard. The web "restart session" path doesn't yet apply resume flags; use the TUI for resume-aware restarts.

## Manual override

To point a session at a different Claude conversation ID without launching it:

```sh
aoe session set-session-id <session-name-or-id> <claude-session-uuid>
```

To clear a stored ID (next launch will start a fresh conversation):

```sh
aoe session set-session-id <session-name-or-id> ""
```

## Disabling

There's no toggle. If you want a fresh conversation, clear the stored ID with the CLI command above, or delete the session and recreate it.

## Storage

The session ID lives in `sessions.json` in your AoE config directory:

- **Linux**: `$XDG_CONFIG_HOME/agent-of-empires/profiles/<profile>/sessions.json`
- **macOS/Windows**: `~/.agent-of-empires/profiles/<profile>/sessions.json`

Look for the `agent_session_id` field on each instance.
