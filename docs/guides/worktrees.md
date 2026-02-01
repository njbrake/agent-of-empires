# Worktrees Reference

Reference documentation for git worktree commands and configuration in `aoe`.

For workflow guidance, see the [Workflow Guide](workflow.md).

## CLI vs TUI Behavior

| Feature | CLI | TUI |
|---------|-----|-----|
| Create new branch | Use `-b` flag | Always creates new branch |
| Use existing branch | Omit `-b` flag | Not supported |
| Branch validation | Checks if branch exists | None (always creates) |

## CLI Commands

```bash
# Create worktree session (new branch)
aoe add . -w feat/my-feature -b

# Create worktree session (existing branch)
aoe add . -w feat/my-feature

# List all worktrees
aoe worktree list

# Show session info
aoe worktree info <session>

# Find orphaned worktrees
aoe worktree cleanup

# Remove session (prompts for worktree cleanup)
aoe remove <session>

# Remove session (keep worktree)
aoe remove <session> --keep-worktree
```

## TUI Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `n` | New session dialog |
| `Tab` | Next field |
| `Shift+Tab` | Previous field |
| `Enter` | Submit and create session |
| `Esc` | Cancel |

When creating a session with a worktree branch name in the TUI, it automatically creates a new branch and worktree.

## Configuration

```toml
[worktree]
enabled = false
path_template = "../{repo-name}-worktrees/{branch}"
bare_repo_path_template = "./{branch}"
auto_cleanup = true
show_branch_in_tui = true
delete_branch_on_cleanup = false
```

### Template Variables

| Variable | Description |
|----------|-------------|
| `{repo-name}` | Repository folder name |
| `{branch}` | Branch name (slashes converted to hyphens) |
| `{session-id}` | First 8 characters of session UUID |

### Path Template Examples

```toml
# Default (sibling directory) - used for non-bare repos
path_template = "../{repo-name}-worktrees/{branch}"

# Bare repo default (worktrees as siblings)
bare_repo_path_template = "./{branch}"

# Nested in repo
path_template = "./worktrees/{branch}"

# Absolute path
path_template = "/absolute/path/to/worktrees/{repo-name}/{branch}"

# With session ID for uniqueness
path_template = "../wt/{branch}-{session-id}"
```

## Cleanup Behavior

| Scenario | Cleanup Prompt? |
|----------|-----------------|
| aoe-managed worktree | Yes |
| Manual worktree | No |
| `--keep-worktree` flag | No (skips prompt) |
| Non-worktree session | No |

## Auto-Detection

AOE automatically detects bare repos and uses `bare_repo_path_template` instead of `path_template`, creating worktrees as siblings within the project directory.

## File Locations

| Item | Path |
|------|------|
| Config | `~/.agent-of-empires/config.toml` |
| Sessions | `~/.agent-of-empires/profiles/<profile>/sessions.json` |

## Error Messages

| Error | Solution |
|-------|----------|
| "Not in a git repository" | Navigate to a git repo first |
| "Worktree already exists" | Use different branch name or add `{session-id}` to template |
| "Failed to remove worktree" | May need manual cleanup with `git worktree remove` |
| "Branch already exists" (CLI) | Branch exists; remove `-b` flag to use existing branch |
