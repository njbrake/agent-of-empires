# Agent of Empires demo: create a Claude session, enable Local remote
# access, then add a second agent (OpenCode) while the server is up.
# Regenerate with: vhs assets/demo.tape

Output docs/assets/demo.gif

Set Shell "bash"
Set FontSize 16
Set Width 1200
Set Height 720
Set Padding 20
Set TypingSpeed 30ms

# Clean slate: wipe the demo profile and any stale tmux sessions.
Hide
Type "rm -rf ~/.config/agent-of-empires/profiles/demo ~/demo-projects"
Enter
Type "mkdir -p ~/demo-projects/api-server ~/demo-projects/web-app"
Enter
Type "for d in ~/demo-projects/api-server ~/demo-projects/web-app; do (cd $d && git init -q && touch README.md && git add . && git commit -q -m init); done"
Enter
Type "tmux kill-server 2>/dev/null; clear"
Enter
Sleep 400ms
Show

# Launch aoe in the demo profile.
Type "./target/release/aoe -p demo"
Enter
Sleep 2000ms

# Dismiss the first-run welcome dialog.
Enter
Sleep 900ms

# === Session 1: API Server with Claude Code ===
Type "n"
Sleep 600ms

Tab
Sleep 200ms
Type "API Server"
Sleep 400ms

Tab
Sleep 200ms
Backspace 80
Type "/root/demo-projects/api-server"
Sleep 400ms

# Tool (claude is default), skip YOLO / Worktree / Sandbox / Group.
Tab
Sleep 150ms
Tab
Sleep 150ms
Tab
Sleep 150ms
Tab
Sleep 150ms

Enter
Sleep 2200ms

# Accept Claude Code's trust-folder dialog.
Enter
Sleep 2500ms

# Give Claude something to do so the preview has motion.
Type "Write me a 1000 word story about Age of Empires"
Sleep 400ms
Enter
Sleep 3500ms

# Detach back to the TUI; Claude keeps writing in the background.
Ctrl+b
Sleep 80ms
Type "d"
Sleep 1200ms

# === Remote access: press R, pick Local, dialog goes Active ===
Type "R"
Sleep 1600ms

# Local is selected by default (Cloudflare Tunnel is dimmed without
# cloudflared installed). Enter starts the local daemon; the dialog
# flips to Active with a QR code and the LAN URL.
Enter
Sleep 3000ms

# Show the URL cycling (Tab rotates LAN / loopback when both exist).
Tab
Sleep 1400ms

# Esc closes the dialog but leaves the daemon running — the status bar
# keeps the serving indicator lit.
Escape
Sleep 1200ms

# === Session 2: Web App with OpenCode, server still running ===
Type "n"
Sleep 600ms

Tab
Sleep 200ms
Type "Web App"
Sleep 400ms

Tab
Sleep 200ms
Backspace 80
Type "/root/demo-projects/web-app"
Sleep 400ms

# Tool: right-arrow from Claude to OpenCode.
Tab
Sleep 200ms
Right
Sleep 400ms

# Skip YOLO / Worktree / Sandbox / Group.
Tab
Sleep 150ms
Tab
Sleep 150ms
Tab
Sleep 150ms

Enter
Sleep 2500ms

# Detach back to the TUI.
Ctrl+b
Sleep 80ms
Type "d"
Sleep 1600ms

# Hide the final q so tmux-detach shell noise doesn't land in the GIF.
Hide
Type "q"
Sleep 400ms
