# Session Resume (Claude)

Agent of Empires can persist Claude Code conversation IDs so sessions resume their prior context after a reboot, an `aoe` upgrade, or a `kill-server`. No more hunting through `/resume` to find the right session.

## How it works

When you launch a Claude session through AoE, AoE generates a UUID and passes it to `claude --session-id <uuid>`. Claude uses that UUID for the conversation; AoE records it in `sessions.json`. On every subsequent launch of the same instance, AoE invokes `claude --resume <uuid>` so the conversation picks up where it left off.

AoE tracks the active session ID via two converging sources:

1. **Hook sidecar (primary, near-instant).** AoE installs `SessionStart` and `UserPromptSubmit` hooks into `~/.claude/settings.json`. These hooks extract the active `session_id` from Claude's stdin payload and write it atomically to `/tmp/aoe-hooks/<instance-id>/session_id`. The poller reads this file before scanning the filesystem, so runtime rotations via `/clear`, `--fork-session`, or `--continue` are picked up within one poll tick (~2 s).
2. **Filesystem scan (fallback).** If the sidecar is absent, stale (> 5 min), or invalid, the poller falls back to scanning `~/.claude/projects/<project>/` for the most recent `.jsonl`. Sibling AoE instances sharing the same project path are filtered out via tmux env (`AOE_CAPTURED_SESSION_ID`) so each session keeps its own UUID.

For sandboxed (Docker) sessions, the filesystem scan runs inside the container via `docker exec` (capped at 5 seconds per call). The hook sidecar is host-only today; sandboxed `/clear` adoption falls back to the filesystem scan and resolves within one poll tick.

## What's covered

- Launch, store, resume across reboots and `aoe` upgrades, in both host and sandboxed modes.
- Runtime rotation via `/clear`, `--fork-session`, or fresh `claude` invocation in the same pane.
- Manual override via the CLI when you want to point a session at a specific conversation.

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
