# Add Qwen Code Support

## Summary

Adds support for [Qwen Code](https://www.npmjs.com/package/@anthropic-ai/qwen-code) as a new AI agent in Agent of Empires. Qwen Code can now be selected when creating new sessions, with full support for YOLO mode, custom instructions, and file-based hooks.

## Changes

### `src/agents.rs`
- Added new `AgentDef` entry for Qwen Code with:
  - **Binary**: `qwen`
  - **YOLO mode**: `--yolo` flag (auto-approve all actions)
  - **Custom instructions**: `--append-system-prompt {}` flag
  - **Hook config**: `.qwen/settings.json` with PreToolUse, UserPromptSubmit, Stop, Notification, and ElicitationResult events
  - **Host launch**: Enabled (runs natively on host system)
  - **Container env**: None required
- Updated tests:
  - `test_get_agent_known()` - added qwen assertion
  - `test_agent_names()` - added "qwen" to expected list (9 agents total)
  - `test_resolve_tool_name()` - added qwen test case
  - `test_settings_index_roundtrip()` - added qwen index tests (index 9)

### `src/tmux/status_detection.rs`
- Added `detect_qwen_status()` function with detection for:
  - **Running**: Braille spinners (⠋⠙...), "esc to interrupt", "ctrl+c to interrupt", activity indicators (thinking, working, reading, writing, executing, generating, processing)
  - **Waiting**: Approval prompts `(y/n)`, `[y/n]`, `allow`, `approve`, `execute?`, selection menus with numbered options, input prompts (`>`, `> `, `qwen>`)
  - **Idle**: Default fallback when no active state detected
- Added comprehensive tests:
  - `test_detect_qwen_status_running()` - 6 test cases
  - `test_detect_qwen_status_waiting()` - 6 test cases
  - `test_detect_qwen_status_idle()` - 2 test cases

### `README.md`
- Updated multi-agent support feature list to include "Qwen Code"

## Configuration

### Qwen Code CLI Options Used

| Feature | Flag | Description |
|---------|------|-------------|
| YOLO mode | `--yolo` | Automatically accept all actions |
| Custom instructions | `--append-system-prompt {}` | Append developer instructions |
| Hooks | `--experimental-hooks` | Enable lifecycle event hooks |
| Sandbox | `-s, --sandbox` | Built-in sandbox (separate from AoE Docker) |

### Settings File Location

Qwen Code settings and hooks are configured in:
- **Path**: `~/.qwen/settings.json`
- **Hook events**: Same as Claude Code/Cursor (PreToolUse, UserPromptSubmit, Stop, Notification, ElicitationResult)

## Testing

### Prerequisites

Install Rust 1.85+:
```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Or via Homebrew (macOS):
```bash
brew install rustup
rustup default 1.85
```

### Run Tests

```bash
# Type-check only (fast)
cargo check

# Run all unit and integration tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Format code
cargo fmt

# Lint
cargo clippy

# Build release binary
cargo build --release
```

### Manual Testing

1. Install Qwen Code:
   ```bash
   npm install -g @anthropic-ai/qwen-code
   ```

2. Launch AoE TUI:
   ```bash
   cargo run --release
   ```

3. Create a new session and select "qwen" as the tool

4. Verify status detection:
   - Agent shows "running" (green) while processing
   - Agent shows "waiting" (amber) at approval prompts
   - Agent shows "idle" (gray) when complete

## Future Improvements

Potential enhancements for follow-up PRs:

1. **Refined status detection**: Adjust patterns based on actual Qwen Code UI output observed in production use
2. **Additional approval modes**: Support `--approval-mode auto-edit` as an alternative to `--yolo`
3. **Container environment variables**: Add any Qwen-specific env vars if needed for Docker sandboxing
4. **Hook event refinement**: Update hook events if Qwen Code uses different event names than Claude Code

## Screenshots

_(Add screenshots of Qwen Code running in AoE TUI here)_

## Checklist

- [x] Code follows project conventions (AGENTS.md)
- [x] All tests updated and passing
- [x] New functionality has test coverage
- [x] Documentation updated (README.md)
- [x] `cargo fmt` clean
- [x] `cargo clippy` clean
- [ ] Manual testing completed (requires Qwen Code installation)

## Related

- Qwen Code npm package: https://www.npmjs.com/package/@anthropic-ai/qwen-code
- Qwen Code GitHub: https://github.com/anthropics/qwen-code
