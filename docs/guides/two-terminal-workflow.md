# Two-Terminal Workflow

A practical workflow for using `aoe` with git worktrees, keeping agent management separate from git operations.

## Philosophy

`aoe` manages your AI coding sessions and provides paired terminals for each session. This separation keeps agent interactions distinct from your shell work while keeping everything organized in one tool.

## Setup

### Terminal 1: Agent Sessions (Agent View)

Run `aoe` here in the default Agent View. This terminal is dedicated to:

- Creating new sessions (one per task/feature)
- Monitoring agent session status
- Attaching to sessions when you need to interact with the agent

```bash
cd ~/scm/my-project
aoe -p <"personal" or "work" etc, or omit arg to use default profile>
```

### Terminal 2: Git & Shell Operations (Terminal View)

Run `aoe` here too, but press `t` to switch to Terminal View. This terminal is for:

- Accessing project-specific terminals at the correct working directory
- Running git commands, builds, and tests
- Any bash/terminal work outside of agent sessions

```bash
cd ~/scm/my-project
aoe -p <same profile as Terminal 1>
# Press 't' to switch to Terminal View
```

The status bar will show `[Term]` when in Terminal View.

## Daily Workflow

### Starting Work

1. **Update main** (Terminal 2 - Terminal View):
   - Select your main project session and press Enter to attach to its terminal
   - Run `git pull origin main`
   - Detach with `Ctrl+b d`

2. **Create a session** (Terminal 1 - Agent View):
   In the aoe TUI, create a new session and fill in the git worktree name with what you want the feature branch to be named.
   This creates a new branch from your updated main and a worktree at `../my-project-worktrees/feat-auth-refactor`.

### During Work

- **Terminal 1 (Agent View)**: Interact with agents, switch between sessions, monitor status
- **Terminal 2 (Terminal View)**: Access paired terminals, run builds, check git status

Each agent session has a paired terminal that:
- Opens at the same `project_path` as the agent
- Runs on the host (not in Docker, even if the agent is sandboxed)
- Is created lazily when you first attach to it

### Finishing a Task

Once an agent is finished and you've committed and pushed the branch to GitHub, filed and merged the PR:
- Switch to Terminal 1 (Agent View)
- Delete the session, which automatically deletes both the git worktree branch and the paired terminal

Note: You cannot delete sessions from Terminal View. Switch back to Agent View to delete.

## Directory Layout

```
~/scm/
├── my-project/              # Main repo (stays on main branch)
│   └── ...
└── my-project-worktrees/    # All worktrees live here
    ├── feat-auth-refactor/
    ├── feat-new-api/
    └── fix-login-bug/
```

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `t` | Toggle between Agent View and Terminal View |
| `Enter` | Attach to agent (Agent View) or terminal (Terminal View) |
| `d` | Delete session (Agent View only) |
| `n` | Create new session |
| `?` | Show help |

## Tips

- **Keep main clean**: Never work directly in the main repo. Always use worktrees.
- **Pull before creating**: Always `git pull` on main before creating new sessions so branches start from recent commits.
- **One task, one session**: Each worktree maps to one aoe session. This keeps context isolated.
- **Let agents stay focused**: The agent in each session only sees its worktree. Git operations happen in the paired terminal.
- **Use the same profile**: Both terminals should use the same profile to see the same sessions.

## Why This Works

| Task | Where |
|------|-------|
| Agent interactions | Terminal 1 (Agent View) |
| Git commits, pushes, PRs | Terminal 2 (Terminal View) |
| Build commands, tests | Terminal 2 (Terminal View) |
| Session management | Terminal 1 (Agent View) |

Both terminals run `aoe`, giving you a unified interface while keeping agent work separate from shell work.
