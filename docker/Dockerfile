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

# Install Node.js 20 (required for Claude Code)
RUN curl -fsSL https://deb.nodesource.com/setup_20.x | bash - \
    && apt-get install -y nodejs \
    && rm -rf /var/lib/apt/lists/*

# Install Claude Code CLI
RUN npm install -g @anthropic-ai/claude-code

# Install OpenCode
RUN curl -fsSL https://opencode.ai/install | bash
ENV PATH="/root/.opencode/bin:${PATH}"

# Create directories for credential mounts
RUN mkdir -p /root/.claude \
    /root/.config/opencode \
    /root/.local/share/opencode \
    /root/.ssh

# Set working directory
WORKDIR /workspace

# Default command - keep container running for docker exec
CMD ["sleep", "infinity"]
