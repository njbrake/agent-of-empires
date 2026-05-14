# Diff comments implementation plan (#928)

**Goal:** Let users comment on lines / ranges in the web diff viewer (GitHub-style) and submit those comments as a single markdown prompt to the cockpit agent.

## Debate summary

**Positions:**
- **gemini-3.1-pro-preview:** Persist captured snippet at authoring time; durable state at provider level with selection state local to viewer; React.memo + CSS hover + stable keyed fragments to avoid render thrashing; drop content-mismatch stale (initially) to avoid false positives; reject multi-tab storage event listener as YAGNI.
- **gpt-5.5:** Same persisted-snippet model; version the storage envelope; explicit side semantics for ranges (new range skips deleted rows, vice versa); content-mismatch stale on top of line-missing stale; provider-wrapped layout because `RightPanel` and `DiffFileViewer` are siblings; gate send button on `cockpit_worker_state === "running"`; render stale comments in a file-level block (not only the dialog); separate pure modules per concern, no `utils.ts` junk drawer.

**Points of agreement (final round):**
- Durable comments are session-scoped, persisted in localStorage with a versioned envelope; cleared after successful send if the checkbox stays checked.
- `DiffComment` must persist `capturedSnippet` (the exact text the user reviewed) and the inferred code-fence `language` at authoring time. Stale prompt assembly never re-extracts from the current diff.
- Stale-status check: a comment whose `(side, [startLine..endLine])` is no longer represented in the current diff is `stale`. Content-mismatch is exposed by `anchor.ts` as a separate flag but not surfaced in v1 UI to avoid false-positive noise.
- Side semantics: clicks on added rows anchor to `new`, clicks on deleted rows anchor to `old`, clicks on equal/context rows default to `new`. Snippet extraction filters rows by side (new-side range skips deleted rows; old-side range skips added rows).
- Cross-hunk ranges disallowed.
- Click-to-start → click-to-end range selection; clicking the same line twice = single-line comment. No drag.
- Banner lives in `RightPanel`'s header (above `DiffFileList`); both surfaces (banner and diff viewer) must consume the same shared comments state, so the state lives in a session-scoped `DiffCommentsProvider` wrapping the relevant layout.
- DiffLine extension is generic: `leadingSlot`, `isHighlighted`, `isRangeEndpoint`, optional `onGutterClick` for the slot's button. Wrap in `React.memo`. Keep hover purely CSS (no React hover state).
- Inline form/card insertion uses keyed fragments (`Fragment key={...}` with stable child keys including `comment.id`).
- Send button is disabled unless `session.cockpit_mode === true` and `session.cockpit_worker_state === "running"`.
- Send dialog is three pieces (settled in round 2): editable intro textarea (top) + read-only Markdown preview of assembled comments (middle) + editable outro textarea (bottom, default "Please address these comments."). On send, the body is `intro + assembled + outro`. No single-editable-textarea (rejected for state-bifurcation reasons).
- Dynamic code fences: outer fence length = max(3, longest consecutive backtick run in snippet + 1).
- Hotkeys: keep `Esc` (cancel active form/range/dialog), `Cmd/Ctrl+Enter` (save in form), `Cmd/Ctrl+Shift+S` (open send dialog when comments exist). Drop `c` for v1 (no row focus model).
- File layout: separate pure modules `buildPrompt.ts`, `extractSnippet.ts`, `anchor.ts`, `storage.ts`, `language.ts`. No `utils.ts` junk drawer.
- Multi-tab storage event coherence: YAGNI for v1.

**Resolved disagreements:**
- **Content-mismatch stale:** gpt-5.5 wanted it surfaced; gemini argued false-positive risk (whitespace/formatter). **Verdict:** compute it inside `anchor.ts` (cheap), but in v1 UI only show `[stale]` when the range is missing. Keep the computation so a later UI iteration can surface `[changed]` without refactoring the anchor logic.
- **Dialog structure:** "single editable textarea" was a stated requirement, but both debaters converged on the three-piece dialog after recognising the bifurcation problem. **Verdict:** adopt the three-piece dialog (intro / preview / outro); the "settled" requirement is reversed because both LLMs and the analysis support it.
- **Multi-tab storage events:** gpt-5.5 wanted it; gemini called it YAGNI. **Verdict:** YAGNI for v1. Write-through on state change is enough.

**Verdict:** Build a session-scoped comments subsystem. Durable state in `DiffCommentsProvider` (wrapping the layout) + persisted snippet/language per comment. Pure modules for prompt/snippet/anchor/storage/language. `DiffLine` gets generic slot props + `React.memo`. Inline form/card injection with keyed fragments. Cockpit-only; gate on worker running. Stale block at top of file viewer for stale comments. Three-piece send dialog. localStorage with versioned envelope. Hotkeys limited to Esc / Cmd-Enter / Cmd-Shift-S.

---

## Task 1: Types + storage + language map

**Files:**
- Create `web/src/components/diff/comments/types.ts`
- Create `web/src/components/diff/comments/storage.ts`
- Create `web/src/components/diff/comments/language.ts`

**Notes:**
- `DiffComment` shape:
  ```ts
  type DiffSide = "old" | "new";
  interface DiffComment {
    id: string;
    repoName?: string;
    filePath: string;
    side: DiffSide;
    startLine: number;
    endLine: number;
    body: string;
    capturedSnippet: string;
    language?: string;
    createdAt: string;
    updatedAt?: string;
  }
  ```
- Storage envelope (versioned):
  ```ts
  interface DiffCommentsStorageV1 {
    version: 1;
    comments: DiffComment[];
    clearAfterSend: boolean;
    introDraft?: string;
    outroDraft?: string;
  }
  ```
- Storage key: `aoe:diff-comments:v1:${sessionId}`. Safe JSON parse with recovery; if `version !== 1`, drop and start fresh.
- `language.ts`: tiny `extensionToLanguage(filePath)` returning code-fence language string (or empty).

## Task 2: Pure snippet extraction

**File:** Create `web/src/components/diff/comments/extractSnippet.ts`

**Contract:**
```ts
function extractSnippetFromHunks(
  hunks: RichDiffHunk[],
  side: DiffSide,
  startLine: number,
  endLine: number,
): { snippet: string; hunkIndex: number; endRowIndex: number } | null
```
- Iterate hunks, find the one whose `(old|new)_start..(start+lines)` covers the requested range.
- Walk lines; for `side="new"` keep rows with `new_line_num != null` and skip pure deletes; for `side="old"` skip pure adds.
- Return `null` if any line number in the range is missing.

## Task 3: Anchor / stale matching

**File:** Create `web/src/components/diff/comments/anchor.ts`

**Contract:**
```ts
interface AnchoredComment {
  comment: DiffComment;
  status: "active" | "stale";
  contentChanged: boolean;
  hunkIndex?: number;
  endRowIndex?: number;
}
function anchorComments(
  comments: DiffComment[],
  filePath: string,
  repoName: string | undefined,
  hunks: RichDiffHunk[],
): AnchoredComment[]
```
- For each comment matching `(repoName, filePath)`: try extraction.
- `null` → `status: "stale"`, no anchor.
- Found → `status: "active"`, `contentChanged = (extracted !== comment.capturedSnippet)`.
- `contentChanged` exposed for future UI; v1 UI only branches on `status`.

## Task 4: Prompt builder

**File:** Create `web/src/components/diff/comments/buildPrompt.ts`

**Contract:**
```ts
function buildCommentsMarkdown(
  comments: DiffComment[],
  opts: { isMultiRepo: boolean },
): string;
function buildFullPrompt(
  comments: DiffComment[],
  intro: string,
  outro: string,
  opts: { isMultiRepo: boolean },
): string;
```
- Sort comments by `(repoName ?? "")`, `filePath`, `startLine`, `side`, `createdAt`.
- Heading: `### [repoA] \`src/foo.rs\` lines 42-45 (new)` (single-line variant: `line 42`).
- Dynamic code fence: `Math.max(3, maxConsecutiveBackticks(snippet) + 1)`.
- Default outro `"Please address these comments."` (caller passes it explicitly so tests are deterministic).
- Trim trailing whitespace; ensure a single blank line between sections.

## Task 5: useDiffComments hook + provider

**Files:**
- Create `web/src/hooks/useDiffComments.ts`
- Add inline `<DiffCommentsProvider>` (or thread the hook through `App.tsx` to avoid context). Decision: lift the hook to `App.tsx` and pass `comments` + actions to both `RightPanel` and the diff viewer; no provider, simpler.

**Hook API:**
```ts
interface UseDiffCommentsResult {
  comments: DiffComment[];
  count: number;
  clearAfterSend: boolean;
  setClearAfterSend(v: boolean): void;
  introDraft: string;
  outroDraft: string;
  setIntroDraft(v: string): void;
  setOutroDraft(v: string): void;
  addComment(input: Omit<DiffComment, "id" | "createdAt">): DiffComment;
  updateComment(id: string, body: string): void;
  deleteComment(id: string): void;
  clearComments(): void;
}
```
- Load from localStorage on mount (by sessionId).
- Write through on every change with versioned envelope.
- Tests: round-trip, corruption recovery, scoped per session.

## Task 6: DiffLine props + memo

**File:** Edit `web/src/components/diff/DiffLine.tsx`

- Add props: `leadingSlot?: React.ReactNode`, `isHighlighted?: boolean`, `isRangeEndpoint?: boolean`.
- Wrap export with `React.memo`.
- Hover state stays CSS-only (`group-hover`).
- Add `group` class on the row; `leadingSlot` renders inside the old-line-num gutter (overlay, not new column — keeps total width stable).
- Tinted background when `isHighlighted`; thicker left border when `isRangeEndpoint`.

## Task 7: Inline form + card components

**Files:**
- Create `web/src/components/diff/comments/CommentForm.tsx`
- Create `web/src/components/diff/comments/CommentCard.tsx`

**CommentForm:**
- Textarea (autofocus, `Cmd/Ctrl+Enter` save, `Esc` cancel).
- Range readout `Comment on lines 42-45 (new)`.
- Save / Cancel buttons.

**CommentCard:**
- Renders `comment.body` via existing `<Markdown text={body} />`.
- `[stale]` chip when `status === "stale"`.
- Edit / Delete actions; Edit toggles into CommentForm.

## Task 8: CommentsBanner

**File:** Create `web/src/components/diff/comments/CommentsBanner.tsx`

- `N comment(s) · Send · Discard all`.
- Send disabled when `!sendEnabled` with tooltip "Cockpit worker not running".
- Visible only when `commentsEnabled` (cockpit_mode true) AND `count > 0`.

## Task 9: SendCommentsDialog

**File:** Create `web/src/components/diff/comments/SendCommentsDialog.tsx`

- Three-piece layout:
  - Intro textarea (top, bound to `introDraft`)
  - Read-only assembled markdown preview (middle, renders via `<Markdown>`)
  - Outro textarea (bottom, bound to `outroDraft`, placeholder "Please address these comments.")
- Checkbox `Clear comments after sending` (bound to `clearAfterSend`).
- Buttons: Cancel / Send.
- Hotkeys: `Cmd/Ctrl+Enter` sends; `Esc` cancels.
- On Send: POST `/api/sessions/:id/cockpit/prompt` with `{ text }`. Body composed at send time, not from stored copy.
- On success: clear if checkbox checked; close.
- On failure: keep dialog open, show inline error.

## Task 10: DiffFileViewer integration

**File:** Edit `web/src/components/diff/DiffFileViewer.tsx`

- Accept `anchored: AnchoredComment[]`, `commentsEnabled: boolean`, `onAddComment`, `onUpdateComment`, `onDeleteComment` props.
- Local state: `selection: { startLine, side, hunkIndex } | null` and `draft: { ... } | null`.
- `+` button: rendered in `leadingSlot` of new-line-num gutter (only on hover via CSS).
- Click handler:
  - First click: set `selection`. Hint banner above hunk: "Click another line to extend, or click the same line again."
  - Second click in same hunk + same side: compute range, extract snippet, open `CommentForm` after the end row.
  - Same-line second click: single-line range.
  - Escape clears selection.
- HunkView render:
  - Stale block at top of file (above first hunk).
  - For each row, after `<DiffLine>`, render any `CommentCard`s whose `endRowIndex` matches and the `CommentForm` if active.
  - Keyed fragments: `Fragment key={\`row-h${hi}-r${i}\`}` with explicit child keys `line-${...}`, `comment-${comment.id}`, `form-${draftKey}`.

## Task 11: Wire through RightPanel + App

**Files:**
- Edit `web/src/components/RightPanel.tsx`: render `<CommentsBanner>` above `<DiffFileList>` when `comments.length > 0`.
- Edit `web/src/App.tsx`: lift `useDiffComments(activeSessionId)`; thread `anchored`/actions through `<DiffFileViewer>` (passed to RightPanel? no — DiffFileViewer lives in main content, not RightPanel).

Need to find where `<DiffFileViewer>` is rendered and thread the props there.

## Task 12: Send dialog open state + global hotkey

- App-level `sendDialogOpen` state.
- `Cmd/Ctrl+Shift+S` global handler (window level, ignore when target is input/textarea/contenteditable) opens dialog when `count > 0 && commentsEnabled`.
- Banner Send button opens same dialog.

## Task 13: Tests

**Files:**
- Create `web/src/components/diff/comments/buildPrompt.test.ts`
- Create `web/src/components/diff/comments/extractSnippet.test.ts`
- Create `web/src/components/diff/comments/anchor.test.ts`

Cover: ordering, dynamic fences, multi-repo prefix, single-line vs range wording, missing range → null, side filtering, stale detection.

(Skip Playwright in this pass; lots of moving parts, will add follow-up.)

## Task 14: Docs

**File:** Edit `docs/guides/diff-view.md`

Add a "Commenting on the diff" section: how to start a comment, range selection, send flow, stale handling.

---

## Notes on scope discipline

- No backend changes. Send endpoint already exists.
- No new dependencies. `marked` and existing `<Markdown>` cover rendering.
- TUI deferred — issue allows it.
- Mobile drag UX deferred — click-click works on touch.
- Multi-tab sync deferred.
- Content-mismatch UI surfacing deferred (computed but not displayed in v1).
