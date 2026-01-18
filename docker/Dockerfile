# aoe-sandbox: Docker image for Agent of Empires sandbox sessions
# This image provides Claude Code and OpenCode in an isolated environment

FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y \
    curl \
    git \
    build-essential \
    ca-certificates \
    gnupg \
    openssh-client \
    ripgrep \
    fzf \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user for running Claude (required for --dangerously-skip-permissions)
# Use UID 1000 to match typical host user UID for proper file permissions on mounted volumes
RUN userdel -r ubuntu 2>/dev/null || true && \
    groupadd -g 1000 sandbox && \
    useradd -m -s /bin/bash -u 1000 -g 1000 sandbox

# Switch to sandbox user for tool installation
USER sandbox
WORKDIR /home/sandbox

# Install Claude Code CLI (official installer)
RUN curl -fsSL https://claude.ai/install.sh | bash
ENV PATH="/home/sandbox/.local/bin:${PATH}"

# Install OpenCode
RUN curl -fsSL https://opencode.ai/install | bash
ENV PATH="/home/sandbox/.opencode/bin:${PATH}"

# Create directories for credential mounts
RUN mkdir -p /home/sandbox/.claude \
    /home/sandbox/.config/opencode \
    /home/sandbox/.local/share/opencode \
    /home/sandbox/.ssh

# Set working directory
WORKDIR /workspace

# Default command - keep container running for docker exec
CMD ["sleep", "infinity"]
