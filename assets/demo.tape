# Agent of Empires Demo
Output docs/assets/demo.gif

Set FontSize 16
Set Width 1200
Set Height 700
Set Padding 20

# Launch aoe with demo profile
Set TypingSpeed 1ms
Type "./target/release/aoe -p demo"
Set TypingSpeed 50ms
Enter
Sleep 3000ms


# === Create first session with opencode ===
# Open new session dialog
Type "n"
Sleep 500ms

# Enter title as "API Server"
Type "API Server"
Tab
Sleep 100ms

# Enter project path (clear default first)
Set TypingSpeed 1ms
Backspace 100
Type "/Users/nbrake/demo-projects/api-server"
Set TypingSpeed 50ms
Sleep 1000ms

# Skip group field
Tab
Sleep 100ms

# Tool field - move right to select OpenCode
Tab
Sleep 100ms
Space
Sleep 2000ms

# Submit
Enter
Sleep 2500ms
Enter
Sleep 1000ms

# Type a prompt
Set TypingSpeed 10ms
Type "Write me a short story about age of empires 2."
Set TypingSpeed 50ms
Enter
Sleep 1000ms

# Detach from tmux (Ctrl+b d)
Ctrl+b
Sleep 50ms
Type "d"
Sleep 1000ms


# ==== Showcase terminal view ====
Type "t"
Sleep 1000ms
Enter
Sleep 1000ms
Type "pwd"
Enter
Sleep 1000ms
Ctrl+b
Sleep 50ms
Type "d"
Sleep 1000ms
Type "t"
Sleep 500ms

# === Create second session with claude code and worktree ===
# Open new session dialog
Type "n"
Sleep 1000ms

# Enter title
Type "Web App"
Sleep 1000ms

# Move to path field
Tab
Sleep 1000ms

# Enter project path (clear default first)
Set TypingSpeed 1ms
Backspace 100
Type "/Users/nbrake/demo-projects/web-app"
Sleep 1000ms

# Move to worktree field
Tab
Tab
Tab
Sleep 1000ms

# Enter worktree branch name
Type "feature-demo"
Set TypingSpeed 50ms
Sleep 1000ms

# Submit
Enter
Sleep 1000ms
Enter
Sleep 1000ms

# Type a prompt
Set TypingSpeed 10ms
Type "Quick! Ask me a multi choice question about age of empires 2, use your tool"
Set TypingSpeed 50ms
Enter
Sleep 1000ms

# Detach from tmux (Ctrl+b d)
Ctrl+b
Sleep 50ms
Type "d"
Sleep 1000ms

# === Create third session with sandbox enabled ===
# Open new session dialog
Type "n"
Sleep 1000ms

# Enter title
Set TypingSpeed 1ms
Type "Chat App"
Sleep 1000ms

# Move to path field
Tab
Set TypingSpeed 50ms
Sleep 1000ms

# Skip group field, tool field, worktree field to reach sandbox
Tab
Tab
Tab
Tab
Sleep 500ms

# Toggle sandbox ON (space to toggle)
Space
Sleep 500ms
Tab # skip custom image
Tab
Sleep 100ms
Space
Sleep 100ms

# Submit
Enter
Sleep 2000ms

# accept warning dialogue if any
Enter
Sleep 1500ms

# Type a prompt
Set TypingSpeed 10ms
Type "Write me a poem about sandboxes and containers."
Set TypingSpeed 50ms
Enter
Sleep 500ms

# Detach from tmux (Ctrl+b d)
Ctrl+b
Sleep 50ms
Type "d"
Sleep 3000ms
Type "k"
Sleep 3000ms
Type "k"
Sleep 3000ms

# Exit aoe

Type "q"
