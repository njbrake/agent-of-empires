#!/bin/bash
# Setup script for generating the demo GIF. Meant to be run from the root of the repo on a mac
# Requires: vhs, tmux, cargo, docker (for sandbox demo)

set -ex
cd "$(dirname "$0")/.."

# Check Docker is running (needed for sandbox demo)
if ! docker info >/dev/null 2>&1; then
    echo "Error: Docker is not running. Please start Docker for the sandbox demo."
    exit 1
fi

# Pull sandbox image to ensure it's available
docker pull ghcr.io/njbrake/aoe-sandbox:latest

# build the project
cargo build --release

# Clear demo profile
rm -rf ~/.agent-of-empires/profiles/demo

# Clean and recreate demo project directories
rm -rf /tmp/demo-projects
mkdir -p /tmp/demo-projects/api-server /tmp/demo-projects/web-app /tmp/demo-projects/chat-app

pushd /tmp/demo-projects/api-server
git init -q
touch README.md
git add .
git commit -q -m "Initial commit"
popd

pushd /tmp/demo-projects/web-app
git init -q
touch README.md
git add .
git commit -q -m "Initial commit"
popd

vhs assets/demo.tape
