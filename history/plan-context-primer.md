# Context-primer-on-load-failure Implementation Plan (#1004)

**Goal:** When the cockpit's `session/load` fails and falls back to `session/new`, give the user a banner button that fetches a markdown primer (recent transcript) from the SQLite event store and pre-fills the composer so they can review/edit before sending.

## Debate Summary

**Positions:**
- **gemini:** Backend Rust builder + new endpoint. Started with single 20k char backward walk, adopted turn-aware + completion snippets + TodoWrite preservation by Round 2.
- **openai:** Backend Rust builder + new endpoint. Hybrid cap 32k/20 turns/per-tool 300 chars + structural framing. Reduced to "global cap + safety rails" by Round 2.
- **grok:** Started frontend-over-`ActivityRow`, pivoted to frontend-over-`/cockpit/replay`. Cap 12k chars / 20 turn-aware pairs. Argued for no new endpoint.

**Points of agreement (Round 2):**
- Turn-aware truncation (do not split mid-turn, mid-tool-sequence)
- Bulk content elision (`new_string`, `old_string`, `content`, `file_text`, `output`, `stdout`, `stderr`, `diff`, `patch`, etc.)
- One-line tool summaries with kind-aware key extraction
- Keep `TodoWrite`/`PlanUpdated`/`Plan`, drop opaque `Think`
- Pre-fill composer with primer (transparency)
- Banner above composer (NOT embedded button inside markdown callout)
- Structural framing: `# Prior cockpit context` / `## Transcript` / `## Current request`
- `before_seq` parameter so primer excludes post-reset events
- Reducer state set on `SessionContextReset` (only when prior prompt existed), cleared on `UserPromptSent`

**Resolved disagreements:**
- **Synthesis location:** gemini+openai backend vs grok frontend → **Verdict: Backend Rust.** Frontend approach via `/cockpit/replay` would fetch the full event log (possibly thousands of events, megabyte payloads) just to discard 90% client-side. Server can `WHERE seq < N` directly. Logic depends on event-schema understanding (tool-call lifecycle merge, turn boundaries, plan events) — duplicating in TS risks schema drift. Server-local transform belongs in the server.
- **Cap size:** 12k (grok) vs 20k (gemini) vs 32k (openai) → **Verdict: 24,000 chars** with turn-aware admission. Compromise.
- **Per-turn caps:** openai's many caps vs gemini's single cap → **Verdict: Global cap + minimal safety rails.** Drop whole older turns first; only truncate within a turn when newest single turn exceeds budget. Mandatory per-tool-line cap (300 chars) because args can be 16KiB.

**Verdict:** Build a pure Rust primer module that walks events backward in turn-aware chunks, emits compact markdown with kind-aware tool summaries and bulk elision, capped at 24k chars. Expose via `GET /api/sessions/{id}/cockpit/context-primer?before_seq=N`. Frontend tracks `contextPrimerAvailable` reducer flag, renders `<ContextPrimerBanner>` above the composer, fetches and pre-fills composer on click. Pure Rust unit tests cover builder; one ACP-level integration test covers load-failure path.

---

### Task 1: Pure Rust primer builder

**Files:**
- Create: `src/cockpit/context_primer.rs`
- Modify: `src/cockpit/mod.rs` (export module)

**Steps:**
1. `pub fn build_context_primer(events: &[(u64, Event)], opts: PrimerOptions) -> ContextPrimer`
2. Walk events, accumulate into `Vec<Turn>`. Turn starts on `UserPromptSent`, ends on `Stopped` (or implicit at next user prompt). Each turn carries user text + accumulated assistant text + merged tool calls (by `tool_call_id`).
3. Tool-call merge: `ToolCallStarted` → create entry. `ToolCallUpdated` → patch title/args. `ToolCallCompleted` → set status (ok/error). Format on render.
4. Tool rendering: kind-aware extraction (`file_path`/`path`, `command`, `pattern`, `query`, `url`, `glob`). Bulk-key denylist for fallback rendering. Truncate to 300 chars.
5. Keep TodoWrite/PlanUpdated as compact "Plan: N done, M in progress, P pending" lines. Drop `ThinkingStarted`/`ThinkingEnded`/opaque thought.
6. Turn-aware cap: include newest complete turns until accumulated chars exceed `max_chars`. If newest single turn alone exceeds cap, truncate within it (preserve user prompt, compact tool lines, assistant tail).
7. Wrap output: `# Prior cockpit context\n\n...\n## Transcript\n\n### Turn N ...\n\n---\n\n## Current request\n\nContinue from where we left off.`
8. Unit tests (8+): basic transcript, multiple turns, before_seq filter, char cap, tool merge, bulk elision, completion status, TodoWrite preservation.

**Constants:**
```rust
pub const DEFAULT_MAX_PRIMER_CHARS: usize = 24_000;
pub const DEFAULT_MAX_PRIMER_TURNS: usize = 20;
pub const MAX_TOOL_SUMMARY_CHARS: usize = 300;
```

---

### Task 2: Event-store `replay_before` helper

**Files:**
- Modify: `src/cockpit/event_store.rs`

**Steps:**
1. Add `pub fn replay_before(&self, session_id: &str, before_seq: u64) -> Vec<(u64, Event)>`.
2. SQL: `SELECT seq, event_json WHERE session_id=?1 AND seq < ?2 ORDER BY seq ASC`.
3. Unit test: insert events with mixed seq values, verify `seq < N` filter.

---

### Task 3: REST endpoint

**Files:**
- Modify: `src/server/api/cockpit.rs` (or wherever cockpit routes live)
- Modify: `src/server/mod.rs` (route wiring)
- Modify: `src/server/api/mod.rs` (export)

**Steps:**
1. `pub async fn get_context_primer(State, Path(session_id), Query(BeforeSeqQuery)) -> Json<ContextPrimerResponse>`.
2. Validate session exists; fetch events via `replay_before`; call `build_context_primer`; return JSON.
3. Wire `GET /api/sessions/:id/cockpit/context-primer?before_seq=N`.

**Response shape:**
```rust
pub struct ContextPrimerResponse {
    pub primer: String,
    pub included_event_count: usize,
    pub included_turn_count: usize,
    pub truncated: bool,
    pub max_chars: usize,
}
```

---

### Task 4: Frontend reducer flag

**Files:**
- Modify: `web/src/lib/cockpitTypes.ts`

**Steps:**
1. Add `contextPrimerAvailable: { resetSeq: number; reason: string } | null` to `CockpitState`.
2. Initial null in `emptyCockpitState`.
3. In `SessionContextReset` handler: if `hasPriorPrompt`, set `contextPrimerAvailable = { resetSeq: frame.seq, reason: event.SessionContextReset.reason }`.
4. In `UserPromptSent` handler: clear to null.
5. Existing context_reset activity row behavior preserved.

---

### Task 5: API client + types

**Files:**
- Modify: `web/src/lib/api.ts`
- Modify: `web/src/lib/types.ts`

**Steps:**
1. Add `ContextPrimerResponse` type.
2. Add `fetchContextPrimer(sessionId: string, beforeSeq: number)` → `Promise<ContextPrimerResponse | null>`.

---

### Task 6: ContextPrimerBanner component

**Files:**
- Create: `web/src/components/cockpit/ContextPrimerBanner.tsx`

**Props:**
```ts
interface Props {
  sessionId: string;
  available: { resetSeq: number; reason: string } | null;
  onInsertPrimer: (text: string) => void;
}
```

Behavior: render nothing if `available` is null. Otherwise: amber-tinted banner with text "Agent lost its prior context" + "Resume with prior context" button. On click: `fetchContextPrimer(sessionId, available.resetSeq)`, call `onInsertPrimer(primer)`. Loading + error states.

---

### Task 7: Wire composer prefill

**Files:**
- Modify: `web/src/components/cockpit/Composer.tsx`
- Modify: `web/src/components/cockpit/CockpitView.tsx`

**Steps:**
1. `Composer` gains `primerPrefill?: { id: string; text: string } | null` prop. Effect: on `primerPrefill.id` change, call `composerRuntime.setText(primerPrefill.text)` + focus textarea + cursor at end.
2. `CockpitView` holds `const [primerPrefill, setPrimerPrefill] = useState<{ id, text } | null>(null)`. Renders `<ContextPrimerBanner ... onInsertPrimer={(text) => setPrimerPrefill({ id: ${seq}-${Date.now()}, text })} />` above composer.

---

### Task 8: Frontend reducer tests

**Files:**
- Modify: `web/src/lib/cockpitTypes.test.ts` (exists)

**Cases:**
- `SessionContextReset` with prior `UserPromptSent` → `contextPrimerAvailable` set, carries `resetSeq` and `reason`.
- `SessionContextReset` with no prior prompt → `contextPrimerAvailable` stays null.
- `UserPromptSent` after `SessionContextReset` → flag cleared.

---

### Task 9: Docs

**Files:**
- Modify: `docs/cockpit.md` (or appropriate page)

Add a short subsection describing the load-failure recovery flow + banner + opt-in primer.

---

### Task 10: Build, fmt, clippy, cargo test, commit

Run `cargo build --features serve`, `cargo fmt`, `cargo clippy --features serve --all-targets`, `cargo test --lib`. Build frontend with `npx vite build`. Run Playwright on touched area if smoke-test possible. Commit.
