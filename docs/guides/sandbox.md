# Docker Sandbox - Quick Reference

## Overview

Docker sandboxing runs your AI coding agents (Claude Code, OpenCode) inside isolated Docker containers while maintaining access to your project files and credentials.

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
| `default_image` | `ghcr.io/njbrake/aoe-sandbox:latest` | Docker image to use |
| `auto_cleanup` | `true` | Remove containers when sessions are deleted |
| `cpu_limit` | (none) | CPU limit (e.g., "4") |
| `memory_limit` | (none) | Memory limit (e.g., "8g") |
| `environment` | `[]` | Env vars to pass through |
| `extra_volumes` | `[]` | Additional volume mounts |

## Volume Mounts

### Automatic Mounts

| Host Path | Container Path | Mode | Purpose |
|-----------|----------------|------|---------|
| Project directory | `/workspace` | RW | Your code |
| `~/.gitconfig` | `/root/.gitconfig` | RO | Git config |
| `~/.ssh/` | `/root/.ssh/` | RO | SSH keys |
| `~/.config/opencode/` | `/root/.config/opencode/` | RO | OpenCode config |

### Persistent Auth Volumes

| Volume Name | Container Path | Purpose |
|-------------|----------------|---------|
| `aoe-claude-auth` | `/root/.claude/` | Claude Code credentials |
| `aoe-opencode-auth` | `/root/.local/share/opencode/` | OpenCode credentials |

**Note:** Auth persists across containers. First session requires authentication, subsequent sessions reuse it.

### Source Code Reference

Volume mounts are defined in `src/session/instance.rs` in the `build_container_config()` method (lines 207-274). The actual Docker `-v` arguments are constructed in `src/docker/container.rs` in the `run_container()` function (lines 89-101).

## Container Naming

Containers are named: `aoe-sandbox-{session_id_first_8_chars}`

Example: `aoe-sandbox-a1b2c3d4`

## How It Works

1. **Session Creation:** When you add a sandboxed session, aoe records the sandbox configuration
2. **Container Start:** When you start the session, aoe creates/starts the Docker container with appropriate volume mounts
3. **tmux + docker exec:** Host tmux runs `docker exec -it <container> claude` (or opencode)
4. **Cleanup:** When you remove the session, the container is automatically deleted


## Environment Variables

Pass API keys through containers by adding them to config:

```toml
[sandbox]
environment = ["ANTHROPIC_API_KEY", "OPENAI_API_KEY"]
```

These variables are read from your host environment and passed to containers.

## Available Images

AOE provides two official sandbox images:

| Image | Description |
|-------|-------------|
| `ghcr.io/njbrake/aoe-sandbox:latest` | Base image with Claude Code, OpenCode, git, ripgrep, fzf |
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

The default sandbox image includes Claude Code, OpenCode, git, and basic development tools. For projects requiring additional dependencies beyond what the dev sandbox provides, you can extend either base image.

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
