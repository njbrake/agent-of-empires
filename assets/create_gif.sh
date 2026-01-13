#!/bin/bash
# Setup script for generating the demo GIF. Meant to be run from the root of the repo on a mac


set -ex
cd "$(dirname "$0")/.."

# build the project
cargo build --release

# Clear demo profile
rm -rf ~/.agent-of-empires/profiles/demo

# Clean and recreate demo project directories
rm -rf /tmp/demo-projects
mkdir -p /tmp/demo-projects/api-server /tmp/demo-projects/web-app

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
