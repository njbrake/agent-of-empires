#!/bin/bash
# Generate the README demo GIF using asciinema + agg.
# Requires: asciinema, agg, tmux, cargo (or a pre-built target/release/aoe)
#
# Usage:
#   ./assets/create_gif.sh

set -euo pipefail
cd "$(dirname "$0")/.."

AOE="$(pwd)/target/release/aoe"
CAST_FILE="$(pwd)/docs/assets/demo.cast"
GIF_FILE="$(pwd)/docs/assets/demo.gif"
DEMO_DIR="${HOME}/demo-projects"
PROFILE="demo"
SESSION="aoe_demo_rec"
COLS=120
ROWS=35

# ── helpers ──────────────────────────────────────────────────────────────────

cleanup() {
    tmux kill-session -t "$SESSION" 2>/dev/null || true
    rm -rf "$DEMO_DIR"
    rm -rf ~/.config/agent-of-empires/profiles/$PROFILE \
           ~/.agent-of-empires/profiles/$PROFILE 2>/dev/null || true
}
trap cleanup EXIT

send() {
    tmux send-keys -t "$SESSION" "$@"
}

type_text() {
    tmux send-keys -t "$SESSION" -l "$1"
}

capture() {
    tmux capture-pane -t "$SESSION" -p
}

wait_for() {
    local text="$1"
    local timeout="${2:-15}"
    local elapsed=0
    while ! capture | grep -qF "$text"; do
        sleep 0.5
        elapsed=$((elapsed + 1))
        if [ "$elapsed" -ge "$((timeout * 2))" ]; then
            echo "ERROR: Timed out waiting for '$text'"
            echo "Current screen:"
            capture
            exit 1
        fi
    done
}

# ── setup ────────────────────────────────────────────────────────────────────

if [ ! -f "$AOE" ]; then
    echo "Building aoe..."
    cargo build --release
fi

cleanup

# Create demo repos
mkdir -p "$DEMO_DIR/api-server" "$DEMO_DIR/web-app" "$DEMO_DIR/chat-app"
for dir in api-server web-app chat-app; do
    pushd "$DEMO_DIR/$dir" > /dev/null
    git init -q
    touch README.md
    git add .
    git commit -q -m "Initial commit"
    popd > /dev/null
done

mkdir -p "$(dirname "$CAST_FILE")"

# ── record ───────────────────────────────────────────────────────────────────

echo "Recording demo..."

# Launch aoe wrapped in asciinema inside a detached tmux session.
# aoe detects TMUX and uses attach-session for sessions,
# so agent output is captured naturally by asciinema.
tmux new-session -d -s "$SESSION" -x "$COLS" -y "$ROWS" \
    "TERM=xterm-256color asciinema rec --overwrite --cols $COLS --rows $ROWS \
     -c '$AOE -p $PROFILE' '$CAST_FILE'"

wait_for "No sessions yet"
sleep 0.8

# ── Create first session: API Server with Claude Code ──
send n
wait_for "New Session"
sleep 0.3

# Tab past Profile to Title
send Tab
sleep 0.2
type_text "API Server"
sleep 0.5

# Path
send Tab
sleep 0.2
for i in $(seq 1 80); do send BSpace; done
sleep 0.1
type_text "$DEMO_DIR/api-server"
sleep 0.5

# Tool (claude is default), skip through remaining fields
send Tab; sleep 0.2
send Tab; sleep 0.1
send Tab; sleep 0.1
send Tab; sleep 0.1

# Submit (auto-attaches to Claude Code session)
send Enter
sleep 3

# Accept Claude Code trust dialog ("Yes, I trust this folder" is selected by default)
send Enter
sleep 3

# Send a prompt to make the session look active
type_text "Write a 1000 word story about Age of Empires"
sleep 0.8
send Enter
sleep 5

# Detach from agent session back to TUI
send C-b
sleep 0.2
send d
wait_for "Agent of Empires"
sleep 1.2

# ── Create second session: Web App with OpenCode + worktree + YOLO ──
send n
wait_for "New Session"
sleep 0.3

send Tab
sleep 0.2
type_text "Web App"
sleep 0.5

# Path
send Tab
sleep 0.2
for i in $(seq 1 80); do send BSpace; done
sleep 0.1
type_text "$DEMO_DIR/web-app"
sleep 0.5

# Tool: move right to OpenCode
send Tab
sleep 0.2
send Right
sleep 0.5

# YOLO mode: toggle ON with Space to show the feature
send Tab
sleep 0.3
send Space
sleep 0.8

# Worktree branch
send Tab
sleep 0.2
type_text "feature/auth"
sleep 0.5

# Skip Group
send Tab
sleep 0.1

# Submit
send Enter
sleep 2.5

# Detach
send C-b
sleep 0.2
send d
wait_for "Agent of Empires"
sleep 1

# ── Create third session: Chat App with Vibe ──
send n
wait_for "New Session"
sleep 0.3

send Tab
sleep 0.2
type_text "Chat App"
sleep 0.5

# Path
send Tab
sleep 0.2
for i in $(seq 1 80); do send BSpace; done
sleep 0.1
type_text "$DEMO_DIR/chat-app"
sleep 0.5

# Tool: move right twice (to Vibe)
send Tab
sleep 0.2
send Right; sleep 0.1
send Right
sleep 0.5

# Skip YOLO, Worktree, Group
send Tab; sleep 0.1
send Tab; sleep 0.1
send Tab; sleep 0.1

# Submit
send Enter
sleep 2.5

# Detach
send C-b
sleep 0.2
send d
wait_for "Agent of Empires"
sleep 1.2

# ── Browse the session list ──
send k
sleep 1.8
send k
sleep 2.2
send j
sleep 1.8
send j
sleep 2

# ── Show terminal view: toggle mode, open a terminal, run pwd, detach ──
send t
wait_for "[Term]"
sleep 1.5
# Enter attaches to a newly-created terminal tmux session
send Enter
sleep 3
# Target the dynamic aoe_term_* session for shell input
TERM_SESSION=$(tmux list-sessions -F "#{session_name}" | grep -E "^aoe_term_" | head -1)
if [ -n "$TERM_SESSION" ]; then
    tmux send-keys -t "$TERM_SESSION" -l "pwd"
    sleep 0.3
    tmux send-keys -t "$TERM_SESSION" Enter
    sleep 1.5
fi
# Detach back to the aoe TUI
send C-b
sleep 0.3
send d
wait_for "Terminals"
sleep 1
# Toggle back to Agent view
send t
wait_for "[Agent]"
sleep 1

# ── Quit ──
send q
sleep 1.5

echo "Converting to GIF..."
agg \
    --font-size 14 \
    --speed 1.5 \
    --idle-time-limit 2 \
    --last-frame-duration 1 \
    --theme github-dark \
    "$CAST_FILE" "$GIF_FILE"

echo "Done! GIF at $GIF_FILE"
ls -lh "$GIF_FILE"
