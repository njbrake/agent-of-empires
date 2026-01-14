#!/bin/bash
# Setup script for generating the demo GIF. Meant to be run from the root of the repo on a mac
# Requires: vhs, tmux, cargo, docker (for sandbox demo)

set -ex
cd "$(dirname "$0")/.."

# Use $HOME instead of /tmp for Docker compatibility on macOS
DEMO_DIR="${HOME}/demo-projects"

cleanup() {
    rm -rf "$DEMO_DIR"
    rm -rf ~/.agent-of-empires/profiles/demo
}

trap cleanup EXIT

# Check Docker is running (needed for sandbox demo)
if ! docker info >/dev/null 2>&1; then
    echo "Error: Docker is not running. Please start Docker for the sandbox demo."
    exit 1
fi

# Pull sandbox image to ensure it's available
docker pull ghcr.io/njbrake/aoe-sandbox:latest

# build the project
cargo build --release

# Clean and recreate demo project directories
rm -rf "$DEMO_DIR"
mkdir -p "$DEMO_DIR/api-server" "$DEMO_DIR/web-app" "$DEMO_DIR/chat-app"

pushd "$DEMO_DIR/api-server"
git init -q
touch README.md
git add .
git commit -q -m "Initial commit"
popd

pushd "$DEMO_DIR/web-app"
git init -q
touch README.md
git add .
git commit -q -m "Initial commit"
popd

pushd "$DEMO_DIR/chat-app"
git init -q
touch README.md
git add .
git commit -q -m "Initial commit"
popd

vhs assets/demo.tape
