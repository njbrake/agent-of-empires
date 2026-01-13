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

## TUI Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `n` | New session dialog |
| `Tab` | Next field |
| `Space` | Toggle sandbox checkbox |
| `Enter` | Submit and create session |
| `Esc` | Cancel |

**Note:** The sandbox checkbox only appears when Docker is available on your system.

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

## Quick Start

```bash
# 1. Verify Docker is installed
docker --version

# 2. Create your first sandboxed session
cd ~/scm/my-project
aoe add --sandbox .

# 3. Start the session (creates container)
aoe session start <session>

# 4. First time: authenticate in Claude/OpenCode
# (stored in persistent volume, won't need again)

# 5. View session in TUI
aoe
# Sessions with [sandbox] indicator run in containers

# 6. Remove when done
aoe remove <session>  # Container auto-cleaned
```

## TUI Session Indicators

| Indicator | Meaning |
|-----------|---------|
| `[sandbox]` | Session runs in Docker container |
| Branch name (cyan) | Session has worktree |

## Container Naming

Containers are named: `aoe-sandbox-{session_id_first_8_chars}`

Example: `aoe-sandbox-a1b2c3d4`

## How It Works

1. **Session Creation:** When you add a sandboxed session, aoe records the sandbox configuration
2. **Container Start:** When you start the session, aoe creates/starts the Docker container with appropriate volume mounts
3. **tmux + docker exec:** Host tmux runs `docker exec -it <container> claude` (or opencode)
4. **Cleanup:** When you remove the session, the container is automatically deleted

## Troubleshooting

| Error | Solution |
|-------|----------|
| "Docker is not installed" | Install Docker: https://docs.docker.com/get-docker/ |
| "Docker daemon is not running" | Start Docker Desktop or `sudo systemctl start docker` |
| "Permission denied" | Add user to docker group: `sudo usermod -aG docker $USER` |
| "Image not found" | Image will be pulled automatically, check network |
| Auth required every time | Check if `aoe-claude-auth` volume exists: `docker volume ls` |

## Manual Container Management

```bash
# List aoe containers
docker ps -a --filter "name=aoe-sandbox"

# Check container logs
docker logs aoe-sandbox-<id>

# Manually remove container
docker rm -f aoe-sandbox-<id>

# List auth volumes
docker volume ls --filter "name=aoe-"

# Remove auth (will require re-authentication)
docker volume rm aoe-claude-auth aoe-opencode-auth
```

## Environment Variables

Pass API keys through containers by adding them to config:

```toml
[sandbox]
environment = ["ANTHROPIC_API_KEY", "OPENAI_API_KEY"]
```

These variables are read from your host environment and passed to containers.

## Custom Docker Images

The default sandbox image includes Claude Code, OpenCode, Node.js, git, and basic development tools. For projects requiring additional dependencies (Python, Rust, Go, databases, etc.), you can extend the base image.

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

### Example Dockerfiles

**Rust Development:**
```dockerfile
FROM ghcr.io/njbrake/aoe-sandbox:latest

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"

# Add common Rust tools
RUN cargo install cargo-watch cargo-edit
```

**Go Development:**
```dockerfile
FROM ghcr.io/njbrake/aoe-sandbox:latest

RUN apt-get update && apt-get install -y golang-go \
    && rm -rf /var/lib/apt/lists/*
ENV GOPATH="/root/go"
ENV PATH="${GOPATH}/bin:${PATH}"
```

**Full-Stack Web Development:**
```dockerfile
FROM ghcr.io/njbrake/aoe-sandbox:latest

# pnpm for faster package management
RUN npm install -g pnpm

# Python for backend/scripts
RUN apt-get update && apt-get install -y python3 python3-pip \
    && rm -rf /var/lib/apt/lists/*

# PostgreSQL client for database access
RUN apt-get update && apt-get install -y postgresql-client \
    && rm -rf /var/lib/apt/lists/*
```

**Machine Learning / Data Science:**
```dockerfile
FROM ghcr.io/njbrake/aoe-sandbox:latest

RUN apt-get update && apt-get install -y \
    python3 \
    python3-pip \
    python3-venv \
    && rm -rf /var/lib/apt/lists/*

RUN pip3 install --break-system-packages \
    numpy \
    pandas \
    scikit-learn \
    matplotlib \
    jupyter
```

### Full Configuration Example

Here's a complete `~/.agent-of-empires/config.toml` with sandbox settings:

```toml
[sandbox]
# Use your custom image by default
default_image = "ghcr.io/yourusername/my-sandbox:latest"

# Auto-enable sandbox for all new sessions
enabled_by_default = true

# Resource limits
cpu_limit = "4"
memory_limit = "8g"

# Pass through API keys from host environment
environment = ["ANTHROPIC_API_KEY", "OPENAI_API_KEY", "DATABASE_URL"]

# Clean up containers when sessions are removed
auto_cleanup = true
```

### Tips for Custom Images

- **Keep images small:** Only install what you need to minimize pull times
- **Use multi-stage builds:** For compiled languages, build in one stage and copy artifacts to final image
- **Pin versions:** Use specific versions (e.g., `python3.11`) for reproducibility
- **Layer caching:** Put frequently changing instructions (like `pip install`) later in the Dockerfile
- **Test locally:** Run `docker run -it my-sandbox:latest bash` to verify your tools work before using with aoe

## Pro Tips

- Use named volumes for consistent auth across sessions
- Set `cpu_limit` and `memory_limit` to prevent runaway containers
- Use `enabled_by_default = true` if you always want sandboxing
- Check `docker ps` if a session seems stuck (container might have issues)
- Combine with worktrees for fully isolated parallel development
