# Worktrees Reference

Reference documentation for git worktree commands and configuration in `aoe`.

For workflow guidance, see the [Workflow Guide](workflow.md).

## CLI vs TUI Behavior

| Feature | CLI | TUI |
|---------|-----|-----|
| Create new branch | Use `-b` flag | Always creates new branch |
| Use existing branch | Omit `-b` flag | "Attach to existing branch" toggle (TUI: `Ctrl+P`; web: in the session step under the branch field) |
| Branch validation | Checks if branch exists | None (always creates) |
| Pick a base branch | `--base-branch <name>` | `Base` field in `Ctrl+P` overlay |

## CLI Commands

```bash
# Create worktree session (new branch, branched off the repo default)
aoe add . -w feat/my-feature -b

# Create worktree session (new branch, branched off a specific base)
aoe add . -w hotfix-1 -b --base-branch release-1.2

# Attach to an existing branch + worktree (or check out the branch into a
# new worktree if no worktree exists yet). The `-b` flag is what flips
# between "create a new branch" and "attach"; omitting it = attach.
aoe add . -w feat/my-feature

# List all worktrees
aoe worktree list

# Show session info
aoe worktree info <session>

# Find orphaned worktrees
aoe worktree cleanup

# Remove session (prompts for worktree cleanup)
aoe remove <session>

# Remove session and delete worktree
aoe remove <session> --delete-worktree
```

`--base-branch` only matters with `--new-branch` / `-b`. The base is
resolved against the remote first (`origin/<base>`), then against a
local branch with that name, so passing a teammate's not-yet-fetched
branch works without a manual `git fetch`. When omitted, the new
branch is based on the repository's default branch (`main`/`master`).

Default-branch detection scores every configured remote (not just
`origin`). In a fork plus `upstream` layout, aoe picks the freshest
`main`/`master` it can find that HEAD descends from. So if `upstream/main`
is ahead of `origin/main` and your local `main`, `aoe add` fetches and
branches off `upstream/main` rather than the stale fork tip. See issue
\#1029 for the rationale.

## TUI Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `n` | New session dialog |
| `Tab` | Next field |
| `Shift+Tab` | Previous field |
| `Enter` | Submit and create session |
| `Esc` | Cancel |

In the TUI, enable the Worktree checkbox to create a new branch and worktree. By default, the worktree name is derived from the session title. Press `Ctrl+P` on the Worktree field to set an explicit `Name`, attach to an existing branch, pick a `Base` branch the new branch is based on (defaults to the repo default), or configure extra repos. `Ctrl+P` on the `Base` field opens a branch picker over local and remote-tracking branches.

The web dashboard's new-session wizard exposes the same control under an "Advanced" disclosure beneath the worktree name input; it shows a typeahead populated from local + remote branches via `GET /api/git/branches?include_remote=true`. The same step also exposes an "Attach to existing branch" toggle that flips the request from "create new branch" to "attach to whichever branch is named" — when on, the server re-uses any existing worktree for that branch and otherwise checks the branch out into a new worktree. Mirrors the TUI / CLI behavior (CLI: omit `-b`). See #969.

## Configuration

```toml
[worktree]
enabled = false
path_template = "../{repo-name}-worktrees/{branch}"
bare_repo_path_template = "./{branch}"
auto_cleanup = true
show_branch_in_tui = true
delete_branch_on_cleanup = false
init_submodules = true
```

### Skipping submodule init

`init_submodules = false` skips the `git submodule update --init --recursive` step that runs after `git worktree add` when the checkout contains a `.gitmodules` file. Useful for repos that vendor deep submodule trees (e.g. OpenROAD-flow-scripts, llvm-project, chromium) where every new session would otherwise sit in `Creating…` for minutes while submodules clone. Per-invocation override on the CLI: `aoe add --worktree <branch> --no-submodules`.

On the delete side, aoe runs `git submodule deinit -f --all` before `git worktree remove` for any worktree with `.gitmodules`, so the panic-button `Force` checkbox is not required just because the worktree has submodules. If git still refuses (e.g. a partially-broken submodule), aoe falls back to clearing `<main>/.git/worktrees/<name>/modules/` and pruning the stale entry manually.

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

## Post-Checkout Hooks

Some repos install pre-commit hooks at the `post-checkout` stage (`uv-sync`, `npm install`, LFS smudge, etc.) that fire when `git worktree add` checks out the new branch. If such a hook fails, the worktree directory and its `.git` pointer have already been created, and the worktree is usable. AOE no longer aborts session creation in that case: the hook output is captured and surfaced as a warning.

| Surface | Where the warning appears |
|---|---|
| CLI (`aoe add`) | `⚠ <message>` line on stderr after `✓ Worktree created successfully` |
| TUI | `Worktree warnings` info dialog opens after the session is added |
| Web | Toast per warning, plus `warnings: string[]` on the `POST /api/sessions` response body |

Common cause: the hook calls a tool (uv, npm, pip) that needs network access or credentials the new worktree does not yet have. Re-run the hook manually inside the worktree once the environment is set up, or disable it for AOE-created worktrees by configuring `core.hooksPath` per checkout.

## Performance & Debug Logging

`create_worktree` is instrumented end-to-end so a slow run can be diagnosed from `debug.log` (`AGENT_OF_EMPIRES_DEBUG=1`):

```
INFO worktree create: start branch=... path=...
INFO worktree create: prune done in 12ms
INFO git fetch origin/main ok in 1.7s
INFO worktree create: fetch step done in 1.7s
INFO worktree create: branch resolve done in 2ms
INFO worktree create: git worktree add done in 90ms (518 files, 5690035 bytes checked out)
INFO worktree create: convert .git file done in 120µs
INFO worktree create: submodules (initialized count=1) done in 2.0s
INFO worktree create: TOTAL 3.9s branch=... path=... warnings=0
```

Network IO (`git fetch`, `git submodule update`) dominates almost every slow run. `git worktree add` itself only checks out tracked files; it does **not** copy `node_modules`, `.venv`, `target/`, or any other gitignored content.

For multi-repo workspaces, the per-repo `create_worktree` calls run concurrently via `std::thread::scope`, so wall-clock time is roughly that of the slowest single repo rather than the sum across repos.

## Cleanup Behavior

| Scenario | Cleanup Prompt? |
|----------|-----------------|
| aoe-managed worktree | Yes |
| Manual worktree | No |
| `--delete-worktree` flag | Yes (deletes worktree) |
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
