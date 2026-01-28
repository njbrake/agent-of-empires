# aoe-sandbox: Docker image for Agent of Empires sandbox sessions
# This image provides Claude Code, OpenCode, Mistral Vibe, Codex CLI, and Gemini CLI in an isolated environment

FROM ubuntu:24.04

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y \
    curl \
    git \
    git-lfs \
    build-essential \
    ca-certificates \
    gnupg \
    openssh-client \
    ripgrep \
    fzf \
    && rm -rf /var/lib/apt/lists/*

# Set up Git LFS hooks globally
RUN git lfs install

# Install Node.js (required for Codex CLI)
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
    && apt-get install -y nodejs \
    && rm -rf /var/lib/apt/lists/*

# Run as root - IS_SANDBOX=1 allows Claude Code to use --dangerously-skip-permissions
WORKDIR /root

# Install Claude Code CLI (official installer)
RUN curl -fsSL https://claude.ai/install.sh | bash
ENV PATH="/root/.local/bin:${PATH}"

# Install OpenCode
RUN curl -fsSL https://opencode.ai/install | bash
ENV PATH="/root/.opencode/bin:${PATH}"

# Install Codex CLI (OpenAI)
RUN npm install -g @openai/codex

# Install Gemini CLI (Google)
RUN npm install -g @google/gemini-cli

# Install Mistral Vibe
RUN curl -LsSf https://mistral.ai/vibe/install.sh | bash
ENV PATH="/root/.vibe/bin:${PATH}"

# Create directories for credential mounts
RUN mkdir -p /root/.claude \
    /root/.config/opencode \
    /root/.local/share/opencode \
    /root/.vibe \
    /root/.codex \
    /root/.gemini \
    /root/.ssh

# Allow Claude Code to use --dangerously-skip-permissions as root
ENV IS_SANDBOX=1

# Set working directory
WORKDIR /workspace

# Default command - keep container running for docker exec
CMD ["sleep", "infinity"]
