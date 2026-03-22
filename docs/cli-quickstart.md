# CLI Quick Reference

A task-oriented cheat sheet for managing sessions from the command line.
For the full flag reference, see [cli/reference.md](cli/reference.md).

## Global Flags

All commands accept these options:

```bash
aoe -p work ...       # Use the "work" profile (separate workspace)
aoe --sidebar-mode    # Launch TUI showing only the session list
```

## Adding Sessions

```bash
# Add current directory with an auto-generated title
aoe add

# Add with a custom title and group
aoe add -t my-feature -g backend

# Add a specific directory
aoe add ~/projects/api -t api-service

# Add and launch immediately
aoe add -l

# Add with a different agent
aoe add -c codex

# Add in a git worktree (creates worktree for the branch)
aoe add -w feature/auth

# Add in a new branch worktree
aoe add -w feature/auth -b

# Add in a Docker sandbox
aoe add -s

# Add with YOLO mode (skip agent permission prompts)
aoe add -y

# Combine flags: sandbox + yolo + launch
aoe add -s -y -l -t sandbox-test

# Add as a sub-session of an existing session
aoe add -P parent-title -t child-task
```

## Session Lifecycle

Sessions are identified by title or ID (or ID prefix).

```bash
aoe session start my-feature     # Start the tmux process
aoe session stop my-feature      # Stop the process
aoe session restart my-feature   # Stop then start
aoe session attach my-feature    # Attach interactively (Ctrl-B D to detach)
aoe session show my-feature      # Print session details
aoe session show --json          # JSON output (auto-detects session in tmux)
aoe session rename my-feature -t new-name
aoe session rename my-feature -g new-group
```

### Capture and Scripting

```bash
aoe session capture my-feature           # Last 50 lines of tmux output
aoe session capture my-feature -n 200    # Last 200 lines
aoe session capture --strip-ansi --json  # Clean output for scripts
aoe session current                      # Auto-detect current session
aoe session current -q                   # Just the session name (for piping)
```

## Listing and Status

```bash
aoe list              # List all sessions in current profile
aoe list --json       # JSON output
aoe list --all        # List across all profiles
aoe status            # Summary of session states
aoe status -v         # Detailed session list
aoe status -q         # Just the waiting count (for scripts)
aoe status --json     # JSON output
```

## Removing Sessions

```bash
aoe remove my-feature                # Remove session (keeps worktree)
aoe remove my-feature --delete-worktree  # Also delete the worktree directory
aoe remove my-feature --delete-branch    # Also delete the git branch
aoe remove my-feature --force            # Force removal with dirty worktree
```

## Groups

Groups organize sessions in the TUI sidebar.

```bash
aoe group list                        # List all groups
aoe group create backend              # Create a group
aoe group delete backend              # Delete a group
aoe group move my-feature backend     # Move session to group
```

You can also set the group when adding: `aoe add -g backend`.

## Profiles

Profiles are independent workspaces, each with their own sessions and groups.

```bash
aoe profile list                 # List all profiles
aoe profile create work          # Create a profile
aoe profile delete work          # Delete a profile
aoe profile rename work personal # Rename a profile
aoe profile default              # Show default profile
aoe profile default work         # Set default profile
```

Use `-p` on any command to target a specific profile:

```bash
aoe -p work add -t task1
aoe -p work list
aoe -p work session start task1
```

## Worktrees

Manage git worktrees created by `aoe add -w`.

```bash
aoe worktree list                # List managed worktrees
aoe worktree info my-feature     # Show worktree details
aoe worktree cleanup             # Remove orphaned worktrees
```

## Repo Config

Initialize per-project settings:

```bash
aoe init              # Create .aoe/config.toml in current directory
aoe init ~/projects/api
```

## Common Workflows

### Spin up a feature branch session

```bash
aoe add -w feature/auth -b -l -g features -t auth
# Creates worktree, new branch, launches immediately, grouped under "features"
```

### Run multiple agents on the same project

```bash
aoe add -t claude-main
aoe add -t codex-review -c codex
aoe session start claude-main
aoe session start codex-review
```

### Sandboxed session with custom image

```bash
aoe add --sandbox-image my-dev-image:latest -y -l -t isolated
```

### Check what needs attention

```bash
aoe status -q    # Returns count of sessions waiting for input
```
