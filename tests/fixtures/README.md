# Status Detection Fixtures

This directory contains terminal screen captures used for golden testing of status detection. Each state (idle, running, waiting_permission, waiting_question) is a directory containing one or more fixture files.

## Directory Structure

```
fixtures/
  claude_code/
    idle/
      001_welcome_screen.txt
      002_after_command.txt
    running/
      001_thinking.txt
    waiting_permission/
      001_bash_command.txt
    waiting_question/
      001_checkbox.txt
  opencode/
    idle/
      001_startup.txt
    running/
      001_generating.txt
    waiting_permission/
      001_bash_command.txt
```

## Adding a New Screen Capture

### Step 1: Get the tool into the desired state

Start the tool (Claude Code or OpenCode) in a tmux session managed by `aoe`, and get it into the state you want to capture:
- `idle`: Tool is waiting for user input
- `running`: Tool is actively processing (thinking, generating, etc.)
- `waiting_permission`: Tool is waiting for user approval
- `waiting_question`: Tool is asking the user a question

### Step 2: Capture the fixture

Run the capture script:

```bash
./scripts/capture-fixtures.sh <tool> <state> <tmux_session> [description]
```

**Arguments:**
- `tool`: `claude` or `opencode`
- `state`: `idle`, `running`, `waiting_permission`, or `waiting_question`
- `tmux_session`: Name of the tmux session (e.g., `aoe_myproject_abc12345`)
- `description`: Optional description for the filename (e.g., `bug_report_123`)

**Examples:**
```bash
# Basic capture
./scripts/capture-fixtures.sh claude running aoe_myproject_abc12345

# With description
./scripts/capture-fixtures.sh claude running aoe_myproject_abc12345 "tool_call"
./scripts/capture-fixtures.sh opencode waiting_permission aoe_task_def67890 "file_edit"
```

The script will:
- Create the state directory if it doesn't exist
- Auto-generate a sequential filename (e.g., `002_tool_call.txt`)
- Capture the last 50 lines of the tmux pane
- Add metadata headers to the fixture file

### Step 3: Verify the capture

1. Review the captured content:
   ```bash
   cat tests/fixtures/claude_code/running/002_tool_call.txt
   ```

2. Update the "Key indicators" comment in the fixture file if needed

3. Run the tests to verify detection works:
   ```bash
   cargo test --test status_detection
   ```

### Step 4: Update detection logic (if needed)

If the test fails, you may need to update the detection logic in `src/tmux/session.rs`:
- `detect_claude_status()` for Claude Code
- `detect_opencode_status()` for OpenCode

## Naming Convention

Fixtures use the format: `NNN_description.txt`

- `NNN`: Zero-padded sequence number (001, 002, 003, ...)
- `description`: Brief snake_case identifier (optional, defaults to `capture`)

The sequence number ensures deterministic test ordering and makes it easy to add multiple examples per state.

## Adding Fixtures for Bug Reports

If someone reports that a state is being detected incorrectly, you can add their screen capture as a new fixture:

1. Ask them to capture the screen using the script (or manually create the file)
2. Add it to the appropriate state directory
3. The test will verify that all fixtures in that directory detect correctly
4. If it fails, update the detection logic to handle the new case

This allows multiple examples per state, making the tests more robust against edge cases and UI variations.
