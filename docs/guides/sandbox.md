# Docker Sandbox: Quick Reference

## Overview

Docker sandboxing runs your AI coding agents (Claude Code, OpenCode, Mistral Vibe, Codex CLI, Gemini CLI) inside isolated Docker containers while maintaining access to your project files and credentials.

**Key Features:**
- One container per session
- Shared authentication across containers (no re-auth needed)
- Automatic container lifecycle management
- Full project access via volume mounts

## CLI vs TUI Behavior

| Feature | CLI | TUI |
|---------|-----|-----|
| Enable sandbox | `--sandbox` flag | Checkbox toggle |
| Custom image | `--sandbox-image <image>` | Not supported |
| Container cleanup | Automatic on remove | Automatic on remove |
| Keep container | `--keep-container` flag | Not supported |

## One-Liner Commands

```bash
# Create sandboxed session
aoe add --sandbox .

# Create sandboxed session with custom image
aoe add --sandbox-image myregistry/custom:v1 .

# Create and launch sandboxed session
aoe add --sandbox -l .

# Remove session (auto-cleans container)
aoe remove <session>

# Remove session but keep container
aoe remove <session> --keep-container
```


**Note:** In the TUI, the sandbox checkbox only appears when Docker is available on your system.

## Default Configuration

```toml
[sandbox]
enabled_by_default = false
yolo_mode_default = false
default_image = "ghcr.io/njbrake/aoe-sandbox:latest"
auto_cleanup = true
cpu_limit = "4"
memory_limit = "8g"
environment = ["ANTHROPIC_API_KEY"]
```

## Configuration Options

| Option | Default | Description |
|--------|---------|-------------|
| `enabled_by_default` | `false` | Auto-enable sandbox for new sessions |
| `yolo_mode_default` | `false` | Skip agent permission prompts in sandboxed sessions |
| `default_image` | `ghcr.io/njbrake/aoe-sandbox:latest` | Docker image to use |
| `auto_cleanup` | `true` | Remove containers when sessions are deleted |
| `cpu_limit` | (none) | CPU limit (e.g., "4") |
| `memory_limit` | (none) | Memory limit (e.g., "8g") |
| `environment` | `[]` | Env var names to pass through from host |
| `environment_values` | `{}` | Env vars with explicit values to inject (see below) |
| `volume_ignores` | `[]` | Directories to exclude from the project mount via anonymous volumes |
| `extra_volumes` | `[]` | Additional volume mounts |
| `default_terminal_mode` | `"host"` | Paired terminal location: `"host"` (on host machine) or `"container"` (inside Docker) |

## Volume Mounts

### Automatic Mounts

| Host Path | Container Path | Mode | Purpose |
|-----------|----------------|------|---------|
| Project directory | `/workspace` | RW | Your code |
| `~/.gitconfig` | `/root/.gitconfig` | RO | Git config |
| `~/.ssh/` | `/root/.ssh/` | RO | SSH keys |
| `~/.config/opencode/` | `/root/.config/opencode/` | RO | OpenCode config |
| `~/.vibe/` | `/root/.vibe/` | RW | Vibe config (if exists) |

### Persistent Auth Volumes

| Volume Name | Container Path | Purpose |
|-------------|----------------|---------|
| `aoe-claude-auth` | `/root/.claude/` | Claude Code credentials |
| `aoe-opencode-auth` | `/root/.local/share/opencode/` | OpenCode credentials |
| `aoe-vibe-auth` | `/root/.vibe/` | Mistral Vibe credentials |
| `aoe-codex-auth` | `/root/.codex/` | Codex CLI credentials |
| `aoe-gemini-auth` | `/root/.gemini/` | Gemini CLI credentials |

**Note:** Auth persists across containers. First session requires authentication, subsequent sessions reuse it.

## Container Naming

Containers are named: `aoe-sandbox-{session_id_first_8_chars}`

Example: `aoe-sandbox-a1b2c3d4`

## How It Works

1. **Session Creation:** When you add a sandboxed session, aoe records the sandbox configuration
2. **Container Start:** When you start the session, aoe creates/starts the Docker container with appropriate volume mounts
3. **tmux + docker exec:** Host tmux runs `docker exec -it <container> <tool>` (claude, opencode, vibe, codex, or gemini)
4. **Cleanup:** When you remove the session, the container is automatically deleted


## Environment Variables

These terminal-related variables are **always** passed through for proper UI/theming:
- `TERM`, `COLORTERM`, `FORCE_COLOR`, `NO_COLOR`

Pass additional variables (like API keys) through containers by adding them to config:

```toml
[sandbox]
environment = ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "GEMINI_API_KEY"]
```

These variables are read from your host environment and passed to containers (in addition to the terminal defaults above).

### Sandbox-Specific Values (`environment_values`)

Use `environment_values` to inject env vars with values that AOE manages directly, independent of your host environment. This is useful for giving sandboxes credentials that differ from (or don't exist on) the host:

```toml
[sandbox.environment_values]
GH_TOKEN = "ghp_sandbox_scoped_token"
CUSTOM_API_KEY = "sk-sandbox-only-key"
```

Values starting with `$` are read from a host env var instead of being used literally. This lets you store the actual secret in your shell profile rather than in the AOE config file:

```toml
[sandbox.environment_values]
GH_TOKEN = "$AOE_GH_TOKEN"   # reads AOE_GH_TOKEN from host, injects as GH_TOKEN
```

```bash
# In your .bashrc / .zshrc
export AOE_GH_TOKEN="ghp_sandbox_scoped_token"
```

If the referenced host env var is not set, the entry is silently skipped.

To use a literal value starting with `$`, double it: `$$LITERAL` is injected as `$LITERAL`.

## Available Images

AOE provides two official sandbox images:

| Image | Description |
|-------|-------------|
| `ghcr.io/njbrake/aoe-sandbox:latest` | Base image with Claude Code, OpenCode, Mistral Vibe, Codex CLI, Gemini CLI, git, ripgrep, fzf |
| `ghcr.io/njbrake/aoe-dev-sandbox:latest` | Extended image with additional dev tools |

### Dev Sandbox Tools

The dev sandbox (`aoe-dev-sandbox`) includes everything in the base image plus:

- **Rust** (rustup, cargo, rustc)
- **uv** (fast Python package manager)
- **Node.js LTS** (via nvm, with npm and npx)
- **GitHub CLI** (gh)

To use the dev sandbox:

```bash
# Per-session
aoe add --sandbox-image ghcr.io/njbrake/aoe-dev-sandbox:latest .

# Or set as default in ~/.agent-of-empires/config.toml
[sandbox]
default_image = "ghcr.io/njbrake/aoe-dev-sandbox:latest"
```

## Custom Docker Images

The default sandbox image includes Claude Code, OpenCode, Mistral Vibe, Codex CLI, Gemini CLI, git, and basic development tools. For projects requiring additional dependencies beyond what the dev sandbox provides, you can extend either base image.

### Step 1: Create a Dockerfile

Create a `Dockerfile` in your project (or a shared location):

```dockerfile
FROM ghcr.io/njbrake/aoe-sandbox:latest

# Example: Add Python for a data science project
RUN apt-get update && apt-get install -y \
    python3 \
    python3-pip \
    python3-venv \
    && rm -rf /var/lib/apt/lists/*

# Install Python packages
RUN pip3 install --break-system-packages \
    pandas \
    numpy \
    requests
```

### Step 2: Build Your Image

```bash
# Build locally
docker build -t my-sandbox:latest .

# Or build and push to a registry
docker build -t ghcr.io/yourusername/my-sandbox:latest .
docker push ghcr.io/yourusername/my-sandbox:latest
```

### Step 3: Configure AOE to Use Your Image

**Option A: Set as default for all sessions**

Add to `~/.agent-of-empires/config.toml`:

```toml
[sandbox]
default_image = "my-sandbox:latest"
# Or with registry:
# default_image = "ghcr.io/yourusername/my-sandbox:latest"
```

**Option B: Use per-session via CLI**

```bash
aoe add --sandbox-image my-sandbox:latest .
```

## Worktrees and Sandboxing

When using git worktrees with sandboxing, there's an important consideration: worktrees have a `.git` file that points back to the main repository's git directory. If this reference points outside the sandboxed directory, git operations inside the container may fail.

### The Problem

With the default worktree template (`../{repo-name}-worktrees/{branch}`):

```
/projects/
  my-repo/
    .git/                    # Main repo's git directory
    src/
  my-repo-worktrees/
    feature-branch/
      .git                   # FILE pointing to /projects/my-repo/.git/...
      src/
```

When sandboxing `feature-branch/`, the container can't access `/projects/my-repo/.git/`.

### The Solution: Bare Repo Pattern

Use the linked worktree bare repo pattern to keep everything in one directory:

```
/projects/my-repo/
  .bare/                     # Bare git repository
  .git                       # FILE: "gitdir: ./.bare"
  main/                      # Worktree (main branch)
  feature/                   # Worktree (feature branch)
```

Now when sandboxing `feature/`, the container has access to the sibling `.bare/` directory.

AOE automatically detects bare repo setups and uses `./{branch}` as the default worktree path template, keeping new worktrees as siblings.

### Quick Setup

```bash
# Convert existing repo to bare repo pattern
cd my-project
mv .git .bare
echo "gitdir: ./.bare" > .git

# Or clone fresh as bare
git clone --bare git@github.com:user/repo.git my-project/.bare
cd my-project
echo "gitdir: ./.bare" > .git
git config remote.origin.fetch "+refs/heads/*:refs/remotes/origin/*"
git fetch origin
git worktree add main main
```

See the [Workflow Guide](workflow.md) for detailed bare repo setup instructions.

## Troubleshooting

### Container killed due to memory (OOM)

**Symptoms:** Your sandboxed session exits unexpectedly, the container disappears, or you see "Killed" in the output. Running `docker inspect <container>` shows `OOMKilled: true`.

**Cause:** On macOS (and Windows), Docker runs inside a Linux VM with a fixed memory ceiling. Docker Desktop defaults to 2 GB for the entire VM. If a container tries to use more memory than the VM has available, the Linux OOM killer terminates it. This commonly happens with AI coding agents that load large language model contexts or process big codebases.

**Fix:**

1. **Increase Docker Desktop VM memory:**
   Open Docker Desktop, go to **Settings > Resources > Advanced**, increase the **Memory** slider (8 GB+ recommended for AI coding agents), then click **Apply & Restart**.

2. **Set a per-container memory limit** in your AOE config (`~/.agent-of-empires/config.toml`) so containers have an explicit allocation rather than competing for the VM's total memory:

   ```toml
   [sandbox]
   memory_limit = "8g"
   ```

   The per-container limit must be less than or equal to the Docker Desktop VM memory. If you set `memory_limit = "8g"` but your VM only has 4 GB, the container will still be OOM-killed.

3. **Verify the fix:** Start a new session and check the container's limit:

   ```bash
   docker stats --no-stream
   ```

   The `MEM LIMIT` column should reflect your configured value.

**Note:** On Linux, Docker runs natively without a VM, so the memory ceiling is your host's physical RAM. You typically only need `memory_limit` on Linux to prevent a single container from consuming all system memory.
