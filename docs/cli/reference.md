# Command-Line Help for `aoe`

This document contains the help content for the `aoe` command-line program.

**Command Overview:**

* [`aoe`↴](#aoe)
* [`aoe add`↴](#aoe-add)
* [`aoe agents`↴](#aoe-agents)
* [`aoe init`↴](#aoe-init)
* [`aoe list`↴](#aoe-list)
* [`aoe logs`↴](#aoe-logs)
* [`aoe remove`↴](#aoe-remove)
* [`aoe send`↴](#aoe-send)
* [`aoe status`↴](#aoe-status)
* [`aoe session`↴](#aoe-session)
* [`aoe session start`↴](#aoe-session-start)
* [`aoe session stop`↴](#aoe-session-stop)
* [`aoe session restart`↴](#aoe-session-restart)
* [`aoe session attach`↴](#aoe-session-attach)
* [`aoe session show`↴](#aoe-session-show)
* [`aoe session rename`↴](#aoe-session-rename)
* [`aoe session capture`↴](#aoe-session-capture)
* [`aoe session current`↴](#aoe-session-current)
* [`aoe session set-session-id`↴](#aoe-session-set-session-id)
* [`aoe group`↴](#aoe-group)
* [`aoe group list`↴](#aoe-group-list)
* [`aoe group create`↴](#aoe-group-create)
* [`aoe group delete`↴](#aoe-group-delete)
* [`aoe group move`↴](#aoe-group-move)
* [`aoe profile`↴](#aoe-profile)
* [`aoe profile list`↴](#aoe-profile-list)
* [`aoe profile create`↴](#aoe-profile-create)
* [`aoe profile delete`↴](#aoe-profile-delete)
* [`aoe profile rename`↴](#aoe-profile-rename)
* [`aoe profile default`↴](#aoe-profile-default)
* [`aoe project`↴](#aoe-project)
* [`aoe project list`↴](#aoe-project-list)
* [`aoe project add`↴](#aoe-project-add)
* [`aoe project remove`↴](#aoe-project-remove)
* [`aoe worktree`↴](#aoe-worktree)
* [`aoe worktree list`↴](#aoe-worktree-list)
* [`aoe worktree info`↴](#aoe-worktree-info)
* [`aoe worktree cleanup`↴](#aoe-worktree-cleanup)
* [`aoe tmux`↴](#aoe-tmux)
* [`aoe tmux status`↴](#aoe-tmux-status)
* [`aoe sounds`↴](#aoe-sounds)
* [`aoe sounds install`↴](#aoe-sounds-install)
* [`aoe sounds list`↴](#aoe-sounds-list)
* [`aoe sounds test`↴](#aoe-sounds-test)
* [`aoe theme`↴](#aoe-theme)
* [`aoe theme list`↴](#aoe-theme-list)
* [`aoe theme export`↴](#aoe-theme-export)
* [`aoe theme dir`↴](#aoe-theme-dir)
* [`aoe serve`↴](#aoe-serve)
* [`aoe url`↴](#aoe-url)
* [`aoe cockpit`↴](#aoe-cockpit)
* [`aoe cockpit doctor`↴](#aoe-cockpit-doctor)
* [`aoe cockpit agents`↴](#aoe-cockpit-agents)
* [`aoe cockpit ps`↴](#aoe-cockpit-ps)
* [`aoe cockpit stop`↴](#aoe-cockpit-stop)
* [`aoe cockpit kill`↴](#aoe-cockpit-kill)
* [`aoe cockpit logs`↴](#aoe-cockpit-logs)
* [`aoe cockpit restart`↴](#aoe-cockpit-restart)
* [`aoe cockpit history`↴](#aoe-cockpit-history)
* [`aoe cockpit status`↴](#aoe-cockpit-status)
* [`aoe cockpit prompt`↴](#aoe-cockpit-prompt)
* [`aoe cockpit approve`↴](#aoe-cockpit-approve)
* [`aoe cockpit cancel`↴](#aoe-cockpit-cancel)
* [`aoe cockpit tail`↴](#aoe-cockpit-tail)
* [`aoe cockpit attach`↴](#aoe-cockpit-attach)
* [`aoe uninstall`↴](#aoe-uninstall)
* [`aoe update`↴](#aoe-update)
* [`aoe completion`↴](#aoe-completion)

## `aoe`

Agent of Empires (aoe) is a terminal session manager that uses tmux to help you manage and monitor AI coding agents like Claude Code and OpenCode.

Run without arguments to launch the TUI dashboard.

**Usage:** `aoe [OPTIONS] [COMMAND]`

###### **Subcommands:**

* `add` - Add a new session
* `agents` - List supported agents and their install status
* `init` - Initialize .agent-of-empires/config.toml in a repository
* `list` - List all sessions
* `logs` - View AoE log files (debug.log, serve.log) with a pretty viewer
* `remove` - Remove a session
* `send` - Send a message to a running agent session
* `status` - Show session status summary
* `session` - Manage session lifecycle (start, stop, attach, etc.)
* `group` - Manage groups for organizing sessions
* `profile` - Manage profiles (separate workspaces)
* `project` - Manage the project registry used by multi-repo session pickers
* `worktree` - Manage git worktrees for parallel development
* `tmux` - tmux integration utilities
* `sounds` - Manage sound effects for agent state transitions
* `theme` - Manage color themes (list, export, customize)
* `serve` - Start a web dashboard for remote session access
* `url` - Print the current dashboard URL of a running `aoe serve` daemon
* `cockpit` - Cockpit (ACP-based native agent rendering) management
* `uninstall` - Uninstall Agent of Empires
* `update` - Update aoe to the latest release
* `completion` - Generate shell completions

###### **Options:**

* `-p`, `--profile <PROFILE>` - Profile to use (separate workspace with its own sessions)
* `--daemon-url <DAEMON_URL>` - Attach to a remote cockpit daemon instead of using the local session list. Equivalent to setting `AOE_DAEMON_URL`; pair with `AOE_DAEMON_TOKEN` for the bearer token. Only meaningful at the no-subcommand `aoe` invocation (the TUI dashboard); ignored otherwise



## `aoe add`

Add a new session

**Usage:** `aoe add [OPTIONS] [PATH]`

###### **Arguments:**

* `<PATH>` - Project directory (defaults to current directory)

  Default value: `.`

###### **Options:**

* `-t`, `--title <TITLE>` - Session title (defaults to folder name)
* `-g`, `--group <GROUP>` - Group path (defaults to parent folder)
* `-c`, `--cmd <COMMAND>` - Command to run (e.g., 'claude' or any other supported agent)
* `-P`, `--parent <PARENT>` - Parent session (creates sub-session, inherits group)
* `-l`, `--launch` - Launch the session immediately after creating
* `-w`, `--worktree <WORKTREE_BRANCH>` - Create session in a git worktree for the specified branch
* `-b`, `--new-branch` - Create a new branch (use with --worktree)
* `--base-branch <BASE_BRANCH>` - Branch to base the new worktree branch on (use with --new-branch). Defaults to the repository's default branch. Useful for stacking work on top of an in-flight PR branch, hot-fixing a release branch, or branching off a teammate's branch
* `-r`, `--repo <EXTRA_REPOS>` - Additional repositories for multi-repo workspace (use with --worktree)
* `--project <PROJECTS>` - Names of registered projects to include as extra repos (use with --worktree). Resolves against the union of global + profile project registries
* `--no-submodules` - Skip `git submodule update --init --recursive` after creating the worktree, overriding the `worktree.init_submodules` config (default true). Useful for repos with large or deeply nested submodule trees that you don't need inside the agent session
* `-s`, `--sandbox` - Run session in a container sandbox
* `--sandbox-image <SANDBOX_IMAGE>` - Custom container image for sandbox (implies --sandbox)
* `-y`, `--yolo` - Enable YOLO mode (skip permission prompts)
* `--trust-hooks` - Automatically trust repository hooks without prompting
* `--extra-args <EXTRA_ARGS>` - Extra arguments to append after the agent binary
* `--cmd-override <CMD_OVERRIDE>` - Override the agent binary command
* `--cockpit` - Use cockpit mode (ACP-based native rendering) for this session. Overrides the default-for-claude setting in cockpit config
* `--no-cockpit` - Force terminal/PTY mode for this session, overriding the default-for-claude cockpit setting
* `--agent <AGENT>` - Pick a specific cockpit agent (e.g., aoe-agent, claude-code). Implies --cockpit
* `--model <MODEL>` - Override the model used by aoe-agent (e.g., claude-opus-4-7, gpt-5, gemini-2.5-pro). Forwarded to the agent at session start



## `aoe agents`

List supported agents and their install status

**Usage:** `aoe agents`



## `aoe init`

Initialize .agent-of-empires/config.toml in a repository

**Usage:** `aoe init [PATH]`

###### **Arguments:**

* `<PATH>` - Directory to initialize (defaults to current directory)

  Default value: `.`



## `aoe list`

List all sessions

**Usage:** `aoe list [OPTIONS]`

###### **Options:**

* `--json` - Output as JSON
* `--all` - List sessions from all profiles



## `aoe logs`

View AoE log files (debug.log, serve.log) with a pretty viewer

**Usage:** `aoe logs [OPTIONS]`

###### **Options:**

* `--debug` - View debug.log (default)
* `--serve` - View serve.log (daemon stdout/stderr)
* `--all` - View both debug.log and serve.log, merged by timestamp
* `-f`, `--follow` - Live-tail the log
* `-n`, `--lines <N>` - Show only the last N lines (fallback viewers; lnav handles its own)
* `--no-pager` - Skip viewer detection; write plain log to stdout
* `--path` - Print the resolved log file path(s) and exit (no viewing)



## `aoe remove`

Remove a session

**Usage:** `aoe remove [OPTIONS] <IDENTIFIER>`

###### **Arguments:**

* `<IDENTIFIER>` - Session ID or title to remove

###### **Options:**

* `--delete-worktree` - Delete worktree directory (default: keep worktree)
* `--delete-branch` - Delete git branch after worktree removal (default: per config)
* `--force` - Force worktree removal even with untracked/modified files
* `--keep-container` - Keep container instead of deleting it (default: delete per config)



## `aoe send`

Send a message to a running agent session

**Usage:** `aoe send [OPTIONS] <IDENTIFIER> <MESSAGE>`

###### **Arguments:**

* `<IDENTIFIER>` - Session ID or title
* `<MESSAGE>` - Message to send to the agent

###### **Options:**

* `--no-revive` - Fail loud on dead/stopped sessions instead of auto-respawning. Default behavior is to revive the session so a `send` after a crash or stop just works; pass this for scripts that want the previous bail-out



## `aoe status`

Show session status summary

**Usage:** `aoe status [OPTIONS]`

###### **Options:**

* `-v`, `--verbose` - Show detailed session list
* `-q`, `--quiet` - Only output waiting count (for scripts)
* `--json` - Output as JSON



## `aoe session`

Manage session lifecycle (start, stop, attach, etc.)

**Usage:** `aoe session <COMMAND>`

###### **Subcommands:**

* `start` - Start a session's tmux process
* `stop` - Stop session process
* `restart` - Restart session (or all sessions with `--all`)
* `attach` - Attach to session interactively
* `show` - Show session details
* `rename` - Rename a session
* `capture` - Capture tmux pane output
* `current` - Auto-detect current session
* `set-session-id` - Set agent session ID for a session



## `aoe session start`

Start a session's tmux process

**Usage:** `aoe session start <IDENTIFIER>`

###### **Arguments:**

* `<IDENTIFIER>` - Session ID or title



## `aoe session stop`

Stop session process

**Usage:** `aoe session stop <IDENTIFIER>`

###### **Arguments:**

* `<IDENTIFIER>` - Session ID or title



## `aoe session restart`

Restart session (or all sessions with `--all`)

**Usage:** `aoe session restart [OPTIONS] [IDENTIFIER]`

###### **Arguments:**

* `<IDENTIFIER>` - Session ID or title (required unless `--all` is passed)

###### **Options:**

* `--all` - Restart every session in the active profile. Useful after `aoe update`, after editing `sandbox.environment`, after a Docker hiccup, or after changing a hook. Mutually exclusive with `identifier`
* `--parallel <PARALLEL>` - Concurrency cap for `--all`. Restarting many sandboxed sessions in parallel pressures dockerd, so the default is intentionally modest. Ignored when `--all` is not set

  Default value: `3`



## `aoe session attach`

Attach to session interactively

**Usage:** `aoe session attach <IDENTIFIER>`

###### **Arguments:**

* `<IDENTIFIER>` - Session ID or title



## `aoe session show`

Show session details

**Usage:** `aoe session show [OPTIONS] [IDENTIFIER]`

###### **Arguments:**

* `<IDENTIFIER>` - Session ID or title (optional, auto-detects in tmux)

###### **Options:**

* `--json` - Output as JSON



## `aoe session rename`

Rename a session

**Usage:** `aoe session rename [OPTIONS] [IDENTIFIER]`

###### **Arguments:**

* `<IDENTIFIER>` - Session ID or title (optional, auto-detects in tmux)

###### **Options:**

* `-t`, `--title <TITLE>` - New title for the session
* `-g`, `--group <GROUP>` - New group for the session (empty string to ungroup)



## `aoe session capture`

Capture tmux pane output

**Usage:** `aoe session capture [OPTIONS] [IDENTIFIER]`

###### **Arguments:**

* `<IDENTIFIER>` - Session ID or title (auto-detects in tmux if omitted)

###### **Options:**

* `-n`, `--lines <LINES>` - Number of lines to capture

  Default value: `50`
* `--strip-ansi` - Strip ANSI escape codes
* `--json` - Output as JSON



## `aoe session current`

Auto-detect current session

**Usage:** `aoe session current [OPTIONS]`

###### **Options:**

* `-q`, `--quiet` - Just session name (for scripting)
* `--json` - Output as JSON



## `aoe session set-session-id`

Set agent session ID for a session

**Usage:** `aoe session set-session-id <IDENTIFIER> <SESSION_ID>`

###### **Arguments:**

* `<IDENTIFIER>` - Session ID or title
* `<SESSION_ID>` - Agent session ID to set (pass empty string to clear)



## `aoe group`

Manage groups for organizing sessions

**Usage:** `aoe group <COMMAND>`

###### **Subcommands:**

* `list` - List all groups
* `create` - Create a new group
* `delete` - Delete a group
* `move` - Move session to group



## `aoe group list`

List all groups

**Usage:** `aoe group list [OPTIONS]`

###### **Options:**

* `--json` - Output as JSON



## `aoe group create`

Create a new group

**Usage:** `aoe group create [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` - Group name

###### **Options:**

* `--parent <PARENT>` - Parent group for creating subgroups



## `aoe group delete`

Delete a group

**Usage:** `aoe group delete [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` - Group name

###### **Options:**

* `--force` - Force delete by moving sessions to default group



## `aoe group move`

Move session to group

**Usage:** `aoe group move <IDENTIFIER> <GROUP>`

###### **Arguments:**

* `<IDENTIFIER>` - Session ID or title
* `<GROUP>` - Target group



## `aoe profile`

Manage profiles (separate workspaces)

**Usage:** `aoe profile [COMMAND]`

###### **Subcommands:**

* `list` - List all profiles
* `create` - Create a new profile
* `delete` - Delete a profile
* `rename` - Rename a profile
* `default` - Show or set default profile



## `aoe profile list`

List all profiles

**Usage:** `aoe profile list`



## `aoe profile create`

Create a new profile

**Usage:** `aoe profile create <NAME>`

###### **Arguments:**

* `<NAME>` - Profile name



## `aoe profile delete`

Delete a profile

**Usage:** `aoe profile delete <NAME>`

###### **Arguments:**

* `<NAME>` - Profile name



## `aoe profile rename`

Rename a profile

**Usage:** `aoe profile rename <OLD_NAME> <NEW_NAME>`

###### **Arguments:**

* `<OLD_NAME>` - Current profile name
* `<NEW_NAME>` - New profile name



## `aoe profile default`

Show or set default profile

**Usage:** `aoe profile default [NAME]`

###### **Arguments:**

* `<NAME>` - Profile name (optional, shows current if not provided)



## `aoe project`

Manage the project registry used by multi-repo session pickers

**Usage:** `aoe project <COMMAND>`

###### **Subcommands:**

* `list` - List registered projects
* `add` - Add a project to the registry
* `remove` - Remove a project from the registry



## `aoe project list`

List registered projects

**Usage:** `aoe project list [OPTIONS]`

###### **Options:**

* `--json` - Output as JSON
* `--scope <SCOPE>` - Filter by scope (default: all)

  Default value: `all`

  Possible values: `all`, `global`, `profile`




## `aoe project add`

Add a project to the registry

**Usage:** `aoe project add [OPTIONS] <PATH>`

###### **Arguments:**

* `<PATH>` - Path to the git repository

###### **Options:**

* `--name <NAME>` - Display name (defaults to the directory's basename)
* `--scope <SCOPE>` - Registry scope. When omitted: defaults to GLOBAL, unless `-p <profile>` was passed at the top level, in which case it defaults to PROFILE (scoping the entry to that profile only)

  Possible values: `global`, `profile`

* `--allow-override` - Allow registering this path even if it already exists in the other scope. Without this flag the command errors when the same canonical path is already registered globally (when adding to profile) or in any profile (when adding globally). When override is allowed and both scopes hold the same path, the profile entry shadows the global one



## `aoe project remove`

Remove a project from the registry

**Usage:** `aoe project remove [OPTIONS] <NAME_OR_PATH>`

###### **Arguments:**

* `<NAME_OR_PATH>` - Project name or path to remove

###### **Options:**

* `--scope <SCOPE>` - Registry scope to remove from. When omitted: defaults to GLOBAL, unless `-p <profile>` was passed at the top level, in which case it defaults to PROFILE

  Possible values: `global`, `profile`




## `aoe worktree`

Manage git worktrees for parallel development

**Usage:** `aoe worktree <COMMAND>`

###### **Subcommands:**

* `list` - List all worktrees in current repository
* `info` - Show worktree information for a session
* `cleanup` - Cleanup orphaned worktrees



## `aoe worktree list`

List all worktrees in current repository

**Usage:** `aoe worktree list`



## `aoe worktree info`

Show worktree information for a session

**Usage:** `aoe worktree info <IDENTIFIER>`

###### **Arguments:**

* `<IDENTIFIER>` - Session ID or title



## `aoe worktree cleanup`

Cleanup orphaned worktrees

**Usage:** `aoe worktree cleanup [OPTIONS]`

###### **Options:**

* `-f`, `--force` - Actually remove worktrees (default is dry-run)



## `aoe tmux`

tmux integration utilities

**Usage:** `aoe tmux <COMMAND>`

###### **Subcommands:**

* `status` - Output session info for use in custom tmux status bar



## `aoe tmux status`

Output session info for use in custom tmux status bar

Add this to your ~/.tmux.conf: set -g status-right "#(aoe tmux status)"

**Usage:** `aoe tmux status [OPTIONS]`

###### **Options:**

* `-f`, `--format <FORMAT>` - Output format (text or json)

  Default value: `text`



## `aoe sounds`

Manage sound effects for agent state transitions

**Usage:** `aoe sounds <COMMAND>`

###### **Subcommands:**

* `install` - Install bundled sound effects
* `list` - List currently installed sounds
* `test` - Test a sound by playing it



## `aoe sounds install`

Install bundled sound effects

**Usage:** `aoe sounds install`



## `aoe sounds list`

List currently installed sounds

**Usage:** `aoe sounds list`



## `aoe sounds test`

Test a sound by playing it

**Usage:** `aoe sounds test <NAME>`

###### **Arguments:**

* `<NAME>` - Sound file name (without extension)



## `aoe theme`

Manage color themes (list, export, customize)

**Usage:** `aoe theme <COMMAND>`

###### **Subcommands:**

* `list` - List all available themes (built-in and custom)
* `export` - Export a built-in theme as a TOML file for customization
* `dir` - Show the custom themes directory path



## `aoe theme list`

List all available themes (built-in and custom)

**Usage:** `aoe theme list`



## `aoe theme export`

Export a built-in theme as a TOML file for customization

**Usage:** `aoe theme export [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` - Theme name to export

###### **Options:**

* `-o`, `--output <OUTPUT>` - Output file path (defaults to <name>.toml in the themes directory)



## `aoe theme dir`

Show the custom themes directory path

**Usage:** `aoe theme dir`



## `aoe serve`

Start a web dashboard for remote session access

**Usage:** `aoe serve [OPTIONS]`

###### **Options:**

* `--port <PORT>` - Port to listen on (default: 8080; debug builds default to 8081 so a `cargo run` instance does not collide with an installed release `aoe`)
* `--host <HOST>` - Host/IP to bind to (use 0.0.0.0 for LAN/VPN access)

  Default value: `127.0.0.1`
* `--no-auth` - Disable authentication (only allowed with localhost binding)
* `--read-only` - Read-only mode: view terminals but cannot send keystrokes
* `--remote` - Expose the dashboard over a public HTTPS tunnel. Prefers Tailscale Funnel when `tailscale` is installed and logged in (stable `.ts.net` URL, installable PWAs survive restarts). Falls back to a Cloudflare quick tunnel otherwise (fresh URL on every restart)
* `--tunnel-name <TUNNEL_NAME>` - Use a named Cloudflare Tunnel (requires prior `cloudflared tunnel create`). Takes precedence over Tailscale auto-detection
* `--no-tailscale` - Skip Tailscale Funnel auto-detection and go straight to Cloudflare. Useful if you have Tailscale installed for unrelated reasons
* `--tunnel-url <TUNNEL_URL>` - Hostname for a named tunnel (e.g., aoe.example.com)
* `--daemon` - Run as a background daemon (detach from terminal)
* `--stop` - Stop a running daemon
* `--status` - Print the running daemon's PID, mode, URLs, and log path. Exits non-zero when no daemon is running. Useful for shell scripts and for testing the cockpit auto-spawn flow without parsing `ps`
* `--passphrase <PASSPHRASE>` - Require a passphrase for login (second-factor auth). Can also be set via AOE_SERVE_PASSPHRASE environment variable
* `--open` - Open the dashboard URL in the default browser once the server is ready. Ignored under --daemon, --remote, SSH (SSH_CONNECTION/SSH_TTY), or when no display server is reachable on Linux/BSD



## `aoe url`

Print the current dashboard URL of a running `aoe serve` daemon

**Usage:** `aoe url [OPTIONS]`

###### **Options:**

* `--all` - Print every labeled URL (Tailscale / LAN / localhost) on its own line. The primary URL is printed first as `primary\t<url>`; alternates use `<label>\t<url>`. The tab-separated format makes the output easy to parse from shell scripts
* `--token-only` - Print only the auth token from the primary URL's `?token=` query parameter. Useful for scripted login flows or pasting into the PWA. Exits non-zero when the URL has no token (e.g. `--no-auth` server)



## `aoe cockpit`

Cockpit (ACP-based native agent rendering) management

**Usage:** `aoe cockpit <COMMAND>`

###### **Subcommands:**

* `doctor` - Verify the cockpit can start: Node runtime, configured agents, provider auth (claude login)
* `agents` - List configured cockpit agents (claude-code, aoe-agent, etc.)
* `ps` - List running cockpit workers (detached or attached)
* `stop` - Gracefully stop a cockpit worker (SIGTERM the runner, agent receives stdin EOF). Sessions can be reattached on the next `aoe serve` only if they are still alive afterward; `stop` destroys the worker
* `kill` - SIGKILL a worker immediately (use when `stop` doesn't take)
* `logs` - Tail the runner's log file for a cockpit session
* `restart` - Restart a wedged cockpit worker: stop the existing runner, then let the daemon's reconciler spawn a fresh one on the next tick
* `history` - Print the persisted transcript for a cockpit session
* `status` - Print live status for a cockpit session: highest/lowest seq, and whether the on-disk retention window has truncated history
* `prompt` - Send a prompt to a cockpit session's agent
* `approve` - Resolve a pending approval (default: allow). Use --always for a session-scoped allow-list entry, --deny to refuse the request
* `cancel` - Cancel the in-flight prompt for a cockpit session
* `tail` - Stream the cockpit broadcast for a session to stdout as JSON lines (one frame per line). Press Ctrl-C to stop
* `attach` - Open the TUI cockpit view directly for a known session id. Combine with `AOE_DAEMON_URL` (+ `AOE_DAEMON_TOKEN`) to attach across machines without going through the home session list



## `aoe cockpit doctor`

Verify the cockpit can start: Node runtime, configured agents, provider auth (claude login)

**Usage:** `aoe cockpit doctor [OPTIONS]`

###### **Options:**

* `--json` - Emit machine-readable JSON instead of a human report
* `--fix` - Attempt safe remediations: install missing claude-code-acp adapter, verify aoe-agent presence, etc. (Reserved for future release; the flag exists so scripts can opt in early.)



## `aoe cockpit agents`

List configured cockpit agents (claude-code, aoe-agent, etc.)

**Usage:** `aoe cockpit agents`



## `aoe cockpit ps`

List running cockpit workers (detached or attached)

**Usage:** `aoe cockpit ps [OPTIONS]`

###### **Options:**

* `--json` - Emit machine-readable JSON instead of a table



## `aoe cockpit stop`

Gracefully stop a cockpit worker (SIGTERM the runner, agent receives stdin EOF). Sessions can be reattached on the next `aoe serve` only if they are still alive afterward; `stop` destroys the worker

**Usage:** `aoe cockpit stop [OPTIONS] [SESSION]`

###### **Arguments:**

* `<SESSION>` - Session id to stop. Mutually exclusive with `--all`

###### **Options:**

* `--all` - Stop every running cockpit worker
* `--timeout-secs <TIMEOUT_SECS>` - Seconds to wait after SIGTERM before escalating to SIGKILL

  Default value: `5`



## `aoe cockpit kill`

SIGKILL a worker immediately (use when `stop` doesn't take)

**Usage:** `aoe cockpit kill <SESSION>`

###### **Arguments:**

* `<SESSION>` - Session id to kill



## `aoe cockpit logs`

Tail the runner's log file for a cockpit session

**Usage:** `aoe cockpit logs [OPTIONS]`

###### **Options:**

* `--session <SESSION>` - Session id whose worker logs to tail
* `--follow` - Follow new lines as they arrive



## `aoe cockpit restart`

Restart a wedged cockpit worker: stop the existing runner, then let the daemon's reconciler spawn a fresh one on the next tick

**Usage:** `aoe cockpit restart <SESSION>`

###### **Arguments:**

* `<SESSION>` - Session id whose worker to restart



## `aoe cockpit history`

Print the persisted transcript for a cockpit session

**Usage:** `aoe cockpit history [OPTIONS] <SESSION>`

###### **Arguments:**

* `<SESSION>` - Cockpit session id

###### **Options:**

* `--since <SINCE>` - Skip events at or below this seq

  Default value: `0`
* `--json` - Emit raw frames as JSON (one frame per line)



## `aoe cockpit status`

Print live status for a cockpit session: highest/lowest seq, and whether the on-disk retention window has truncated history

**Usage:** `aoe cockpit status [OPTIONS] <SESSION>`

###### **Arguments:**

* `<SESSION>` - Cockpit session id

###### **Options:**

* `--json` - Emit machine-readable JSON instead of a human report



## `aoe cockpit prompt`

Send a prompt to a cockpit session's agent

**Usage:** `aoe cockpit prompt <SESSION> <TEXT>`

###### **Arguments:**

* `<SESSION>` - Cockpit session id
* `<TEXT>` - Prompt text. Pass `-` to read from stdin



## `aoe cockpit approve`

Resolve a pending approval (default: allow). Use --always for a session-scoped allow-list entry, --deny to refuse the request

**Usage:** `aoe cockpit approve [OPTIONS] <SESSION> <NONCE>`

###### **Arguments:**

* `<SESSION>` - Cockpit session id
* `<NONCE>` - Approval nonce, as printed in the pending-approval banner

###### **Options:**

* `--always` - Allow this kind of operation for the rest of the session
* `--deny` - Refuse the request



## `aoe cockpit cancel`

Cancel the in-flight prompt for a cockpit session

**Usage:** `aoe cockpit cancel <SESSION>`

###### **Arguments:**

* `<SESSION>` - Cockpit session id



## `aoe cockpit tail`

Stream the cockpit broadcast for a session to stdout as JSON lines (one frame per line). Press Ctrl-C to stop

**Usage:** `aoe cockpit tail [OPTIONS] <SESSION>`

###### **Arguments:**

* `<SESSION>` - Cockpit session id

###### **Options:**

* `--since <SINCE>` - Start at this seq (default 0 = full replay then live)

  Default value: `0`



## `aoe cockpit attach`

Open the TUI cockpit view directly for a known session id. Combine with `AOE_DAEMON_URL` (+ `AOE_DAEMON_TOKEN`) to attach across machines without going through the home session list

**Usage:** `aoe cockpit attach <SESSION>`

###### **Arguments:**

* `<SESSION>` - Cockpit session id



## `aoe uninstall`

Uninstall Agent of Empires

**Usage:** `aoe uninstall [OPTIONS]`

###### **Options:**

* `--keep-data` - Keep data directory (sessions, config, logs)
* `--keep-tmux-config` - Keep tmux configuration
* `--dry-run` - Show what would be removed without removing
* `-y` - Skip confirmation prompts



## `aoe update`

Update aoe to the latest release

**Usage:** `aoe update [OPTIONS]`

###### **Options:**

* `-y`, `--yes` - Skip confirmation prompt
* `--check` - Print update status and exit (no install)
* `--dry-run` - Detect install method and print what would happen, no download



## `aoe completion`

Generate shell completions

**Usage:** `aoe completion <SHELL>`

###### **Arguments:**

* `<SHELL>` - Shell to generate completions for

  Possible values: `bash`, `elvish`, `fish`, `powershell`, `zsh`




<hr/>

<small><i>
    This document was generated automatically by
    <a href="https://crates.io/crates/clap-markdown"><code>clap-markdown</code></a>.
</i></small>
