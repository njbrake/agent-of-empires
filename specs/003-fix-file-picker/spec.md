# Feature Specification: Fix File Picker Tab/Enter Behavior

**Feature Branch**: `003-fix-file-picker`
**Created**: 2026-02-08
**Status**: Draft
**Input**: User description: "Please fix the ctrl+p file selector. It has some weird behavior when I try to select a path I'm typing, pressing tab vs enter has some strange behavior..."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Tab Autocompletes Filter into Directory Navigation (Priority: P1)

When a user types partial text in the file picker filter to narrow results, pressing Tab should feel like shell-style tab completion: it expands the filter into the matched directory and navigates into it. Currently, Tab navigates into the highlighted directory but clears the filter text, creating a disconnect between what the user typed and what happened. The highlighted item may not even match the filter text if the user has also used arrow keys. Users expect Tab to act on the filter match, not just the highlight.

The fix: when a single directory matches the filter, Tab should navigate into it (regardless of highlight position). When multiple directories match, Tab should navigate into the highlighted one from the filtered list. This aligns Tab behavior with the shell autocomplete mental model that terminal users expect.

**Why this priority**: This is the core confusion reported. Tab is universally associated with autocompletion in path-related contexts (shells, file dialogs). The disconnect between "what I typed" and "what Tab does" is the primary source of strange behavior.

**Independent Test**: Can be tested by opening the file picker, typing a partial directory name, pressing Tab, and verifying the picker navigates into the matching directory with the path updated in the title bar.

**Acceptance Scenarios**:

1. **Given** the file picker is open with directories ["alpha", "beta", "gamma"] visible, **When** the user types "al" to filter down to "alpha" and presses Tab, **Then** the picker navigates into the "alpha" directory, the filter clears, and the title bar shows the updated path ending in "/alpha".
2. **Given** the file picker is open and the filter matches exactly one directory, **When** the user presses Tab, **Then** the picker navigates into that single matching directory regardless of which item is highlighted.
3. **Given** the file picker is open with a filter that matches multiple directories, **When** the user presses Tab, **Then** the picker navigates into the currently highlighted directory from the filtered results.
4. **Given** the file picker is open with a filter that matches no directories, **When** the user presses Tab, **Then** nothing happens (the picker remains in the current state).

---

### User Story 2 - Enter Confirms Selection in All States (Priority: P2)

When a user presses Enter, the picker should always close with a meaningful selection. Currently, if the filter matches nothing, Enter silently does nothing -- the picker stays open with no feedback. This leaves the user confused about why nothing happened. Enter should always close the picker: either with the highlighted directory or, if nothing matches, with the current working directory.

**Why this priority**: Enter is the universal "confirm" key. Silent no-ops break user trust and make the picker feel broken.

**Independent Test**: Can be tested by opening the file picker, filtering to an empty list, pressing Enter, and verifying the picker closes with the current working directory as the selection.

**Acceptance Scenarios**:

1. **Given** the file picker is open with a highlighted directory, **When** the user presses Enter, **Then** the picker closes and returns the full path of the highlighted directory.
2. **Given** the file picker is open with a filter that matches no directories, **When** the user presses Enter, **Then** the picker closes and returns the current working directory path.
3. **Given** the file picker is open with "../" highlighted, **When** the user presses Enter, **Then** the picker closes and returns the parent directory path.

---

### User Story 3 - Clearer Hint Bar Labels (Priority: P3)

The hint bar at the bottom currently shows "Tab cd" and "Enter select". While technically correct, "cd" is jargon that may not clearly communicate "navigate into this directory without closing." The labels should make the Tab vs Enter distinction intuitive at a glance.

**Why this priority**: Better labeling reduces confusion and helps users build the correct mental model for Tab (drill deeper) vs Enter (confirm and close).

**Independent Test**: Can be tested by opening the file picker and reading the hint bar to verify the labels communicate the distinction.

**Acceptance Scenarios**:

1. **Given** the file picker is open, **When** the user looks at the hint bar, **Then** the Tab action label clearly communicates "navigate into / open directory" and the Enter label clearly communicates "confirm selection and close."

---

### Edge Cases

- Tab on "../" navigates to parent directory without closing the picker (existing correct behavior).
- Tab at filesystem root (no parent): "../" is not shown, Tab navigates into highlighted directory normally.
- Enter at filesystem root with no filter: selects "/" (root) and closes.
- Backspace after Tab navigation (empty filter): goes to parent directory (existing correct behavior, should not change).
- Filter that partially matches "../" (typing "."): "../" should appear in filtered results (existing correct behavior).
- Very long directory names that exceed the display width: should not affect Tab/Enter behavior.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: When exactly one directory matches the active filter, pressing Tab MUST navigate into that directory regardless of highlight position.
- **FR-002**: When multiple directories match the active filter, pressing Tab MUST navigate into the currently highlighted directory from the filtered list.
- **FR-003**: When no directories match the active filter, pressing Tab MUST have no effect.
- **FR-004**: Pressing Enter with a highlighted directory MUST select that directory, close the picker, and return its full path.
- **FR-005**: Pressing Enter when the filtered list is empty MUST select the current working directory and close the picker (not silently do nothing).
- **FR-006**: The hint bar MUST use labels that clearly differentiate Tab ("enter dir" or similar) from Enter ("select" or similar).
- **FR-007**: After Tab navigation, the filter MUST clear and the directory listing MUST refresh to show contents of the newly entered directory.
- **FR-008**: Tab on "../" MUST navigate to the parent directory without closing the picker.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Users can navigate to a deeply nested directory (5+ levels) using Tab and partial-name typing without arrow keys.
- **SC-002**: The picker never silently ignores a keypress -- Tab and Enter always produce a visible result or the hint bar explains why no action was taken (e.g., no matches).
- **SC-003**: All existing file picker unit tests continue to pass with no regressions.
- **SC-004**: The path returned by Enter always matches what the user sees highlighted or the current working directory shown in the title bar.

## Assumptions

- The file picker is exclusively a directory picker (files are not shown or selectable). This is the current behavior and is correct for the use case.
- Tab-completion behavior (navigate into a directory) is the expected mental model from shell users, the primary audience for a terminal TUI application.
- The current Backspace behavior (go to parent when filter is empty, delete character when filter has text) is correct and should not change.
- The current Ctrl+H toggle for hidden files is correct and should not change.
- Changing Enter on an empty filtered list from "do nothing" to "select current directory" is a reasonable default because the user has already navigated to that directory and may want to confirm it.
