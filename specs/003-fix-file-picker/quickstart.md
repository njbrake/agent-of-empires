# Quickstart: Fix File Picker Tab/Enter Behavior

**Branch**: `003-fix-file-picker`

## Prerequisites

- Rust toolchain (edition 2021, MSRV 1.74)
- `tmux` installed (for integration tests; unit tests work without it)

## Files to Modify

Only one file needs changes:

```
src/tui/components/dir_picker.rs
```

## Changes Summary

### 1. Tab Logic (lines 151-168)

Replace the Tab handler to check filter match count:
- **1 match**: Navigate into that directory (ignore highlight position)
- **Multiple matches**: Navigate into highlighted item from filtered list
- **0 matches**: No-op

### 2. Enter Logic (lines 131-134)

Remove the `if filtered_len == 0 { return DirPickerResult::Continue; }` guard. The existing fallthrough code at the end of the Enter handler already returns `Selected(cwd)`, which is the correct behavior.

### 3. Hint Bar (line 346)

Change `" cd  "` to `" enter dir  "`.

### 4. Tests (lines 672-687+)

- Update `test_enter_on_empty_filtered_list_does_nothing` to expect `Selected(cwd)` and `!is_active()`
- Add `test_tab_single_match_autocompletes`
- Add `test_tab_no_match_does_nothing`

## Verification

```bash
cargo test            # All tests pass
cargo clippy          # No warnings
cargo fmt --check     # Formatting clean
```

## Manual Testing

1. `cargo run --release`
2. Press `n` to open New Session dialog
3. Focus Path field, press Ctrl+P
4. Type a partial directory name, verify Tab navigates into the matching directory
5. Type a filter that matches nothing, verify Enter closes and selects the current directory
6. Check hint bar shows "Tab enter dir" instead of "Tab cd"
