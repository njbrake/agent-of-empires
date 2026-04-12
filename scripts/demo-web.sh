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
# Create stub agent binaries (so aoe finds "claude" etc. without running real agents)
# Pattern from tests/e2e/harness.rs — stubs run bash instead of real agents
# ---------------------------------------------------------------------------
STUB_DIR="$DEMO_TMPDIR/stubs"
mkdir -p "$STUB_DIR"
for agent in claude opencode codex; do
  cat > "$STUB_DIR/$agent" << 'STUB'
#!/bin/sh
exec bash
STUB
  chmod +x "$STUB_DIR/$agent"
done
export PATH="$STUB_DIR:$PATH"

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
# --cmd-override bash ensures bash runs instead of the real agent binary.
# Stubs on PATH above handle tool detection (which claude), but the tmux
# server may use its own PATH, so --cmd-override is the reliable mechanism.
ID1=$($AOE add "$DEMO_DIR/api-server" -t "API Server" -c claude --cmd-override bash 2>&1 | grep "ID:" | awk '{print $2}')
ID2=$($AOE add "$DEMO_DIR/web-app" -t "Web App" -c opencode --cmd-override bash 2>&1 | grep "ID:" | awk '{print $2}')
ID3=$($AOE add "$DEMO_DIR/chat-app" -t "Chat App" -c codex --cmd-override bash 2>&1 | grep "ID:" | awk '{print $2}')

echo "  API Server: $ID1"
echo "  Web App:    $ID2"
echo "  Chat App:   $ID3"

# Start sessions (creates tmux windows with bash shells)
$AOE session start "$ID1"
$AOE session start "$ID2"
$AOE session start "$ID3"

# ---------------------------------------------------------------------------
# Fake agent status via hook files
# ---------------------------------------------------------------------------
mkdir -p "/tmp/aoe-hooks/$ID1" "/tmp/aoe-hooks/$ID2" "/tmp/aoe-hooks/$ID3"
echo "running" > "/tmp/aoe-hooks/$ID1/status"
echo "waiting" > "/tmp/aoe-hooks/$ID2/status"
echo "idle"    > "/tmp/aoe-hooks/$ID3/status"

# ---------------------------------------------------------------------------
# Pre-seed terminal content to look like Claude Code output
# ---------------------------------------------------------------------------
sleep 1  # let tmux sessions initialize

TMUX1="aoe_API_Server_${ID1:0:8}"
TMUX2="aoe_Web_App_${ID2:0:8}"
TMUX3="aoe_Chat_App_${ID3:0:8}"

# Create scripts that output realistic Claude Code-style terminal content,
# then hold the terminal open with `cat` (blocks forever, keeps pane alive).
cat > "$DEMO_TMPDIR/fake-claude-1.sh" << 'FAKECLAUDE'
#!/bin/sh
clear
printf '\033[1m> Refactor the request handler to use async/await\033[0m\n\n'
printf 'I'\''ll analyze the current request handler and refactor it.\n\n'
printf '\033[2m● \033[0m\033[1mRead\033[0m src/main.rs\n\n'
printf 'The current handler uses blocking I/O. I'\''ll refactor it to use\n'
printf 'async/await for better concurrency.\n\n'
printf '\033[2m● \033[0m\033[1mEdit\033[0m src/main.rs\n'
printf '  \033[32m+ async fn handle_request(req: Request) -> Response {\033[0m\n'
printf '  \033[32m+     let data = fetch_data(&req).await?;\033[0m\n'
printf '  \033[32m+     Ok(Response::json(data))\033[0m\n'
printf '  \033[32m+ }\033[0m\n\n'
printf 'The handler is now async. Let me verify the changes compile.\n\n'
printf '\033[2m● \033[0m\033[1mBash\033[0m cargo check\n\n'
printf '\033[2m  Compiling api-server v0.1.0\n'
printf '   Finished dev [unoptimized] target(s) in 1.2s\033[0m\n\n'
printf '\033[32m✓\033[0m Changes compile successfully.\n\n'
exec cat
FAKECLAUDE

cat > "$DEMO_TMPDIR/fake-claude-2.sh" << 'FAKECLAUDE'
#!/bin/sh
clear
printf '\033[1m> Add a Dashboard component with responsive layout\033[0m\n\n'
printf 'I'\''ll create the Dashboard component with a responsive grid.\n\n'
printf '\033[2m● \033[0m\033[1mRead\033[0m src/App.tsx\n\n'
printf 'I see the main App component. Let me add a Dashboard.\n\n'
printf '\033[2m● \033[0m\033[1mWrite\033[0m src/Dashboard.tsx\n\n'
printf '\033[1;35m  Do you want me to create this new file?\033[0m\n'
printf '\033[2m  src/Dashboard.tsx\033[0m\n\n'
printf '  \033[33mYes\033[0m  / No\n'
exec cat
FAKECLAUDE

cat > "$DEMO_TMPDIR/fake-claude-3.sh" << 'FAKECLAUDE'
#!/bin/sh
clear
printf '\033[1m> Review the Go module structure\033[0m\n\n'
printf 'Let me look at the project layout.\n\n'
printf '\033[2m● \033[0m\033[1mBash\033[0m find src -type f -name "*.go"\n\n'
printf '  src/main.go\n\n'
printf 'This is a single-file Go project. The code looks clean.\n'
printf 'No issues found in the module structure.\n\n'
printf '\033[32m✓\033[0m Review complete. The project follows standard Go conventions.\n\n'
printf '\033[2m$\033[0m \033[?25l'
exec cat
FAKECLAUDE

chmod +x "$DEMO_TMPDIR/fake-claude-1.sh" "$DEMO_TMPDIR/fake-claude-2.sh" "$DEMO_TMPDIR/fake-claude-3.sh"

# Kill the bash shells and replace with our fake claude scripts
tmux send-keys -t "$TMUX1" "exec $DEMO_TMPDIR/fake-claude-1.sh" Enter
tmux send-keys -t "$TMUX2" "exec $DEMO_TMPDIR/fake-claude-2.sh" Enter
tmux send-keys -t "$TMUX3" "exec $DEMO_TMPDIR/fake-claude-3.sh" Enter

sleep 2  # let terminal content render

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
