# Two-Terminal Workflow

A practical workflow for using `aoe` with git worktrees, keeping agent management separate from git operations.

## Philosophy

`aoe` manages your AI coding sessions—nothing more. All git operations, terminal commands, and MCP configuration happen in separate terminals. This separation keeps `aoe` lightweight and lets each tool do what it does best.

## Setup

### Terminal 1: Agent Sessions

Run `aoe` here. This terminal is dedicated to:

- Creating new sessions (one per task/feature)
- Monitoring session status
- Attaching to sessions when you need to interact with the agent

```bash
cd ~/scm/my-project
aoe -p <"personal" or "work" etc, or omit arg to use default profile>
```

### Terminal 2: Git & Shell Operations

Use this terminal for:

- Monitoring git status across all worktrees
- Running `git pull` on main to keep it current
- Any bash/terminal work outside of agent sessions

## Daily Workflow

### Starting Work

1. **Update main** (Terminal 2):
   ```bash
   cd ~/path/to/my-project
   git pull origin main
   ```

2. **Create a session** (Terminal 1):
   in the aoe TUI, creat a new session and fill in the git worktree name with what you want the feature branch to be named
   This creates a new branch from your updated main and a worktree at `../my-project-worktrees/feat-auth-refactor`.

### During Work

- **Terminal 1**: Interact with agents, switch between sessions
- **Terminal 2**: Monitor changes, run builds, check git status


### Finishing a Task

Once an agent is finished and I've git committed and pushed the branch to github, filed and merged the PR, then
in the aoe TUI I delete the session, which automatically also deletes the git worktree branch.

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

## Tips

- **Keep main clean**: Never work directly in the main repo. Always use worktrees.
- **Pull before creating**: Always `git pull` on main before creating new sessions so branches start from recent commits.
- **One task, one session**: Each worktree maps to one aoe session. This keeps context isolated.
- **Let agents stay focused**: The agent in each session only sees its worktree. Git operations happen outside.

## Why This Works

| Task | Where |
|------|-------|
| Agent interactions | Terminal 1 (aoe) |
| Git commits, pushes, PRs | Terminal 2 |
| Build commands, tests | Terminal 2 |
| MCP configuration | Outside aoe |
| Session management | Terminal 1 (aoe) |

This separation means `aoe` never gets in the way of your existing tools and workflows.
