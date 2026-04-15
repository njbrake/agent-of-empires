# Agent of Empires demo: create a session, then enable and disable remote access.
# Regenerate with: vhs assets/demo.tape

Output docs/assets/demo.gif

Set Shell "bash"
Set FontSize 16
Set Width 1200
Set Height 720
Set Padding 20
Set TypingSpeed 40ms

# Clean slate: wipe the demo profile and stale agent sessions.
Hide
Type "rm -rf ~/.config/agent-of-empires/profiles/demo ~/demo-projects"
Enter
Type "mkdir -p ~/demo-projects/api-server && (cd ~/demo-projects/api-server && git init -q && touch README.md && git add . && git commit -q -m init)"
Enter
Type "tmux kill-server 2>/dev/null; clear"
Enter
Sleep 500ms
Show

# Launch aoe in the demo profile.
Type "./target/release/aoe -p demo"
Enter
Sleep 2500ms

# Dismiss the first-run welcome dialog.
Enter
Sleep 1200ms

# === Create one session so remote access has something to show ===
Type "n"
Sleep 800ms

# Tab past Profile onto Title.
Tab
Sleep 300ms
Type "API Server"
Sleep 600ms

# Path: clear default and enter demo path.
Tab
Sleep 300ms
Backspace 80
Type "/root/demo-projects/api-server"
Sleep 600ms

# Keep remaining defaults (tool = claude, no worktree, no sandbox).
Tab
Sleep 200ms
Tab
Sleep 200ms
Tab
Sleep 200ms
Tab
Sleep 200ms

# Submit.
Enter
Sleep 2500ms

# Accept Claude Code's trust-folder dialog.
Enter
Sleep 3000ms

# Detach back to the TUI.
Ctrl+b
Sleep 100ms
Type "d"
Sleep 1500ms

# === Remote access: open, start Local mode, then stop ===
# Press R to open the Serve dialog.
Type "R"
Sleep 2000ms

# The mode picker is on "Local" by default; Tunnel is dimmed without
# cloudflared installed. Enter starts the local server and moves the
# dialog to Active (QR + URL + passphrase if Tunnel; URL-only for Local).
Enter
Sleep 3500ms

# Tab cycles between available URLs (e.g. LAN vs loopback) in Local mode.
Tab
Sleep 1800ms
Tab
Sleep 1800ms

# Stop the daemon from inside the dialog.
Type "S"
Sleep 1800ms

# Back on the home screen. Quit.
Type "q"
Sleep 800ms
