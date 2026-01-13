# Batch Process GitHub Issues

One way you could leverage `aoe` is to use a script like [batch-issues.sh](./batch-issues.sh) to tackle planning out how to solve a host of issues.

## Usage

```bash
./docs/guides/batch-issues.sh --repo OWNER/REPO --path PATH [OPTIONS]
```

### Required Arguments

| Argument | Description |
|----------|-------------|
| `--repo OWNER/REPO` | GitHub repository (e.g., `mozilla-ai/any-llm`) |
| `--path PATH` | Path to local clone of the repository |

### Optional Arguments

| Argument | Description |
|----------|-------------|
| `-p, --profile NAME` | aoe profile to use (default: derived from repo name) |
| `-g, --group NAME` | Session group name (default: derived from repo name) |
| `-s, --sandbox IMG` | Custom Docker sandbox image (sandbox enabled by default) |
| `-n, --dry-run` | Preview what would be created without making changes |
| `-l, --limit NUM` | Process only the first N issues |
| `-h, --help` | Show help message |

## Examples

### Basic Usage

```bash
# Process all open issues in a repository
./docs/guides/batch-issues.sh \
    --repo mozilla-ai/any-llm \
    --path ~/scm/any-llm
```


## How It Works

The script uses:

- `gh issue list` to fetch open issues from GitHub
- `aoe add` with `--worktree` to create isolated sessions
- `aoe session start` to launch each session
- `tmux send-keys` to automatically accept the permissions prompt

Each session runs Claude Code with:
- `--dangerously-skip-permissions` for autonomous operation
- `--permission-mode plan` to have Claude propose solutions before implementing
