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
* [`aoe session rename`↴](#aoe-session-rename)
* [`aoe session capture`↴](#aoe-session-capture)
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
* [`aoe profile rename`↴](#aoe-profile-rename)
* [`aoe profile default`↴](#aoe-profile-default)
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
* [`aoe uninstall`↴](#aoe-uninstall)
* [`aoe completion`↴](#aoe-completion)

## `aoe`

▨ kokorro (aoe) is a terminal session manager that uses tmux to help you manage and monitor AI coding agents like Claude Code and OpenCode.

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
* `tmux` — tmux integration utilities
* `sounds` — Manage sound effects for agent state transitions
* `uninstall` — Uninstall ▨ kokorro
* `completion` — Generate shell completions

###### **Options:**

* `-p`, `--profile <PROFILE>` — Profile to use (separate workspace with its own sessions)
* `--sidebar-mode` — Launch in sidebar mode (shows only the session list, no preview)



## `aoe add`

Add a new session

**Usage:** `aoe add [OPTIONS] [PATH]`

###### **Arguments:**

* `<PATH>` — Project directory (defaults to current directory)

  Default value: `.`

###### **Options:**

* `-t`, `--title <TITLE>` — Session title (defaults to folder name)
* `-g`, `--group <GROUP>` — Group path (defaults to parent folder)
* `-c`, `--cmd <COMMAND>` — Command to run (e.g., 'claude' or any other supported agent)
* `-P`, `--parent <PARENT>` — Parent session (creates sub-session, inherits group)
* `-l`, `--launch` — Launch the session immediately after creating
* `-w`, `--worktree <WORKTREE_BRANCH>` — Create session in a git worktree for the specified branch
* `-b`, `--new-branch` — Create a new branch (use with --worktree)
* `-r`, `--repo <EXTRA_REPOS>` — Additional repositories for multi-repo workspace (use with --worktree)
* `-s`, `--sandbox` — Run session in Docker sandbox
* `--sandbox-image <SANDBOX_IMAGE>` — Custom Docker image for sandbox (implies --sandbox)
* `-y`, `--yolo` — Enable YOLO mode (skip permission prompts)
* `--trust-hooks` — Automatically trust repository hooks without prompting
* `--extra-args <EXTRA_ARGS>` — Extra arguments to append after the agent binary
* `--cmd-override <CMD_OVERRIDE>` — Override the agent binary command



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
* `--delete-branch` — Delete git branch after worktree removal (default: per config)
* `--force` — Force worktree removal even with untracked/modified files
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
* `rename` — Rename a session
* `capture` — Capture tmux pane output
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



## `aoe session rename`

Rename a session

**Usage:** `aoe session rename [OPTIONS] [IDENTIFIER]`

###### **Arguments:**

* `<IDENTIFIER>` — Session ID or title (optional, auto-detects in tmux)

###### **Options:**

* `-t`, `--title <TITLE>` — New title for the session
* `-g`, `--group <GROUP>` — New group for the session (empty string to ungroup)



## `aoe session capture`

Capture tmux pane output

**Usage:** `aoe session capture [OPTIONS] [IDENTIFIER]`

###### **Arguments:**

* `<IDENTIFIER>` — Session ID or title (auto-detects in tmux if omitted)

###### **Options:**

* `-n`, `--lines <LINES>` — Number of lines to capture

  Default value: `50`
* `--strip-ansi` — Strip ANSI escape codes
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
* `rename` — Rename a profile
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



## `aoe profile rename`

Rename a profile

**Usage:** `aoe profile rename <OLD_NAME> <NEW_NAME>`

###### **Arguments:**

* `<OLD_NAME>` — Current profile name
* `<NEW_NAME>` — New profile name



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



## `aoe uninstall`

Uninstall ▨ kokorro

**Usage:** `aoe uninstall [OPTIONS]`

###### **Options:**

* `--keep-data` — Keep data directory (sessions, config, logs)
* `--keep-tmux-config` — Keep tmux configuration
* `--dry-run` — Show what would be removed without removing
* `-y` — Skip confirmation prompts



## `aoe completion`

Generate shell completions

**Usage:** `aoe completion <SHELL>`

###### **Arguments:**

* `<SHELL>` — Shell to generate completions for

  Possible values: `bash`, `elvish`, `fish`, `powershell`, `zsh`




<hr/>

<small><i>
    This document was generated automatically by
    <a href="https://crates.io/crates/clap-markdown"><code>clap-markdown</code></a>.
</i></small>
