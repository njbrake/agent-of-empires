# Command-Line Help for `aoe`

This document contains the help content for the `aoe` command-line program.

**Command Overview:**

* [`aoe`↴](#aoe)
* [`aoe add`↴](#aoe-add)
* [`aoe init`↴](#aoe-init)
* [`aoe list`↴](#aoe-list)
* [`aoe remove`↴](#aoe-remove)
* [`aoe status`↴](#aoe-status)
* [`aoe session`↴](#aoe-session)
* [`aoe session start`↴](#aoe-session-start)
* [`aoe session stop`↴](#aoe-session-stop)
* [`aoe session restart`↴](#aoe-session-restart)
* [`aoe session attach`↴](#aoe-session-attach)
* [`aoe session show`↴](#aoe-session-show)
* [`aoe session current`↴](#aoe-session-current)
* [`aoe group`↴](#aoe-group)
* [`aoe group list`↴](#aoe-group-list)
* [`aoe group create`↴](#aoe-group-create)
* [`aoe group delete`↴](#aoe-group-delete)
* [`aoe group move`↴](#aoe-group-move)
* [`aoe profile`↴](#aoe-profile)
* [`aoe profile list`↴](#aoe-profile-list)
* [`aoe profile create`↴](#aoe-profile-create)
* [`aoe profile delete`↴](#aoe-profile-delete)
* [`aoe profile default`↴](#aoe-profile-default)
* [`aoe worktree`↴](#aoe-worktree)
* [`aoe worktree list`↴](#aoe-worktree-list)
* [`aoe worktree info`↴](#aoe-worktree-info)
* [`aoe worktree cleanup`↴](#aoe-worktree-cleanup)
* [`aoe sounds`↴](#aoe-sounds)
* [`aoe sounds install`↴](#aoe-sounds-install)
* [`aoe sounds list`↴](#aoe-sounds-list)
* [`aoe sounds test`↴](#aoe-sounds-test)
* [`aoe tmux`↴](#aoe-tmux)
* [`aoe tmux status`↴](#aoe-tmux-status)
* [`aoe uninstall`↴](#aoe-uninstall)

## `aoe`

Agent of Empires (aoe) is a terminal session manager that uses tmux to help you manage and monitor AI coding agents like Claude Code and OpenCode.

Run without arguments to launch the TUI dashboard.

**Usage:** `aoe [OPTIONS] [COMMAND]`

###### **Subcommands:**

* `add` — Add a new session
* `init` — Initialize .aoe/config.toml in a repository
* `list` — List all sessions
* `remove` — Remove a session
* `status` — Show session status summary
* `session` — Manage session lifecycle (start, stop, attach, etc.)
* `group` — Manage groups for organizing sessions
* `profile` — Manage profiles (separate workspaces)
* `worktree` — Manage git worktrees for parallel development
* `sounds` — Manage sound effects for agent state transitions
* `tmux` — tmux integration utilities
* `uninstall` — Uninstall Agent of Empires

###### **Options:**

* `-p`, `--profile <PROFILE>` — Profile to use (separate workspace with its own sessions)



## `aoe add`

Add a new session

**Usage:** `aoe add [OPTIONS] [PATH]`

###### **Arguments:**

* `<PATH>` — Project directory (defaults to current directory)

  Default value: `.`

###### **Options:**

* `-t`, `--title <TITLE>` — Session title (defaults to folder name)
* `-g`, `--group <GROUP>` — Group path (defaults to parent folder)
* `-c`, `--cmd <COMMAND>` — Command to run (e.g., 'claude', 'opencode', 'vibe', 'codex', 'gemini')
* `-P`, `--parent <PARENT>` — Parent session (creates sub-session, inherits group)
* `-l`, `--launch` — Launch the session immediately after creating
* `-w`, `--worktree <WORKTREE_BRANCH>` — Create session in a git worktree for the specified branch
* `-b`, `--new-branch` — Create a new branch (use with --worktree)
* `-s`, `--sandbox` — Run session in Docker sandbox
* `--sandbox-image <SANDBOX_IMAGE>` — Custom Docker image for sandbox (implies --sandbox)
* `--trust-hooks` — Automatically trust repository hooks without prompting



## `aoe init`

Initialize .aoe/config.toml in a repository

**Usage:** `aoe init [PATH]`

###### **Arguments:**

* `<PATH>` — Directory to initialize (defaults to current directory)

  Default value: `.`



## `aoe list`

List all sessions

**Usage:** `aoe list [OPTIONS]`

###### **Options:**

* `--json` — Output as JSON
* `--all` — List sessions from all profiles



## `aoe remove`

Remove a session

**Usage:** `aoe remove [OPTIONS] <IDENTIFIER>`

###### **Arguments:**

* `<IDENTIFIER>` — Session ID or title to remove

###### **Options:**

* `--delete-worktree` — Delete worktree directory (default: keep worktree)
* `--keep-container` — Keep container instead of deleting it (default: delete per config)



## `aoe status`

Show session status summary

**Usage:** `aoe status [OPTIONS]`

###### **Options:**

* `-v`, `--verbose` — Show detailed session list
* `-q`, `--quiet` — Only output waiting count (for scripts)
* `--json` — Output as JSON



## `aoe session`

Manage session lifecycle (start, stop, attach, etc.)

**Usage:** `aoe session <COMMAND>`

###### **Subcommands:**

* `start` — Start a session's tmux process
* `stop` — Stop session process
* `restart` — Restart session
* `attach` — Attach to session interactively
* `show` — Show session details
* `current` — Auto-detect current session



## `aoe session start`

Start a session's tmux process

**Usage:** `aoe session start <IDENTIFIER>`

###### **Arguments:**

* `<IDENTIFIER>` — Session ID or title



## `aoe session stop`

Stop session process

**Usage:** `aoe session stop <IDENTIFIER>`

###### **Arguments:**

* `<IDENTIFIER>` — Session ID or title



## `aoe session restart`

Restart session

**Usage:** `aoe session restart <IDENTIFIER>`

###### **Arguments:**

* `<IDENTIFIER>` — Session ID or title



## `aoe session attach`

Attach to session interactively

**Usage:** `aoe session attach <IDENTIFIER>`

###### **Arguments:**

* `<IDENTIFIER>` — Session ID or title



## `aoe session show`

Show session details

**Usage:** `aoe session show [OPTIONS] [IDENTIFIER]`

###### **Arguments:**

* `<IDENTIFIER>` — Session ID or title (optional, auto-detects in tmux)

###### **Options:**

* `--json` — Output as JSON



## `aoe session current`

Auto-detect current session

**Usage:** `aoe session current [OPTIONS]`

###### **Options:**

* `-q`, `--quiet` — Just session name (for scripting)
* `--json` — Output as JSON



## `aoe group`

Manage groups for organizing sessions

**Usage:** `aoe group <COMMAND>`

###### **Subcommands:**

* `list` — List all groups
* `create` — Create a new group
* `delete` — Delete a group
* `move` — Move session to group



## `aoe group list`

List all groups

**Usage:** `aoe group list [OPTIONS]`

###### **Options:**

* `--json` — Output as JSON



## `aoe group create`

Create a new group

**Usage:** `aoe group create [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — Group name

###### **Options:**

* `--parent <PARENT>` — Parent group for creating subgroups



## `aoe group delete`

Delete a group

**Usage:** `aoe group delete [OPTIONS] <NAME>`

###### **Arguments:**

* `<NAME>` — Group name

###### **Options:**

* `--force` — Force delete by moving sessions to default group



## `aoe group move`

Move session to group

**Usage:** `aoe group move <IDENTIFIER> <GROUP>`

###### **Arguments:**

* `<IDENTIFIER>` — Session ID or title
* `<GROUP>` — Target group



## `aoe profile`

Manage profiles (separate workspaces)

**Usage:** `aoe profile [COMMAND]`

###### **Subcommands:**

* `list` — List all profiles
* `create` — Create a new profile
* `delete` — Delete a profile
* `default` — Show or set default profile



## `aoe profile list`

List all profiles

**Usage:** `aoe profile list`



## `aoe profile create`

Create a new profile

**Usage:** `aoe profile create <NAME>`

###### **Arguments:**

* `<NAME>` — Profile name



## `aoe profile delete`

Delete a profile

**Usage:** `aoe profile delete <NAME>`

###### **Arguments:**

* `<NAME>` — Profile name



## `aoe profile default`

Show or set default profile

**Usage:** `aoe profile default [NAME]`

###### **Arguments:**

* `<NAME>` — Profile name (optional, shows current if not provided)



## `aoe worktree`

Manage git worktrees for parallel development

**Usage:** `aoe worktree <COMMAND>`

###### **Subcommands:**

* `list` — List all worktrees in current repository
* `info` — Show worktree information for a session
* `cleanup` — Cleanup orphaned worktrees



## `aoe worktree list`

List all worktrees in current repository

**Usage:** `aoe worktree list`



## `aoe worktree info`

Show worktree information for a session

**Usage:** `aoe worktree info <IDENTIFIER>`

###### **Arguments:**

* `<IDENTIFIER>` — Session ID or title



## `aoe worktree cleanup`

Cleanup orphaned worktrees

**Usage:** `aoe worktree cleanup [OPTIONS]`

###### **Options:**

* `-f`, `--force` — Actually remove worktrees (default is dry-run)



## `aoe sounds`

Manage sound effects for agent state transitions

**Usage:** `aoe sounds <COMMAND>`

###### **Subcommands:**

* `install` — Install bundled sound effects
* `list` — List currently installed sounds
* `test` — Test a sound by playing it



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

* `<NAME>` — Sound file name (without extension)



## `aoe tmux`

tmux integration utilities

**Usage:** `aoe tmux <COMMAND>`

###### **Subcommands:**

* `status` — Output session info for use in custom tmux status bar



## `aoe tmux status`

Output session info for use in custom tmux status bar

Add this to your ~/.tmux.conf: set -g status-right "#(aoe tmux status)"

**Usage:** `aoe tmux status [OPTIONS]`

###### **Options:**

* `-f`, `--format <FORMAT>` — Output format (text or json)

  Default value: `text`



## `aoe uninstall`

Uninstall Agent of Empires

**Usage:** `aoe uninstall [OPTIONS]`

###### **Options:**

* `--keep-data` — Keep data directory (sessions, config, logs)
* `--keep-tmux-config` — Keep tmux configuration
* `--dry-run` — Show what would be removed without removing
* `-y` — Skip confirmation prompts



<hr/>

<small><i>
    This document was generated automatically by
    <a href="https://crates.io/crates/clap-markdown"><code>clap-markdown</code></a>.
</i></small>
