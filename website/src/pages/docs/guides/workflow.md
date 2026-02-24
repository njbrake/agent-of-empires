---
layout: ../../../layouts/Docs.astro
title: Workflow Guide
description: Recommended setup and daily workflow for using AoE with git worktrees and bare repos.
---

This guide covers the recommended setup and daily workflow for using `aoe` with git worktrees.

## Project Setup: Bare Git Repos

The recommended way to set up a project is using a "bare repo" structure. This keeps your main repository and all worktrees organized under a single directory:

```
my-project/
  .bare/               # Bare git repository
  .git                 # File pointing to .bare
  main/                # Worktree for main branch
  feat-api/            # Worktree for feature branch
  fix-bug/             # Another worktree
```

### Initial Setup

```bash
# Clone as bare repo
git clone --bare git@github.com:user/repo.git my-project/.bare

cd my-project

# Create .git file pointing to bare repo
echo "gitdir: ./.bare" > .git

# Configure fetch to get all branches
git config remote.origin.fetch "+refs/heads/*:refs/remotes/origin/*"
git fetch origin

# Create your main worktree
git worktree add main main
```

Now when you run `aoe` from `my-project/`, new worktrees are created as siblings (e.g., `my-project/feat-api/`) rather than in a separate directory.

### Why Bare Repos?

- **Clean organization**: Everything lives under one project directory
- **Sandbox-friendly**: All paths stay within the project root (important for Docker sandboxing)
- **Easy navigation**: Switch between branches by switching directories

## Single-Window Workflow

Run `aoe` in a single terminal and toggle between views:

| Key | View | Purpose |
|-----|------|---------|
| (default) | Agent View | Manage and interact with AI coding agents |
| `t` | Terminal View | Access paired terminals for git, builds, tests |

### Daily Workflow

**1. Start your day**

```bash
cd ~/scm/my-project
aoe
```

You'll see your sessions in Agent View. Keep one session on `main` for general questions and pulling updates.

**2. Update main** (Terminal View)

- Press `t` to switch to Terminal View
- Select your main session, press `Enter` to attach to its terminal
- Run `git pull origin main`
- Detach with `Ctrl+b d`
- Press `t` to return to Agent View

**3. Create a new session**

- Press `n` to open the new session dialog
- Fill in the worktree field with your branch name (e.g., `feat/auth-refactor`)
- Press `Enter`

This creates:
- A new branch from your current HEAD
- A new worktree at `./feat-auth-refactor/`
- A new session with an agent working in that worktree

**4. Work on your feature** (Agent View)

- Select your session and press `Enter` to attach
- Interact with the agent
- Detach with `Ctrl+b d` when done

**5. Run builds/tests** (Terminal View)

- Press `t` to switch to Terminal View
- Select the same session, press `Enter`
- Run your build commands, tests, git operations
- Detach with `Ctrl+b d`

**6. Clean up when done**

- In Agent View, select the session and press `d` to delete
- Answer `Y` to also remove the worktree

## Tips

- **Keep one session on main**: Use it for codebase questions and its terminal for `git pull`
- **One task, one session**: Each worktree maps to one aoe session. Keeps context isolated.
- **Pull before creating**: Always update main before creating new sessions so branches start fresh
- **Let agents stay focused**: Git operations happen in the paired terminal, not in agent sessions

## Keyboard Reference

| Key | Action |
|-----|--------|
| `t` | Toggle between Agent View and Terminal View |
| `D` | Open [Diff View](/docs/guides/diff-view/) to review git changes |
| `Enter` | Attach to agent (Agent View) or terminal (Terminal View) |
| `n` | Create new session |
| `d` | Delete session (Agent View only) |
| `?` | Show help |
| `Ctrl+b d` | Detach from tmux (return to aoe) |

## Non-Bare Repos

If you're not using a bare repo setup, aoe defaults to creating worktrees in a sibling directory:

```
~/scm/
  my-project/              # Your repo (stays on main)
  my-project-worktrees/    # Worktrees created here
    feat-auth-refactor/
    fix-bug/
```

You can customize this with `path_template` in your config. See the [Worktrees Reference](/docs/guides/worktrees/) for details.
