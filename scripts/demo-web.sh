#!/bin/bash
# Generate reproducible web dashboard demo GIF + screenshots.
# Mirrors assets/create_gif.sh pattern for the web dashboard.
#
# Requires: git, cargo, tmux, node, npx, ffmpeg
# Output:   docs/assets/web-demo.gif, web-demo.mp4, web-dashboard.png,
#           web-diff.png, web-help.png

set -euo pipefail
cd "$(dirname "$0")/.."

# ---------------------------------------------------------------------------
# Dependency checks
# ---------------------------------------------------------------------------
for cmd in git cargo tmux node npx ffmpeg curl; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "Error: $cmd is required but not found on PATH."
    exit 1
  fi
done

# ---------------------------------------------------------------------------
# Isolated HOME (prevents user sessions from appearing in the demo)
# ---------------------------------------------------------------------------
DEMO_TMPDIR=$(mktemp -d)
ORIG_HOME="$HOME"
export HOME="$DEMO_TMPDIR"
export XDG_CONFIG_HOME="$DEMO_TMPDIR/.config"

SERVER_PID=""

cleanup() {
  echo "Cleaning up..."
  [ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2>/dev/null || true
  # Kill any demo tmux sessions
  tmux list-sessions -F '#{session_name}' 2>/dev/null \
    | grep '^aoe_' \
    | while read -r s; do tmux kill-session -t "$s" 2>/dev/null || true; done
  # Clean hook status files created for the demo
  rm -rf /tmp/aoe-hooks/ 2>/dev/null || true
  rm -rf "$DEMO_TMPDIR" 2>/dev/null || true
}
trap cleanup EXIT

# Pre-seed config to skip welcome dialog and update checks
CONFIG_DIR="$DEMO_TMPDIR/.config/agent-of-empires"
mkdir -p "$CONFIG_DIR/profiles/default"
cat > "$CONFIG_DIR/config.toml" << 'TOML'
[updates]
check_enabled = false

[app_state]
has_seen_welcome = true
last_seen_version = "999.0.0"
TOML

# ---------------------------------------------------------------------------
# Copy Claude Code credentials so the real agent can authenticate.
# The isolated HOME has no ~/.claude/ by default.
# ---------------------------------------------------------------------------
if [ -d "$ORIG_HOME/.claude" ]; then
  mkdir -p "$DEMO_TMPDIR/.claude"
  for f in .credentials.json .claude.json settings.json statsig.json; do
    [ -f "$ORIG_HOME/.claude/$f" ] && cp "$ORIG_HOME/.claude/$f" "$DEMO_TMPDIR/.claude/$f"
  done
else
  echo "Error: ~/.claude/ not found. Real Claude Code requires authentication."
  exit 1
fi

# ---------------------------------------------------------------------------
# Create demo git repos with unstaged changes (for diff panel)
# ---------------------------------------------------------------------------
DEMO_DIR="$DEMO_TMPDIR/projects"

mkdir -p "$DEMO_DIR/api-server/src"
(
  cd "$DEMO_DIR/api-server"
  git init -q
  echo 'fn main() { println!("hello"); }' > src/main.rs
  git add . && git commit -q -m "init"
  # Unstaged change shows in diff panel
  printf '\nfn handle_request() { todo!() }\n' >> src/main.rs
)

mkdir -p "$DEMO_DIR/web-app/src"
(
  cd "$DEMO_DIR/web-app"
  git init -q
  echo '{}' > package.json
  echo 'export default function App() { return null; }' > src/App.tsx
  git add . && git commit -q -m "init"
  printf '\nexport function Dashboard() { return <div />; }\n' >> src/App.tsx
)

mkdir -p "$DEMO_DIR/chat-app/src"
(
  cd "$DEMO_DIR/chat-app"
  git init -q
  echo 'package main' > src/main.go
  git add . && git commit -q -m "init"
  printf '\nfunc handleChat() { /* TODO */ }\n' >> src/main.go
)

# ---------------------------------------------------------------------------
# Pre-trust demo directories in Claude Code's config so it skips the
# workspace trust dialog when sessions start.
# ---------------------------------------------------------------------------
python3 << PYEOF
import json
with open("$DEMO_TMPDIR/.claude/.claude.json") as f:
    data = json.load(f)
projects = data.get("projects", {})
trust = {"allowedTools": [], "hasTrustDialogAccepted": True, "hasCompletedProjectOnboarding": True}
for path in ["$DEMO_DIR/api-server", "$DEMO_DIR/web-app", "$DEMO_DIR/chat-app"]:
    projects[path] = trust.copy()
data["projects"] = projects
with open("$DEMO_TMPDIR/.claude/.claude.json", "w") as f:
    json.dump(data, f)
PYEOF

# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------
AOE="./target/release/aoe"
if [ ! -f "$AOE" ]; then
  echo "Building aoe with web dashboard support..."
  cargo build --features serve --release
fi

# ---------------------------------------------------------------------------
# Create sessions (no --launch to avoid blocking)
# ---------------------------------------------------------------------------
echo "Creating demo sessions..."
# Use claude -p (print mode) which skips the workspace trust dialog.
# Each session runs a different prompt to show varied Claude Code output.
# "exec cat" holds the pane alive after claude exits so status detection works.
CMD1="bash -c 'claude -p \"Read src/main.rs and suggest improvements\" --dangerously-skip-permissions 2>/dev/null; exec cat'"
CMD2="bash -c 'claude -p \"What does this project do? Be brief.\" --dangerously-skip-permissions 2>/dev/null; exec cat'"
CMD3="bash -c 'claude -p \"List all source files\" --dangerously-skip-permissions 2>/dev/null; exec cat'"
ID1=$($AOE add "$DEMO_DIR/api-server" -t "API Server" -c claude --cmd-override "$CMD1" 2>&1 | grep "ID:" | awk '{print $2}')
ID2=$($AOE add "$DEMO_DIR/web-app" -t "Web App" -c claude --cmd-override "$CMD2" 2>&1 | grep "ID:" | awk '{print $2}')
ID3=$($AOE add "$DEMO_DIR/chat-app" -t "Chat App" -c claude --cmd-override "$CMD3" 2>&1 | grep "ID:" | awk '{print $2}')

echo "  API Server: $ID1"
echo "  Web App:    $ID2"
echo "  Chat App:   $ID3"

# Start sessions (creates tmux windows with bash shells)
$AOE session start "$ID1"
$AOE session start "$ID2"
$AOE session start "$ID3"

# ---------------------------------------------------------------------------
# Wait for Claude Code to run prompts and produce output
# ---------------------------------------------------------------------------
echo "Waiting for Claude Code to finish (this may take 30-60s)..."
sleep 45

# ---------------------------------------------------------------------------
# Start server
# ---------------------------------------------------------------------------
echo "Starting aoe serve..."
$AOE serve --no-auth &
SERVER_PID=$!

# Wait for server to be ready
for i in $(seq 1 30); do
  if curl -sf http://localhost:8080/api/sessions >/dev/null 2>&1; then
    echo "Server ready."
    break
  fi
  if [ "$i" -eq 30 ]; then
    echo "Error: server did not start within 30 seconds."
    exit 1
  fi
  sleep 1
done

# Wait for status poll cycle (server polls every 2s)
sleep 4

# ---------------------------------------------------------------------------
# Install Playwright browser if needed, then record
# ---------------------------------------------------------------------------
echo "Running Playwright demo recording..."
cd web
npx playwright install chromium 2>/dev/null || true
npx playwright test --config playwright.demo.config.ts
cd ..

# ---------------------------------------------------------------------------
# Convert video to GIF and MP4
# ---------------------------------------------------------------------------
echo "Converting video..."
VIDEO=$(find target/demo-recordings -name 'video.webm' -type f 2>/dev/null | head -1)
if [ -z "$VIDEO" ]; then
  echo "Error: no video file found in target/demo-recordings/"
  exit 1
fi

mkdir -p docs/assets

# GIF: two-pass palette for quality
ffmpeg -y -i "$VIDEO" \
  -vf "fps=10,scale=1280:-1:flags=lanczos,split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse" \
  -loop 0 docs/assets/web-demo.gif 2>/dev/null

# MP4: h264, web-compatible
ffmpeg -y -i "$VIDEO" \
  -c:v libx264 -preset slow -crf 22 \
  docs/assets/web-demo.mp4 2>/dev/null

# Check GIF size; reduce if over 5MB
GIF_SIZE=$(stat -c%s docs/assets/web-demo.gif 2>/dev/null || stat -f%z docs/assets/web-demo.gif 2>/dev/null || echo 0)
if [ "$GIF_SIZE" -gt 5242880 ]; then
  echo "GIF is ${GIF_SIZE} bytes (>5MB), reducing to 960x540..."
  ffmpeg -y -i "$VIDEO" \
    -vf "fps=10,scale=960:-1:flags=lanczos,split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse" \
    -loop 0 docs/assets/web-demo.gif 2>/dev/null

  GIF_SIZE=$(stat -c%s docs/assets/web-demo.gif 2>/dev/null || stat -f%z docs/assets/web-demo.gif 2>/dev/null || echo 0)
  if [ "$GIF_SIZE" -gt 5242880 ]; then
    echo "GIF still >5MB, reducing to 8fps..."
    ffmpeg -y -i "$VIDEO" \
      -vf "fps=8,scale=960:-1:flags=lanczos,split[s0][s1];[s0]palettegen[p];[s1][p]paletteuse" \
      -loop 0 docs/assets/web-demo.gif 2>/dev/null
  fi
fi

# ---------------------------------------------------------------------------
# Done
# ---------------------------------------------------------------------------
echo ""
echo "Demo artifacts:"
ls -lh docs/assets/web-demo.gif docs/assets/web-demo.mp4 \
       docs/assets/web-dashboard.png docs/assets/web-diff.png \
       docs/assets/web-help.png 2>/dev/null || true
echo ""
echo "Done! To embed in README:"
echo '  ![Web Dashboard](docs/assets/web-demo.gif)'
