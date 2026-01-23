# Git Worktree - Quick Reference

## CLI vs TUI Behavior

| Feature | CLI | TUI |
|---------|-----|-----|
| Create new branch | Use `-b` flag | Always creates new branch |
| Use existing branch | Omit `-b` flag | Not supported |
| Branch validation | Checks if branch exists | None (always creates) |

## One-Liner Commands

```bash
# Create worktree session (new branch)
aoe add . -w feat/my-feature -b

# Create worktree session (existing branch)
aoe add . -w feat/my-feature

# List all worktrees
aoe worktree list

# Show session info
aoe worktree info <session>

# Find orphans
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
| `←/→` | Toggle tool selection (when on tool field) |
| `Enter` | Submit and create session |
| `Esc` | Cancel |

**Note:** When creating a session with a worktree branch name in the TUI, it will automatically create a new branch and worktree.

## Default Configuration

```toml
[worktree]
enabled = false
path_template = "../{repo-name}-worktrees/{branch}"
bare_repo_path_template = "./{branch}"  # Used for bare repo setups
auto_cleanup = true
show_branch_in_tui = true
```

## Template Variables

- `{repo-name}` - Repository folder name
- `{branch}` - Branch name (slashes → hyphens)
- `{session-id}` - First 8 chars of UUID

## Common Path Templates

```toml
# Default (sibling directory)
path_template = "../{repo-name}-worktrees/{branch}"

# Nested in repo
path_template = "./worktrees/{branch}"

# Absolute path
path_template = "/absolute/path/to/worktrees/{repo-name}/{branch}"

# With session ID
path_template = "../wt/{branch}-{session-id}"
```

## Quick Start

```bash
# 1. Enable worktrees (first time only)
cd ~/scm/my-project
aoe add . -w feat/test -b

# 2. Create parallel sessions in TUI
aoe
# Press 'n' and fill in the "Worktree (optional)" field:
#   - Title: UI Changes, Worktree: feat/ui-changes
#   - Title: API Changes, Worktree: feat/api-changes
#   - Title: Urgent Fix, Worktree: fix/urgent-bug
# Each session will create a new branch and worktree automatically

# 3. View all worktrees
aoe worktree list

# 4. Work and cleanup
aoe remove <session>  # Answer Y to delete worktree
```

## Cleanup Behavior

| Scenario | Cleanup Prompt? |
|----------|-----------------|
| aoe-managed worktree | ✅ Yes (Y/n) |
| Manual worktree | ❌ No |
| `--keep-worktree` flag | ❌ No (skips prompt) |
| Non-worktree session | ❌ No |

## Workflow Examples

### CLI Workflow
```bash
# Create 3 parallel feature sessions
cd ~/scm/my-app
aoe add . -w feat/ui -b
aoe add . -w feat/api -b
aoe add . -w feat/db -b

# View all
aoe worktree list

# Work in TUI
aoe  # See all 3 with branch names

# When done
aoe remove <id>  # Cleans up worktree
```

### TUI Workflow
```bash
# Launch TUI
cd ~/scm/my-app
aoe

# Press 'n' to open new session dialog
# Fill in fields:
#   Title: Feature UI (or leave empty for random name)
#   Path: . (current directory)
#   Group: (optional)
#   Tool: claude (or select your tool)
#   Worktree (optional): feat/ui-changes
# Press Enter

# Creates:
#   ✅ New branch: feat/ui-changes
#   ✅ New worktree: ../my-app-worktrees/feat-ui-changes
#   ✅ New session attached to worktree
#   ✅ Launches you into the session

# Repeat for more parallel sessions
```

## File Locations

- **Config:** `~/.agent-of-empires/config.toml`
- **Sessions:** `~/.agent-of-empires/profiles/<profile>/sessions.json`
- **Default Worktrees:** `../<repo-name>-worktrees/`

## Error Messages

| Error | Solution |
|-------|----------|
| "Not in a git repository" | Navigate to git repo first |
| "Worktree already exists" | Use different branch name or session-id in template |
| "Failed to remove worktree" | May need manual cleanup with `git worktree remove` |
| "Branch already exists" (CLI only) | Branch exists; remove `-b` flag to use existing branch |

## Pro Tips

- ✅ Use descriptive branch names (visible in TUI)
- ✅ Check preview panel before starting work
- ✅ Run `aoe worktree cleanup` periodically
- ✅ Use `--keep-worktree` when preserving work
- ✅ Keep main repo on main/master branch

## Bare Repo Workflow (Recommended for Sandboxing)

The "linked worktree bare repo" pattern keeps all worktrees as siblings under one directory. This is especially useful when combined with Docker sandboxing, since everything stays in one place.

### Why Use This Pattern?

With the default worktree template (`../{repo-name}-worktrees/{branch}`), worktrees are created outside the main repo. This causes issues with sandboxing because the worktree's `.git` file points back to the main repo, which may be outside the sandbox.

The bare repo pattern solves this by keeping everything together:

```
my-project/
  .bare/               # Bare git repository
  .git                 # File: "gitdir: ./.bare"
  main/                # Worktree for main branch
  feat-api/            # Worktree for feature branch
  fix-bug/             # Worktree for bugfix branch
```

### Setting Up a Bare Repo

**For a new project:**

```bash
# Clone as bare into a .bare directory
git clone --bare git@github.com:user/repo.git my-project/.bare

# Create the .git file pointing to .bare
cd my-project
echo "gitdir: ./.bare" > .git

# Configure the bare repo to work with worktrees
git config remote.origin.fetch "+refs/heads/*:refs/remotes/origin/*"
git fetch origin

# Create your first worktree (e.g., main branch)
git worktree add main main
```

**For an existing project:**

```bash
cd my-project

# Move the existing .git directory to .bare
mv .git .bare

# Create .git file pointing to .bare
echo "gitdir: ./.bare" > .git

# Your existing files become the "main" worktree conceptually
# Create additional worktrees as needed
git worktree add feat-api feat/api
```

### AOE Auto-Detection

AOE automatically detects bare repo setups and uses a different default path template:

| Repo Type | Default Template |
|-----------|------------------|
| Regular | `../{repo-name}-worktrees/{branch}` |
| Bare | `./{branch}` |

This means worktrees are created as siblings (e.g., `./feat-api/`) rather than in a separate directory.

### Configuration

You can customize the bare repo template in `~/.agent-of-empires/config.toml`:

```toml
[worktree]
path_template = "../{repo-name}-worktrees/{branch}"  # For regular repos
bare_repo_path_template = "./{branch}"               # For bare repos
```

### Example Workflow with Sandboxing

```bash
# Set up bare repo (one-time)
git clone --bare git@github.com:user/my-app.git my-app/.bare
cd my-app
echo "gitdir: ./.bare" > .git
git config remote.origin.fetch "+refs/heads/*:refs/remotes/origin/*"
git fetch origin
git worktree add main main
cd main

# Create sandboxed sessions with worktrees
aoe add --sandbox -w feat/auth -b .
aoe add --sandbox -w feat/api -b .

# Result:
# my-app/
#   .bare/
#   .git
#   main/
#   feat-auth/    # Sandboxed session 1
#   feat-api/     # Sandboxed session 2
```

Each worktree can be sandboxed with access to the shared `.bare` directory, enabling isolated parallel development.
