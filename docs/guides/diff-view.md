# Diff View

The diff view lets you review changes between your working directory and a base branch (like `main`), then edit files directly.

## Opening Diff View

From the main screen, press `D` to open the diff view. It shows:
- **Left panel**: List of changed files with status indicators (M=modified, A=added, D=deleted)
- **Right panel**: Diff content for the selected file

The diff is computed against the base branch (defaults to `main` or your repo's default branch).

Auto-detection scores every configured remote, not just `origin`. In a fork plus `upstream` layout, the diff compares against `upstream/main` when that tip is fresher than your local `main` and `origin/main`. The chosen ref is whichever candidate HEAD descends from with the most recent commit time, so a worktree branched off `upstream/main` does not see the gap between your stale fork-main and the actual branch point as session changes.

## Navigation

| Key | Action |
|-----|--------|
| `j` / `k` or `↑` / `↓` | Navigate between files |
| Scroll wheel | Scroll through diff content |
| `PgUp` / `PgDn` | Page through diff |
| `g` / `G` | Jump to top / bottom of diff |

## Editing Files

Press `e` or `Enter` to open the selected file in your editor (`$EDITOR`, or vim/nano if not set).

After saving and exiting, the diff view refreshes automatically to show your changes.

## Other Commands

| Key | Action |
|-----|--------|
| `b` | Change base branch (persists per-session as `base_branch_override`) |
| `r` | Refresh the diff |
| `?` | Show help |
| `Esc` | Close diff view |

## Per-session base override

Each session has an optional `base_branch_override` that takes
precedence over the profile default and auto-detection. Use it when
the eventual PR target differs from the project default (stacked PRs,
hotfix off `release/*`, branch rename). The override is sticky across
restarts and only affects the comparison, not the worktree itself
(no rebase). See #970.

- **Web dashboard**: click the `vs <ref>` chip in the diff header, pick
  a branch from the typeahead (local + remote-only), or use
  "Reset to auto-detected" to clear.
- **TUI diff view**: press `b`, pick a branch; the choice is persisted
  to `sessions.json` and restored on next launch.
- **CLI**: `aoe session set-base <session> <branch>` to set,
  `aoe session set-base <session> --clear` to clear.

## Configuration

In your config file (`~/.config/agent-of-empires/config.toml` on Linux, `~/.agent-of-empires/config.toml` on macOS):

```toml
[diff]
# Default branch to compare against (auto-detected if not set)
default_branch = "main"

# Lines of context around changes (default: 3)
context_lines = 3
```

## Tips: See Changes While Editing

The diff view shows you where changes are before you edit. For an even better experience, you can install editor plugins that show git diff markers in the gutter while you edit:

### Vim

Install [vim-gitgutter](https://github.com/airblade/vim-gitgutter) or [vim-signify](https://github.com/mhinz/vim-signify). These show `+`, `-`, and `~` markers in the sign column for added, removed, and modified lines.

With vim-plug:
```vim
Plug 'airblade/vim-gitgutter'
```

### Nano

Nano doesn't have a plugin system, so there's no equivalent. Use the diff view to note line numbers before editing, or consider switching to vim for this workflow.

### Other Editors

- **Emacs**: [git-gutter](https://github.com/emacsorphanage/git-gutter)
- **VS Code**: Built-in git gutter support
- **Sublime Text**: [GitGutter](https://packagecontrol.io/packages/GitGutter)

## Workflow Example

1. Press `D` to open diff view
2. Use `j`/`k` to browse changed files
3. Scroll to review each file's changes
4. Press `e` to edit a file that needs work
5. Save and exit the editor
6. Continue reviewing (diff auto-refreshes)
7. Press `Esc` when done
