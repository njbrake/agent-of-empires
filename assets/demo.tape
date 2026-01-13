# Agent of Empires Demo
Output assets/demo.gif

Set FontSize 16
Set Width 1200
Set Height 700
Set Padding 20

# Launch aoe with demo profile
Set TypingSpeed 1ms
Type "./target/release/aoe -p demo"
Enter
Sleep 500ms

Set TypingSpeed 50ms

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
Set TypingSpeed 50ms
Type "/tmp/demo-projects/api-server"
Sleep 100ms

# Skip group field
Tab
Sleep 50ms

# Tool field - move right to select OpenCode
Tab
Space
Sleep 100ms

# Submit
Enter
Sleep 2000ms

# Type a prompt
Set TypingSpeed 20ms
Type "Write me a shortstory about age of empires 2."
Set TypingSpeed 50ms
Enter
Sleep 500ms

# Detach from tmux (Ctrl+b d)
Ctrl+b
Sleep 50ms
Type "d"
Sleep 1000ms

# === Create second session with claude code and worktree ===
# Open new session dialog
Type "n"
Sleep 500ms

# Enter title
Type "Web App"
Sleep 500ms

# Move to path field
Tab
Sleep 500ms

# Enter project path (clear default first)
Set TypingSpeed 1ms
Backspace 100
Set TypingSpeed 50ms
Type "/tmp/demo-projects/web-app"
Sleep 500ms

# Move to worktree field
Tab
Tab
Tab
Sleep 500ms

# Enter worktree branch name
Type "feature-demo"
Sleep 500ms

# Submit
Enter
Sleep 500ms

# accept warning dialogue
Enter
Sleep 500ms

# Type a prompt
Set TypingSpeed 20ms
Type "Quick! Ask me a multi choice question about age of empires 2, use your tool"
Set TypingSpeed 50ms
Enter
Sleep 500ms

# Detach from tmux (Ctrl+b d)
Ctrl+b
Sleep 50ms
Type "d"
Sleep 500ms

# === Create third session with sandbox enabled ===
# Open new session dialog
Type "n"
Sleep 500ms

# Enter title
Type "Chat App"
Sleep 500ms

# Move to path field
Tab
Sleep 500ms

# Enter project path (clear default first)
Set TypingSpeed 1ms
Backspace 100
Set TypingSpeed 50ms
Type "/tmp/demo-projects/chat-app"
Sleep 500ms

# Skip group field, tool field, worktree field to reach sandbox
Tab
Tab
Tab
Tab
Sleep 500ms

# Toggle sandbox ON (space to toggle)
Space
Sleep 500ms

# Submit
Enter
Sleep 500ms

# accept warning dialogue if any
Enter
Sleep 1000ms

# Type a prompt
Set TypingSpeed 20ms
Type "Write me a poem about sandboxes and containers."
Set TypingSpeed 50ms
Enter
Sleep 500ms

# Detach from tmux (Ctrl+b d)
Ctrl+b
Sleep 50ms
Type "d"
Sleep 500ms

# Wait for statuses to resolve
Type "j"
Sleep 3000ms
Type "j"
Sleep 3000ms
Type "k"
Sleep 3000ms
Type "k"
Sleep 3000ms

# Exit aoe

Type "q"
