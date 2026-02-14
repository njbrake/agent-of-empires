# Research: Fix File Picker Tab/Enter Behavior

**Branch**: `003-fix-file-picker` | **Date**: 2026-02-11

## Research Findings

### R-001: Tab Autocomplete Behavior in Shell-Style Pickers

**Decision**: When exactly one directory matches the filter, Tab navigates into it regardless of highlight position. When multiple match, Tab navigates into the highlighted item from the filtered list. When none match, Tab is a no-op.

**Rationale**: This mirrors bash/zsh tab-completion semantics. Terminal TUI users expect Tab to act on what they typed (the filter), not on an independently-managed highlight cursor. The single-match case is the key fix: the user typed enough to uniquely identify a directory, so Tab should just go there.

**Alternatives considered**:
- Always use highlight (current behavior): Rejected because it breaks the shell mental model. Users type "al" expecting Tab to enter "alpha/", but if they previously arrowed to a different item, Tab goes there instead.
- Always use first filter match (ignore highlight): Rejected because when multiple items match, the highlight is the only way to disambiguate. Arrow keys within a filtered list is a valid interaction.

### R-002: Enter on Empty Filtered List

**Decision**: Enter on an empty filtered list selects the current working directory (cwd) and closes the picker.

**Rationale**: Enter is the universal "confirm" action. The current behavior (silent no-op, picker stays open) is confusing. Returning the cwd is the most reasonable default since the user has already navigated there and the filter just happens to match nothing. This is also what happens when `selected >= filtered_len` in the existing code (line 148-149), so the fix is simply removing the early return guard.

**Alternatives considered**:
- Show an error/flash message: Rejected as over-engineered for a keyboard-driven TUI. The user can see the "(empty directory)" message already.
- Cancel (close without selection): Rejected because that's what Esc does. Enter and Esc should have distinct behaviors.

### R-003: Hint Bar Label for Tab

**Decision**: Change "Tab cd" to "Tab enter dir" in the DirPicker hint bar.

**Rationale**: "cd" is Unix jargon that assumes shell knowledge. "enter dir" communicates both the action (navigate into) and the target (a directory) without requiring shell vocabulary. It also distinguishes from "Enter select" which confirms and closes.

**Alternatives considered**:
- "Tab open": Too vague, could mean "open in editor" or similar.
- "Tab navigate": Too long and still somewhat abstract.
- "Tab drill": Non-standard terminology.

### R-004: Existing Test Coverage Impact

**Decision**: Update the existing test `test_enter_on_empty_filtered_list_does_nothing` to reflect the new behavior (Enter selects cwd instead of being a no-op). Add new tests for single-match Tab autocomplete.

**Rationale**: The test name and assertion directly encode the current (buggy) behavior. The fix changes the contract, so the test must change too. No existing test covers the single-match Tab autocomplete scenario, so a new test is needed.

**Alternatives considered**: None. Tests must match the implementation contract.

### R-005: ListPicker Enter on Empty Filter

**Decision**: No change to ListPicker. The current behavior (return `Cancelled` on empty filter) is acceptable for Groups and Branches since users pick from a known list, and cancellation is a reasonable fallback.

**Rationale**: The spec identifies this as P2 but notes it "should never happen" in practice for the ListPicker's use cases (groups and branches). The DirPicker fix is the priority. ListPicker's `Cancelled` return on empty filter is already handled gracefully by the caller (picker closes, field unchanged).

**Alternatives considered**:
- Return first item: Could cause unexpected selection if user just fat-fingered the filter.
- Keep picker open: Would create the same silent no-op problem as DirPicker.
